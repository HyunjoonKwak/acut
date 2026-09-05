//! 크기만 줄인 사본 — 지각 해시(pHash)로 같은 사진의 리사이즈·재압축본을 묶는다.
//!
//! 완전 중복은 바이트가 같아야 잡는다. 4단계 «그림 해시»(`image_hash`)도 JPEG
//! 코드스트림이 같아야 하므로 «메타데이터만 다른 사본»까지다. **크기를 줄이거나
//! 다시 인코딩한 사본은 픽셀 자체가 달라져 둘 다 놓친다.** 우리집 사진관은 그것을
//! 잡고 있었고, 이 갈래가 그 판정을 옮겨 온 것이다.
//!
//! 판정은 사진관(`backend/app/photos/hashing.py`)을 그대로 따른다: 회색조 →
//! 32×32 로 줄이기 → DCT-II 저주파 8×8 → 중앙값 임계 → 64비트. 해밍 거리 잣대도
//! 같다 — 0~2 같은 사진 · 5 이하 닮음 · 10 이상 다름.
//!
//! 해시는 **썸네일에서** 잰다. 원본을 열지 않으므로 디스크가 빠지거나 NAS 에
//! 있어도 된다. 사진관도 시놀로지 썸네일에서 잰다 — 그래서 두 앱의 값이 견줄 만하다.
//!
//! 전수 비교는 안 한다. 14만 장이면 짝이 100억이다. 두 단으로 줄인다:
//!   1. **같은 해시끼리 먼저 뭉친다.** 단색·검은 화면처럼 해시가 몰리는 자리가
//!      제곱으로 터지는 것을 여기서 막는다 (사진관은 이 단이 없다).
//!   2. 서로 다른 해시만 8비트씩 8밴드로 쪼개, 한 밴드라도 같은 것끼리만 견준다.
//!      문턱이 8보다 작으면 이 버킷질은 **정확하다** — 다른 비트가 8개 미만이면
//!      비둘기집 원리로 성한 밴드가 반드시 하나 남는다.
//!
//! **해시만으로는 부족하다.** 64비트는 «닮은 사진»과 «같은 사진»을 다 잡는다. 그래서
//! 이은 짝마다 **화소를 직접 견준다** — 16×16 밝기와 8×8 색차의 평균 절대 차(MAD).
//! 밝기만 보면 휘도가 같은 서로 다른 색 편집본을 같은 사진으로 오인하므로 색차가 마지막
//! 안전판이다. 밝기 쪽 실측
//! (2026-09-01, 실제 라이브러리):
//!
//! ```text
//!   사본 · 다른 폴더 같은 크기      MAD  0.05
//!   사본 · 1280 ↔ 4000 축소본            0.00
//!   사본 · 2048 ↔ 2592                  1.61
//!   사본 · 3605 ↔ 10218                 3.12
//!   연사 · 대부도 0060 ↔ 0061            4.41
//!   연사 · 20140423 185430 ↔ 185431      5.63
//!   연사 · 대부도 0059 ↔ 0060            9.24
//!   같은 컷 JPEG ↔ RAW                  13.05
//! ```
//!
//! CLIP 벡터로는 못 가른다 — 이미 있는 것이라 먼저 재 봤지만, 크게 줄인 사본은
//! 썸네일이 달라져 코사인이 0.7까지 떨어졌다(진짜 사본의 87%가 0.98 미만). 벡터는
//! 뜻을 보고 MAD 는 그림을 본다. 여기 필요한 것은 그림 쪽이다.
//!
//! 묶음은 **씨앗 기준**이다 — 화소가 가장 많은 것을 씨앗으로, 그와 **직접** 닮은 것만
//! 넣는다. 사슬로 이으면 삼각대에 두고 찍은 이웃 컷이 통째로 한 무리가 된다(실측:
//! 하와이 81장). 가로세로 비도 씨앗과 견준다 — 비가 다르면 잘라 낸 사진이다.
//!
//! 마지막으로 **연사를 걸러 낸다.** 64비트 해시는 «닮은 사진»과 «같은 사진»을 다 잡는다 —
//!    실측(2026-09-01, 14.3만 장)에서 가장 큰 무리가 `IMG_0040.CR2`~`IMG_0059.CR2`,
//!    연달아 찍은 서로 다른 RAW 11장이었다. 표본 넷이 모두 파일명이 연번이고 촬영
//!    시각이 같은 초였다. **한 폴더 안에 있고 해상도까지 같으면 연사로 보고 버린다** —
//!    사본은 다른 자리에 생기거나(다른 폴더·다른 라이브러리) 크기가 달라진다.
//!    연사는 «같은 순간»(kind 2)이 시계로 판정하는 것이 제 일이다.

use crate::db::conn::{Db, Result};
use crate::media::cache;
use image::imageops::FilterType;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

pub const KIND: i32 = 4;

/// 해밍 거리 문턱. 사진관의 잣대는 0~2 «같은 사진» · 5 이하 «닮음» 이다. 우리는
/// «같은 사진의 사본»만 찾으므로 그 사이인 4 를 기본으로 둔다 — 다시 인코딩하면
/// 한두 비트가 흔들리고, 크게 줄이면 그보다 더 흔들린다.
pub const DEFAULT_THRESHOLD: u32 = 4;

/// 64비트를 8비트씩. 문턱이 8보다 작으면 버킷질에 놓치는 짝이 없다(비둘기집).
const BANDS: u32 = 8;
/// DCT 입력 한 변
const N: usize = 32;
/// 저주파 블록 한 변 → 64비트
const LOW: usize = 8;
const MIN_GROUP: usize = 2;
/// 가로세로 비 허용 오차(비율). 줄이기는 비를 지키지만 반올림으로 조금 어긋난다.
const AR_TOLERANCE: f64 = 0.02;
/// DB 에 쓰는 단위
const CHUNK: usize = 512;
/// 밝기 서명 한 변 — 16×16 256바이트.
const SIG: usize = 16;
/// 색차 서명 한 변 — Cb·Cr 8×8씩 128바이트. 밝기와 합쳐 14.3만 장에 약 55MB.
const CHROMA_SIG: usize = 8;
const SIGNATURE_VERSION: u8 = 1;
const LUMA_BYTES: usize = SIG * SIG;
const CHROMA_BYTES: usize = CHROMA_SIG * CHROMA_SIG * 2;
const SIG_BYTES: usize = 1 + LUMA_BYTES + CHROMA_BYTES;
/// 같은 그림으로 볼 평균 절대 차의 상한. 위 실측에서 사본은 3.1 아래, 연사는 4.4 위였다.
/// 3.5 는 그 사이다 — 넉넉히 잡으면 연사가 섞이고, 좁히면 크게 줄인 사본을 놓친다.
const MAX_MAD: f64 = 3.5;
/// 재압축·축소 때 색차가 밝기보다 조금 더 흔들리는 것을 허용하되 색 편집본은 막는다.
const MAX_CHROMA_MAD: f64 = 8.0;

