//! 폴더 비교 — 내용이 완전히 같은 폴더들.
//!
//! 사용자의 말: «후보1번에도 있고 후보2번에도 있고 공용에도 있으면 셋을 한꺼번에
//! 보여 주고, 둘이면 둘. 폴더가 완전히 같은데 사진은 뭐하러 보여 주나.»
//!
//! 폴더의 «서명» = 바로 아래 파일들의 전체 해시를 정렬해 이어 붙인 것. 서명이 같은
//! 폴더끼리 한 묶음이다. 파일 하나라도 해시가 없으면(크기가 유일해 후보조차 아니었던
//! 파일) 그 폴더는 어디와도 같을 수 없으니 뺀다. 하위 폴더가 있는 폴더는 묶지 않는다 —
//! 하위는 저마다 따로 비교한다.

use rusqlite::{params, Connection, Transaction};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use super::apply::ApplyAll;

#[derive(Debug, Clone, Serialize)]
pub struct FolderIn {
    pub folder_id: i64,
    pub library_id: i64,
    pub library: String,
    /// 라이브러리 기준 경로
    pub folder: String,
    pub area: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderSet {
    /// 같은 내용의 폴더들 — 정착 구역이 앞에
    pub folders: Vec<FolderIn>,
    pub files: i64,
    /// 폴더 하나의 용량 — 하나만 남기면 (n-1)배가 빈다
    pub bytes: i64,
    /// 이 묶음의 파일 중 지우기 표시가 아직 없는 것이 있나
    pub pending: bool,
}

fn in_lib(lib_rel: &str, rel: &str) -> String {
    rel.strip_prefix(lib_rel)
        .map(|s| s.trim_start_matches('/'))
        .unwrap_or(rel)
        .to_string()
}

/// 하위 폴더 행이 있는 (볼륨, 경로) 집합. `folders.parent_id`는 스캐너가 채우지 않아
/// 그걸로 걸러 봐야 아무것도 안 걸러진다 — 경로의 위 폴더를 셈해서 만든다 (리뷰 H5)
fn parents_with_children(c: &Connection) -> rusqlite::Result<HashSet<(String, String)>> {
    let mut st = c.prepare("SELECT volume_uuid, rel_path FROM folders")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = HashSet::new();
    for row in rows {
        let (vol, rel) = row?;
        // 위 폴더 전부 — 중간 폴더는 사진이 바로 아래 없으면 행이 없어서 바로 위만 보면 놓친다
        for p in ancestors(&rel) {
            out.insert((vol.clone(), p));
        }
    }
    Ok(out)
}

/// `a/b/c` → `a/b`, `a`, `` (뿌리)
fn ancestors(rel: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = rel;
    while let Some((p, _)) = cur.rsplit_once('/') {
        out.push(p.to_string());
        cur = p;
    }
    if !rel.is_empty() {
        out.push(String::new());
    }
    out
}

/// 볼륨이 지금 붙어 있나 — 볼륨마다 한 번만 본다
struct Online(HashMap<String, bool>);
impl Online {
    fn new() -> Self {
        Online(HashMap::new())
    }
    fn is(&mut self, vol: &str) -> bool {
        *self
            .0
            .entry(vol.to_string())
            .or_insert_with(|| crate::db::volumes::find_mount(vol).is_some())
    }
}

/// 내용이 완전히 같은 폴더 묶음들 — 비는 용량 큰 순.
pub fn identical_sets(c: &Connection, limit: usize) -> rusqlite::Result<Vec<FolderSet>> {
    // 파일이 전부 해시된 폴더만 서명을 만든다. group_concat 의 순서는 안쪽 ORDER BY 를 따른다.
    let mut st = c.prepare(
        "WITH tot AS (
           SELECT folder_id, COUNT(*) n, SUM(full_hash IS NULL) nohash, SUM(size) bytes,
                  SUM(culling_flag = 0) pend
           FROM files WHERE trashed_at IS NULL GROUP BY folder_id),
         sig AS (
           SELECT folder_id, group_concat(full_hash, ',') s
           FROM (SELECT folder_id, full_hash FROM files
                 WHERE trashed_at IS NULL AND full_hash IS NOT NULL
                 ORDER BY folder_id, full_hash)
           GROUP BY folder_id)
         SELECT fo.id, fo.library_id, l.name, l.rel_path, fo.rel_path, fo.area,
                t.n, t.bytes, t.pend, sig.s, fo.volume_uuid
         FROM sig
         JOIN tot t ON t.folder_id = sig.folder_id
         JOIN folders fo ON fo.id = sig.folder_id
         JOIN libraries l ON l.id = fo.library_id
         WHERE t.nohash = 0 AND t.n >= 1",
    )?;
    let rows = st.query_map([], |r| {
        let lib_rel: String = r.get(3)?;
        let rel: String = r.get(4)?;
        Ok((
            FolderIn {
                folder_id: r.get(0)?,
                library_id: r.get(1)?,
                library: r.get(2)?,
                folder: in_lib(&lib_rel, &rel),
                area: r.get(5)?,
            },
            r.get::<_, i64>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, i64>(8)?,
            r.get::<_, String>(9)?,
            r.get::<_, String>(10)?,
            rel,
        ))
    })?;
    // 하위 폴더가 있으면 «바로 아래 파일만 같다»일 뿐이다 — 사용자는 폴더째 같다고 읽고
    // Finder 에서 지운다. 하위는 저마다 따로 견준다. 빠진 디스크의 폴더는 지금 확인할 수
    // 없으니 견주지 않는다
    let kids = parents_with_children(c)?;
    let mut online = Online::new();
    let mut by_sig: HashMap<String, (Vec<FolderIn>, i64, i64, bool)> = HashMap::new();
    for row in rows {
        let (f, n, bytes, pend, s, vol, rel) = row?;
        if kids.contains(&(vol.clone(), rel)) || !online.is(&vol) {
            continue;
        }
        let e = by_sig.entry(s).or_insert((Vec::new(), n, bytes, false));
        e.0.push(f);
        e.3 |= pend > 0;
    }
    let mut out: Vec<FolderSet> = by_sig
        .into_values()
        .filter(|(fs, _, _, _)| fs.len() >= 2)
        .map(|(mut fs, files, bytes, pending)| {
            // 정착 구역이 앞에, 그다음은 라이브러리·경로 순 — 남길 것이 맨 앞
            fs.sort_by(|a, b| {
                let sa = !(a.area == 1 || a.area == 2);
                let sb = !(b.area == 1 || b.area == 2);
                sa.cmp(&sb)
                    .then(a.library_id.cmp(&b.library_id))
                    .then(a.folder.cmp(&b.folder))
            });
            FolderSet { folders: fs, files, bytes, pending }
        })
        .collect();
    // 경로 순 — 용량 순으로 두면 «2016, 2023, 2019…» 뒤죽박죽으로 보인다 (사용자 지적).
    // 묶음의 이름은 정착 구역 폴더(맨 앞)의 경로
    out.sort_by(|a, b| {
        a.folders[0]
            .folder
            .cmp(&b.folders[0].folder)
            .then(a.folders[0].library_id.cmp(&b.folders[0].library_id))
    });
    out.truncate(limit);
    Ok(out)
}

