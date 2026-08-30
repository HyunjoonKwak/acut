//! 사진 없는 폴더 정리 — 사진을 다 치우고 나면 카메라 메모(IMG_xxxx.TXT)·썸네일(.thm)·zip 같은
//! 사진 아닌 파일만 남은 «껍데기» 폴더가 남는다. 앱은 사진만 알아서 그 폴더를 지우지 못했다
//! (실측 2026-08-30: 연도별 156개 폴더·1,555파일·737MB). 여기서 세어 보여 주고, 고른 것을 라이브러리
//! 휴지통(`.acut/휴지통/_폴더/<경로>`)으로 통째로 옮긴다 — Finder 로 되살릴 수 있고, 휴지통 비우기에서 같이 사라진다.

use crate::db::conn::{Db, Result};
use crate::db::libraries;
use crate::ops::trash::{free_path, move_file, trash_root};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn bad(msg: impl Into<String>) -> crate::db::conn::DbError {
    crate::db::conn::DbError::Invalid(msg.into())
}

fn is_junk(name: &str) -> bool {
    name == ".DS_Store" || name.starts_with("._") || name == "Thumbs.db" || name == "desktop.ini"
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Husk {
    /// 라이브러리 기준 경로
    pub rel: String,
    pub files: usize,
    pub bytes: u64,
    /// 확장자별 개수, 많은 것부터(넷까지)
    pub kinds: Vec<(String, usize)>,
}

/// 라이브러리 안에서 «사진이 하나도 없는데 파일은 있는» 폴더들 — 위 폴더부터, 그 아래는 안 내려간다
pub fn list(db: &Db, library_id: i64) -> Result<Vec<Husk>> {
    let lib = libraries::get(db, library_id)?.ok_or_else(|| bad("등록되지 않은 라이브러리입니다"))?;
    let dir = lib.dir.clone().ok_or_else(|| bad("디스크가 연결되어 있지 않습니다"))?;
    // 사진이 있는 폴더(볼륨 기준 경로, NFC)
    let live: Vec<String> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT DISTINCT fo.rel_path FROM files fi JOIN folders fo ON fo.id = fi.folder_id
             WHERE fo.library_id = ?1 AND fi.trashed_at IS NULL",
        )?;
        let it = st.query_map([library_id], |r| r.get(0))?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    let live_set: HashSet<&str> = live.iter().map(String::as_str).collect();
    // 어떤 폴더 아래에 사진이 있나 = 그 폴더가 사진 폴더의 위 폴더인가
    let mut has_photos: HashSet<String> = HashSet::new();
    for l in &live {
        let mut cur = l.as_str();
        loop {
            has_photos.insert(cur.to_string());
            match cur.rsplit_once('/') {
                Some((p, _)) => cur = p,
                None => break,
            }
        }
        has_photos.insert(String::new());
    }
    let mut out = Vec::new();
    walk(&dir, &dir, &lib.rel_path, &live_set, &has_photos, &mut out);
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(out)
}

fn walk(root: &Path, dir: &Path, lib_rel: &str, live: &HashSet<&str>, has_photos: &HashSet<String>, out: &mut Vec<Husk>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let name = crate::scan::nfc(&e.file_name().to_string_lossy());
        if name == ".acut" || crate::scan::kinds::is_skipped_dir(&name) {
            continue;
        }
        let p = e.path();
        let rel_in_lib = crate::scan::nfc(&p.strip_prefix(root).map(|r| r.to_string_lossy().into_owned()).unwrap_or_default());
        let vol_rel = if lib_rel.is_empty() { rel_in_lib.clone() } else { format!("{lib_rel}/{rel_in_lib}") };
        if has_photos.contains(&vol_rel) || live.contains(vol_rel.as_str()) {
            walk(root, &p, lib_rel, live, has_photos, out);
            continue;
        }
        // 이 아래엔 사진이 없다 — 파일이 있으면 껍데기
        let mut files = 0usize;
        let mut bytes = 0u64;
        let mut kinds: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for f in walkdir::WalkDir::new(&p).follow_links(false).into_iter().filter_map(|x| x.ok()) {
            if !f.file_type().is_file() {
                continue;
            }
            let n = f.file_name().to_string_lossy().into_owned();
            if is_junk(&n) {
                continue;
            }
            files += 1;
            bytes += f.metadata().map(|m| m.len()).unwrap_or(0);
            let ext = n.rsplit_once('.').map(|(_, x)| x.to_ascii_lowercase()).unwrap_or_else(|| "(없음)".into());
            *kinds.entry(ext).or_default() += 1;
        }
        if files == 0 {
            continue; // 빈 폴더(찌꺼기만) — 지울 것도 없다
        }
        let mut k: Vec<(String, usize)> = kinds.into_iter().collect();
        k.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        k.truncate(4);
        out.push(Husk { rel: rel_in_lib, files, bytes, kinds: k });
    }
}

