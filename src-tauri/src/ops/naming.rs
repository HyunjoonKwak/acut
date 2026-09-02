//! 이벤트 이름 제안.
//!
//! 사람이 폴더명을 처음부터 타이핑하게 두면 정리를 안 하게 된다. 이미 쓴
//! 이름들이 가장 좋은 재료다 — 실제 라이브러리에 4,476개의 폴더명이 있고
//! `주원`·`상윤`·`생일`·`가족여행`처럼 되풀이되는 낱말이 뚜렷하다.
//!
//! 근거 네 갈래 (사양대로):
//!   1. 같은 날짜의 지난 이벤트 — 생일·기념일은 해마다 돌아온다
//!   2. 가까운 장소 — 그 좌표 근처에서 찍었던 지난 이벤트
//!   3. 앞 폴더 연속성 — 어제가 여행이면 오늘은 그 여행의 다음 날
//!   4. 자주 쓰는 낱말 — 위 셋이 비어도 뭔가는 내놓는다

use crate::db::conn::{Db, Result};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Suggestion {
    pub title: String,
    /// 왜 이걸 권하는지. 화면에 작게 붙는다.
    pub why: String,
    pub score: i32,
}

/// 폴더 이름에서 날짜와 제목을 가른다.
///
/// 실제로 쓰이는 꼴: `2003-02-08 아버지 환갑잔치`, `2024.08.27 제주`,
/// `20240827_여행`, 그리고 날짜가 없는 `황금부엉이`.
pub fn parse_folder(name: &str) -> (Option<String>, String) {
    let b = name.as_bytes();
    let digits_at =
        |i: usize, n: usize| i + n <= b.len() && b[i..i + n].iter().all(|c| c.is_ascii_digit());

    // YYYY-MM-DD / YYYY.MM.DD / YYYY_MM_DD
    if digits_at(0, 4) && b.len() >= 10 {
        let s1 = b[4];
        let s2 = b[7];
        if matches!(s1, b'-' | b'.' | b'_') && s1 == s2 && digits_at(5, 2) && digits_at(8, 2) {
            let date = format!("{}-{}-{}", &name[0..4], &name[5..7], &name[8..10]);
            return (Some(date), trim_title(&name[10..]));
        }
    }
    // YYYYMMDD
    if digits_at(0, 8) {
        let date = format!("{}-{}-{}", &name[0..4], &name[4..6], &name[6..8]);
        return (Some(date), trim_title(&name[8..]));
    }
    (None, name.trim().to_string())
}

fn trim_title(rest: &str) -> String {
    rest.trim_start_matches([' ', '_', '-', '.', '~'])
        .trim()
        .to_string()
}

/// `거제통영 가족여행 2일차` → (`거제통영 가족여행`, 2)
pub fn split_day_suffix(title: &str) -> (String, Option<u32>) {
    let t = title.trim_end();
    let Some(rest) = t.strip_suffix("일차") else {
        return (t.to_string(), None);
    };
    let digits: String = rest
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect();
    if digits.is_empty() {
        return (t.to_string(), None);
    }
    let n: u32 = digits
        .chars()
        .rev()
        .collect::<String>()
        .parse()
        .unwrap_or(1);
    let head = rest[..rest.len() - digits.len()].trim_end().to_string();
    (head, Some(n))
}

/// 앞 이벤트가 이어지면 다음 날차를 붙인다.
pub fn next_day(title: &str) -> String {
    let (head, n) = split_day_suffix(title);
    if head.is_empty() {
        return title.to_string();
    }
    format!("{head} {}일차", n.unwrap_or(1) + 1)
}

/// 제목에서 낱말을 뽑는다. 너무 짧거나 숫자뿐인 것은 버린다.
pub fn words(title: &str) -> Vec<String> {
    title
        .split(|c: char| c.is_whitespace() || matches!(c, '-' | '_' | ',' | '(' | ')'))
        .map(str::trim)
        .filter(|w| {
            w.chars().count() >= 2 && !w.chars().all(|c| c.is_ascii_digit()) && !w.ends_with("일차")
        })
        .map(crate::scan::nfc)
        .collect()
}