/// 폴더 묶음 하나를 처리한다 — `keep` 폴더의 파일은 남김, `drops` 폴더의 파일은 지우기 표시.
/// 이 폴더들 안에서만 얽힌 완전 중복 무리는 확정으로 돌려 개별 비교에 다시 안 나오게.
pub fn apply_set(tx: &Transaction, keep: i64, drops: &[i64]) -> rusqlite::Result<ApplyAll> {
    if drops.contains(&keep) || drops.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let kept = tx.execute(
        "UPDATE files SET culling_flag = 1 WHERE folder_id = ?1 AND trashed_at IS NULL",
        [keep],
    )?;
    // 남길 폴더에 **같은 내용이 지금 있는** 파일만 지우기 표시 — 목록을 본 뒤 남길 폴더에서
    // 파일이 지워졌거나 바뀌었으면 그 사본은 마지막 한 벌이다 (리뷰 C12)
    let mut rejected = 0;
    for d in drops {
        rejected += tx.execute(
            "UPDATE files SET culling_flag = 2
             WHERE folder_id = ?1 AND trashed_at IS NULL AND culling_flag <> 1
               AND full_hash IS NOT NULL
               AND full_hash IN (SELECT k.full_hash FROM files k
                                 WHERE k.folder_id = ?2 AND k.trashed_at IS NULL AND k.full_hash IS NOT NULL)",
            params![d, keep],
        )?;
    }
    let all: Vec<i64> = std::iter::once(keep).chain(drops.iter().copied()).collect();
    let list = all.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    let groups = tx.execute(
        &format!(
            "UPDATE groups SET state = 1 WHERE kind = 0 AND state = 0
               AND id IN (SELECT m.group_id FROM group_members m JOIN files f ON f.id = m.file_id
                          WHERE f.folder_id IN ({list}))
               AND NOT EXISTS (SELECT 1 FROM group_members m JOIN files f ON f.id = m.file_id
                               WHERE m.group_id = groups.id AND f.folder_id NOT IN ({list}))"
        ),
        params![],
    )?;
    Ok(ApplyAll { groups, kept, rejected, skipped: 0 })
}