/// `cos[u][x] = cos(pi * (2x+1) * u / 2N)` — 우리가 쓰는 8개 계수만.
static COS: LazyLock<[[f64; N]; LOW]> = LazyLock::new(|| {
    let mut t = [[0f64; N]; LOW];
    for (u, row) in t.iter_mut().enumerate() {
        for (x, c) in row.iter_mut().enumerate() {
            *c =
                (std::f64::consts::PI * (2.0 * x as f64 + 1.0) * u as f64 / (2.0 * N as f64)).cos();
        }
    }
    t
});

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct PhashProgress {
    /// 어느 단계인가 — "fill"(해시 재기) 또는 "group"(묶기)
    pub phase: &'static str,
    /// 해시를 채워야 하는 사진 수
    pub fill_total: usize,
    pub fill_done: usize,
    /// 썸네일을 못 읽은 수
    pub fill_failed: usize,
    /// 해시가 있어 묶기 대상이 된 사진 수
    pub photos: usize,
    /// 서로 다른 해시 값의 수 — 같은 값끼리는 먼저 뭉친다
    pub distinct: usize,
    /// 실제로 견준 짝의 수
    pub compared: usize,
    pub groups: usize,
    pub members: usize,
    pub reclaimable: i64,
    /// 연사로 보고 버린 무리 — «같은 순간»이 맡는다
    pub bursts: usize,
}

/// 그림 한 장의 64비트 지각 해시.
///
/// 사진관 구현을 그대로 옮겼으므로 **DC 계수(dct[0][0])도 중앙값에 넣는다.** 값이
/// 가장 커서 그 비트는 늘 1 이 되지만, 중앙값은 순위로 정해지므로 판정이 흔들리지
/// 않는다. 두 앱의 해시를 견줄 수 있어야 해서 일부러 같게 뒀다.
pub fn phash_of(path: &Path) -> Option<u64> {
    signature_of(path).map(|(h, _)| h)
}

/// 해시와 화소 서명을 한 번에. 그림을 두 번 열지 않기 위해서다 —
/// 서명은 32×32 를 2×2 씩 뭉쳐 16×16 으로 줄인 것이다.
pub fn signature_of(path: &Path) -> Option<(u64, Vec<u8>)> {
    let img = image::open(path).ok()?;
    // 회색조로 바꾼 **뒤** 줄인다 — 사진관(PIL)의 차례와 같게
    let gray = image::imageops::resize(&img.to_luma8(), N as u32, N as u32, FilterType::Lanczos3);
    let px = gray.as_raw();
    let mut sig = Vec::with_capacity(SIG_BYTES);
    sig.push(SIGNATURE_VERSION);
    sig.extend(shrink_to_signature(px));

    // pHash 후보는 사진관과 맞추기 위해 회색조 그대로 두되, 최종 확인에는 색을 남긴다.
    // RGB 자체보다 Cb·Cr가 밝기 변화와 색 변화를 갈라 주므로 작은 8×8이면 충분하다.
    let rgb = image::imageops::resize(
        &img.to_rgb8(),
        CHROMA_SIG as u32,
        CHROMA_SIG as u32,
        FilterType::Lanczos3,
    );
    for p in rgb.pixels() {
        let [r, g, b] = p.0.map(f64::from);
        let cb = 128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b;
        let cr = 128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b;
        sig.push(cb.round().clamp(0.0, 255.0) as u8);
        sig.push(cr.round().clamp(0.0, 255.0) as u8);
    }
    debug_assert_eq!(sig.len(), SIG_BYTES);
    Some((phash_of_gray(px), sig))
}

/// 32×32 → 16×16. 2×2 네 칸의 평균.
fn shrink_to_signature(px: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; SIG * SIG];
    for y in 0..SIG {
        for x in 0..SIG {
            let s: u32 = (0..2)
                .flat_map(|dy| (0..2).map(move |dx| (dy, dx)))
                .map(|(dy, dx)| px[(y * 2 + dy) * N + (x * 2 + dx)] as u32)
                .sum();
            out[y * SIG + x] = (s / 4) as u8;
        }
    }
    out
}

/// 두 서명의 평균 절대 차 — 같은 그림이면 0 에 가깝다.
fn mad(a: &[u8], b: &[u8]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return f64::MAX;
    }
    let sum: u32 = a.iter().zip(b).map(|(&x, &y)| x.abs_diff(y) as u32).sum();
    sum as f64 / a.len() as f64
}

/// 버전이 맞는 밝기·색차 서명인가. 옛 256바이트 회색조 서명은 여기서 거절하며
/// `jobs`가 다시 계산한다.
fn signatures_alike(a: &[u8], b: &[u8]) -> bool {
    if a.len() != SIG_BYTES
        || b.len() != SIG_BYTES
        || a[0] != SIGNATURE_VERSION
        || b[0] != SIGNATURE_VERSION
    {
        return false;
    }
    let luma = 1..1 + LUMA_BYTES;
    let chroma = 1 + LUMA_BYTES..SIG_BYTES;
    mad(&a[luma.clone()], &b[luma]) <= MAX_MAD
        && mad(&a[chroma.clone()], &b[chroma]) <= MAX_CHROMA_MAD
}

