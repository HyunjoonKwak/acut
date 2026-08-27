//! 사이드바 폴더 트리.
//!
//! 스캐너는 **파일이 든 폴더만** 기록한다. `연도별/2001/정동진`에만 사진이
//! 있으면 `연도별`과 `연도별/2001`은 DB에 없다. 그대로 늘어놓으면 3,161줄이
//! 평평하게 쏟아져 읽을 수도, 접을 수도 없다.
//!
//! 그래서 잎(leaf) 경로들로부터 중간 마디를 만들어 내고, 장수는 아래에서
//! 위로 합친다.

use std::collections::BTreeMap;

/// 트리 한 줄. 프론트는 `depth`만큼 들여쓰고 `path`로 접고 편다.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Node {
    /// DB의 폴더 id. 중간 마디는 실제 폴더 행이 없어 None이다.
    pub id: Option<i64>,
    /// 라이브러리 루트 기준 경로. 접기·필터의 열쇠다.
    pub path: String,
    /// 볼륨 기준 경로. 필터를 보낼 때 쓴다.
    pub rel_path: String,
    pub name: String,
    pub depth: usize,
    /// 자기 자신과 모든 하위 폴더의 사진 수
    pub file_count: i64,
    pub has_children: bool,
}

/// DB에서 읽은 잎 하나.
pub struct Leaf {
    pub id: i64,
    /// 라이브러리 루트 기준
    pub path: String,
    /// 볼륨 기준
    pub rel_path: String,
    pub file_count: i64,
}

/// 잎들로부터 전체 트리를 만든다. 결과는 경로순(부모가 자식보다 먼저)이다.
///
/// 라이브러리 루트 바로 아래에 있는 사진은 `path`가 빈 문자열인 잎으로 온다.
/// 그건 마디를 만들지 않고 건너뛴다 — 사이드바의 "전체"가 그 역할을 한다.
pub fn build(leaves: Vec<Leaf>, library_rel: &str) -> Vec<Node> {
    // 경로 → (id, 자기 폴더의 장수). BTreeMap이라 순회하면 경로순이다.
    let mut own: BTreeMap<String, (Option<i64>, i64)> = BTreeMap::new();

    for l in &leaves {
        if l.path.is_empty() {
            continue; // 루트 자신
        }
        own.entry(l.path.clone())
            .and_modify(|e| {
                e.0 = Some(l.id);
                e.1 += l.file_count;
            })
            .or_insert((Some(l.id), l.file_count));

        // 조상 마디를 만들어 둔다 (아직 장수는 0)
        let mut acc = String::new();
        let segs: Vec<&str> = l.path.split('/').collect();
        for i in 0..segs.len().saturating_sub(1) {
            if i > 0 {
                acc.push('/');
            }
            acc.push_str(segs[i]);
            own.entry(acc.clone()).or_insert((None, 0));
        }
    }

    // 장수를 아래에서 위로 합친다. 경로 문자열만으로 조상을 알 수 있다.
    let mut total: BTreeMap<String, i64> = own.keys().map(|k| (k.clone(), 0)).collect();
    for (path, (_, n)) in &own {
        if *n == 0 {
            continue;
        }
        let mut acc = String::new();
        for (i, seg) in path.split('/').enumerate() {
            if i > 0 {
                acc.push('/');
            }
            acc.push_str(seg);
            if let Some(t) = total.get_mut(&acc) {
                *t += n;
            }
        }
    }

    let keys: Vec<String> = own.keys().cloned().collect();
    keys.iter()
        .map(|path| {
            let (id, _) = own[path];
            let depth = path.matches('/').count();
            let name = path.rsplit('/').next().unwrap_or(path).to_string();
            let prefix = format!("{path}/");
            Node {
                id,
                rel_path: if library_rel.is_empty() {
                    path.clone()
                } else {
                    format!("{library_rel}/{path}")
                },
                has_children: keys.iter().any(|k| k.starts_with(&prefix)),
                file_count: total[path],
                path: path.clone(),
                name,
                depth,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: i64, path: &str, n: i64) -> Leaf {
        Leaf {
            id,
            path: path.to_string(),
            rel_path: path.to_string(),
            file_count: n,
        }
    }

    #[test]
    fn middle_folders_are_invented() {
        // 스캐너는 잎만 준다. 「연도별」과 「연도별/2001」은 DB에 없다.
        let t = build(
            vec![leaf(1, "연도별/2001/정동진", 10), leaf(2, "연도별/2002/제주", 5)],
            "",
        );
        let paths: Vec<&str> = t.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["연도별", "연도별/2001", "연도별/2001/정동진", "연도별/2002", "연도별/2002/제주"]
        );
        // 만들어 낸 마디는 DB id가 없다
        assert_eq!(t[0].id, None);
        assert_eq!(t[2].id, Some(1));
    }

    #[test]
    fn counts_roll_up() {
        let t = build(
            vec![leaf(1, "연도별/2001/정동진", 10), leaf(2, "연도별/2002/제주", 5)],
            "",
        );
        let by = |p: &str| t.iter().find(|n| n.path == p).unwrap().file_count;
        assert_eq!(by("연도별"), 15, "아래 것을 다 더한다");
        assert_eq!(by("연도별/2001"), 10);
        assert_eq!(by("연도별/2001/정동진"), 10);
    }

    #[test]
    fn depth_starts_at_zero_under_the_library_root() {
        let t = build(vec![leaf(1, "연도별/2001/정동진", 1)], "MERGE/사진통합작업");
        assert_eq!(t[0].depth, 0, "라이브러리 바로 아래가 0단계");
        assert_eq!(t[2].depth, 2);
        // 볼륨 기준 경로는 라이브러리 앞부분을 되붙인다
        assert_eq!(t[2].rel_path, "MERGE/사진통합작업/연도별/2001/정동진");
    }

    #[test]
    fn leaves_know_they_have_no_children() {
        let t = build(vec![leaf(1, "a/b", 1), leaf(2, "a/c/d", 1)], "");
        let by = |p: &str| t.iter().find(|n| n.path == p).unwrap();
        assert!(by("a").has_children);
        assert!(!by("a/b").has_children, "형제가 있어도 자식은 아니다");
        assert!(by("a/c").has_children);
        assert!(!by("a/c/d").has_children);
    }

    #[test]
    fn a_folder_that_is_both_leaf_and_parent() {
        // 「연도별」에 사진이 직접 있으면서 하위 폴더도 있는 경우
        let t = build(vec![leaf(1, "연도별", 3), leaf(2, "연도별/2001", 7)], "");
        let by = |p: &str| t.iter().find(|n| n.path == p).unwrap();
        assert_eq!(by("연도별").id, Some(1), "실제 폴더 행이 있다");
        assert!(by("연도별").has_children);
        assert_eq!(by("연도별").file_count, 10, "자기 것 + 아래 것");
    }

    #[test]
    fn photos_at_the_library_root_make_no_node() {
        // 루트 직속 사진은 사이드바의 "전체"가 맡는다
        let t = build(vec![leaf(1, "", 5), leaf(2, "a", 1)], "");
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].path, "a");
    }

    #[test]
    fn empty_input() {
        assert!(build(vec![], "").is_empty());
    }
}