/// 두 좌표가 몇 km 떨어져 있는가 (하버사인).
pub fn km_between(a: (f64, f64), b: (f64, f64)) -> f64 {
    let r = 6371.0_f64;
    let (dlat, dlon) = ((b.0 - a.0).to_radians(), (b.1 - a.1).to_radians());
    let h = (dlat / 2.0).sin().powi(2)
        + a.0.to_radians().cos() * b.0.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    2.0 * r * h.sqrt().asin()
}

/// 후보를 모아 점수순으로 정리한다. 같은 제목은 한 번만.
fn rank(mut all: Vec<Suggestion>, limit: usize) -> Vec<Suggestion> {
    all.sort_by(|a, b| b.score.cmp(&a.score).then(a.title.cmp(&b.title)));
    let mut seen = std::collections::HashSet::new();
    all.retain(|s| !s.title.is_empty() && seen.insert(s.title.clone()));
    all.truncate(limit);
    all
}

/// 고른 사진들의 촬영 시각·좌표.
struct Facts {
    /// 가장 이른 촬영일 `YYYY-MM-DD`
    date: String,
    lat: Option<f64>,
    lon: Option<f64>,
}

fn facts(db: &Db, ids: &[i64]) -> Result<Option<Facts>> {
    if ids.is_empty() {
        return Ok(None);
    }
    let list = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    db.read(|c| {
        c.query_row(
            &format!(
                "SELECT date(MIN(taken_at),'unixepoch','localtime'),
                        AVG(gps_lat), AVG(gps_lon)
                 FROM files WHERE id IN ({list})"
            ),
            [],
            |r| {
                Ok(r.get::<_, Option<String>>(0)?.map(|date| Facts {
                    date,
                    lat: r.get(1).ok().flatten(),
                    lon: r.get(2).ok().flatten(),
                }))
            },
        )
    })
}

/// 이미 쓰인 폴더 이름들. (이름, 대표 좌표, 사진 수)
struct Known {
    date: Option<String>,
    title: String,
    lat: Option<f64>,
    lon: Option<f64>,
}

/// 폴더 전체를 훑는 이름·좌표 자료. 이벤트 후보가 여러 묶음이어도 이 값은
/// 한 번만 읽는다. 후보마다 `folders × files GROUP BY`를 되풀이하면 사진 수가
/// 아니라 후보 수만큼 같은 대형 질의가 실행된다.
pub(crate) struct SuggestionIndex {
    folders: Vec<Known>,
}