/// 32×32 회색조 화소(행 우선)에서 해시를 낸다 — 시험이 그림 없이 부를 수 있게 갈랐다.
fn phash_of_gray(px: &[u8]) -> u64 {
    debug_assert_eq!(px.len(), N * N);
    // 가로 방향 DCT — 행마다 저주파 8개만
    let mut rows = [[0f64; LOW]; N];
    for (y, row) in rows.iter_mut().enumerate() {
        for (u, out) in row.iter_mut().enumerate() {
            let mut s = 0f64;
            for x in 0..N {
                s += px[y * N + x] as f64 * COS[u][x];
            }
            *out = s;
        }
    }
    // 세로 방향 DCT — 저주파 블록 8×8
    let mut values = [0f64; LOW * LOW];
    for v in 0..LOW {
        for u in 0..LOW {
            let mut s = 0f64;
            for (y, row) in rows.iter().enumerate() {
                s += row[u] * COS[v][y];
            }
            values[v * LOW + u] = s;
        }
    }
    let mut sorted = values;
    sorted.sort_by(f64::total_cmp);
    // 짝수 개라 가운데 둘의 평균 — 파이썬 statistics.median 과 같게
    let mid = (sorted[LOW * LOW / 2 - 1] + sorted[LOW * LOW / 2]) / 2.0;
    let mut bits = 0u64;
    for v in values {
        bits = (bits << 1) | u64::from(v > mid);
    }
    bits
}

pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

struct Job {
    id: i64,
    thumb: PathBuf,
}

/// 한 장을 잰 결과 — 파일 id 와 (해시, 화소 서명). 썸네일을 못 읽으면 `None`.
type Signed = (i64, Option<(u64, Vec<u8>)>);

/// 할 것 — 해시가 없고 썸네일은 있는 사진들. 영상(kind 1)은 뺀다.
fn jobs(db: &Db, cache_base: &Path) -> Result<Vec<Job>> {
    let rows: Vec<(i64, i64, String)> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.id, fo.library_id, t.rel_path
               FROM files fi
               JOIN folders fo ON fo.id = fi.folder_id
               JOIN thumbs t ON t.file_id = fi.id AND t.state = 1
              WHERE (fi.phash IS NULL OR fi.psig IS NULL
                     OR length(fi.psig) <> ?1 OR substr(fi.psig,1,1) <> X'01')
                AND fi.kind <> 1
                AND fi.trashed_at IS NULL
                AND t.src_size = fi.size
                AND t.src_mtime = COALESCE(fi.modified_at, 0)
                AND t.rel_path IS NOT NULL",
        )?;
        let it = st.query_map([SIG_BYTES as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
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

/// 빠진 해시를 채운다. 청크마다 DB 에 쓴다 — 도중에 멈춰도 한 것은 남는다.
pub fn fill(
    db: &Db,
    cache_base: &Path,
    cancel: &AtomicBool,
    on_progress: &(impl Fn(&PhashProgress) + Sync + Send),
    progress: &Mutex<PhashProgress>,
) -> Result<()> {
    let list = jobs(db, cache_base)?;
    {
        let mut p = progress.lock().unwrap();
        p.phase = "fill";
        p.fill_total = list.len();
        on_progress(&p.clone());
    }
    for chunk in list.chunks(CHUNK) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let out: Vec<Signed> = chunk
            .par_iter()
            .map(|j| (j.id, signature_of(&j.thumb)))
            .collect();
        db.transaction(|tx| {
            let mut up = tx.prepare("UPDATE files SET phash = ?2, psig = ?3 WHERE id = ?1")?;
            for (id, r) in &out {
                // 못 읽은 것은 비워 둔다 — 썸네일이 다시 만들어지면 그때 잰다
                if let Some((h, sig)) = r {
                    up.execute(rusqlite::params![id, *h as i64, sig])?;
                }
            }
            Ok(())
        })?;
        let mut p = progress.lock().unwrap();
        p.fill_failed += out.iter().filter(|(_, h)| h.is_none()).count();
        p.fill_done += out.len();
        on_progress(&p.clone());
    }
    Ok(())
}

struct Row {
    id: i64,
    folder_id: i64,
    hash: u64,
    size: i64,
    width: i64,
    height: i64,
    sharpness: Option<f64>,
    /// 폴더의 영역 — 내사진·공용에 있는 것이 대표가 된다
    area: i32,
    /// 버전 + 16×16 밝기 + 8×8 CbCr — 정말 같은 그림인지 견주는 데 쓴다
    sig: Vec<u8>,
}

impl Row {
    fn ar(&self) -> f64 {
        self.width as f64 / self.height as f64
    }
    fn pixels(&self) -> i64 {
        self.width * self.height
    }
}

/// 완전 중복(kind 0)이 이미 «뺄 것»으로 표시한 사본들. 여기서는 빼고 본다 —
/// 바이트가 같은 사본은 저기서 처리하고, 이 탭은 «줄인 사본»만 보여 준다.
fn already_excluded(db: &Db) -> Result<HashSet<i64>> {
    db.read(|c| {
        let mut st = c.prepare(
            "SELECT m.file_id FROM group_members m JOIN groups g ON g.id = m.group_id
             WHERE g.kind = 0 AND g.state = 0 AND m.is_best = 0",
        )?;
        let it = st.query_map([], |r| r.get::<_, i64>(0))?;
        it.collect::<rusqlite::Result<HashSet<_>>>()
    })
}

fn load(db: &Db, skip: &HashSet<i64>) -> Result<Vec<Row>> {
    let rows: Vec<Row> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.id, fi.folder_id, fi.phash, fi.size, fi.width, fi.height, fi.sharpness,
                    fo.area, fi.psig
               FROM files fi JOIN folders fo ON fo.id = fi.folder_id
              WHERE fi.phash IS NOT NULL AND fi.psig IS NOT NULL AND fi.kind <> 1
                AND fi.trashed_at IS NULL
                AND length(fi.psig) = ?1 AND substr(fi.psig,1,1) = X'01'
                AND fi.width > 0 AND fi.height > 0
              ORDER BY fi.id",
        )?;
        let it = st.query_map([SIG_BYTES as i64], |r| {
            Ok(Row {
                id: r.get(0)?,
                folder_id: r.get(1)?,
                hash: r.get::<_, i64>(2)? as u64,
                size: r.get(3)?,
                width: r.get(4)?,
                height: r.get(5)?,
                sharpness: r.get(6)?,
                area: r.get(7)?,
                sig: r.get(8)?,
            })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    Ok(rows.into_iter().filter(|r| !skip.contains(&r.id)).collect())
}

struct UnionFind(Vec<usize>);

impl UnionFind {
    fn new(n: usize) -> Self {
        Self((0..n).collect())
    }
    fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra] = rb;
        }
    }
}

