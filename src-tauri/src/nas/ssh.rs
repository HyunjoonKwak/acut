//! NAS — SSH와 rsync로. DSM API가 아니라 배관공의 도구다.
//!
//! 이 맥에는 `ssh nas`(포트 72, 키)가 이미 있고 NAS에는 rsync가 있다.
//! 내려받기·확인·비우기 셋 다 rsync/ssh 한 줄로 끝나고, 18,500개를 하나씩
//! HTTPS로 부르는 것보다 훨씬 빠르며 끊겨도 이어받는다. 자격증명은 저장하지
//! 않는다 — ssh 설정이 든다.
//!
//! rsync는 DSM의 rsync용 sshd(포트 22)로만 받아 준다 — 일반 sshd(72)로 들어온
//! rsync는 setuid 래퍼가 «Permission denied»를 낸다. 그래서 rsync는 `-p 22`,
//! 셸 명령(확인·비우기)은 ssh 별칭(72)으로 간다. 22번 sshd는 셸을 안 준다.
//! known_hosts에서 22번은 `[host]:22`가 아니라 맨 호스트명으로 찾는다 —
//! 같은 호스트 키를 이미 믿고 있으니 처음 보는 이름은 받아들인다(accept-new).
//!
//! DSM의 휴지통(#recycle)은 이 NAS에서 꺼져 있다. 그래서 «삭제»는 1차 구역
//! 안의 `#trash/`로 옮기는 것이다 — nas_photo가 공용에 쓰는 것과 같은 이름.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    /// ssh 설정의 Host 이름
    pub host: String,
    /// 1차 구역 — 폰·가족·구글포토가 먼저 닿는 곳. 처리 대기열.
    pub zone1: String,
    /// 개인(2차) — 맥 «내사진»과 1:1
    pub photos: String,
    /// 공용 — 맥 «공용»과 1:1
    pub shared: String,
    /// 내려받을 때 빼는 것 — 쉼표로
    pub exclude: String,
    /// rsync가 붙는 SSH 포트. DSM의 rsync 서비스는 제 sshd(기본 22)로만 받는다 —
    /// 일반 sshd(이 맥은 72)로 들어온 rsync는 setuid 래퍼가 거절한다 (실측).
    #[serde(default = "default_rsync_port")]
    pub rsync_port: u16,
}

fn default_rsync_port() -> u16 {
    22
}

impl Default for Config {
    fn default() -> Self {
        Config {
            host: "nas".into(),
            zone1: "/volume1/homes/luckyguy/Personal".into(),
            photos: "/volume1/homes/luckyguy/Photos".into(),
            shared: "/volume1/photo".into(),
            exclude: "@eaDir,#recycle,#trash,_quarantine,.DS_Store,Thumbs.db".into(),
            rsync_port: 22,
        }
    }
}

/// 1차 구역 안의 휴지통 폴더 이름
pub const TRASH_DIR: &str = "#trash";

#[derive(Debug, Clone, serde::Serialize)]
pub struct Status {
    pub online: bool,
    pub hostname: String,
    pub free_bytes: Option<u64>,
    pub zone1_files: Option<u64>,
    pub error: Option<String>,
    /// 이 맥의 rsync — 경로와 판. macOS 내장 openrsync는 옵션이 달라 못 쓴다.
    pub rsync: String,
    pub rsync_ok: bool,
}

/// 쓸 rsync — Homebrew 것을 먼저. GUI 앱의 PATH에는 /opt/homebrew/bin이 없어
/// 그냥 `rsync`라고 하면 macOS 내장 openrsync(프로토콜 29)가 잡힌다.
pub fn rsync_bin() -> std::path::PathBuf {
    for p in ["/opt/homebrew/bin/rsync", "/usr/local/bin/rsync"] {
        if Path::new(p).is_file() {
            return Path::new(p).to_path_buf();
        }
    }
    std::path::PathBuf::from("rsync")
}

/// (설명, 쓸 수 있나)
pub fn rsync_version() -> (String, bool) {
    let bin = rsync_bin();
    match Command::new(&bin).arg("--version").output() {
        Ok(o) => {
            let first = String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").trim().to_string();
            let ok = first.starts_with("rsync") && !first.contains("openrsync");
            (format!("{first} — {}", bin.display()), ok)
        }
        Err(e) => (format!("rsync 없음 ({e})"), false),
    }
}

