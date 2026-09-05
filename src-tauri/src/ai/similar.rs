//! 비슷한 사진 — 벡터 사이 각도.
//!
//! 색인은 따로 없다. 8만 장 × 512 = 4천만 곱셈이 20ms라 매번 다 훑는다.
//! 벡터를 메모리에 한 번 올려 두고(160MB) 임베딩이 바뀌면 다시 올린다.

use super::clip::{from_blob, DIM};
use super::Result;
use crate::db::conn::Db;

pub struct Index {
    ids: Vec<i64>,
    /// ids와 같은 순서로 붙여 놓은 벡터들 (각각 DIM)
    vecs: Vec<f32>,
}

impl Index {
    pub fn load(db: &Db) -> Result<Self> {
        let rows: Vec<(i64, Vec<u8>)> = db.read(|c| {
            let mut st = c.prepare(
                "SELECT id, embedding FROM files WHERE embedding IS NOT NULL AND trashed_at IS NULL",
            )?;
            let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            it.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        Ok(Self::from_rows(
            rows.into_iter().map(|(id, b)| (id, from_blob(&b))),
        ))
    }

    pub fn from_rows(rows: impl Iterator<Item = (i64, Vec<f32>)>) -> Self {
        let mut ids = Vec::new();
        let mut vecs = Vec::new();
        for (id, v) in rows {
            if v.len() != DIM {
                continue; // 다른 모델로 만든 옛 벡터
            }
            ids.push(id);
            vecs.extend_from_slice(&v);
        }
        Self { ids, vecs }
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    fn vec_of(&self, id: i64) -> Option<&[f32]> {
        let i = self.ids.iter().position(|x| *x == id)?;
        Some(&self.vecs[i * DIM..(i + 1) * DIM])
    }

    /// 이 사진과 가까운 것 `k`장 — 자기 자신은 뺀다. (id, 닮은 정도 0–1)
    pub fn similar(&self, id: i64, k: usize) -> Vec<(i64, f32)> {
        let Some(q) = self.vec_of(id) else {
            return Vec::new();
        };
        self.similar_to(q, k, Some(id))
    }

    /// 벡터로 찾는다 — 글로 찾기가 이걸 쓴다.
    pub fn similar_to(&self, q: &[f32], k: usize, exclude: Option<i64>) -> Vec<(i64, f32)> {
        let mut scored: Vec<(i64, f32)> = self
            .ids
            .iter()
            .enumerate()
            .filter(|(_, id)| Some(**id) != exclude)
            .map(|(i, id)| {
                let v = &self.vecs[i * DIM..(i + 1) * DIM];
                let dot: f32 = v.iter().zip(q).map(|(a, b)| a * b).sum();
                (*id, dot)
            })
            .collect();
        // 위 k개만 정확히 — 전부 정렬할 것 없다
        let k = k.min(scored.len());
        if k == 0 {
            return Vec::new();
        }
        scored.select_nth_unstable_by(k - 1, |a, b| b.1.total_cmp(&a.1));
        scored.truncate(k);
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(mut v: Vec<f32>) -> Vec<f32> {
        v.resize(DIM, 0.0);
        super::super::clip::normalize(&v)
    }

    #[test]
    fn nearest_first_and_self_excluded() {
        let idx = Index::from_rows(
            vec![
                (1, unit(vec![1.0, 0.0])),
                (2, unit(vec![0.9, 0.1])),
                (3, unit(vec![0.0, 1.0])),
                (4, unit(vec![-1.0, 0.0])),
            ]
            .into_iter(),
        );
        let r = idx.similar(1, 2);
        assert_eq!(r.iter().map(|x| x.0).collect::<Vec<_>>(), vec![2, 3]);
        assert!(r[0].1 > 0.99 && r[0].1 <= 1.0 + 1e-6);
        assert!(!r.iter().any(|x| x.0 == 1), "자기 자신은 없다");
    }

    #[test]
    fn unknown_id_gives_nothing_and_k_is_capped() {
        let idx = Index::from_rows(vec![(1, unit(vec![1.0]))].into_iter());
        assert!(idx.similar(99, 5).is_empty());
        assert!(idx.similar(1, 5).is_empty(), "혼자면 비슷한 것도 없다");
    }

    /// 다른 모델로 만든 옛 벡터(길이가 다른 것)는 조용히 뺀다
    #[test]
    fn wrong_length_vectors_are_skipped() {
        let idx = Index::from_rows(vec![(1, vec![1.0; 10]), (2, unit(vec![1.0]))].into_iter());
        assert_eq!(idx.len(), 1);
    }
}