fn known_folders(db: &Db) -> Result<Vec<Known>> {
    db.read(|c| {
        let mut st = c.prepare(
            "SELECT fo.name, AVG(fi.gps_lat), AVG(fi.gps_lon)
             FROM folders fo LEFT JOIN files fi ON fi.folder_id = fo.id
             WHERE fo.file_count > 0
             GROUP BY fo.id",
        )?;
        let it = st.query_map([], |r| {
            let name: String = r.get(0)?;
            let (date, title) = parse_folder(&name);
            Ok(Known {
                date,
                title,
                lat: r.get(1)?,
                lon: r.get(2)?,
            })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })
}

impl SuggestionIndex {
    pub(crate) fn load(db: &Db) -> Result<Self> {
        Ok(Self {
            folders: known_folders(db)?,
        })
    }

    pub(crate) fn suggest(&self, db: &Db, ids: &[i64], limit: usize) -> Result<Vec<Suggestion>> {
        let Some(f) = facts(db, ids)? else {
            return Ok(Vec::new());
        };
        Ok(suggest_from(&f, &self.folders, limit))
    }
}

/// 이벤트 이름 후보. 점수가 높은 것부터.
pub fn suggest(db: &Db, ids: &[i64], limit: usize) -> Result<Vec<Suggestion>> {
    SuggestionIndex::load(db)?.suggest(db, ids, limit)
}

fn suggest_from(f: &Facts, folders: &[Known], limit: usize) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = Vec::new();
    let md = f.date.get(5..).unwrap_or("");

    for k in folders {
        if k.title.is_empty() {
            continue;
        }
        // 1. 같은 날짜 (해마다 돌아오는 것들)
        if let Some(d) = &k.date {
            if d != &f.date && d.get(5..) == Some(md) {
                out.push(Suggestion {
                    title: k.title.clone(),
                    why: format!("{}에도 이 이름을 썼습니다", &d[..4]),
                    score: 100,
                });
            }
            // 3. 어제 = 이어지는 여행
            if is_previous_day(d, &f.date) {
                out.push(Suggestion {
                    title: next_day(&k.title),
                    why: format!("어제 「{}」에서 이어집니다", k.title),
                    score: 120,
                });
            }
        }
        // 2. 가까운 장소
        if let (Some(la), Some(lo), Some(kla), Some(klo)) = (f.lat, f.lon, k.lat, k.lon) {
            let d = km_between((la, lo), (kla, klo));
            if d < 3.0 {
                out.push(Suggestion {
                    title: k.title.clone(),
                    why: format!("{:.1}km 안에서 찍었던 곳", d),
                    score: 90 - (d * 10.0) as i32,
                });
            }
        }
    }

    // 4. 자주 쓰는 낱말 — 위가 다 비어도 뭔가는 내놓는다.
    // **날짜가 붙은 폴더의 낱말만** 센다. 그게 사람이 이름 지은 이벤트다.
    // 안 그러면 도구가 만든 `output` 같은 폴더가 82번으로 1등을 한다(실측).
    let mut freq: std::collections::HashMap<String, i32> = std::collections::HashMap::new();
    for k in folders.iter().filter(|k| k.date.is_some()) {
        for w in words(&k.title) {
            *freq.entry(w).or_default() += 1;
        }
    }
    let mut common: Vec<(String, i32)> = freq.into_iter().filter(|(_, n)| *n >= 3).collect();
    common.sort_by(|a, b| b.1.cmp(&a.1));
    for (w, n) in common.into_iter().take(8) {
        out.push(Suggestion {
            title: w,
            why: format!("{n}번 쓴 낱말"),
            score: 10,
        });
    }

    rank(out, limit)
}

/// `b`가 `a`의 다음 날인가. 문자열 날짜를 그대로 비교한다.
fn is_previous_day(a: &str, b: &str) -> bool {
    let days = |s: &str| -> Option<i64> {
        let y: i64 = s.get(0..4)?.parse().ok()?;
        let m: i64 = s.get(5..7)?.parse().ok()?;
        let d: i64 = s.get(8..10)?.parse().ok()?;
        // 1970년 이후만 다루면 되므로 단순한 민력(civil) 환산으로 충분하다
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        Some(era * 146_097 + doe - 719_468)
    };
    matches!((days(a), days(b)), (Some(x), Some(y)) if y - x == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_split_into_date_and_title() {
        // 실제 라이브러리에 있는 꼴들
        assert_eq!(
            parse_folder("2003-02-08 아버지 환갑잔치"),
            (Some("2003-02-08".into()), "아버지 환갑잔치".into())
        );
        assert_eq!(
            parse_folder("2024.08.27 제주"),
            (Some("2024-08-27".into()), "제주".into())
        );
        assert_eq!(
            parse_folder("20240827_여행"),
            (Some("2024-08-27".into()), "여행".into())
        );
        assert_eq!(parse_folder("황금부엉이"), (None, "황금부엉이".into()));
        // 날짜만 있는 폴더도 흔하다
        assert_eq!(
            parse_folder("2003-02-02"),
            (Some("2003-02-02".into()), "".into())
        );
    }

    #[test]
    fn a_year_alone_is_not_a_date() {
        assert_eq!(parse_folder("2016"), (None, "2016".into()));
        assert_eq!(parse_folder("연도별"), (None, "연도별".into()));
    }

    #[test]
    fn day_counters_increment() {
        assert_eq!(
            split_day_suffix("거제통영 가족여행 2일차"),
            ("거제통영 가족여행".into(), Some(2))
        );
        assert_eq!(split_day_suffix("하와이"), ("하와이".into(), None));
        assert_eq!(next_day("하와이"), "하와이 2일차");
        assert_eq!(next_day("하와이 2일차"), "하와이 3일차");
        assert_eq!(next_day("하와이 10일차"), "하와이 11일차");
    }

    #[test]
    fn words_skip_noise() {
        assert_eq!(
            words("거제통영 가족여행 2일차"),
            vec!["거제통영", "가족여행"]
        );
        assert_eq!(words("2024 생일"), vec!["생일"], "숫자만인 것은 버린다");
        assert!(words("a").is_empty(), "한 글자는 낱말로 치지 않는다");
    }

    #[test]
    fn previous_day_across_month_and_year_ends() {
        assert!(is_previous_day("2024-08-26", "2024-08-27"));
        assert!(is_previous_day("2024-07-31", "2024-08-01"), "달을 넘어도");
        assert!(is_previous_day("2023-12-31", "2024-01-01"), "해를 넘어도");
        assert!(is_previous_day("2024-02-29", "2024-03-01"), "윤년도");
        assert!(!is_previous_day("2024-08-27", "2024-08-27"));
        assert!(!is_previous_day("2024-08-27", "2024-08-29"));
        assert!(
            !is_previous_day("2024-08-27", "2024-08-26"),
            "거꾸로는 아니다"
        );
    }

    #[test]
    fn distance_is_roughly_right() {
        // 서울시청 ↔ 강남역 약 8.5km
        let d = km_between((37.5665, 126.9780), (37.4979, 127.0276));
        assert!((7.0..10.0).contains(&d), "{d}");
        assert!(km_between((37.5, 127.0), (37.5, 127.0)) < 0.001);
    }

    /// 도구가 만든 폴더(`output` 82개)가 낱말 순위 1등을 하던 실측 문제.
    /// 날짜가 붙은 폴더만 세면 사람이 지은 이름만 남는다.
    #[test]
    fn only_dated_folders_feed_the_word_list() {
        let dated = parse_folder("2024-08-27 가족여행");
        let plain = parse_folder("output");
        assert!(dated.0.is_some());
        assert!(plain.0.is_none(), "날짜 없는 폴더는 이벤트가 아니다");
    }

    #[test]
    fn ranking_drops_duplicates_and_keeps_the_best() {
        let s = |t: &str, n: i32| Suggestion {
            title: t.into(),
            why: String::new(),
            score: n,
        };
        let r = rank(vec![s("여행", 10), s("생일", 50), s("여행", 90)], 10);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].title, "여행", "같은 제목은 높은 점수만 남는다");
        assert_eq!(r[0].score, 90);
    }
}