/// 연사인가 — 한 폴더 안에 있고 해상도까지 같으면. 사본은 다른 자리에 생기거나
/// 크기가 달라지므로, 둘 다 아닌 것은 «같은 장면을 여러 번 찍은 것»으로 본다.
fn looks_like_a_burst(rows: &[Row], m: &[usize]) -> bool {
    let first = &rows[m[0]];
    m.iter().all(|&i| {
        rows[i].folder_id == first.folder_id
            && rows[i].width == first.width
            && rows[i].height == first.height
    })
}

/// 가로세로 비가 같은가 — 줄이기는 비를 지킨다. 반올림 오차만 봐 준다.
fn same_aspect(a: &Row, b: &Row) -> bool {
    (a.ar() - b.ar()).abs() <= AR_TOLERANCE * a.ar()
}

/// 한 덩어리를 **씨앗 기준**으로 가른다 — 사슬로 잇지 않는다.
///
/// A~B, B~C 라고 A 와 C 를 한 무리에 두면 연달아 찍은 컷이 통째로 묶인다. 실측
/// (2026-09-01): 하와이 `__0197`~`__0236` 의 CR2+JPG 81장이 한 무리가 됐다 —
/// 삼각대에 두고 찍은 이웃 컷들이 서로서로 문턱 안이었기 때문이다. 「비슷한 장면」이
/// 같은 함정을 같은 방법으로 피한다.
///
/// 씨앗은 **화소가 가장 많은 것**. 남길 것이 씨앗이 되어야 «씨앗과 닮았나»가 곧
/// «이것을 빼도 되나»가 된다.
fn cluster_around_seeds(rows: &[Row], mut idxs: Vec<usize>) -> Vec<Vec<usize>> {
    idxs.sort_by(|&a, &b| rows[b].pixels().cmp(&rows[a].pixels()));
    let mut used = vec![false; idxs.len()];
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut i = 0usize;
    while i < idxs.len() {
        if used[i] {
            i += 1;
            continue;
        }
        used[i] = true;
        let seed = idxs[i];
        let mut m = vec![seed];
        for j in (i + 1)..idxs.len() {
            if used[j] {
                continue;
            }
            let c = idxs[j];
            if same_aspect(&rows[seed], &rows[c]) && signatures_alike(&rows[seed].sig, &rows[c].sig)
            {
                used[j] = true;
                m.push(c);
            }
        }
        if m.len() >= MIN_GROUP {
            out.push(m);
        }
        i += 1;
    }
    out
}

fn clear_groups(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM group_members WHERE group_id IN (SELECT id FROM groups WHERE kind = ?1)",
        [KIND],
    )?;
    tx.execute("DELETE FROM groups WHERE kind = ?1", [KIND])?;
    Ok(())
}