/// rsync가 남긴 stderr를 사람 말로. Synology의 /usr/bin/rsync는 setuid 래퍼라
/// ssh 인증이 됐어도 DSM의 rsync 서비스·사용자 권한이 없으면 «Permission denied»를 낸다.
pub fn explain(stderr: &str) -> String {
    let t = stderr.trim();
    if t.contains("Permission denied") {
        return "NAS가 rsync를 거절했습니다 — DSM 제어판 › 파일 서비스 › rsync에서 «rsync 서비스 사용»을 켜고, 설정의 rsync 포트가 DSM의 rsync용 SSH 포트(기본 22)와 같은지 보세요. (ssh 접속 자체는 됩니다)".into();
    }
    if t.contains("Could not resolve hostname") {
        return format!("ssh 설정에 그 호스트가 없습니다: {t}");
    }
    if t.contains("Connection timed out") || t.contains("Connection refused") || t.contains("No route to host") {
        return format!("NAS에 닿지 않습니다 — 켜져 있고 같은 네트워크인지 보세요: {t}");
    }
    t.to_string()
}

fn ssh_base(cfg: &Config) -> Command {
    let mut c = Command::new("ssh");
    c.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", "LogLevel=ERROR", &cfg.host]);
    c
}

/// 셸에 넣을 경로 — 작은따옴표로 감싼다
fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// 연결이 되나, 남은 공간과 1차 구역의 파일 수는
pub fn check(cfg: &Config) -> Status {
    let (rsync, rsync_ok) = rsync_version();
    let script = format!(
        "hostname; df -Pk {z} | tail -1 | awk '{{print $4}}'; find {z} -type f ! -path '*/@eaDir/*' ! -path '*/{t}/*' ! -path '*/#recycle/*' | wc -l",
        z = q(&cfg.zone1),
        t = TRASH_DIR
    );
    let out = ssh_base(cfg).arg(script).output();
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            let mut lines = text.lines().map(str::trim);
            let hostname = lines.next().unwrap_or("").to_string();
            let free_bytes = lines.next().and_then(|s| s.parse::<u64>().ok()).map(|kb| kb * 1024);
            let zone1_files = lines.next().and_then(|s| s.parse().ok());
            Status { online: true, hostname, free_bytes, zone1_files, error: None, rsync, rsync_ok }
        }
        Ok(o) => Status {
            online: false,
            hostname: String::new(),
            free_bytes: None,
            zone1_files: None,
            error: Some(explain(&String::from_utf8_lossy(&o.stderr))),
            rsync,
            rsync_ok,
        },
        Err(e) => Status {
            online: false,
            hostname: String::new(),
            free_bytes: None,
            zone1_files: None,
            error: Some(e.to_string()),
            rsync,
            rsync_ok,
        },
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct PullProgress {
    /// 옮긴 항목 / 전체 항목 (rsync의 to-chk)
    pub done: usize,
    pub total: usize,
    pub percent: u8,
    pub current: String,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Pulled {
    /// 새로 받은 파일들 — 상대경로
    pub files: Vec<String>,
    pub cancelled: bool,
}

/// rsync가 쓸 ssh 명령 — rsync용 포트로
fn rsync_ssh(cfg: &Config) -> String {
    format!("ssh -p {} -o BatchMode=yes -o LogLevel=ERROR -o StrictHostKeyChecking=accept-new", cfg.rsync_port)
}

fn excludes(cfg: &Config) -> Vec<String> {
    cfg.exclude
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .flat_map(|p| ["--exclude".to_string(), p.to_string()])
        .collect()
}

/// rsync 진행 줄 — `12,345  45%  1.2MB/s  0:00:03 (xfr#12, to-chk=88/200)`
fn parse_progress(line: &str) -> Option<(u8, usize, usize)> {
    let pct = line.split_whitespace().find(|w| w.ends_with('%'))?.trim_end_matches('%').parse().ok()?;
    let (done, total) = match line.find("to-chk=") {
        Some(i) => {
            let rest = &line[i + 7..];
            let end = rest.find(')').unwrap_or(rest.len());
            let mut it = rest[..end].split('/');
            let left: usize = it.next()?.trim().parse().ok()?;
            let total: usize = it.next()?.trim().parse().ok()?;
            (total.saturating_sub(left), total)
        }
        None => (0, 0),
    };
    Some((pct, done, total))
}

/// 1차 구역을 `dest`로 내려받는다. 이어받기·증분은 rsync가 한다.
pub fn pull(
    cfg: &Config,
    dest: &Path,
    cancel: &AtomicBool,
    on_progress: impl Fn(&PullProgress),
) -> std::io::Result<Pulled> {
    std::fs::create_dir_all(dest)?;
    let src = format!("{}:{}/", cfg.host, cfg.zone1.trim_end_matches('/'));
    let (_, ok) = rsync_version();
    if !ok {
        return Err(std::io::Error::other("이 맥의 rsync가 macOS 내장 openrsync라 쓸 수 없습니다 — 터미널에서 `brew install rsync` 뒤 다시 하세요"));
    }
    let mut cmd = Command::new(rsync_bin());
    cmd.args(["-a", "--partial", "--no-inc-recursive", "--info=progress2", "--out-format=%n", "-e", &rsync_ssh(cfg)]);
    cmd.args(excludes(cfg));
    cmd.arg(&src).arg(format!("{}/", dest.to_string_lossy()));
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().unwrap();
    let mut out = Pulled::default();
    let mut p = PullProgress::default();
    // rsync는 진행 줄을 \r로 덮어쓴다 — \r과 \n 둘 다 줄 끝으로 본다
    let mut reader = BufReader::new(stdout);
    let mut buf = Vec::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            out.cancelled = true;
            break;
        }
        buf.clear();
        let n = read_until_any(&mut reader, &mut buf)?;
        if n == 0 {
            break;
        }
        let line = String::from_utf8_lossy(&buf).trim().to_string();
        if line.is_empty() {
            continue;
        }
        if let Some((pct, done, total)) = parse_progress(&line) {
            p.percent = pct;
            if total > 0 {
                p.done = done;
                p.total = total;
            }
            on_progress(&p);
        } else if !line.ends_with('/') {
            p.current = line.clone();
            out.files.push(line);
        }
    }
    let status = child.wait()?;
    if !status.success() && !out.cancelled {
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            use std::io::Read;
            let _ = e.read_to_string(&mut err);
        }
        return Err(std::io::Error::other(format!("rsync 실패 ({status}): {}", explain(&err))));
    }
    Ok(out)
}

