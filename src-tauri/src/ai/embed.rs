//! 임베딩 배치 — 아직 벡터가 없는 사진을 훑어 채운다.
//!
//! 썸네일이 있는 것만 한다(그림은 썸네일에서 만든다). 스캔처럼 청크마다
//! DB에 쓰고 진행을 알린다 — 8만 장 도중에 앱이 죽어도 한 것은 남는다.

use super::clip::{self, Clip};
use super::Result;
use crate::db::conn::Db;
use crate::media::cache;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct EmbedProgress {
    pub total: usize,
    pub done: usize,
    pub failed: usize,
}

struct Job {
    id: i64,
    thumb: PathBuf,
}

/// 할 것 — 벡터가 없고 썸네일은 있는 사진들.
fn jobs(db: &Db, cache_base: &Path) -> Result<Vec<Job>> {
    let rows: Vec<(i64, i64, String)> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.id, fo.library_id, t.rel_path
               FROM files fi
               JOIN folders fo ON fo.id = fi.folder_id
               JOIN thumbs t ON t.file_id = fi.id AND t.state = 1
              WHERE fi.embedding IS NULL AND fi.trashed_at IS NULL AND t.rel_path IS NOT NULL",
        )?;
        let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    Ok(rows
        .into_iter()
        .map(|(id, lib, rel)| Job { id, thumb: cache::cache_root(cache_base, lib).join(rel) })
        .collect())
}

/// 한 번에 모델에 넣는 장수. 크면 빠르지만 메모리를 먹는다.
const BATCH: usize = 16;
/// DB에 쓰는 단위
const CHUNK: usize = 256;

pub fn run(
    db: &Db,
    model: &Path,
    cache_base: &Path,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(&EmbedProgress) + Sync + Send,
) -> Result<EmbedProgress> {
    let list = jobs(db, cache_base)?;
    let progress = Mutex::new(EmbedProgress { total: list.len(), ..Default::default() });
    on_progress(&progress.lock().unwrap().clone());
    if list.is_empty() {
        return Ok(progress.into_inner().unwrap());
    }
    let clip = Clip::load(model)?;
    let mut last = Instant::now();

    for chunk in list.chunks(CHUNK) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        // 전처리는 병렬로 — 그림 디코딩이 모델보다 오래 걸린다
        let pre: Vec<(i64, Option<Vec<f32>>)> = chunk
            .par_iter()
            .map(|j| (j.id, Clip::preprocess(&j.thumb).ok()))
            .collect();

        let mut out: Vec<(i64, Vec<u8>)> = Vec::with_capacity(chunk.len());
        let mut failed = pre.iter().filter(|(_, v)| v.is_none()).count();
        let good: Vec<(i64, Vec<f32>)> = pre.into_iter().filter_map(|(id, v)| v.map(|v| (id, v))).collect();
        for b in good.chunks(BATCH) {
            let inputs: Vec<Vec<f32>> = b.iter().map(|(_, v)| v.clone()).collect();
            match clip.embed(&inputs) {
                Ok(vecs) => {
                    for ((id, _), v) in b.iter().zip(vecs) {
                        out.push((*id, clip::to_blob(&v)));
                    }
                }
                Err(e) => {
                    log::warn!("임베딩 실패 {}장: {e}", b.len());
                    failed += b.len();
                }
            }
            let mut p = progress.lock().unwrap();
            p.done += b.len();
            if last.elapsed() >= Duration::from_millis(100) {
                last = Instant::now();
                on_progress(&p.clone());
            }
        }
        db.transaction(|tx| {
            let mut up = tx.prepare("UPDATE files SET embedding = ?2 WHERE id = ?1")?;
            for (id, blob) in &out {
                up.execute(rusqlite::params![id, blob])?;
            }
            Ok(())
        })?;
        let mut p = progress.lock().unwrap();
        p.failed += failed;
        p.done += failed;
        on_progress(&p.clone());
    }
    let out = progress.into_inner().unwrap();
    Ok(out)
}

/// 몇 장이 됐고 몇 장이 남았나 — 설정 화면이 보여 준다.
pub fn counts(db: &Db) -> Result<(i64, i64)> {
    Ok(db.read(|c| {
        c.query_row(
            // 전체는 «벡터를 만들 수 있는 것» — 썸네일이 없는 파일(못 만든 영상·깨진
            // 사진)은 세지 않는다. 안 그러면 «21장 남음»이 영영 안 사라진다.
            "SELECT COUNT(embedding), COUNT(*) FROM files f
             WHERE trashed_at IS NULL
               AND (embedding IS NOT NULL
                    OR EXISTS (SELECT 1 FROM thumbs t WHERE t.file_id = f.id AND t.state = 1))",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 실제 썸네일 캐시에서 256장 — 전처리(병렬)와 모델(16장씩)의 처리량.
    /// `cargo test --release --lib ai::embed::tests::throughput -- --ignored --nocapture`
    #[test]
    #[ignore = "실제 캐시·모델 필요"]
    fn throughput_on_real_thumbnails() {
        let home = std::env::var("HOME").unwrap();
        let base = PathBuf::from(&home).join("Library/Application Support/com.acut.media");
        // ACUT_CLIP_MODEL=vision_model_fp16.onnx ACUT_CLIP_BATCH=32 로 바꿔 잰다
        let name = std::env::var("ACUT_CLIP_MODEL").unwrap_or_else(|_| "vision_model.onnx".into());
        let batch: usize = std::env::var("ACUT_CLIP_BATCH").ok().and_then(|s| s.parse().ok()).unwrap_or(BATCH);
        let model = base.join("models/clip-vit-b32").join(&name);
        if !model.is_file() {
            eprintln!("모델 없음 — 건너뜀");
            return;
        }
        // thumbs/<lib>/<aa>/<hash>.jpg 에서 256장
        let mut files = Vec::new();
        for lib in std::fs::read_dir(base.join("thumbs")).unwrap().flatten() {
            // .DS_Store 같은 파일이 섞여 있을 수 있다
            let Ok(shards) = std::fs::read_dir(lib.path()) else { continue };
            for shard in shards.flatten() {
                if let Ok(rd) = std::fs::read_dir(shard.path()) {
                    for f in rd.flatten() {
                        if f.path().extension().map(|e| e == "jpg").unwrap_or(false) {
                            files.push(f.path());
                            if files.len() >= 256 {
                                break;
                            }
                        }
                    }
                }
                if files.len() >= 256 {
                    break;
                }
            }
            if files.len() >= 256 {
                break;
            }
        }
        if files.is_empty() {
            eprintln!("썸네일 없음 — 건너뜀");
            return;
        }
        let clip = Clip::load(&model).unwrap();
        let t0 = Instant::now();
        let pre: Vec<Vec<f32>> = files.par_iter().filter_map(|p| Clip::preprocess(p).ok()).collect();
        let t_pre = t0.elapsed();
        let t1 = Instant::now();
        let mut n = 0;
        for b in pre.chunks(batch) {
            n += clip.embed(b).unwrap().len();
        }
        let t_emb = t1.elapsed();
        let per = (t_pre + t_emb).as_secs_f64() * 1000.0 / n as f64;
        eprintln!(
            "\n[{name} · 배치 {batch}] {n}장 · 전처리 {:.0}ms · 모델 {:.0}ms · 장당 {per:.1}ms · 초당 {:.0}장 · 78,857장이면 {:.1}분",
            t_pre.as_millis(), t_emb.as_millis(), 1000.0 / per, 78_857.0 * per / 60_000.0
        );
        assert_eq!(n, pre.len());
    }
}