/// 두 폴더 사이의 완전 중복 무리를 한꺼번에 — `keep` 폴더 것을 남기고 `drop` 폴더 것에 제외
/// 표시. 두 폴더 밖까지 얽힌 무리는 건너뛴다(그건 개별 비교에서). 개별 비교의 «이 폴더 쌍
/// 전부 이렇게» 단추가 쓴다.
pub fn apply_pair(tx: &Transaction, keep: i64, drop: i64, dry_run: bool) -> rusqlite::Result<ApplyAll> {
    if keep == drop {
        return Err(rusqlite::Error::InvalidQuery);
    }
    tx.execute_batch(
        "DROP TABLE IF EXISTS temp.todo; CREATE TEMP TABLE todo(id INTEGER PRIMARY KEY);",
    )?;
    tx.execute(
        "INSERT INTO temp.todo
         SELECT g.id FROM groups g
         WHERE g.kind = 0 AND g.state = 0
           AND EXISTS (SELECT 1 FROM group_members m JOIN files f ON f.id = m.file_id
                       WHERE m.group_id = g.id AND f.folder_id = ?1)
           AND EXISTS (SELECT 1 FROM group_members m JOIN files f ON f.id = m.file_id
                       WHERE m.group_id = g.id AND f.folder_id = ?2)
           AND NOT EXISTS (SELECT 1 FROM group_members m JOIN files f ON f.id = m.file_id
                           WHERE m.group_id = g.id AND f.folder_id NOT IN (?1, ?2))",
        params![keep, drop],
    )?;
    let groups = tx.query_row("SELECT COUNT(*) FROM temp.todo", [], |r| r.get::<_, i64>(0))? as usize;
    if dry_run {
        // 세기만 — 남길 폴더의 구성원이 남김, 나머지 중 아직 «남김»이 아닌 것이 제외
        let kept = tx.query_row(
            "SELECT COUNT(DISTINCT m.file_id) FROM group_members m JOIN files f ON f.id = m.file_id
             WHERE m.group_id IN (SELECT id FROM temp.todo) AND f.folder_id = ?1",
            [keep],
            |r| r.get::<_, i64>(0),
        )? as usize;
        let rejected = tx.query_row(
            "SELECT COUNT(DISTINCT m.file_id) FROM group_members m JOIN files f ON f.id = m.file_id
             WHERE m.group_id IN (SELECT id FROM temp.todo) AND f.folder_id <> ?1 AND f.culling_flag <> 1",
            [keep],
            |r| r.get::<_, i64>(0),
        )? as usize;
        tx.execute_batch("DROP TABLE temp.todo;")?;
        return Ok(ApplyAll { groups, kept, rejected, skipped: 0 });
    }
    tx.execute(
        "UPDATE group_members SET is_best = CASE WHEN file_id IN (SELECT id FROM files WHERE folder_id = ?1) THEN 1 ELSE 0 END
         WHERE group_id IN (SELECT id FROM temp.todo)",
        [keep],
    )?;
    let kept = tx.execute(
        "UPDATE files SET culling_flag = 1 WHERE id IN (
           SELECT file_id FROM group_members WHERE group_id IN (SELECT id FROM temp.todo) AND is_best = 1)",
        [],
    )?;
    // 이미 «남김»인 파일은 내리지 않는다 — 다른 갈래와 같은 규칙 (리뷰 C11)
    let rejected = tx.execute(
        "UPDATE files SET culling_flag = 2 WHERE culling_flag <> 1 AND id IN (
           SELECT file_id FROM group_members WHERE group_id IN (SELECT id FROM temp.todo) AND is_best = 0)",
        [],
    )?;
    tx.execute("UPDATE groups SET state = 1 WHERE id IN (SELECT id FROM temp.todo)", [])?;
    tx.execute_batch("DROP TABLE temp.todo;")?;
    Ok(ApplyAll { groups, kept, rejected, skipped: 0 })
}

/// 두 폴더 비교의 «전부» — 짝마다 `apply_set`. 한 트랜잭션이라 화면이 짝마다 명령을 보내며
/// 잠금 없이 두 루프가 얽히던 길이 없다. 못 한 짝은 세어 알린다
#[derive(Debug, Clone, Default, Serialize)]
pub struct PairsApplied {
    pub applied: usize,
    pub failed: usize,
    pub first_error: Option<String>,
    pub kept: usize,
    pub rejected: usize,
}

