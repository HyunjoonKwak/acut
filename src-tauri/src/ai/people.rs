//! 사람 — 사진마다 얼굴을 찾아 벡터를 남기고, 가까운 얼굴끼리 한 사람으로 묶는다.
//!
//! 두 단계다. **찾기**는 썸네일마다 한 번(YuNet → 정렬 → SFace), 결과는
//! `faces`에, 끝난 표시는 `files.faces_at`에 — 얼굴이 없어도 표시는 남아
//! 다음에 다시 보지 않는다. **묶기**는 아직 사람이 없는 얼굴을 기존 사람의
//! 중심과 견줘 가깝으면 넣고 아니면 새 사람을 만든다. 이름은 사용자가 붙인다.
//!
//! 썸네일에서 24px보다 작은 얼굴은 버린다 — 누구인지 알아볼 수 없는 벡터는
//! 엉뚱한 사람에 붙는다.

use super::faces::{align, Detector, Recognizer};
use super::models::{self, ModelId};
use super::{clip, Result};
use crate::db::conn::Db;
use crate::media::cache;
use rayon::prelude::*;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 썸네일에서 이보다 작은 얼굴은 버린다 (픽셀)
pub const MIN_FACE_PX: f32 = 24.0;
/// 이 위면 같은 사람으로 본다. OpenCV가 말하는 «같은 사람» 문턱은 0.363이지만
/// 묶기는 틀리면 남의 사진이 섞이므로 더 높인다. 실측: 다른 사람끼리 ≤0.34.
pub const SAME_PERSON: f32 = 0.5;
/// 동시에 도는 작업 수 — 스레드마다 모델 한 벌(SFace 40MB)
const WORKERS: usize = 4;
const CHUNK: usize = 64;

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct FaceProgress {
    pub total: usize,
    pub done: usize,
    pub faces: usize,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct ClusterResult {
    pub assigned: usize,
    pub new_persons: usize,
    pub persons: usize,
}

struct Job {
    id: i64,
    thumb: PathBuf,
}

struct FaceRow {
    bbox: String,
    emb: Vec<f32>,
}

fn jobs(db: &Db, cache_base: &Path) -> Result<Vec<Job>> {
    let rows: Vec<(i64, i64, String)> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.id, fo.library_id, t.rel_path
               FROM files fi
               JOIN folders fo ON fo.id = fi.folder_id
               JOIN thumbs t ON t.file_id = fi.id AND t.state = 1
              WHERE fi.faces_at IS NULL AND fi.kind <> 1 AND fi.trashed_at IS NULL",
        )?;
        let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    Ok(rows
        .into_iter()
        .map(|(id, lib, rel)| Job {
            id,
            thumb: cache::cache_root(cache_base, lib).join(rel),
        })
        .collect())
}

thread_local! {
    static MODELS: RefCell<Option<Arc<(Detector, Recognizer)>>> = const { RefCell::new(None) };
}

/// 이 스레드의 모델 한 벌 — 처음 쓸 때 올린다
fn with_models<T>(
    det: &Path,
    rec: &Path,
    f: impl FnOnce(&Detector, &Recognizer) -> T,
) -> Result<T> {
    let m = MODELS.with(|slot| -> Result<Arc<(Detector, Recognizer)>> {
        let mut s = slot.borrow_mut();
        if s.is_none() {
            *s = Some(Arc::new((
                Detector::load(det, 1)?,
                Recognizer::load(rec, 1)?,
            )));
        }
        Ok(Arc::clone(s.as_ref().unwrap()))
    })?;
    Ok(f(&m.0, &m.1))
}