#[cfg(test)]
mod real {
    use super::*;
    use crate::db::conn::Db;

    /// 실제 라이브러리로 돌려 본다 (사본을 쓴다).
    /// `cargo test --lib ops::naming::real -- --ignored --nocapture`
    #[test]
    #[ignore = "실제 DB 사본 필요"]
    fn suggestions_on_the_real_library() {
        let live = dirs_next_home().join("Library/Application Support/com.acut.media/acut-v2.db");
        if !live.is_file() {
            eprintln!("실제 DB 없음 — 건너뜀");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let copy = tmp.path().join("copy.db");
        std::fs::copy(&live, &copy).unwrap();
        let db = Db::open(&copy).unwrap();

        // 아무 폴더에서 한 줌 골라 본다
        let ids: Vec<i64> = db
            .read(|c| {
                let mut st = c.prepare(
                    "SELECT id FROM files WHERE folder_id =
                       (SELECT folder_id FROM files GROUP BY folder_id
                         ORDER BY COUNT(*) DESC LIMIT 1) LIMIT 20",
                )?;
                let it = st.query_map([], |r| r.get(0))?;
                it.collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap();
        assert!(!ids.is_empty());

        let t = std::time::Instant::now();
        let out = suggest(&db, &ids, 12).unwrap();
        println!("\n제안 {:.0}ms", t.elapsed().as_secs_f64() * 1000.0);
        for s in &out {
            println!("  {:<24} {:>4}  {}", s.title, s.score, s.why);
        }
        assert!(!out.is_empty(), "실제 라이브러리라면 뭔가는 나와야 한다");
    }

    fn dirs_next_home() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("HOME").unwrap())
    }
}