/// 해시를 채우고 묶는다. 결과는 `groups` (kind 4).
pub fn scan(
    db: &Db,
    cache_base: &Path,
    threshold: u32,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(&PhashProgress) + Sync + Send,
) -> Result<PhashProgress> {
    let progress = Mutex::new(PhashProgress {
        phase: "fill",
        ..Default::default()
    });
    fill(db, cache_base, &cancel, &on_progress, &progress)?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(progress.into_inner().unwrap());
    }

    let skip = already_excluded(db)?;
    let rows = load(db, &skip)?;
    {
        let mut p = progress.lock().unwrap();
        p.phase = "group";
        p.photos = rows.len();
        on_progress(&p.clone());
    }
    if rows.len() < MIN_GROUP {
        // 대상이 사라진 것도 새 결과다. 여기서 옛 그룹을 두면 휴지통 이동·삭제 뒤
        // «다시 찾기»가 끝나도 유령 그룹이 남는다.
        db.transaction(clear_groups)?;
        return Ok(progress.into_inner().unwrap());
    }

    // 1단: 같은 해시끼리 뭉친다
    let mut by_hash: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, r) in rows.iter().enumerate() {
        by_hash.entry(r.hash).or_default().push(i);
    }
    let hashes: Vec<u64> = by_hash.keys().copied().collect();
    let members_of: Vec<&Vec<usize>> = hashes.iter().map(|h| &by_hash[h]).collect();
    {
        let mut p = progress.lock().unwrap();
        p.distinct = hashes.len();
        on_progress(&p.clone());
    }

    // 2단: 밴드 버킷 — 한 밴드라도 같은 해시끼리만 견준다
    let mut buckets: HashMap<(u32, u8), Vec<usize>> = HashMap::new();
    for (i, &h) in hashes.iter().enumerate() {
        for band in 0..BANDS {
            buckets
                .entry((band, (h >> (8 * band)) as u8))
                .or_default()
                .push(i);
        }
    }
    let lists: Vec<&Vec<usize>> = buckets.values().filter(|v| v.len() >= 2).collect();
    let compared = std::sync::atomic::AtomicUsize::new(0);
    let links: Vec<(usize, usize)> = lists
        .par_iter()
        .flat_map(|idxs| {
            if cancel.load(Ordering::Relaxed) {
                return Vec::new();
            }
            let mut out = Vec::new();
            let mut n = 0usize;
            for a in 0..idxs.len() {
                for b in (a + 1)..idxs.len() {
                    let (i, j) = (idxs[a], idxs[b]);
                    n += 1;
                    if hamming(hashes[i], hashes[j]) <= threshold {
                        out.push((i, j));
                    }
                }
            }
            compared.fetch_add(n, Ordering::Relaxed);
            out
        })
        .collect();
    {
        let mut p = progress.lock().unwrap();
        p.compared = compared.load(Ordering::Relaxed);
        on_progress(&p.clone());
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(progress.into_inner().unwrap());
    }

    // 잇기는 **사진 단위**로 한다. 해시는 후보를 좁힐 뿐이고, 실제로 이을지는
    // 화소 서명(MAD)이 정한다 — 이것이 없으면 연사 프레임이 사본으로 묶인다
    // (실측: 하와이 106장, 대부도 0059~0065).
    let mut uf = UnionFind::new(rows.len());
    let alike = |a: usize, b: usize| signatures_alike(&rows[a].sig, &rows[b].sig);
    // 해시가 같아도 그림까지 같아야 잇는다. 한 자리에 여럿이면 앞선 대표들과만
    // 견준다 — 짝을 다 보면 제곱이 되고, 같은 그림끼리는 어차피 한 대표로 모인다.
    for idxs in by_hash.values() {
        let mut reps: Vec<usize> = Vec::new();
        for &i in idxs {
            match reps.iter().find(|&&r| alike(i, r)) {
                Some(&r) => uf.union(i, r),
                None => reps.push(i),
            }
        }
    }
    for (a, b) in links {
        for &ra in members_of[a] {
            for &rb in members_of[b] {
                if alike(ra, rb) {
                    uf.union(ra, rb);
                }
            }
        }
    }
    let mut comps: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..rows.len() {
        let root = uf.find(i);
        comps.entry(root).or_default().push(i);
    }

    // 3단: 가로세로 비로 가르고, 둘 이상 남은 것만 무리로
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut bursts = 0usize;
    for (_, idxs) in comps {
        if idxs.len() < MIN_GROUP {
            continue;
        }
        for g in cluster_around_seeds(&rows, idxs) {
            if g.len() < MIN_GROUP {
                continue;
            }
            if looks_like_a_burst(&rows, &g) {
                bursts += 1;
            } else {
                groups.push(g);
            }
        }
    }

    let mut reclaimable = 0i64;
    let mut n_members = 0usize;
    db.transaction(|tx| {
        clear_groups(tx)?;
        let mut ins_g = tx.prepare(
            "INSERT INTO groups(kind, reason, size_bytes, state, created_at)
             VALUES(?1, ?2, ?3, 0, strftime('%s','now'))",
        )?;
        let mut ins_m = tx.prepare(
            "INSERT INTO group_members(group_id, file_id, is_best, score) VALUES(?1,?2,?3,?4)",
        )?;
        for m in &groups {
            // **가장 큰 것을 남긴다** — 이 갈래의 뜻이 «줄인 사본을 뺀다» 이므로
            // 화소 수가 먼저다. 같으면 정리된 자리(내사진·공용), 그다음 큰 파일.
            let settled = |r: &Row| i32::from(r.area == 1 || r.area == 2);
            let best = *m
                .iter()
                .max_by(|&&a, &&b| {
                    let (ra, rb) = (&rows[a], &rows[b]);
                    ra.pixels()
                        .cmp(&rb.pixels())
                        .then(settled(ra).cmp(&settled(rb)))
                        .then(ra.size.cmp(&rb.size))
                })
                .unwrap();
            let total: i64 = m.iter().map(|&i| rows[i].size).sum();
            let saved = total - rows[best].size;
            reclaimable += saved;
            n_members += m.len();
            let (bw, bh) = (rows[best].width, rows[best].height);
            let smaller = m
                .iter()
                .filter(|&&i| rows[i].pixels() < rows[best].pixels())
                .count();
            let reason = if smaller > 0 {
                format!("{bw}×{bh} · 줄인 사본 {smaller}장")
            } else {
                format!("{bw}×{bh} · 다시 저장한 사본 {}장", m.len() - 1)
            };
            ins_g.execute(rusqlite::params![KIND, reason, saved])?;
            let gid = tx.last_insert_rowid();
            for &i in m {
                ins_m.execute(rusqlite::params![
                    gid,
                    rows[i].id,
                    i == best,
                    rows[i].sharpness
                ])?;
            }
        }
        Ok(())
    })?;

    let mut p = progress.into_inner().unwrap();
    p.bursts = bursts;
    p.groups = groups.len();
    p.members = n_members;
    p.reclaimable = reclaimable;
    on_progress(&p);
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 32×32 회색조 — 사진관 쪽 시험과 **같은 식**으로 만든다.
    fn pixels() -> Vec<u8> {
        (0..32)
            .flat_map(|y| (0..32).map(move |x| (((x * 7 + y * 13 + (x * y) % 11) * 3) % 256) as u8))
            .collect()
    }

    /// 우리집 사진관이 같은 화소에 내놓는 값. 리사이즈는 라이브러리마다 다르므로
    /// **줄이기를 뺀 나머지**(DCT·중앙값·비트)가 같은지를 못 박는다. 이 값이 어긋나면
    /// 두 앱이 같은 사진을 다르게 보게 된다.
    ///
    /// 얻은 법: photo_gallery/backend 에서
    /// `Image.frombytes("L",(32,32),data)` → `app.photos.hashing.phash_hex`.
    #[test]
    fn matches_the_gallery_on_the_same_pixels() {
        assert_eq!(phash_of_gray(&pixels()), 0xad42_4c63_bd93_9d23);
    }

    fn save(path: &Path, w: u32, h: u32) {
        // 결이 있는 그림 — 밋밋하면 해시가 다 같아져 시험이 뜻을 잃는다
        let img = image::RgbImage::from_fn(w, h, |x, y| {
            let fx = x as f32 / w as f32;
            let fy = y as f32 / h as f32;
            let v = ((fx * 6.0).sin() * 90.0 + (fy * 9.0).cos() * 70.0 + 128.0) as u8;
            let s = if (x * 5 / w) % 2 == (y * 5 / h) % 2 {
                40
            } else {
                0
            };
            image::Rgb([v.saturating_add(s), v, v.saturating_sub(s)])
        });
        img.save(path).unwrap();
    }

    #[test]
    fn a_shrunk_copy_stays_within_the_threshold() {
        let d = tempfile::tempdir().unwrap();
        let (big, small) = (d.path().join("big.png"), d.path().join("small.png"));
        save(&big, 800, 600);
        save(&small, 200, 150); // 같은 그림을 1/4 로
        let (a, sa) = signature_of(&big).unwrap();
        let (b, sb) = signature_of(&small).unwrap();
        assert!(
            hamming(a, b) <= DEFAULT_THRESHOLD,
            "줄인 사본인데 {}비트나 달랐다 ({a:016x} vs {b:016x})",
            hamming(a, b)
        );
        assert!(
            signatures_alike(&sa, &sb),
            "줄인 사본의 색차 안전판이 너무 좁다"
        );
    }

    #[test]
    fn equal_luminance_but_different_colours_are_not_the_same_picture() {
        let luma = |rgb: image::Rgb<u8>| {
            image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(1, 1, rgb))
                .to_luma8()
                .get_pixel(0, 0)[0]
        };
        let red = image::Rgb([255, 0, 0]);
        let target = luma(red);
        // image 크레이트가 정확히 같은 밝기로 바꾸는 초록색을 고른다. 밝기 서명만
        // 있었다면 두 평면은 MAD 0으로 반드시 통과했다.
        let green = (0..=255)
            .map(|g| image::Rgb([0, g, 0]))
            .find(|&rgb| luma(rgb) == target)
            .expect("붉은색과 같은 밝기의 초록색");
        assert_ne!(red, green);

        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("red.png"), d.path().join("green.png"));
        image::RgbImage::from_pixel(64, 64, red).save(&a).unwrap();
        image::RgbImage::from_pixel(64, 64, green).save(&b).unwrap();
        let (_, sa) = signature_of(&a).unwrap();
        let (_, sb) = signature_of(&b).unwrap();
        assert_eq!(mad(&sa[1..1 + LUMA_BYTES], &sb[1..1 + LUMA_BYTES]), 0.0);
        assert!(
            !signatures_alike(&sa, &sb),
            "색차가 큰 편집본을 같은 사진으로 봤다"
        );
    }

    #[test]
    fn a_different_picture_is_far_away() {
        let d = tempfile::tempdir().unwrap();
        let (a_p, b_p) = (d.path().join("a.png"), d.path().join("b.png"));
        save(&a_p, 400, 300);
        image::RgbImage::from_fn(400, 300, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 30])
        })
        .save(&b_p)
        .unwrap();
        let (a, b) = (phash_of(&a_p).unwrap(), phash_of(&b_p).unwrap());
        assert!(
            hamming(a, b) > DEFAULT_THRESHOLD,
            "다른 그림인데 {}비트만 달랐다",
            hamming(a, b)
        );
    }

    #[test]
    fn an_unreadable_file_is_skipped_not_fatal() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("nope.png");
        std::fs::write(&p, b"not an image").unwrap();
        assert_eq!(phash_of(&p), None);
    }

    // ── DB ────────────────────────────────────────────────────────────────

    fn db() -> (tempfile::TempDir, Db) {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(d.path().join("t.db")).unwrap();
        (d, db)
    }

    /// (id, phash, size, width, height, area, pic)
    /// `pic` 는 **어느 그림인가** — 같으면 화소 서명이 같아 MAD 0, 다르면 40 이상 벌어진다.
    /// 해시가 이어도 그림이 다르면 안 묶인다는 것을 시험이 말할 수 있게 하려고 둔다.
    type SeedItem = (i64, u64, i64, i64, i64, i32, u8);

    /// 그림 하나를 나타내는 서명 — 밝기만 다른 평면. 다른 `pic` 끼리는 MAD 가
    /// 40 이상 벌어져 문턱(3.5)을 훌쩍 넘는다.
    fn flat_sig(value: u8) -> Vec<u8> {
        let mut sig = vec![value; SIG_BYTES];
        sig[0] = SIGNATURE_VERSION;
        sig
    }

    fn sig_of(pic: u8) -> Vec<u8> {
        flat_sig(pic.saturating_mul(40))
    }

    fn seed(db: &Db, items: &[SeedItem]) {
        db.transaction(|tx| {
            tx.execute(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library')",
                [],
            )?;
            for area in [0, 1, 2] {
                tx.execute(
                    "INSERT INTO folders(id,volume_uuid,rel_path,name,area)
                     VALUES(?1,'V',?2,?2,?1)",
                    rusqlite::params![area, format!("f{area}")],
                )?;
            }
            for (id, hash, size, w, h, area, pic) in items {
                tx.execute(
                    "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,
                        scanned_at,width,height,phash,psig)
                     VALUES(?1,?2,?3,?4,0,1000,0,0,?5,?6,?7,?8)",
                    rusqlite::params![
                        id,
                        area,
                        format!("f{id}.jpg"),
                        size,
                        w,
                        h,
                        *hash as i64,
                        sig_of(*pic)
                    ],
                )?;
            }
            Ok(())
        })
        .unwrap();
    }

    fn run(db: &Db) -> PhashProgress {
        let d = tempfile::tempdir().unwrap();
        scan(
            db,
            d.path(),
            DEFAULT_THRESHOLD,
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
        .unwrap()
    }

    fn members_of(db: &Db) -> Vec<(i64, bool)> {
        db.read(|c| {
            let mut st = c.prepare(
                "SELECT m.file_id, m.is_best FROM group_members m
                 JOIN groups g ON g.id = m.group_id WHERE g.kind = 4 ORDER BY m.file_id",
            )?;
            let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            it.collect()
        })
        .unwrap()
    }

    /// 한 비트 다른 해시 — 다시 인코딩하면 이만큼 흔들린다
    const H: u64 = 0x0f0f_0f0f_0f0f_0f0f;

    #[test]
    fn groups_a_shrunk_copy_and_keeps_the_bigger_one() {
        let (_d, db) = db();
        seed(
            &db,
            &[
                (1, H, 500, 400, 300, 0, 1),        // 줄인 사본
                (2, H ^ 1, 4000, 1600, 1200, 0, 1), // 원본
            ],
        );
        let p = run(&db);
        assert_eq!((p.groups, p.members), (1, 2));
        assert_eq!(
            members_of(&db),
            vec![(1, false), (2, true)],
            "큰 것이 대표여야 한다"
        );
        assert_eq!(p.reclaimable, 500);
    }

    #[test]
    fn does_not_group_pictures_that_are_far_apart() {
        let (_d, db) = db();
        seed(
            &db,
            &[
                (1, 0, 100, 400, 300, 0, 1),
                (2, u64::MAX, 100, 400, 300, 0, 2),
            ],
        );
        assert_eq!(run(&db).groups, 0);
    }

    /// 가로세로 비가 다르면 «줄인 사본»이 아니라 잘라 낸 사진이다
    #[test]
    fn splits_a_crop_with_a_different_aspect_ratio() {
        let (_d, db) = db();
        seed(
            &db,
            &[
                (1, H, 500, 400, 300, 0, 1),        // 4:3
                (2, H ^ 1, 4000, 1600, 1200, 0, 1), // 4:3 — 1과 한 무리
                (3, H ^ 2, 900, 400, 400, 0, 1),    // 1:1 — 갈라져 나가 혼자 남는다
            ],
        );
        let p = run(&db);
        assert_eq!((p.groups, p.members), (1, 2));
        assert_eq!(
            members_of(&db).iter().map(|m| m.0).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    /// 완전 중복이 이미 «뺄 것»으로 표시한 사본은 여기서 또 보여 주지 않는다
    #[test]
    fn leaves_out_copies_the_exact_duplicate_pass_already_took() {
        let (_d, db) = db();
        seed(
            &db,
            &[
                (1, H, 500, 400, 300, 0, 1),
                (2, H, 500, 400, 300, 0, 1),
                (3, H ^ 1, 4000, 1600, 1200, 0, 1),
            ],
        );
        db.transaction(|tx| {
            tx.execute(
                "INSERT INTO groups(id,kind,reason,size_bytes,state,created_at)
                 VALUES(9,0,'같음',500,0,0)",
                [],
            )?;
            // 1이 대표, 2는 뺄 것 — 2는 이 갈래에서 빠져야 한다
            tx.execute(
                "INSERT INTO group_members(group_id,file_id,is_best) VALUES(9,1,1)",
                [],
            )?;
            tx.execute(
                "INSERT INTO group_members(group_id,file_id,is_best) VALUES(9,2,0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let p = run(&db);
        assert_eq!((p.groups, p.members), (1, 2));
        assert_eq!(
            members_of(&db).iter().map(|m| m.0).collect::<Vec<_>>(),
            vec![1, 3]
        );
    }

    /// 화소 수가 같으면 정리된 자리(내사진·공용)에 있는 것이 대표가 된다
    #[test]
    fn prefers_the_settled_copy_when_the_size_is_the_same() {
        let (_d, db) = db();
        seed(
            &db,
            &[(1, H, 100, 400, 300, 0, 1), (2, H, 100, 400, 300, 2, 1)],
        );
        run(&db);
        assert_eq!(members_of(&db), vec![(1, false), (2, true)]);
    }

    #[test]
    fn rerunning_replaces_old_groups() {
        let (_d, db) = db();
        seed(
            &db,
            &[
                (1, H, 500, 400, 300, 0, 1),
                (2, H ^ 1, 4000, 1600, 1200, 0, 1),
            ],
        );
        run(&db);
        let p = run(&db);
        assert_eq!(p.groups, 1);
        assert_eq!(members_of(&db).len(), 2);
    }

    #[test]
    fn rerunning_with_fewer_than_two_photos_clears_old_groups() {
        let (_d, db) = db();
        seed(
            &db,
            &[
                (1, H, 500, 400, 300, 0, 1),
                (2, H ^ 1, 4000, 1600, 1200, 0, 1),
            ],
        );
        assert_eq!(run(&db).groups, 1);
        db.write(|c| c.execute("UPDATE files SET trashed_at=1 WHERE id=2", []))
            .unwrap();
        let p = run(&db);
        assert_eq!((p.groups, p.members), (0, 0));
        assert!(
            members_of(&db).is_empty(),
            "대상이 한 장뿐인데 이전 그룹이 남았다"
        );
    }

    #[test]
    fn an_old_grayscale_signature_is_scheduled_for_recalculation() {
        let (d, db) = db();
        seed(&db, &[(1, H, 500, 400, 300, 0, 1)]);
        db.transaction(|tx| {
            tx.execute(
                "INSERT INTO libraries(id,volume_uuid,rel_path,name,area) VALUES(9,'V','','t',0)",
                [],
            )?;
            tx.execute("UPDATE folders SET library_id=9", [])?;
            tx.execute(
                "UPDATE files SET psig=?1 WHERE id=1",
                [vec![42u8; SIG * SIG]],
            )?;
            tx.execute(
                "INSERT INTO thumbs(file_id,rel_path,src_size,src_mtime,state)
                 VALUES(1,'old.jpg',500,0,1)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        assert_eq!(jobs(&db, d.path()).unwrap().len(), 1);
    }

    #[test]
    fn a_stale_thumbnail_is_never_used_to_recalculate_the_signature() {
        let (d, db) = db();
        seed(&db, &[(1, H, 500, 400, 300, 0, 1)]);
        db.transaction(|tx| {
            tx.execute(
                "INSERT INTO libraries(id,volume_uuid,rel_path,name,area) VALUES(9,'V','','t',0)",
                [],
            )?;
            tx.execute("UPDATE folders SET library_id=9", [])?;
            tx.execute(
                "UPDATE files SET phash=NULL, psig=NULL, modified_at=20 WHERE id=1",
                [],
            )?;
            tx.execute(
                "INSERT INTO thumbs(file_id,rel_path,src_size,src_mtime,state)
                 VALUES(1,'stale.jpg',500,10,1)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        assert!(jobs(&db, d.path()).unwrap().is_empty());

        db.write(|c| c.execute("UPDATE thumbs SET src_mtime=20 WHERE file_id=1", []))
            .unwrap();
        assert_eq!(jobs(&db, d.path()).unwrap().len(), 1);
    }

    #[test]
    fn the_reason_says_what_is_kept_and_how_many_are_smaller() {
        let (_d, db) = db();
        seed(
            &db,
            &[
                (1, H, 500, 400, 300, 0, 1),
                (2, H ^ 1, 4000, 1600, 1200, 0, 1),
            ],
        );
        run(&db);
        let reason: String = db
            .read(|c| c.query_row("SELECT reason FROM groups WHERE kind = 4", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(reason, "1600×1200 · 줄인 사본 1장");
    }

    /// 같은 해시가 아주 많아도 짝이 제곱으로 터지지 않는다 — 1단(같은 해시 뭉치기)이 하는 일.
    #[test]
    fn a_pile_of_identical_hashes_stays_one_group_without_exploding() {
        let (_d, db) = db();
        // 폴더를 갈라 둔다 — 한 폴더에 몰아 두면 연사로 보고 버린다
        let items: Vec<SeedItem> = (1..=300)
            .map(|i| (i, H, 100, 400, 300, (i % 3) as i32, 1))
            .collect();
        seed(&db, &items);
        let p = run(&db);
        assert_eq!(p.distinct, 1, "해시가 하나로 뭉쳐야 한다");
        assert_eq!(p.compared, 0, "견줄 짝이 없어야 한다");
        assert_eq!((p.groups, p.members), (1, 300));
    }

    /// 한 폴더 안 · 같은 해상도 = 연사. 실측에서 가장 큰 무리가 이것이었다
    /// (`IMG_0040.CR2`~`IMG_0059.CR2`, 서로 다른 사진 11장).
    #[test]
    fn drops_a_burst_that_sits_in_one_folder_at_one_size() {
        let (_d, db) = db();
        // 해시까지 똑같은 연사 — 1단에서 뭉친 다음 여기서 걸러져야 한다
        seed(
            &db,
            &[
                (1, H, 3000, 5760, 3840, 0, 1),
                (2, H, 3100, 5760, 3840, 0, 1),
                (3, H, 3050, 5760, 3840, 0, 1),
            ],
        );
        let p = run(&db);
        assert_eq!((p.groups, p.bursts), (0, 1), "연사는 «같은 순간»이 맡는다");
        assert!(members_of(&db).is_empty());
    }

    /// 같은 해상도라도 **다른 폴더**에 있으면 사본이다 — 정리하다 두 자리에 남은 것
    #[test]
    fn keeps_a_same_size_copy_that_lives_in_another_folder() {
        let (_d, db) = db();
        seed(
            &db,
            &[
                (1, H, 3000, 5760, 3840, 0, 1),
                (2, H, 3000, 5760, 3840, 2, 1),
            ],
        );
        let p = run(&db);
        assert_eq!((p.groups, p.bursts), (1, 0));
        assert_eq!(members_of(&db).len(), 2);
    }

    /// **해시가 이어도 그림이 다르면 안 묶는다.** 64비트 해시는 닮은 사진을 자주
    /// 같은 값으로 낸다 — 화소 서명이 마지막 문지기다.
    #[test]
    fn a_matching_hash_over_a_different_picture_is_not_linked() {
        let (_d, db) = db();
        // 폴더도 크기도 갈라 둔다 — 오직 **서명**이 막는 것을 본다
        seed(
            &db,
            &[(1, H, 3000, 5760, 3840, 0, 1), (2, H, 300, 1440, 960, 2, 5)],
        );
        let p = run(&db);
        assert_eq!((p.groups, p.bursts), (0, 0));
    }

    /// 서명이 없는 사진은 아직 잴 수 없으니 대상에서 빠진다 — 잘못 묶는 것보다 낫다
    #[test]
    fn a_photo_without_a_signature_is_left_out() {
        let (_d, db) = db();
        seed(
            &db,
            &[(1, H, 500, 400, 300, 0, 1), (2, H, 4000, 1600, 1200, 0, 1)],
        );
        db.transaction(|tx| {
            tx.execute("UPDATE files SET psig = NULL WHERE id = 2", [])?;
            Ok(())
        })
        .unwrap();
        assert_eq!(run(&db).groups, 0);
    }

    /// 반대로 **크기가 달라졌으면** 해시가 한두 비트 흔들려도 잇는다 — 이게 이 갈래의 뜻이다
    #[test]
    fn a_near_hash_at_a_different_size_is_a_resized_copy() {
        let (_d, db) = db();
        seed(
            &db,
            &[
                (1, H, 300, 1440, 960, 0, 1),
                (2, H ^ 3, 3000, 5760, 3840, 0, 1),
            ],
        );
        let p = run(&db);
        assert_eq!(p.groups, 1);
        assert_eq!(members_of(&db), vec![(1, false), (2, true)]);
    }

    /// A~B, B~C 라도 A 와 C 가 안 닮았으면 한 무리가 아니다 — 씨앗과 **직접** 닮아야 한다.
    /// 실측에서 이 사슬이 하와이 이웃 컷 81장을 한 무리로 만들었다.
    #[test]
    fn does_not_chain_through_a_middle_picture() {
        let (_d, db) = db();
        // 서명 40 · 43 · 46 — 이웃끼리는 3(문턱 3.5 안), 양 끝은 6(밖)
        seed(
            &db,
            &[
                (1, H, 4000, 1600, 1200, 0, 1),
                (2, H, 500, 800, 600, 0, 1),
                (3, H, 300, 400, 300, 0, 1),
            ],
        );
        db.transaction(|tx| {
            tx.execute("UPDATE files SET psig = ?1 WHERE id = 2", [flat_sig(43)])?;
            tx.execute("UPDATE files SET psig = ?1 WHERE id = 3", [flat_sig(46)])?;
            Ok(())
        })
        .unwrap();
        let p = run(&db);
        // 씨앗은 가장 큰 1. 2는 닮아 들어오고, 3은 씨앗과 6 이나 떨어져 못 들어온다.
        assert_eq!((p.groups, p.members), (1, 2));
        assert_eq!(
            members_of(&db).iter().map(|m| m.0).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    /// 실제 라이브러리 사본으로 — 시간·무리 수·확보 용량을 본다.
    /// `ACUT_DB_COPY=/path/copy.db ACUT_CACHE=<앱데이터> cargo test --release --lib cull::phash::tests::real -- --ignored --nocapture`
    #[test]
    #[ignore = "실제 DB 사본 필요"]
    fn real_library_copy() {
        let Ok(path) = std::env::var("ACUT_DB_COPY") else {
            return;
        };
        let cache = std::env::var("ACUT_CACHE").unwrap_or_default();
        let db = Db::open(path).unwrap();
        let thr: u32 = std::env::var("ACUT_PHASH_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_THRESHOLD);
        let t = std::time::Instant::now();
        let p = scan(
            &db,
            Path::new(&cache),
            thr,
            Arc::new(AtomicBool::new(false)),
            |q| {
                if q.phase == "fill" && q.fill_done % 20_000 == 0 && q.fill_done > 0 {
                    eprintln!("  해시 {}/{}", q.fill_done, q.fill_total);
                }
            },
        )
        .unwrap();
        eprintln!(
            "\n[줄인 사본] 문턱 {thr} · 해시 {}장(실패 {}) · 대상 {}장 · 서로 다른 해시 {} · 짝 {} · {}무리 {}장 · 연사로 버림 {}무리 · 확보 {:.1} GB · {:.1}초",
            p.fill_done, p.fill_failed, p.photos, p.distinct, p.compared,
            p.groups, p.members, p.bursts, p.reclaimable as f64 / 1024f64.powi(3), t.elapsed().as_secs_f64()
        );
        let sizes: Vec<i64> = db
            .read(|c| {
                let mut st = c.prepare(
                    "SELECT COUNT(*) FROM group_members m JOIN groups g ON g.id = m.group_id
                     WHERE g.kind = 4 GROUP BY g.id ORDER BY 1 DESC LIMIT 5",
                )?;
                let it = st.query_map([], |r| r.get(0))?;
                it.collect()
            })
            .unwrap();
        eprintln!("가장 큰 무리들: {sizes:?}");
    }
}