/// 썸네일 한 장의 얼굴들. 상자는 그림 크기에 대한 비율(0~1)로 적는다 —
/// 어느 크기의 그림에 얹어도 맞다.
fn faces_of(det: &Detector, rec: &Recognizer, path: &Path) -> Result<Vec<FaceRow>> {
    let img = image::open(path)?.to_rgb8();
    let (w, h) = (img.width() as f32, img.height() as f32);
    let mut out = Vec::new();
    for f in det.detect(&img)? {
        if f.w < MIN_FACE_PX || f.h < MIN_FACE_PX {
            continue;
        }
        let emb = rec.embed(&align(&img, &f.kps))?;
        let bbox = format!(
            r#"{{"x":{:.4},"y":{:.4},"w":{:.4},"h":{:.4},"s":{:.2}}}"#,
            f.x / w,
            f.y / h,
            f.w / w,
            f.h / h,
            f.score
        );
        out.push(FaceRow { bbox, emb });
    }
    Ok(out)
}

pub fn run(
    db: &Db,
    app_data: &Path,
    cache_base: &Path,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(&FaceProgress) + Sync + Send,
) -> Result<FaceProgress> {
    let list = jobs(db, cache_base)?;
    let progress = Mutex::new(FaceProgress {
        total: list.len(),
        ..Default::default()
    });
    on_progress(&progress.lock().unwrap().clone());
    if list.is_empty() {
        return Ok(progress.into_inner().unwrap());
    }
    let det = models::path(app_data, ModelId::FaceDetect);
    let rec = models::path(app_data, ModelId::FaceEmbed);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(WORKERS)
        .build()
        .map_err(|e| super::AiError::Other(e.to_string()))?;
    let mut last = Instant::now();

    for chunk in list.chunks(CHUNK) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let results: Vec<(i64, Vec<FaceRow>)> = pool.install(|| {
            chunk
                .par_iter()
                .map(|j| {
                    let faces = with_models(&det, &rec, |d, r| faces_of(d, r, &j.thumb))
                        .and_then(|r| r)
                        .unwrap_or_else(|e| {
                            log::warn!("얼굴 찾기 실패 {}: {e}", j.thumb.display());
                            Vec::new()
                        });
                    (j.id, faces)
                })
                .collect()
        });
        let now = chrono::Utc::now().timestamp();
        let n_faces: usize = results.iter().map(|(_, f)| f.len()).sum();
        db.transaction(|tx| {
            let mut ins =
                tx.prepare("INSERT INTO faces(file_id, bbox, embedding) VALUES(?1, ?2, ?3)")?;
            let mut mark = tx.prepare("UPDATE files SET faces_at = ?2 WHERE id = ?1")?;
            for (id, faces) in &results {
                for f in faces {
                    ins.execute(rusqlite::params![id, f.bbox, clip::to_blob(&f.emb)])?;
                }
                mark.execute(rusqlite::params![id, now])?;
            }
            Ok(())
        })?;
        let mut p = progress.lock().unwrap();
        p.done += chunk.len();
        p.faces += n_faces;
        if last.elapsed() >= Duration::from_millis(100) {
            last = Instant::now();
            on_progress(&p.clone());
        }
    }
    let out = progress.into_inner().unwrap();
    on_progress(&out);
    Ok(out)
}