/// \r 또는 \n까지 읽는다
fn read_until_any(r: &mut impl BufRead, buf: &mut Vec<u8>) -> std::io::Result<usize> {
    let mut total = 0;
    loop {
        let avail = r.fill_buf()?;
        if avail.is_empty() {
            return Ok(total);
        }
        let pos = avail.iter().position(|&b| b == b'\r' || b == b'\n');
        match pos {
            Some(i) => {
                buf.extend_from_slice(&avail[..i]);
                r.consume(i + 1);
                return Ok(total + i + 1);
            }
            None => {
                let n = avail.len();
                buf.extend_from_slice(avail);
                r.consume(n);
                total += n;
            }
        }
    }
}

/// 로컬 폴더가 NAS 폴더에 다 있나 — 없거나 크기가 다른 파일의 상대경로.
/// rsync 시험 실행(-n): «보낼 것»이 곧 «NAS에 없는 것»이다. 실제로는 아무것도 안 보낸다.
pub fn missing_on_nas(cfg: &Config, local: &Path, remote: &str) -> std::io::Result<Vec<String>> {
    let (_, ok) = rsync_version();
    if !ok {
        return Err(std::io::Error::other("이 맥의 rsync가 macOS 내장 openrsync라 쓸 수 없습니다 — 터미널에서 `brew install rsync` 뒤 다시 하세요"));
    }
    let out = Command::new(rsync_bin())
        .args(["-n", "-a", "--size-only", "--out-format=%n", "-e", &rsync_ssh(cfg)])
        .args(excludes(cfg))
        .arg(format!("{}/", local.to_string_lossy()))
        .arg(format!("{}:{}/", cfg.host, remote.trim_end_matches('/')))
        .output()?;
    if !out.status.success() {
        return Err(std::io::Error::other(format!(
            "rsync 실패: {}",
            explain(&String::from_utf8_lossy(&out.stderr))
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.ends_with('/') && *l != "./")
        .map(str::to_string)
        .collect())
}

/// 1차 구역의 파일들을 그 안의 `#trash/`로 옮긴다. 옮긴 것의 상대경로를 돌려준다.
pub fn trash_in_zone1(cfg: &Config, rels: &[String]) -> std::io::Result<Vec<String>> {
    if rels.is_empty() {
        return Ok(Vec::new());
    }
    // 목록은 stdin으로 NUL 구분 — 이름에 무엇이 들어 있어도 된다
    let script = format!(
        "cd {z} || exit 3; while IFS= read -r -d '' f; do d=$(dirname \"$f\"); mkdir -p \"{t}/$d\" && mv -n -- \"$f\" \"{t}/$f\" && printf '%s\\0' \"$f\"; done",
        z = q(&cfg.zone1),
        t = TRASH_DIR
    );
    let mut child = ssh_base(cfg).arg(script).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    {
        let mut stdin = child.stdin.take().unwrap();
        for r in rels {
            stdin.write_all(r.as_bytes())?;
            stdin.write_all(&[0])?;
        }
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(std::io::Error::other(format!(
            "ssh 실패: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(out
        .stdout
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_line_parses_percent_and_to_chk() {
        assert_eq!(
            parse_progress("     12,345,678  45%   12.34MB/s    0:00:05 (xfr#12, to-chk=88/200)"),
            Some((45, 112, 200))
        );
        assert_eq!(parse_progress("  1,000  3%  1.0MB/s  0:00:01"), Some((3, 0, 0)));
        assert_eq!(parse_progress("2024/여행/a.jpg"), None);
    }

    #[test]
    fn explains_the_synology_rsync_wrapper_refusal() {
        let m = explain("Permission denied, please try again.\nrsync: connection unexpectedly closed");
        assert!(m.contains("DSM 제어판"));
        assert!(explain("ssh: Could not resolve hostname nasroot").contains("호스트가 없습니다"));
        assert_eq!(explain("  odd  "), "odd");
    }

    /// 실제 NAS — 작은 폴더 하나를 받아 본다. `ACUT_NAS_DIR=/volume1/.../_정리 cargo test --lib nas::ssh::tests::real -- --ignored --nocapture`
    #[test]
    #[ignore = "실제 NAS 필요"]
    fn real_pull_small_folder() {
        let Ok(dir) = std::env::var("ACUT_NAS_DIR") else { return };
        let cfg = Config { zone1: dir, ..Default::default() };
        let d = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let last = std::cell::RefCell::new(PullProgress::default());
        let t = std::time::Instant::now();
        let r = pull(&cfg, d.path(), &cancel, |p| *last.borrow_mut() = p.clone()).unwrap();
        let last = last.into_inner();
        eprintln!("\n받음 {}개 · 마지막 진행 {}/{} {}% · {:.1}초 · 취소 {}", r.files.len(), last.done, last.total, last.percent, t.elapsed().as_secs_f64(), r.cancelled);
        for f in r.files.iter().take(3) {
            eprintln!("  {f}");
        }
        assert!(!r.cancelled);
        // 두 번째는 받을 것이 없다 — 증분
        let r2 = pull(&cfg, d.path(), &cancel, |_| {}).unwrap();
        assert_eq!(r2.files.len(), 0);
        let miss = missing_on_nas(&cfg, d.path(), &cfg.zone1).unwrap();
        eprintln!("확인: NAS에 없는 것 {}개 (0이어야)", miss.len());
        assert!(miss.is_empty());
    }

    #[test]
    fn homebrew_rsync_is_preferred_and_openrsync_is_refused() {
        let (desc, ok) = rsync_version();
        // 이 맥에는 Homebrew rsync가 있다 — 없는 맥이면 openrsync라 false여야 한다
        assert_eq!(ok, !desc.contains("openrsync") && desc.starts_with("rsync"), "{desc}");
    }

    #[test]
    fn excludes_become_rsync_flags() {
        let cfg = Config { exclude: "@eaDir, #trash,,".into(), ..Default::default() };
        assert_eq!(excludes(&cfg), vec!["--exclude", "@eaDir", "--exclude", "#trash"]);
    }

    #[test]
    fn shell_quoting_survives_apostrophes() {
        assert_eq!(q("a'b"), "'a'\\''b'");
    }

    #[test]
    fn read_until_any_splits_on_cr_and_lf() {
        let data = b"one\rtwo\nthree";
        let mut r = BufReader::new(&data[..]);
        let mut buf = Vec::new();
        read_until_any(&mut r, &mut buf).unwrap();
        assert_eq!(buf, b"one");
        buf.clear();
        read_until_any(&mut r, &mut buf).unwrap();
        assert_eq!(buf, b"two");
        buf.clear();
        read_until_any(&mut r, &mut buf).unwrap();
        assert_eq!(buf, b"three");
    }
}