pub fn apply_pairs(tx: &Transaction, pairs: &[(i64, i64)]) -> rusqlite::Result<PairsApplied> {
    let mut out = PairsApplied::default();
    for &(keep, drop) in pairs {
        match apply_set(tx, keep, &[drop]) {
            Ok(r) => {
                out.applied += 1;
                out.kept += r.kept;
                out.rejected += r.rejected;
            }
            Err(rusqlite::Error::InvalidQuery) => {
                out.failed += 1;
                out.first_error.get_or_insert_with(|| "같은 폴더를 남기고 지울 수는 없습니다".to_string());
            }
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize)]
pub struct PairRow {
    /// A 뿌리 아래 폴더 (뿌리 기준 경로). 없으면 B 에만 있는 폴더
    pub a: Option<FolderIn>,
    pub b: Option<FolderIn>,
    pub files_a: i64,
    pub files_b: i64,
    /// 바로 아래 파일이 전부 같다
    pub same: bool,
    /// 이름이 같은 폴더끼리, 양쪽에 똑같이 있는 파일 수
    pub common: i64,
    /// 같은 쪽 하나를 지우면 비는 용량(same 일 때) — 아니면 공통 파일의 용량
    pub bytes: i64,
    /// 이미 지우기 표시된 파일 수 — 한쪽이 전부면 그 짝은 «처리됨»
    pub flagged_a: i64,
    pub flagged_b: i64,
}

struct Agg {
    info: FolderIn,
    /// 뿌리 기준 상대경로 — 이름이 같은 폴더를 찾는 열쇠
    sub: String,
    files: i64,
    bytes: i64,
    /// culling_flag = 2 인 파일 수
    flagged: i64,
    hashes: Vec<String>,
    all_hashed: bool,
    has_children: bool,
}

/// 뿌리는 (볼륨, 볼륨 기준 경로)다 — «연도별»처럼 사진이 바로 아래 없는 폴더는 `folders`
/// 행이 없어서 id 로는 가리킬 수 없다 (실측: 후보1번/연도별을 골랐는데 «없는 폴더»).
fn folders_under(c: &Connection, vol: &str, rel: &str) -> rusqlite::Result<Vec<Agg>> {
    let esc = crate::db::query::escape_like(rel);
    let mut st = c.prepare(
        "SELECT fo.id, fo.rel_path, fo.area, l.id, l.name, l.rel_path, f.full_hash, f.size,
                f.culling_flag = 2
         FROM folders fo
         JOIN libraries l ON l.id = fo.library_id
         LEFT JOIN files f ON f.folder_id = fo.id AND f.trashed_at IS NULL
         WHERE fo.volume_uuid = ?1 AND (fo.rel_path = ?2 OR fo.rel_path LIKE ?3 || '/%' ESCAPE '\\')
         ORDER BY fo.rel_path, f.full_hash",
    )?;
    let mut out: Vec<Agg> = Vec::new();
    let rows = st.query_map(params![vol, rel, esc], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i32>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, Option<i64>>(7)?,
            r.get::<_, Option<bool>>(8)?.unwrap_or(false),
        ))
    })?;
    for row in rows {
        let (id, fo_rel, area, lib_id, lib_name, lib_rel, hash, size, flagged) = row?;
        if out.last().map(|a| a.info.folder_id) != Some(id) {
            let sub = fo_rel
                .strip_prefix(rel)
                .map(|s| s.trim_start_matches('/').to_string())
                .unwrap_or_else(|| fo_rel.clone());
            out.push(Agg {
                info: FolderIn {
                    folder_id: id,
                    library_id: lib_id,
                    library: lib_name,
                    folder: in_lib(&lib_rel, &fo_rel),
                    area,
                },
                sub,
                files: 0,
                bytes: 0,
                flagged: 0,
                hashes: Vec::new(),
                all_hashed: true,
                has_children: false,
            });
        }
        let cur = out.last_mut().unwrap();
        if let Some(size) = size {
            cur.files += 1;
            cur.bytes += size;
            cur.flagged += flagged as i64;
            match hash {
                Some(h) => cur.hashes.push(h),
                None => cur.all_hashed = false,
            }
        }
    }
    // 하위 폴더 유무는 이 결과 안에서 안다 — 뿌리 아래 폴더는 전부 여기 들어 있다
    let parents: HashSet<String> = out.iter().flat_map(|a| ancestors(&a.sub)).collect();
    for a in &mut out {
        a.has_children = parents.contains(&a.sub);
    }
    Ok(out)
}