/// 고른 껍데기 폴더들을 라이브러리 휴지통의 `_폴더/` 아래로 통째로 옮긴다. 옮긴 수를 돌려준다
pub fn to_trash(db: &Db, library_id: i64, rels: &[String]) -> Result<(usize, Option<String>)> {
    let lib = libraries::get(db, library_id)?.ok_or_else(|| bad("등록되지 않은 라이브러리입니다"))?;
    let dir = lib.dir.clone().ok_or_else(|| bad("디스크가 연결되어 있지 않습니다"))?;
    let bin = trash_root(&dir).join("_폴더");
    let mut moved = 0;
    let mut first_err: Option<String> = None;
    for rel in rels {
        // 라이브러리 밖으로 새지 않게
        if rel.is_empty() || rel.starts_with('/') || rel.split('/').any(|s| s == "..") {
            first_err.get_or_insert(format!("이상한 경로: {rel}"));
            continue;
        }
        let src = dir.join(rel);
        if !src.is_dir() {
            continue;
        }
        let dest = free_path(bin.join(rel));
        match move_file(&src, &dest) {
            Ok(()) => moved += 1,
            Err(e) => {
                first_err.get_or_insert(format!("{rel}: {e}"));
            }
        }
    }
    // 그래서 빈 위 폴더도 정리
    for rel in rels {
        if let Some(parent) = Path::new(rel).parent() {
            let p: PathBuf = dir.join(parent);
            crate::ops::trash::prune_empty_dirs(&p, &dir);
        }
    }
    Ok((moved, first_err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::scan_test;

    #[test]
    fn husks_are_folders_with_files_but_no_photos_and_go_to_the_folder_bin() {
        let dir = tempfile::tempdir().unwrap();
        // 2003/여행: 사진 있음 · 2003/메모: txt 만 · 2004: 하위에 zip 만
        for (d, files) in [("2003/여행", vec![("a.jpg", "photo")]), ("2003/메모", vec![("IMG_1.TXT", "note"), ("IMG_2.TXT", "note")]), ("2004/x", vec![("old.zip", "zip")])] {
            let p = dir.path().join(d);
            std::fs::create_dir_all(&p).unwrap();
            for (n, body) in files {
                std::fs::write(p.join(n), body.as_bytes().repeat(20)).unwrap();
            }
        }
        std::fs::write(dir.path().join("2004/.DS_Store"), b"").unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        scan_test(&db, dir.path(), 0, |_| {}).unwrap();
        let lib: i64 = db.read(|c| c.query_row("SELECT id FROM libraries", [], |r| r.get(0))).unwrap();
        let h = list(&db, lib).unwrap();
        let rels: Vec<&str> = h.iter().map(|x| x.rel.as_str()).collect();
        assert_eq!(rels, ["2003/메모", "2004"], "위 폴더부터, 사진 있는 2003/여행은 아니다: {h:?}");
        assert_eq!(h[0].files, 2);
        assert_eq!(h[0].kinds[0], ("txt".to_string(), 2));
        let (n, err) = to_trash(&db, lib, &["2003/메모".into(), "2004".into()]).unwrap();
        assert_eq!((n, err), (2, None));
        assert!(!dir.path().join("2003/메모").exists());
        assert!(!dir.path().join("2004").exists());
        assert!(dir.path().join(".acut/휴지통/_폴더/2003/메모/IMG_1.TXT").is_file(), "통째로 휴지통 _폴더 아래로");
        assert!(dir.path().join("2003/여행/a.jpg").is_file(), "사진 폴더는 그대로");
        assert!(list(&db, lib).unwrap().is_empty());
        assert!(to_trash(&db, lib, &["../etc".into()]).unwrap().1.is_some(), "밖으로 새는 경로는 거절");
    }
}