/// (얼굴을 찾은 사진, 찾을 수 있는 사진, 얼굴 수, 사람 수)
pub fn counts(db: &Db) -> Result<(i64, i64, i64, i64)> {
    Ok(db.read(|c| {
        c.query_row(
            "SELECT
               (SELECT COUNT(*) FROM files fi WHERE fi.faces_at IS NOT NULL AND fi.trashed_at IS NULL),
               (SELECT COUNT(*) FROM files fi
                 WHERE fi.kind <> 1 AND fi.trashed_at IS NULL
                   AND (fi.faces_at IS NOT NULL
                        OR EXISTS (SELECT 1 FROM thumbs t WHERE t.file_id = fi.id AND t.state = 1))),
               (SELECT COUNT(*) FROM faces),
               (SELECT COUNT(*) FROM persons)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
    })?)
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

struct Person {
    id: i64,
    center: Vec<f32>,
    n: usize,
}

impl Person {
    fn add(&mut self, e: &[f32]) {
        let n = self.n as f32;
        let mixed: Vec<f32> = self.center.iter().zip(e).map(|(c, x)| c * n + x).collect();
        self.center = clip::normalize(&mixed);
        self.n += 1;
    }
}

/// 아직 사람이 없는 얼굴을 묶는다. 기존 사람의 중심과 가까우면 그 사람, 아니면 새 사람.
pub fn cluster(db: &Db) -> Result<ClusterResult> {
    let mut persons: Vec<Person> = {
        let rows: Vec<(i64, Vec<u8>)> = db.read(|c| {
            let mut st = c.prepare("SELECT person_id, embedding FROM faces WHERE person_id IS NOT NULL ORDER BY person_id")?;
            let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        let mut out: Vec<Person> = Vec::new();
        for (pid, blob) in rows {
            let e = clip::from_blob(&blob);
            match out.last_mut() {
                Some(p) if p.id == pid => p.add(&e),
                _ => out.push(Person {
                    id: pid,
                    center: e,
                    n: 1,
                }),
            }
        }
        out
    };
    let loose: Vec<(i64, i64, Vec<f32>)> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT id, file_id, embedding FROM faces WHERE person_id IS NULL ORDER BY id",
        )?;
        let it = st.query_map([], |r| {
            let blob: Vec<u8> = r.get(2)?;
            Ok((r.get(0)?, r.get(1)?, clip::from_blob(&blob)))
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    if loose.is_empty() {
        return Ok(ClusterResult {
            persons: persons.len(),
            ..Default::default()
        });
    }

    // (얼굴, 사람) — 사람이 새것이면 id는 음수 자리표, 나중에 진짜 id로 바꾼다
    let mut assign: Vec<(i64, usize)> = Vec::with_capacity(loose.len());
    let mut fresh: Vec<i64> = Vec::new(); // 새 사람의 대표 file_id
    let before = persons.len();
    for (fid, file_id, e) in &loose {
        let best = persons
            .iter()
            .enumerate()
            .map(|(i, p)| (i, dot(&p.center, e)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((i, s)) if s >= SAME_PERSON => {
                persons[i].add(e);
                assign.push((*fid, i));
            }
            _ => {
                persons.push(Person {
                    id: -(fresh.len() as i64) - 1,
                    center: e.clone(),
                    n: 1,
                });
                fresh.push(*file_id);
                assign.push((*fid, persons.len() - 1));
            }
        }
    }

    db.transaction(|tx| {
        // 새 사람부터 만들어 진짜 id를 받는다
        for (k, cover) in fresh.iter().enumerate() {
            tx.execute(
                "INSERT INTO persons(name, cover_file) VALUES(NULL, ?1)",
                [cover],
            )?;
            persons[before + k].id = tx.last_insert_rowid();
        }
        let mut up = tx.prepare("UPDATE faces SET person_id = ?2 WHERE id = ?1")?;
        for (fid, i) in &assign {
            up.execute(rusqlite::params![fid, persons[*i].id])?;
        }
        Ok(())
    })?;
    Ok(ClusterResult {
        assigned: assign.len(),
        new_persons: fresh.len(),
        persons: persons.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> (tempfile::TempDir, Db) {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(d.path().join("t.db")).unwrap();
        (d, db)
    }

    fn unit(v: &[f32]) -> Vec<f32> {
        clip::normalize(v)
    }

    /// 파일 몇 개와 얼굴들을 심는다: (file_id, 벡터)
    fn seed(db: &Db, faces: &[(i64, Vec<f32>)]) {
        db.transaction(|tx| {
            tx.execute("INSERT INTO volumes(uuid,name,role) VALUES('V','t','library')", [])?;
            tx.execute("INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','f','f',1)", [])?;
            for fid in 1..=4 {
                tx.execute(
                    "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at)
                     VALUES(?1,1,?2,1,0,0,0,0)",
                    rusqlite::params![fid, format!("f{fid}.jpg")],
                )?;
            }
            for (file, v) in faces {
                tx.execute(
                    "INSERT INTO faces(file_id, bbox, embedding) VALUES(?1, '{}', ?2)",
                    rusqlite::params![file, clip::to_blob(v)],
                )?;
            }
            Ok(())
        })
        .unwrap();
    }

    fn person_of(db: &Db, face_id: i64) -> Option<i64> {
        db.read(|c| {
            c.query_row(
                "SELECT person_id FROM faces WHERE id = ?1",
                [face_id],
                |r| r.get(0),
            )
        })
        .unwrap()
    }

    #[test]
    fn close_faces_become_one_person_and_far_ones_another() {
        let (_d, db) = db();
        seed(
            &db,
            &[
                (1, unit(&[1.0, 0.0, 0.0])),
                (2, unit(&[0.9, 0.1, 0.0])), // 1과 가깝다 (코사인 0.99)
                (3, unit(&[0.0, 1.0, 0.0])), // 멀다
            ],
        );
        let r = cluster(&db).unwrap();
        assert_eq!((r.assigned, r.new_persons, r.persons), (3, 2, 2));
        assert_eq!(person_of(&db, 1), person_of(&db, 2));
        assert_ne!(person_of(&db, 1), person_of(&db, 3));
    }

    #[test]
    fn a_later_run_only_touches_loose_faces_and_reuses_people() {
        let (_d, db) = db();
        seed(&db, &[(1, unit(&[1.0, 0.0, 0.0]))]);
        cluster(&db).unwrap();
        let first = person_of(&db, 1).unwrap();
        db.transaction(|tx| {
            tx.execute(
                "INSERT INTO faces(file_id, bbox, embedding) VALUES(2, '{}', ?1)",
                [clip::to_blob(&unit(&[0.95, 0.05, 0.0]))],
            )?;
            Ok(())
        })
        .unwrap();
        let r = cluster(&db).unwrap();
        assert_eq!((r.assigned, r.new_persons), (1, 0));
        assert_eq!(person_of(&db, 2), Some(first));
    }

    #[test]
    fn new_person_gets_the_first_face_as_cover() {
        let (_d, db) = db();
        seed(&db, &[(3, unit(&[0.0, 0.0, 1.0]))]);
        cluster(&db).unwrap();
        let cover: i64 = db
            .read(|c| c.query_row("SELECT cover_file FROM persons", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(cover, 3);
    }

    #[test]
    fn person_center_moves_toward_new_members() {
        let mut p = Person {
            id: 1,
            center: unit(&[1.0, 0.0]),
            n: 1,
        };
        p.add(&unit(&[0.0, 1.0]));
        assert!((p.center[0] - p.center[1]).abs() < 1e-5);
        assert_eq!(p.n, 2);
    }

    /// 실제 DB 사본으로 — 처음 N장만 찾고 묶어 속도와 결과를 본다.
    /// `ACUT_DB_COPY=… ACUT_LIMIT=2000 cargo test --release --lib ai::people::tests::real -- --ignored --nocapture`
    #[test]
    #[ignore = "실제 DB 사본과 얼굴 모델 필요"]
    fn real_library_copy() {
        let Ok(copy) = std::env::var("ACUT_DB_COPY") else {
            return;
        };
        let home = std::env::var("HOME").unwrap();
        let base =
            std::path::PathBuf::from(&home).join("Library/Application Support/com.acut.media");
        if !models::face_present(&base) {
            eprintln!("얼굴 모델 없음 — 건너뜀");
            return;
        }
        let limit: i64 = std::env::var("ACUT_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2000);
        let db = Db::open(copy).unwrap();
        // 처음 N장만 남기고 나머지는 «본 것»으로 표시한다
        db.transaction(|tx| {
            tx.execute("UPDATE files SET faces_at = NULL", [])?;
            tx.execute("DELETE FROM faces", [])?;
            tx.execute("DELETE FROM persons", [])?;
            tx.execute(
                "UPDATE files SET faces_at = 0 WHERE id NOT IN (
                   SELECT fi.id FROM files fi JOIN thumbs t ON t.file_id = fi.id AND t.state = 1
                    WHERE fi.kind <> 1 AND fi.trashed_at IS NULL ORDER BY fi.taken_at DESC LIMIT ?1)",
                [limit],
            )?;
            Ok(())
        })
        .unwrap();
        let t = std::time::Instant::now();
        let p = run(&db, &base, &base, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        let dt = t.elapsed().as_secs_f64();
        eprintln!(
            "\n[얼굴 찾기] {}장 · 얼굴 {}개 · {dt:.1}초 · 초당 {:.0}장",
            p.done,
            p.faces,
            p.done as f64 / dt
        );
        let t = std::time::Instant::now();
        let c = cluster(&db).unwrap();
        eprintln!(
            "[묶기] 얼굴 {} → 사람 {}명 (새 {}) · {:.2}초",
            c.assigned,
            c.persons,
            c.new_persons,
            t.elapsed().as_secs_f64()
        );
        let sizes: Vec<i64> = db
            .read(|c| {
                let mut st = c.prepare(
                    "SELECT COUNT(*) FROM faces GROUP BY person_id ORDER BY 1 DESC LIMIT 8",
                )?;
                let it = st.query_map([], |r| r.get(0))?;
                it.collect()
            })
            .unwrap();
        eprintln!("큰 사람들 얼굴 수: {sizes:?}");
        // 눈으로 볼 수 있게 — 상위 세 사람, 얼굴 넷씩 한 장에 (ACUT_OUT)
        if let Ok(out) = std::env::var("ACUT_OUT") {
            let rows: Vec<(i64, i64, String, String)> = db
                .read(|c| {
                    let mut st = c.prepare(
                        "SELECT f.person_id, fo.library_id, t.rel_path, f.bbox FROM faces f
                           JOIN files fi ON fi.id = f.file_id
                           JOIN folders fo ON fo.id = fi.folder_id
                           JOIN thumbs t ON t.file_id = fi.id AND t.state = 1
                          WHERE f.person_id IN (SELECT person_id FROM faces GROUP BY person_id ORDER BY COUNT(*) DESC LIMIT 3)
                          ORDER BY f.person_id, f.id",
                    )?;
                    let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
                    it.collect()
                })
                .unwrap();
            let side = 96u32;
            let mut sheet = image::RgbImage::new(side * 4, side * 3);
            let mut row = -1i32;
            let mut last = -1i64;
            let mut col = 0u32;
            for (pid, lib, rel, bbox) in rows {
                if pid != last {
                    last = pid;
                    row += 1;
                    col = 0;
                }
                if col >= 4 || row >= 3 {
                    continue;
                }
                let v: serde_json::Value = serde_json::from_str(&bbox).unwrap();
                let path = cache::cache_root(&base, lib).join(&rel);
                let Ok(img) = image::open(&path) else {
                    continue;
                };
                let img = img.to_rgb8();
                let (w, h) = (img.width() as f32, img.height() as f32);
                let g = |k: &str| v[k].as_f64().unwrap() as f32;
                let (cx, cy) = ((g("x") + g("w") / 2.0) * w, (g("y") + g("h") / 2.0) * h);
                let half = (g("w") * w).max(g("h") * h) * 0.8;
                let x0 = (cx - half).max(0.0) as u32;
                let y0 = (cy - half).max(0.0) as u32;
                let cw = ((cx + half).min(w) as u32).saturating_sub(x0).max(1);
                let ch = ((cy + half).min(h) as u32).saturating_sub(y0).max(1);
                let crop = image::imageops::crop_imm(&img, x0, y0, cw, ch).to_image();
                let small = image::imageops::resize(
                    &crop,
                    side,
                    side,
                    image::imageops::FilterType::Triangle,
                );
                image::imageops::replace(
                    &mut sheet,
                    &small,
                    (col * side) as i64,
                    (row as u32 * side) as i64,
                );
                col += 1;
            }
            let p = format!("{out}/people_sheet.png");
            sheet.save(&p).unwrap();
            eprintln!("얼굴 모음: {p}");
        }
    }

    #[test]
    fn counts_before_anything_is_scanned() {
        let (_d, db) = db();
        seed(&db, &[]);
        let (done, total, faces, persons) = counts(&db).unwrap();
        // 썸네일이 없으니 «찾을 수 있는 사진»도 0
        assert_eq!((done, total, faces, persons), (0, 0, 0, 0));
    }
}