/// 두 뿌리가 서로를 품는가 — 같은 폴더가 양쪽 목록에 들어 제 짝이 되는 길을 막는다
pub fn roots_overlap((a_vol, a_rel): (&str, &str), (b_vol, b_rel): (&str, &str)) -> bool {
    if a_vol != b_vol {
        return false;
    }
    let under = |root: &str, p: &str| root.is_empty() || p == root || p.starts_with(&format!("{root}/"));
    under(a_rel, b_rel) || under(b_rel, a_rel)
}

/// 두 폴더(와 그 아래)를 견준다 — «후보1번/연도별»과 «후보2번»처럼.
///
/// 내용이 완전히 같은 폴더끼리 짝(same), 이름이 같은데 내용이 다른 폴더끼리는 공통
/// 파일 수와 함께 짝(partial), 한쪽에만 있는 폴더는 홀로. 같은 쪽 큰 것부터.
pub fn compare_two(
    c: &Connection,
    (a_vol, a_rel): (&str, &str),
    (b_vol, b_rel): (&str, &str),
) -> rusqlite::Result<Vec<PairRow>> {
    if roots_overlap((a_vol, a_rel), (b_vol, b_rel)) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let a = folders_under(c, a_vol, a_rel)?;
    let b = folders_under(c, b_vol, b_rel)?;
    // B 쪽 서명·이름 색인
    let sig = |g: &Agg| -> Option<String> {
        (g.files > 0 && g.all_hashed && !g.has_children).then(|| g.hashes.join(","))
    };
    let mut b_by_sig: HashMap<String, Vec<usize>> = HashMap::new();
    let mut b_by_sub: HashMap<String, usize> = HashMap::new();
    for (i, g) in b.iter().enumerate() {
        if let Some(s) = sig(g) {
            b_by_sig.entry(s).or_default().push(i);
        }
        b_by_sub.entry(g.sub.clone()).or_insert(i);
    }
    let mut used_b = vec![false; b.len()];
    // (정렬 열쇠 = 뿌리 기준 경로, 줄)
    let mut out: Vec<(String, PairRow)> = Vec::new();
    for ga in &a {
        if ga.files == 0 && !ga.has_children {
            continue; // 빈 폴더
        }
        // 1) 내용이 같은 B 폴더 — 이름까지 같은 것을 먼저
        if let Some(s) = sig(ga) {
            if let Some(cands) = b_by_sig.get(&s) {
                let pick = cands
                    .iter()
                    .copied()
                    .filter(|&i| !used_b[i])
                    .min_by_key(|&i| (b[i].sub != ga.sub, i));
                if let Some(i) = pick {
                    used_b[i] = true;
                    out.push((ga.sub.clone(), PairRow {
                        a: Some(ga.info.clone()),
                        b: Some(b[i].info.clone()),
                        files_a: ga.files,
                        files_b: b[i].files,
                        same: true,
                        common: ga.files,
                        bytes: ga.bytes,
                        flagged_a: ga.flagged,
                        flagged_b: b[i].flagged,
                    }));
                    continue;
                }
            }
        }
        // 2) 이름이 같은 B 폴더 — 공통 파일 수
        if let Some(&i) = b_by_sub.get(&ga.sub) {
            if !used_b[i] {
                used_b[i] = true;
                let gb = &b[i];
                let mut counts: HashMap<&str, i64> = HashMap::new();
                for h in &gb.hashes {
                    *counts.entry(h.as_str()).or_default() += 1;
                }
                let mut common = 0i64;
                let mut bytes = 0i64;
                let per = if ga.files > 0 { ga.bytes / ga.files } else { 0 };
                for h in &ga.hashes {
                    if let Some(n) = counts.get_mut(h.as_str()) {
                        if *n > 0 {
                            *n -= 1;
                            common += 1;
                            bytes += per;
                        }
                    }
                }
                out.push((ga.sub.clone(), PairRow {
                    a: Some(ga.info.clone()),
                    b: Some(gb.info.clone()),
                    files_a: ga.files,
                    files_b: gb.files,
                    same: false,
                    common,
                    bytes,
                    flagged_a: ga.flagged,
                    flagged_b: gb.flagged,
                }));
                continue;
            }
        }
        // 3) A 에만
        out.push((ga.sub.clone(), PairRow {
            a: Some(ga.info.clone()),
            b: None,
            files_a: ga.files,
            files_b: 0,
            same: false,
            common: 0,
            bytes: 0,
            flagged_a: ga.flagged,
            flagged_b: 0,
        }));
    }
    for (i, gb) in b.iter().enumerate() {
        if used_b[i] || (gb.files == 0 && !gb.has_children) {
            continue;
        }
        out.push((gb.sub.clone(), PairRow {
            a: None,
            b: Some(gb.info.clone()),
            files_a: 0,
            files_b: gb.files,
            same: false,
            common: 0,
            bytes: 0,
            flagged_a: 0,
            flagged_b: gb.flagged,
        }));
    }
    // 경로 순 — Finder 를 나란히 놓은 것처럼 읽힌다. «같은 것 → 부분 → 한쪽만, 용량 큰
    // 순»으로 두었더니 «오름차순도 내림차순도 아니다»(사용자 지적)
    out.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.same.cmp(&y.1.same)));
    Ok(out.into_iter().map(|(_, r)| r).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cull::dedup;
    use crate::db::conn::Db;
    use crate::scan::scan_test;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    /// a/, b/, c/ 는 내용이 같은 폴더(파일 이름은 달라도 된다). d/ 는 한 장이 다르다.
    fn setup() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let x = b"X ".repeat(500);
        let y = b"Y ".repeat(700);
        for (d, names) in [("a", ["1.jpg", "2.jpg"]), ("b", ["one.jpg", "two.jpg"]), ("c", ["1.jpg", "2.jpg"])] {
            let p = dir.path().join(d);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join(names[0]), &x).unwrap();
            std::fs::write(p.join(names[1]), &y).unwrap();
        }
        let p = dir.path().join("d");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("1.jpg"), &x).unwrap();
        std::fs::write(p.join("3.jpg"), b"Z ".repeat(700)).unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        scan_test(&db, dir.path(), 1, |_| {}).unwrap();
        db.write(|c| {
            c.execute("UPDATE folders SET area = 0", [])?;
            c.execute("UPDATE folders SET area = 2 WHERE rel_path LIKE '%c'", [])
        })
        .unwrap();
        dedup::scan(&db, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        (dir, db)
    }

    #[test]
    fn finds_folders_with_identical_contents() {
        let (_d, db) = setup();
        let sets = db.read(|c| identical_sets(c, 100)).unwrap();
        assert_eq!(sets.len(), 1, "{sets:?}");
        let s = &sets[0];
        let names: Vec<&str> = s.folders.iter().map(|f| f.folder.as_str()).collect();
        assert_eq!(names, ["c", "a", "b"], "정착 구역(c)이 맨 앞");
        assert_eq!(s.files, 2);
        assert!(s.pending);
    }

    #[test]
    fn applying_a_set_marks_the_other_folders_and_settles_their_groups() {
        let (_d, db) = setup();
        let sets = db.read(|c| identical_sets(c, 100)).unwrap();
        let s = &sets[0];
        let keep = s.folders[0].folder_id;
        let drops: Vec<i64> = s.folders[1..].iter().map(|f| f.folder_id).collect();
        let r = db.transaction(|tx| apply_set(tx, keep, &drops)).unwrap();
        assert_eq!((r.kept, r.rejected), (2, 4));
        // d/1.jpg 는 a·b·c 밖에도 있어 그 무리는 미결로 남는다; y(2.jpg) 무리는 확정
        assert_eq!(r.groups, 1, "{r:?}");
        let pending = db.read(|c| identical_sets(c, 100)).unwrap();
        assert!(!pending[0].pending, "처리한 묶음은 pending 이 아니다");
    }

    #[test]
    fn ancestors_walk_up_to_the_root() {
        assert_eq!(ancestors("a/b/c"), ["a/b", "a", ""]);
        assert_eq!(ancestors("a"), [""]);
        assert!(ancestors("").is_empty());
    }

    #[test]
    fn roots_that_contain_each_other_overlap() {
        assert!(roots_overlap(("v", "통합전후보"), ("v", "통합전후보/후보1번")));
        assert!(roots_overlap(("v", "통합전후보/후보1번"), ("v", "통합전후보")));
        assert!(roots_overlap(("v", "a"), ("v", "a")));
        assert!(roots_overlap(("v", ""), ("v", "x")), "볼륨 뿌리는 전부를 품는다");
        assert!(!roots_overlap(("v", "후보1"), ("v", "후보10")), "이름 앞만 같은 것");
        assert!(!roots_overlap(("v1", "a"), ("v2", "a")), "다른 볼륨");
    }

    #[test]
    fn a_folder_with_subfolders_is_never_called_identical() {
        // a/ 안에 하위 폴더 a/inner/ 가 생기면 a 는 «바로 아래만 같다»일 뿐 — 묶지 않는다
        let (dir, db) = setup();
        let inner = dir.path().join("a/inner");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("q.jpg"), b"Q ".repeat(300)).unwrap();
        scan_test(&db, dir.path(), 0, |_| {}).unwrap();
        dedup::scan(&db, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        let sets = db.read(|c| identical_sets(c, 100)).unwrap();
        assert_eq!(sets.len(), 1, "{sets:?}");
        let names: Vec<&str> = sets[0].folders.iter().map(|f| f.folder.as_str()).collect();
        assert!(!names.contains(&"a"), "하위 폴더가 있는 a 는 빠진다: {names:?}");
        assert_eq!(names, ["c", "b"]);
    }

    #[test]
    fn apply_set_only_drops_files_that_still_have_a_copy_in_the_kept_folder() {
        let (dir, db) = setup();
        let sets = db.read(|c| identical_sets(c, 100)).unwrap();
        let s = &sets[0];
        let keep = s.folders[0].folder_id; // c
        let drops: Vec<i64> = s.folders[1..].iter().map(|f| f.folder_id).collect();
        // 목록을 본 뒤 남길 폴더(c)에서 2.jpg 가 사라졌다(휴지통) — 그 내용은 이제 a·b 에만 있다
        std::fs::remove_file(dir.path().join("c/2.jpg")).unwrap();
        db.write(|c| {
            c.execute(
                "UPDATE files SET trashed_at = 1 WHERE folder_id = ?1 AND name = '2.jpg'",
                [keep],
            )
        })
        .unwrap();
        let r = db.transaction(|tx| apply_set(tx, keep, &drops)).unwrap();
        assert_eq!(r.rejected, 2, "a/1, b/one 만 지우기 표시 — 2.jpg 사본은 남는다: {r:?}");
        let flagged: Vec<String> = db
            .read(|c| {
                let mut st = c.prepare("SELECT name FROM files WHERE culling_flag = 2 ORDER BY name")?;
                let it = st.query_map([], |r| r.get(0))?;
                it.collect()
            })
            .unwrap();
        assert_eq!(flagged, ["1.jpg", "one.jpg"]);
    }

    #[test]
    fn compare_two_rejects_overlapping_roots_and_lists_in_path_order() {
        let (dir, db) = setup();
        // 뿌리 아래 여러 폴더: root/{a,b,c,d} 를 통째로 다른 곳과 견주려면 뿌리가 둘 필요 —
        // 여기서는 «뿌리(전체) 대 a» 가 겹친다는 것만 본다
        let vol: String = db
            .read(|c| c.query_row("SELECT volume_uuid FROM folders LIMIT 1", [], |r| r.get(0)))
            .unwrap();
        let a_rel: String = db
            .read(|c| c.query_row("SELECT rel_path FROM folders WHERE rel_path LIKE '%a'", [], |r| r.get(0)))
            .unwrap();
        let root_rel = a_rel.rsplit_once('/').map(|(p, _)| p.to_string()).unwrap_or_default();
        let e = db.read(|c| compare_two(c, (&vol, &root_rel), (&vol, &a_rel)));
        assert!(e.is_err(), "겹치는 뿌리는 거절한다: {e:?}");
        // 경로 순: 두 뿌리 아래 폴더가 여럿일 때 sub 오름차순 — x/{1,2} 대 y/{2,1}
        for (d, names) in [("x/1", ["p.jpg"]), ("x/2", ["p.jpg"]), ("y/1", ["p.jpg"]), ("y/2", ["p.jpg"])] {
            let p = dir.path().join(d);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join(names[0]), d.as_bytes().repeat(200)).unwrap();
        }
        scan_test(&db, dir.path(), 0, |_| {}).unwrap();
        dedup::scan(&db, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        let x = format!("{root_rel}/x").trim_start_matches('/').to_string();
        let y = format!("{root_rel}/y").trim_start_matches('/').to_string();
        let rows = db.read(|c| compare_two(c, (&vol, &x), (&vol, &y))).unwrap();
        let subs: Vec<String> = rows.iter().map(|r| r.a.as_ref().or(r.b.as_ref()).unwrap().folder.clone()).collect();
        let mut sorted = subs.clone();
        sorted.sort();
        assert_eq!(subs, sorted, "경로 오름차순: {subs:?}");
        assert!(rows.iter().all(|r| !r.same), "내용이 다 다르니 같은 짝은 없다");
    }

    /// a/ (작업대): 사본 둘 + 혼자인 것 하나 + T7끼리만 겹치는 둘.
    /// b/ (공용): 원본 하나. c/ (공용): b 와 겹치는 것 하나 — 정착 구역 안 겹침.
    fn setup_pair() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, c) = (dir.path().join("a"), dir.path().join("b"), dir.path().join("c"));
        for d in [&a, &b, &c] {
            std::fs::create_dir_all(d).unwrap();
        }
        let same = b"SAME CONTENT ".repeat(100);
        let inner = b"INNER ONLY ".repeat(100);
        let pair = b"PAIR IN NAS ".repeat(100);
        std::fs::write(a.join("20200101_120000.jpg"), &same).unwrap();
        std::fs::write(a.join("copy.jpg"), &same).unwrap();
        std::fs::write(a.join("alone.jpg"), b"unique").unwrap();
        std::fs::write(a.join("20200102_120000.jpg"), &inner).unwrap();
        std::fs::write(a.join("inner-copy.jpg"), &inner).unwrap();
        std::fs::write(b.join("20200101_120001.jpg"), &same).unwrap();
        std::fs::write(b.join("20200103_120000.jpg"), &pair).unwrap();
        std::fs::write(c.join("20200103_120001.jpg"), &pair).unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        scan_test(&db, dir.path(), 1, |_| {}).unwrap();
        db.write(|cn| {
            cn.execute("UPDATE folders SET area = 0", [])?;
            cn.execute("UPDATE folders SET area = 2 WHERE rel_path LIKE '%b' OR rel_path LIKE '%c'", [])
        })
        .unwrap();
        dedup::scan(&db, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        (dir, db)
    }

    #[test]
    fn pair_apply_keeps_one_folder_and_marks_the_other() {
        let (_d, db) = setup_pair();
        let (fb, fc): (i64, i64) = db
            .read(|c| {
                Ok((
                    c.query_row("SELECT id FROM folders WHERE rel_path LIKE '%b'", [], |r| r.get(0))?,
                    c.query_row("SELECT id FROM folders WHERE rel_path LIKE '%c'", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        // 먼저 세어 보기 — 아무것도 안 바꾼다
        let dry = db.transaction(|tx| apply_pair(tx, fc, fb, true)).unwrap();
        assert_eq!((dry.groups, dry.kept, dry.rejected), (1, 1, 1));
        let untouched: i64 = db
            .read(|c| c.query_row("SELECT COUNT(*) FROM files WHERE culling_flag <> 0", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(untouched, 0, "dry_run 은 판정을 안 바꾼다");
        // c 를 남기고 b 것에 표시 — 대표가 b 였어도 뒤집힌다
        let r = db.transaction(|tx| apply_pair(tx, fc, fb, false)).unwrap();
        assert_eq!((r.groups, r.kept, r.rejected), (1, 1, 1));
        let flag: i32 = db
            .read(|c| c.query_row("SELECT culling_flag FROM files WHERE name = '20200103_120000.jpg'", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(flag, 2, "b 의 것이 제외 표시");
        assert!(db.transaction(|tx| apply_pair(tx, fb, fb, false)).is_err(), "같은 폴더끼리는 거절");
    }

    #[test]
    fn pairs_apply_counts_failures_without_aborting_the_batch() {
        let (_d, db) = setup();
        let sets = db.read(|c| identical_sets(c, 100)).unwrap();
        let s = &sets[0];
        let (keep, drop) = (s.folders[0].folder_id, s.folders[1].folder_id);
        let r = db
            .transaction(|tx| apply_pairs(tx, &[(keep, keep), (keep, drop)]))
            .unwrap();
        assert_eq!((r.applied, r.failed), (1, 1), "{r:?}");
        assert!(r.first_error.is_some());
        assert_eq!(r.rejected, 2);
    }

    #[test]
    fn two_roots_pair_identical_and_partial_folders() {
        let (_d, db) = setup();
        let root_of = |name: &str| -> (String, String) {
            db.read(|c| {
                c.query_row(
                    "SELECT volume_uuid, rel_path FROM folders WHERE rel_path LIKE ?1",
                    [format!("%{name}")],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
            })
            .unwrap()
        };
        let (a, b, d) = (root_of("a"), root_of("b"), root_of("d"));
        // a 와 b 는 내용이 같다 — 뿌리끼리도 짝이 된다
        let rows = db.read(|c| compare_two(c, (&a.0, &a.1), (&b.0, &b.1))).unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].same && rows[0].common == 2);
        // a 와 d 는 한 장만 겹친다 — 뿌리 이름은 다르지만 뿌리끼리는 sub 가 같다("")
        let rows = db.read(|c| compare_two(c, (&a.0, &a.1), (&d.0, &d.1))).unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(!rows[0].same);
        assert_eq!((rows[0].common, rows[0].files_a, rows[0].files_b), (1, 2, 2));
    }
}
