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
    /// `folders` 와 같은 순서 — 각 폴더 나무의 폴더 행 id 들(하위 포함). 표시·휴지통으로가 이 목록에 건다
    pub ids: Vec<Vec<i64>>,
    pub files: i64,
    /// 폴더 하나의 용량 — 하나만 남기면 (n-1)배가 빈다
    pub bytes: i64,
    /// 이 묶음의 파일 중 제외 표시가 아직 없는 것이 있나
    pub pending: bool,
    /// 묶음 안에서 제외 표시된 파일 수 — «표시한 N장 치우기»
    pub flagged: i64,
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

/// 볼륨이 지금 붙어 있나, 폴더가 디스크에 아직 있나 — 마운트는 볼륨마다 한 번만 찾는다.
/// Finder 에서 지운 폴더의 행이 DB 에 남아 있을 수 있다(감시는 «폴더가 안 보이면 지우지
/// 않는다», 리뷰 C2). 그런 폴더를 견주면 «없는 폴더를 읽는다»가 된다 (실측 2026-08-30: 269개)
struct Disk(HashMap<String, Option<std::path::PathBuf>>);
impl Disk {
    fn new() -> Self {
        Disk(HashMap::new())
    }
    fn mount(&mut self, vol: &str) -> Option<std::path::PathBuf> {
        self.0
            .entry(vol.to_string())
            .or_insert_with(|| crate::db::volumes::find_mount(vol))
            .clone()
    }
    fn online(&mut self, vol: &str) -> bool {
        self.mount(vol).is_some()
    }
    fn dir_exists(&mut self, vol: &str, rel: &str) -> bool {
        self.mount(vol).map(|m| m.join(rel).is_dir()).unwrap_or(false)
    }
}

/// 내용이 완전히 같은 폴더 묶음들 — **나무째** 본다(하위 폴더까지 합친 내용). 위 폴더끼리 같으면
/// 아래 폴더는 따로 안 나온다. 경로 순.
///
/// 실측(2026-08-30): 바로 아래 파일만 보던 때는 하위 폴더가 있는 폴더를 통째로 뺐고, 그래서
/// 껍질 벗기듯 여러 번 돌아야 했다 — 두 폴더 비교와 같은 나무 판정으로 맞춘다
pub fn identical_sets(c: &Connection, limit: usize) -> rusqlite::Result<Vec<FolderSet>> {
    struct Row {
        info: FolderIn,
        vol: String,
        rel: String,
        n: i64,
        bytes: i64,
        pend: i64,
        flagged: i64,
        nohash: i64,
        hashes: Vec<String>,
    }
    let mut st = c.prepare(
        "WITH tot AS (
           SELECT folder_id, COUNT(*) n, SUM(full_hash IS NULL) nohash, SUM(size) bytes,
                  SUM(culling_flag = 0) pend, SUM(culling_flag = 2) flagged
           FROM files WHERE trashed_at IS NULL GROUP BY folder_id),
         sig AS (
           SELECT folder_id, group_concat(full_hash, ',') s
           FROM (SELECT folder_id, full_hash FROM files
                 WHERE trashed_at IS NULL AND full_hash IS NOT NULL
                 ORDER BY folder_id, full_hash)
           GROUP BY folder_id)
         SELECT fo.id, fo.library_id, l.name, l.rel_path, fo.rel_path, fo.area,
                t.n, t.bytes, t.pend, t.flagged, t.nohash, sig.s, fo.volume_uuid
         FROM folders fo
         JOIN libraries l ON l.id = fo.library_id
         JOIN tot t ON t.folder_id = fo.id
         LEFT JOIN sig ON sig.folder_id = fo.id",
    )?;
    let rows: Vec<Row> = st
        .query_map([], |r| {
            let lib_rel: String = r.get(3)?;
            let rel: String = r.get(4)?;
            let s: Option<String> = r.get(11)?;
            Ok(Row {
                info: FolderIn {
                    folder_id: r.get(0)?,
                    library_id: r.get(1)?,
                    library: r.get(2)?,
                    folder: in_lib(&lib_rel, &rel),
                    area: r.get(5)?,
                },
                vol: r.get(12)?,
                rel,
                n: r.get(6)?,
                bytes: r.get(7)?,
                pend: r.get(8)?,
                flagged: r.get(9)?,
                nohash: r.get(10)?,
                hashes: s.map(|s| s.split(',').map(str::to_string).collect()).unwrap_or_default(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // 나무 합치기 — 폴더마다 (해시 → 장수) 다중집합을 위 폴더들에 더한다. 중간 폴더 행이 없으면 건너뛴다
    let index: HashMap<(String, String), usize> =
        rows.iter().enumerate().map(|(i, r)| ((r.vol.clone(), r.rel.clone()), i)).collect();
    struct Tree {
        counts: HashMap<String, i64>,
        files: i64,
        bytes: i64,
        pend: i64,
        flagged: i64,
        nohash: i64,
        ids: Vec<i64>,
    }
    let mut trees: Vec<Tree> = rows
        .iter()
        .map(|r| {
            let mut counts: HashMap<String, i64> = HashMap::new();
            for h in &r.hashes {
                *counts.entry(h.clone()).or_default() += 1;
            }
            Tree { counts, files: r.n, bytes: r.bytes, pend: r.pend, flagged: r.flagged, nohash: r.nohash, ids: vec![r.info.folder_id] }
        })
        .collect();
    for i in 0..rows.len() {
        for anc in ancestors(&rows[i].rel) {
            if let Some(&j) = index.get(&(rows[i].vol.clone(), anc)) {
                let (own_counts, own) = (rows[i].hashes.clone(), (rows[i].n, rows[i].bytes, rows[i].pend, rows[i].flagged, rows[i].nohash, rows[i].info.folder_id));
                let t = &mut trees[j];
                for h in own_counts {
                    *t.counts.entry(h).or_default() += 1;
                }
                t.files += own.0;
                t.bytes += own.1;
                t.pend += own.2;
                t.flagged += own.3;
                t.nohash += own.4;
                t.ids.push(own.5);
            }
        }
    }

    // 서명 = 정렬한 (해시:장수). 해시 없는 파일이 하나라도 있으면 어디와도 같을 수 없다
    let mut disk = Disk::new();
    let mut by_sig: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, t) in trees.iter().enumerate() {
        if t.files == 0 || t.nohash > 0 {
            continue;
        }
        if !disk.online(&rows[i].vol) || !disk.dir_exists(&rows[i].vol, &rows[i].rel) {
            continue;
        }
        let mut parts: Vec<String> = t.counts.iter().map(|(h, n)| format!("{h}:{n}")).collect();
        parts.sort();
        by_sig.entry(parts.join(",")).or_default().push(i);
    }
    // 위 폴더끼리 같은 묶음이 있으면 그 아래는 안 낸다 — 얕은 것부터 보며 덮인 자리를 적는다
    let mut sets: Vec<Vec<usize>> = by_sig.into_values().filter(|v| v.len() >= 2).collect();
    sets.sort_by_key(|v| v.iter().map(|&i| rows[i].rel.matches('/').count()).min().unwrap_or(0));
    let mut covered: Vec<(String, String)> = Vec::new(); // (vol, rel) — 이 아래는 덮였다
    let is_covered = |vol: &str, rel: &str, covered: &[(String, String)]| {
        covered.iter().any(|(v, r)| v == vol && (rel == r || rel.starts_with(&format!("{r}/"))))
    };
    let mut out: Vec<FolderSet> = Vec::new();
    for members in sets {
        if members.iter().all(|&i| is_covered(&rows[i].vol, &rows[i].rel, &covered)) {
            continue;
        }
        for &i in &members {
            covered.push((rows[i].vol.clone(), rows[i].rel.clone()));
        }
        let mut fs: Vec<(FolderIn, Vec<i64>)> =
            members.iter().map(|&i| (rows[i].info.clone(), trees[i].ids.clone())).collect();
        // 정착 구역이 앞에, 그다음은 라이브러리·경로 순 — 남길 것이 맨 앞
        fs.sort_by(|(a, _), (b, _)| {
            let sa = !(a.area == 1 || a.area == 2);
            let sb = !(b.area == 1 || b.area == 2);
            sa.cmp(&sb).then(a.library_id.cmp(&b.library_id)).then(a.folder.cmp(&b.folder))
        });
        let first = members[0];
        let pending = members.iter().any(|&i| trees[i].pend > 0);
        let flagged = members.iter().map(|&i| trees[i].flagged).sum();
        let (folders, ids): (Vec<FolderIn>, Vec<Vec<i64>>) = fs.into_iter().unzip();
        out.push(FolderSet { folders, ids, files: trees[first].files, bytes: trees[first].bytes, pending, flagged });
    }
    // 경로 순 — 묶음의 이름은 정착 구역 폴더(맨 앞)의 경로
    out.sort_by(|a, b| {
        a.folders[0]
            .folder
            .cmp(&b.folders[0].folder)
            .then(a.folders[0].library_id.cmp(&b.folders[0].library_id))
    });
    out.truncate(limit);
    Ok(out)
}

/// 폴더 묶음 하나를 처리한다 — `keep` 폴더의 파일은 남김, `drops` 폴더의 파일은 제외 표시.
/// 이 폴더들 안에서만 얽힌 완전 중복 무리는 확정으로 돌려 개별 비교에 다시 안 나오게.
pub fn apply_set(tx: &Transaction, keep: i64, drops: &[i64]) -> rusqlite::Result<ApplyAll> {
    apply_trees(tx, &[keep], drops)
}

/// 폴더 «나무» 둘 — 남길 쪽 폴더들(`keep`)과 제외할 쪽 폴더들(`drop`). 두 폴더 비교가 하위
/// 폴더까지 통째로 짝지을 때 쓴다. 제외는 **남길 쪽 나무 어딘가에 같은 내용이 지금 있는**
/// 파일에만 붙는다 — 남길 쪽에 없는 사진이 지워지는 일은 없다 (리뷰 C12).
///
/// 이미 «남김»(1)인 파일은 내리지 않는다 — 남김은 결정이다. 남김이 붙은 폴더는 비교 화면이
/// 애초에 제외 후보로 올리지 않는다(`kept_a`/`kept_b`). 지우고 싶으면 먼저 «표시 취소»
pub fn apply_trees(tx: &Transaction, keep: &[i64], drop: &[i64]) -> rusqlite::Result<ApplyAll> {
    if keep.is_empty() || drop.is_empty() || keep.iter().any(|k| drop.contains(k)) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let keep_list = keep.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    let drop_list = drop.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    let kept = tx.execute(
        &format!("UPDATE files SET culling_flag = 1 WHERE folder_id IN ({keep_list}) AND trashed_at IS NULL"),
        [],
    )?;
    let rejected = tx.execute(
        &format!(
            "UPDATE files SET culling_flag = 2
             WHERE folder_id IN ({drop_list}) AND trashed_at IS NULL AND culling_flag <> 1
               AND full_hash IS NOT NULL
               AND full_hash IN (SELECT k.full_hash FROM files k
                                 WHERE k.folder_id IN ({keep_list}) AND k.trashed_at IS NULL AND k.full_hash IS NOT NULL)"
        ),
        [],
    )?;
    let list = format!("{keep_list},{drop_list}");
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

/// 폴더 비교로 붙인 표시를 되돌린다 — 이 폴더들 안의 «남김/제외»를 미판정으로, 닫았던 완전 중복
/// 무리는 다시 연다. 휴지통에 이미 간 것은 여기서 안 다룬다(휴지통 화면의 되돌리기).
/// (표시를 되돌린 장수, 다시 연 무리 수)
pub fn unapply_folders(tx: &Transaction, folder_ids: &[i64]) -> rusqlite::Result<(usize, usize)> {
    if folder_ids.is_empty() {
        return Ok((0, 0));
    }
    let list = folder_ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    let files = tx.execute(
        &format!(
            "UPDATE files SET culling_flag = 0
             WHERE folder_id IN ({list}) AND trashed_at IS NULL AND culling_flag IN (1, 2)"
        ),
        [],
    )?;
    let groups = tx.execute(
        &format!(
            "UPDATE groups SET state = 0 WHERE kind = 0 AND state = 1
               AND id IN (SELECT m.group_id FROM group_members m JOIN files f ON f.id = m.file_id
                          WHERE f.folder_id IN ({list}))"
        ),
        [],
    )?;
    Ok((files, groups))
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

pub fn apply_pairs(tx: &Transaction, pairs: &[(Vec<i64>, Vec<i64>)]) -> rusqlite::Result<PairsApplied> {
    let mut out = PairsApplied::default();
    for (keep, drop) in pairs {
        match apply_trees(tx, keep, drop) {
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
    /// 이미 제외 표시된 파일 수 — 한쪽이 전부면 그 짝은 «처리됨»
    pub flagged_a: i64,
    pub flagged_b: i64,
    /// «남김»이 붙은 파일 수 — 남김은 결정이라 그쪽은 제외 후보가 아니다
    pub kept_a: i64,
    pub kept_b: i64,
    /// B 쪽 사진이 전부 A 쪽(하위 폴더 포함)에 있다 — B 를 지워도 잃는 것이 없다
    pub b_in_a: bool,
    /// A 쪽 사진이 전부 B 쪽에 있다
    pub a_in_b: bool,
    /// 이 줄이 대표하는 폴더 행들(하위 폴더 포함) — 표시·휴지통으로가 이 목록에 건다
    pub a_ids: Vec<i64>,
    pub b_ids: Vec<i64>,
}

struct Agg {
    info: FolderIn,
    /// 뿌리 기준 상대경로 — 이름이 같은 폴더를 찾는 열쇠
    sub: String,
    files: i64,
    bytes: i64,
    /// culling_flag = 2 인 파일 수
    flagged: i64,
    /// culling_flag = 1 인 파일 수
    kept: i64,
    hashes: Vec<String>,
    all_hashed: bool,
    has_children: bool,
}

/// 뿌리는 (볼륨, 볼륨 기준 경로)다 — «연도별»처럼 사진이 바로 아래 없는 폴더는 `folders`
/// 행이 없어서 id 로는 가리킬 수 없다 (실측: 후보1번/연도별을 골랐는데 «없는 폴더»).
fn folders_under(c: &Connection, vol: &str, rel: &str) -> rusqlite::Result<(Vec<Agg>, usize)> {
    let esc = crate::db::query::escape_like(rel);
    let mut st = c.prepare(
        "SELECT fo.id, fo.rel_path, fo.area, l.id, l.name, l.rel_path, f.full_hash, f.size,
                f.culling_flag = 2, f.culling_flag = 1
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
            r.get::<_, Option<bool>>(9)?.unwrap_or(false),
        ))
    })?;
    for row in rows {
        let (id, fo_rel, area, lib_id, lib_name, lib_rel, hash, size, flagged, kept) = row?;
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
                kept: 0,
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
            cur.kept += kept as i64;
            match hash {
                Some(h) => cur.hashes.push(h),
                None => cur.all_hashed = false,
            }
        }
    }
    // 사진이 바로 아래 없는 폴더 행(휴지통 파일만 가리키거나 빈 것)은 견줄 것이 없다 — 먼저 뺀다.
    // 그다음 디스크에서 사라진 폴더(Finder 에서 지운 것)를 뺀다 — DB 행만 남아 «없는 폴더»를 읽지 않게.
    // 실측(2026-08-30): 다시 스캔 뒤에도 «없는 폴더 N개»가 떴는데, 전부 사진이 0장인 옛 폴더 행이었다
    out.retain(|a| a.files > 0);
    let mut disk = Disk::new();
    let before = out.len();
    out.retain(|a| {
        let rel_full = if a.sub.is_empty() { rel.to_string() } else if rel.is_empty() { a.sub.clone() } else { format!("{rel}/{}", a.sub) };
        disk.dir_exists(vol, &rel_full)
    });
    let missing = before - out.len();
    // 하위 폴더 유무는 이 결과 안에서 안다 — 뿌리 아래 폴더는 전부 여기 들어 있다
    let parents: HashSet<String> = out.iter().flat_map(|a| ancestors(&a.sub)).collect();
    for a in &mut out {
        a.has_children = parents.contains(&a.sub);
    }
    Ok((out, missing))
}

/// 두 폴더 비교의 결과 — 줄들과, 디스크에 없어 뺀 폴더 수
#[derive(Debug, Clone, Serialize)]
pub struct Compared {
    pub rows: Vec<PairRow>,
    pub missing: usize,
}

/// 두 뿌리가 서로를 품는가 — 같은 폴더가 양쪽 목록에 들어 제 짝이 되는 길을 막는다
pub fn roots_overlap((a_vol, a_rel): (&str, &str), (b_vol, b_rel): (&str, &str)) -> bool {
    if a_vol != b_vol {
        return false;
    }
    let under = |root: &str, p: &str| root.is_empty() || p == root || p.starts_with(&format!("{root}/"));
    under(a_rel, b_rel) || under(b_rel, a_rel)
}

/// 폴더 짝 «보기» — 두 나무의 사진을 나란히. 내용이 같은 사진은 서로 `twin` 으로 잇는다
#[derive(Debug, Clone, Serialize)]
pub struct PairPhoto {
    pub file_id: i64,
    pub name: String,
    /// 나무 뿌리 기준 상대 폴더(하위 폴더면 그 이름) — 빈 문자열이면 바로 아래
    pub sub: String,
    pub size: i64,
    pub taken_at: i64,
    pub culling_flag: i32,
    pub library_id: i64,
    pub thumb: Option<String>,
    /// 반대쪽에 있는 같은 내용의 사진 — 없으면 None
    pub twin: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairPhotos {
    pub a: Vec<PairPhoto>,
    pub b: Vec<PairPhoto>,
}

fn photos_in(c: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<(PairPhoto, Option<String>, String)>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let list = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    let mut st = c.prepare(&format!(
        "SELECT f.id, f.name, fo.rel_path, f.size, f.taken_at, f.culling_flag, fo.library_id, t.rel_path, f.full_hash
         FROM files f JOIN folders fo ON fo.id = f.folder_id
         LEFT JOIN thumbs t ON t.file_id = f.id AND t.state = 1
         WHERE f.folder_id IN ({list}) AND f.trashed_at IS NULL
         ORDER BY fo.rel_path, f.name"
    ))?;
    let rows = st.query_map([], |r| {
        Ok((
            PairPhoto {
                file_id: r.get(0)?,
                name: r.get(1)?,
                sub: String::new(),
                size: r.get(3)?,
                taken_at: r.get::<_, Option<i64>>(4)?.unwrap_or(0),
                culling_flag: r.get(5)?,
                library_id: r.get(6)?,
                thumb: r.get(7)?,
                twin: None,
            },
            r.get::<_, Option<String>>(8)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    rows.collect()
}

/// 두 나무의 사진 — 같은 내용끼리 1:1 로 짝짓는다(장수까지). 폴더 경로는 뿌리 아래만 보인다
pub fn pair_photos(c: &Connection, a_ids: &[i64], b_ids: &[i64]) -> rusqlite::Result<PairPhotos> {
    let mut a = photos_in(c, a_ids)?;
    let mut b = photos_in(c, b_ids)?;
    let strip = |rows: &mut Vec<(PairPhoto, Option<String>, String)>| {
        // 뿌리 = 가장 짧은 폴더 경로
        let root = rows.iter().map(|r| r.2.clone()).min_by_key(|p| p.len()).unwrap_or_default();
        for r in rows.iter_mut() {
            r.0.sub = r.2.strip_prefix(&root).map(|s| s.trim_start_matches('/').to_string()).unwrap_or_default();
        }
    };
    strip(&mut a);
    strip(&mut b);
    let mut by_hash: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, (_, h, _)) in b.iter().enumerate() {
        if let Some(h) = h {
            by_hash.entry(h.clone()).or_default().push(i);
        }
    }
    for (pa, h, _) in a.iter_mut() {
        let Some(h) = h else { continue };
        if let Some(list) = by_hash.get_mut(h) {
            if let Some(i) = list.pop() {
                pa.twin = Some(b[i].0.file_id);
                b[i].0.twin = Some(pa.file_id);
            }
        }
    }
    Ok(PairPhotos { a: a.into_iter().map(|r| r.0).collect(), b: b.into_iter().map(|r| r.0).collect() })
}

/// 폴더 나무 하나의 «내용» — 하위 폴더까지 합친 해시 다중집합
struct Tree {
    /// 이 나무에 든 폴더들의 순번(`Agg` 목록 기준)
    members: Vec<usize>,
    files: i64,
    bytes: i64,
    flagged: i64,
    kept: i64,
    counts: HashMap<String, i64>,
    all_hashed: bool,
}

fn tree_of(aggs: &[Agg], root: usize) -> Tree {
    let sub = &aggs[root].sub;
    let members: Vec<usize> = (0..aggs.len())
        .filter(|&i| i == root || sub.is_empty() || aggs[i].sub.starts_with(&format!("{sub}/")))
        .collect();
    let mut t = Tree { members: Vec::new(), files: 0, bytes: 0, flagged: 0, kept: 0, counts: HashMap::new(), all_hashed: true };
    for &i in &members {
        let g = &aggs[i];
        t.files += g.files;
        t.bytes += g.bytes;
        t.flagged += g.flagged;
        t.kept += g.kept;
        t.all_hashed &= g.all_hashed;
        for h in &g.hashes {
            *t.counts.entry(h.clone()).or_default() += 1;
        }
    }
    t.members = members;
    t
}

/// `inner` 의 파일이 전부 `outer` 에 있나 (같은 내용은 장수까지)
fn contained(inner: &Tree, outer: &Tree) -> bool {
    inner.all_hashed
        && inner.files > 0
        && inner.counts.iter().all(|(h, n)| outer.counts.get(h).copied().unwrap_or(0) >= *n)
}

/// 두 폴더(와 그 아래)를 견준다 — «후보1번/연도별»과 «후보2번»처럼.
///
/// 폴더는 **나무째** 본다: 하위 폴더까지 합친 내용으로 «B 쪽이 A 에 다 있다 / A 쪽이 B 에 다
/// 있다 / 둘 다(똑같다)»를 가린다. 한쪽이 다른 쪽에 다 들어 있으면 그 나무는 한 줄로 끝나고
/// 하위 폴더는 따로 안 나온다 — 실측: `2011-04-24(주말농장-2번째)` 는 후보1번에만 «블로그»
/// 하위 폴더가 더 있어 «똑같음»이 아니었지만 후보2번 쪽 191장은 전부 후보1번에 있었다.
/// 어느 쪽도 다른 쪽을 품지 못하면 «부분»으로 적고 하위 폴더는 저마다 제 줄로 내려간다.
pub fn compare_two(
    c: &Connection,
    (a_vol, a_rel): (&str, &str),
    (b_vol, b_rel): (&str, &str),
) -> rusqlite::Result<Compared> {
    if roots_overlap((a_vol, a_rel), (b_vol, b_rel)) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let (a, miss_a) = folders_under(c, a_vol, a_rel)?;
    let (b, miss_b) = folders_under(c, b_vol, b_rel)?;
    let mut b_by_sub: HashMap<&str, usize> = HashMap::new();
    for (i, g) in b.iter().enumerate() {
        b_by_sub.entry(g.sub.as_str()).or_insert(i);
    }
    let mut used_a = vec![false; a.len()];
    let mut used_b = vec![false; b.len()];
    // (정렬 열쇠 = 뿌리 기준 경로, 줄)
    let mut out: Vec<(String, PairRow)> = Vec::new();
    // 경로 순으로 — 위 폴더가 먼저 나와 나무째 짝지어지면 아래는 건너뛴다
    let mut order: Vec<usize> = (0..a.len()).collect();
    order.sort_by(|&x, &y| a[x].sub.cmp(&a[y].sub));
    for ia in order {
        if used_a[ia] {
            continue;
        }
        let ga = &a[ia];
        let Some(&ib) = b_by_sub.get(ga.sub.as_str()) else {
            // 이름이 같은 짝이 없다 — 사진이 있으면 «A 에만»
            if ga.files > 0 {
                used_a[ia] = true;
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
                    kept_a: ga.kept,
                    kept_b: 0,
                    b_in_a: false,
                    a_in_b: false,
                    a_ids: vec![ga.info.folder_id],
                    b_ids: Vec::new(),
                }));
            }
            continue;
        };
        if used_b[ib] {
            continue;
        }
        let ta = tree_of(&a, ia);
        let tb = tree_of(&b, ib);
        let b_in_a = contained(&tb, &ta);
        let a_in_b = contained(&ta, &tb);
        if b_in_a || a_in_b {
            // 나무째 한 줄 — 하위 폴더는 이 줄이 대표한다
            for &i in &ta.members {
                used_a[i] = true;
            }
            for &i in &tb.members {
                used_b[i] = true;
            }
            let common = ta.files.min(tb.files);
            out.push((ga.sub.clone(), PairRow {
                a: Some(ga.info.clone()),
                b: Some(b[ib].info.clone()),
                files_a: ta.files,
                files_b: tb.files,
                same: b_in_a && a_in_b,
                common,
                // 지울 수 있는 쪽의 용량 — 둘 다면 작은 쪽
                bytes: if b_in_a && a_in_b { ta.bytes.min(tb.bytes) } else if b_in_a { tb.bytes } else { ta.bytes },
                flagged_a: ta.flagged,
                flagged_b: tb.flagged,
                kept_a: ta.kept,
                kept_b: tb.kept,
                b_in_a,
                a_in_b,
                a_ids: ta.members.iter().map(|&i| a[i].info.folder_id).collect(),
                b_ids: tb.members.iter().map(|&i| b[i].info.folder_id).collect(),
            }));
            continue;
        }
        // 부분 — 바로 아래 파일끼리 겹치는 수. 하위 폴더는 저마다 제 줄로
        used_a[ia] = true;
        used_b[ib] = true;
        let gb = &b[ib];
        if ga.files == 0 && gb.files == 0 {
            continue;
        }
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
            kept_a: ga.kept,
            kept_b: gb.kept,
            b_in_a: false,
            a_in_b: false,
            a_ids: vec![ga.info.folder_id],
            b_ids: vec![gb.info.folder_id],
        }));
    }
    for (i, gb) in b.iter().enumerate() {
        if used_b[i] || gb.files == 0 {
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
            kept_a: 0,
            kept_b: gb.kept,
            b_in_a: false,
            a_in_b: false,
            a_ids: Vec::new(),
            b_ids: vec![gb.info.folder_id],
        }));
    }
    // 경로 순 — Finder 를 나란히 놓은 것처럼 읽힌다 (사용자 지적: «오름차순도 내림차순도 아니다»)
    out.sort_by(|x, y| x.0.cmp(&y.0));
    Ok(Compared { rows: out.into_iter().map(|(_, r)| r).collect(), missing: miss_a + miss_b })
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

    /// 하위 폴더까지 똑같은 두 나무는 위 폴더 한 줄로만 나온다
    #[test]
    fn identical_trees_are_reported_once_at_the_top() {
        let dir = tempfile::tempdir().unwrap();
        for root in ["P", "Q"] {
            for (sub, body) in [("2016", "AAAA"), ("2016/x", "BBBB"), ("2016/y", "CCCC")] {
                let p = dir.path().join(root).join(sub);
                std::fs::create_dir_all(&p).unwrap();
                std::fs::write(p.join("a.jpg"), body.as_bytes().repeat(200)).unwrap();
            }
        }
        let db = Db::open(dir.path().join("t.db")).unwrap();
        scan_test(&db, dir.path(), 0, |_| {}).unwrap();
        dedup::scan(&db, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        let sets = db.read(|c| identical_sets(c, 100)).unwrap();
        assert_eq!(sets.len(), 1, "P/2016 ≡ Q/2016 한 줄뿐 — x·y 는 따로 안 나온다: {sets:?}");
        assert_eq!(sets[0].files, 3, "나무째 3장");
        assert_eq!(sets[0].ids[0].len(), 3, "폴더 행 셋(2016, x, y)");
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
        let rows = db.read(|c| compare_two(c, (&vol, &x), (&vol, &y))).unwrap().rows;
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
    fn unapply_clears_marks_and_reopens_groups() {
        let (_d, db) = setup();
        let sets = db.read(|c| identical_sets(c, 100)).unwrap();
        let s = &sets[0];
        let keep = s.folders[0].folder_id;
        let drops: Vec<i64> = s.folders[1..].iter().map(|f| f.folder_id).collect();
        db.transaction(|tx| apply_set(tx, keep, &drops)).unwrap();
        let marked: i64 = db
            .read(|c| c.query_row("SELECT COUNT(*) FROM files WHERE culling_flag <> 0", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(marked, 6);
        let all: Vec<i64> = std::iter::once(keep).chain(drops.iter().copied()).collect();
        let (files, groups) = db.transaction(|tx| unapply_folders(tx, &all)).unwrap();
        assert_eq!((files, groups), (6, 1), "여섯 장 미판정으로, 닫았던 무리 하나 다시 연다");
        let marked: i64 = db
            .read(|c| c.query_row("SELECT COUNT(*) FROM files WHERE culling_flag <> 0", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(marked, 0);
        let again = db.read(|c| identical_sets(c, 100)).unwrap();
        assert!(again[0].pending, "묶음이 다시 미결이 된다");
        assert_eq!(db.transaction(|tx| unapply_folders(tx, &[])).unwrap(), (0, 0));
    }

    /// «남김»은 결정 — 앞선 짝에서 붙은 남김이 있는 폴더는 다시 제외되지 않고, 비교 화면도
    /// 그쪽을 제외 후보로 올리지 않는다(kept_a/kept_b)
    #[test]
    fn a_kept_tree_is_not_demoted_and_not_offered() {
        let (_d, db) = setup();
        let sets = db.read(|c| identical_sets(c, 100)).unwrap();
        let s = &sets[0];
        let (c_id, a_id, b_id) = (s.folders[0].folder_id, s.folders[1].folder_id, s.folders[2].folder_id);
        // 1) a 를 남기고 b 를 제외 → a 의 두 장에 «남김»
        db.transaction(|tx| apply_trees(tx, &[a_id], &[b_id])).unwrap();
        let kept: i64 = db
            .read(|c| c.query_row("SELECT COUNT(*) FROM files WHERE folder_id = ?1 AND culling_flag = 1", [a_id], |r| r.get(0)))
            .unwrap();
        assert_eq!(kept, 2);
        // 2) c 를 남기고 a 를 제외하려 해도 a 의 «남김»은 내려가지 않는다
        let r = db.transaction(|tx| apply_trees(tx, &[c_id], &[a_id])).unwrap();
        assert_eq!(r.rejected, 0, "{r:?}");
        // 비교 화면도 a 쪽을 후보로 안 올린다 — kept_a 가 보인다
        let (vol, a_rel, c_rel): (String, String, String) = db
            .read(|c| {
                let vol: String = c.query_row("SELECT volume_uuid FROM folders WHERE id = ?1", [a_id], |r| r.get(0))?;
                let a_rel: String = c.query_row("SELECT rel_path FROM folders WHERE id = ?1", [a_id], |r| r.get(0))?;
                let c_rel: String = c.query_row("SELECT rel_path FROM folders WHERE id = ?1", [c_id], |r| r.get(0))?;
                Ok((vol, a_rel, c_rel))
            })
            .unwrap();
        let rows = db.read(|c| compare_two(c, (&vol, &a_rel), (&vol, &c_rel))).unwrap().rows;
        assert_eq!(rows.len(), 1);
        // 2)에서 c 가 남는 쪽이 됐으니 c 에도 «남김» — 양쪽 다 남김이면 어느 쪽도 후보가 아니다
        assert!(rows[0].same && rows[0].kept_a == 2 && rows[0].kept_b == 2, "{:?}", rows[0]);
    }

    #[test]
    fn pair_photos_link_identical_photos_one_to_one() {
        let (_d, db) = setup();
        let ids = |name: &str| -> i64 {
            db.read(|c| c.query_row("SELECT id FROM folders WHERE rel_path LIKE ?1", [format!("%{name}")], |r| r.get(0)))
                .unwrap()
        };
        let (a, d) = (ids("a"), ids("d"));
        let p = db.read(|c| pair_photos(c, &[a], &[d])).unwrap();
        assert_eq!((p.a.len(), p.b.len()), (2, 2));
        // a/1.jpg(x) ↔ d/1.jpg(x) 만 같다; a/2.jpg(y) 와 d/3.jpg(z) 는 짝이 없다
        let a1 = p.a.iter().find(|x| x.name == "1.jpg").unwrap();
        let d1 = p.b.iter().find(|x| x.name == "1.jpg").unwrap();
        assert_eq!(a1.twin, Some(d1.file_id));
        assert_eq!(d1.twin, Some(a1.file_id));
        assert!(p.a.iter().find(|x| x.name == "2.jpg").unwrap().twin.is_none());
        assert!(p.b.iter().find(|x| x.name == "3.jpg").unwrap().twin.is_none());
        assert!(p.a.iter().all(|x| x.sub.is_empty()), "뿌리 바로 아래면 sub 는 비어 있다");
    }

    #[test]
    fn pairs_apply_counts_failures_without_aborting_the_batch() {
        let (_d, db) = setup();
        let sets = db.read(|c| identical_sets(c, 100)).unwrap();
        let s = &sets[0];
        let (keep, drop) = (s.folders[0].folder_id, s.folders[1].folder_id);
        let r = db
            .transaction(|tx| apply_pairs(tx, &[(vec![keep], vec![keep]), (vec![keep], vec![drop])]))
            .unwrap();
        assert_eq!((r.applied, r.failed), (1, 1), "{r:?}");
        assert!(r.first_error.is_some());
        assert_eq!(r.rejected, 2);
    }

    /// 후보1번에만 «블로그» 하위 폴더가 더 있는 경우 — 후보2번 쪽은 전부 후보1번에 있으니
    /// «B 쪽이 A 에 다 있음»으로 한 줄에 잡히고, 하위 폴더는 따로 안 나온다
    #[test]
    fn a_tree_that_contains_the_other_side_is_paired_whole() {
        let (dir, db) = setup();
        let blog = dir.path().join("a/블로그");
        std::fs::create_dir_all(&blog).unwrap();
        std::fs::write(blog.join("b1.jpg"), b"BLOG ".repeat(300)).unwrap();
        scan_test(&db, dir.path(), 0, |_| {}).unwrap();
        dedup::scan(&db, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        let root_of = |name: &str| -> (String, String, i64) {
            db.read(|c| {
                c.query_row(
                    "SELECT volume_uuid, rel_path, id FROM folders WHERE rel_path LIKE ?1",
                    [format!("%{name}")],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
            })
            .unwrap()
        };
        let (a, b) = (root_of("a"), root_of("b"));
        let rows = db.read(|c| compare_two(c, (&a.0, &a.1), (&b.0, &b.1))).unwrap().rows;
        assert_eq!(rows.len(), 1, "하위 폴더 «블로그»는 제 줄로 안 나온다: {rows:?}");
        let r = &rows[0];
        assert!(r.b_in_a && !r.a_in_b && !r.same, "{r:?}");
        assert_eq!((r.files_a, r.files_b), (3, 2), "A 는 나무째 3장");
        assert_eq!(r.a_ids.len(), 2, "A 쪽 폴더 행 둘(a, a/블로그)");
        assert_eq!(r.b_ids, vec![b.2]);
        // 거꾸로 견줘도 같은 판정 — 이번엔 «A 쪽이 B 에 다 있음»
        let rows = db.read(|c| compare_two(c, (&b.0, &b.1), (&a.0, &a.1))).unwrap().rows;
        assert!(rows[0].a_in_b && !rows[0].b_in_a);
        // 나무째 표시 — B(2장) 제외, A 쪽은 남김. «블로그»의 한 장은 B 에 없으니 제외 대상이 아니다
        let out = db.transaction(|tx| apply_trees(tx, &r.a_ids, &r.b_ids)).unwrap();
        assert_eq!((out.kept, out.rejected), (3, 2), "{out:?}");
        assert!(db.transaction(|tx| apply_trees(tx, &r.a_ids, &r.a_ids)).is_err(), "겹치는 나무는 거절");
    }

    /// Finder 에서 지운 폴더의 행이 남아 있어도 «없는 폴더»를 읽지 않는다
    #[test]
    fn folders_deleted_on_disk_are_left_out_and_counted() {
        let (dir, db) = setup();
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
        let (a, b) = (root_of("a"), root_of("b"));
        std::fs::remove_dir_all(dir.path().join("b")).unwrap(); // DB 행은 그대로
        let r = db.read(|c| compare_two(c, (&a.0, &a.1), (&b.0, &b.1))).unwrap();
        assert_eq!(r.missing, 1, "{r:?}");
        assert!(r.rows.iter().all(|row| row.b.is_none()), "사라진 B 는 짝이 되지 않는다: {:?}", r.rows);
        let sets = db.read(|c| identical_sets(c, 100)).unwrap();
        assert!(sets.iter().all(|s| s.folders.iter().all(|f| f.folder != "b")), "폴더 비교도 뺀다: {sets:?}");
    }

    /// 실제 DB 로 — `ACUT_LIVE_DB=<acut-v2.db> cargo test --lib real_db_compare -- --ignored --nocapture`
    /// (앱이 열어 둔 DB 도 읽기 전용으로 열린다)
    #[test]
    #[ignore = "실제 DB"]
    fn real_db_compare_missing() {
        let Ok(path) = std::env::var("ACUT_LIVE_DB") else { return };
        let c = Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .unwrap();
        let vol: String = c
            .query_row("SELECT volume_uuid FROM libraries WHERE name = '통합전후보'", [], |r| r.get(0))
            .unwrap();
        let a_root = std::env::var("ACUT_A").unwrap_or_else(|_| "통합전후보/후보1번/연도별".into());
        let b_root = std::env::var("ACUT_B").unwrap_or_else(|_| "통합전후보/후보2번".into());
        let r = compare_two(&c, (&vol, &a_root), (&vol, &b_root)).unwrap();
        eprintln!("A={a_root} B={b_root} rows {} missing {}", r.rows.len(), r.missing);
        // B 쪽을 지워도 되는데 아직 표시가 안 된 짝 — 왜 표시가 안 붙나
        let pending: Vec<&PairRow> = r.rows.iter().filter(|x| x.b_in_a && x.b.is_some() && x.flagged_b < x.files_b).collect();
        eprintln!("pending b_in_a {}", pending.len());
        let pend_a: Vec<&PairRow> = r.rows.iter().filter(|x| x.a_in_b && x.a.is_some() && x.flagged_a < x.files_a).collect();
        eprintln!("pending a_in_b {}", pend_a.len());
        for x in pend_a.iter().take(8) {
            let ids = x.a_ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
            let flags: String = c
                .prepare(&format!("SELECT culling_flag, COUNT(*) FROM files WHERE folder_id IN ({ids}) AND trashed_at IS NULL GROUP BY culling_flag"))
                .unwrap()
                .query_map([], |r| Ok(format!("{}×{}", r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
                .unwrap()
                .map(|v| v.unwrap())
                .collect::<Vec<_>>()
                .join(" ");
            eprintln!("  A {} | files a/b {}/{} flagged_a {} | A 판정: {flags}", x.a.as_ref().unwrap().folder, x.files_a, x.files_b, x.flagged_a);
        }
        for x in pending.iter().take(5) {
            eprintln!(
                "  {} | files a/b {}/{} flagged {}/{} a_ids {} b_ids {} same {}",
                x.b.as_ref().unwrap().folder, x.files_a, x.files_b, x.flagged_a, x.flagged_b, x.a_ids.len(), x.b_ids.len(), x.same
            );
        }
        if std::env::var_os("ACUT_LIVE_WRITE").is_some() {
            if let Some(x) = pending.first() {
                let mut c2 = Connection::open(&path).unwrap();
                let tx = c2.transaction().unwrap();
                let out = apply_trees(&tx, &x.a_ids, &x.b_ids);
                eprintln!("apply_trees → {out:?}");
                let n: i64 = tx
                    .query_row(
                        &format!(
                            "SELECT COUNT(*) FROM files WHERE folder_id IN ({}) AND trashed_at IS NULL AND full_hash IN (SELECT full_hash FROM files WHERE folder_id IN ({}) AND trashed_at IS NULL)",
                            x.b_ids.iter().map(i64::to_string).collect::<Vec<_>>().join(","),
                            x.a_ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
                        ),
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                eprintln!("B 파일 중 A 나무에 같은 해시가 있는 것: {n} / {}", x.files_b);
                drop(tx); // 되돌린다
            }
        }
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
        let rows = db.read(|c| compare_two(c, (&a.0, &a.1), (&b.0, &b.1))).unwrap().rows;
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].same && rows[0].b_in_a && rows[0].a_in_b && rows[0].common == 2);
        // a 와 d 는 한 장만 겹친다 — 뿌리 이름은 다르지만 뿌리끼리는 sub 가 같다("")
        let rows = db.read(|c| compare_two(c, (&a.0, &a.1), (&d.0, &d.1))).unwrap().rows;
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(!rows[0].same);
        assert_eq!((rows[0].common, rows[0].files_a, rows[0].files_b), (1, 2, 2));
    }
}
