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
    /// DB의 폴더 id. 중간 마디와 라이브러리 마디는 실제 폴더 행이 없어 None이다.
    pub id: Option<i64>,
    /// 이 줄이 속한 라이브러리. 폴더를 누르면 그 라이브러리로 함께 옮겨간다.
    pub library_id: i64,
    /// 라이브러리 루트 기준 경로. 접기·필터의 열쇠다.
    pub path: String,
    /// 볼륨 기준 경로. 필터를 보낼 때 쓴다.
    pub rel_path: String,
    pub name: String,
    pub depth: usize,
    /// 자기 자신과 모든 하위 폴더의 사진 수
    pub file_count: i64,
    pub has_children: bool,
    /// 라이브러리 자신인가. 누르면 폴더가 아니라 라이브러리 전체로 간다.
    ///
    /// 경로 모양(`#3`)으로 알아내지 않는다 — 실제로 `#0_사진백업…`처럼
    /// `#`으로 시작하는 폴더가 있어서 진짜 폴더를 라이브러리로 잘못 본다.
    pub is_library: bool,
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
pub fn build(leaves: Vec<Leaf>, library_rel: &str, library_id: i64) -> Vec<Node> {
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
        for (i, seg) in segs.iter().enumerate().take(segs.len().saturating_sub(1)) {
            if i > 0 {
                acc.push('/');
            }
            acc.push_str(seg);
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
                library_id,
                is_library: false,
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

/// 라이브러리 이름을 머리 마디로 얹고 그 아래 트리를 한 칸 민다.
///
/// 라이브러리를 안 고른 채로 「앨범」을 열면 예전에는 빈 화면이었다. 어디서
/// 라이브러리를 골라야 하는지 알 수 없어 하위 폴더가 아예 안 보였다. 이제는
/// 라이브러리 자체가 트리의 뿌리다.
///
/// 접기 열쇠(`path`)에 `#<id>/`를 앞에 붙인다. 서로 다른 라이브러리에 같은
/// 이름의 폴더가 있어도 하나를 펴면 둘 다 펴지는 일이 없다.
pub fn under_root(children: Vec<Node>, library_id: i64, name: &str, file_count: i64) -> Vec<Node> {
    let root = format!("#{library_id}");
    let mut out = Vec::with_capacity(children.len() + 1);
    out.push(Node {
        id: None,
        library_id,
        is_library: true,
        has_children: !children.is_empty(),
        path: root.clone(),
        rel_path: String::new(),
        name: name.to_string(),
        depth: 0,
        file_count,
    });
    out.extend(children.into_iter().map(|n| Node {
        path: format!("{root}/{}", n.path),
        depth: n.depth + 1,
        ..n
    }));
    out
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
            vec![
                leaf(1, "연도별/2001/정동진", 10),
                leaf(2, "연도별/2002/제주", 5),
            ],
            "",
            7,
        );
        let paths: Vec<&str> = t.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "연도별",
                "연도별/2001",
                "연도별/2001/정동진",
                "연도별/2002",
                "연도별/2002/제주"
            ]
        );
        // 만들어 낸 마디는 DB id가 없다
        assert_eq!(t[0].id, None);
        assert_eq!(t[2].id, Some(1));
    }

    #[test]
    fn counts_roll_up() {
        let t = build(
            vec![
                leaf(1, "연도별/2001/정동진", 10),
                leaf(2, "연도별/2002/제주", 5),
            ],
            "",
            7,
        );
        let by = |p: &str| t.iter().find(|n| n.path == p).unwrap().file_count;
        assert_eq!(by("연도별"), 15, "아래 것을 다 더한다");
        assert_eq!(by("연도별/2001"), 10);
        assert_eq!(by("연도별/2001/정동진"), 10);
    }

    #[test]
    fn depth_starts_at_zero_under_the_library_root() {
        let t = build(
            vec![leaf(1, "연도별/2001/정동진", 1)],
            "MERGE/사진통합작업",
            7,
        );
        assert_eq!(t[0].depth, 0, "라이브러리 바로 아래가 0단계");
        assert_eq!(t[2].depth, 2);
        // 볼륨 기준 경로는 라이브러리 앞부분을 되붙인다
        assert_eq!(t[2].rel_path, "MERGE/사진통합작업/연도별/2001/정동진");
    }

    #[test]
    fn leaves_know_they_have_no_children() {
        let t = build(vec![leaf(1, "a/b", 1), leaf(2, "a/c/d", 1)], "", 7);
        let by = |p: &str| t.iter().find(|n| n.path == p).unwrap();
        assert!(by("a").has_children);
        assert!(!by("a/b").has_children, "형제가 있어도 자식은 아니다");
        assert!(by("a/c").has_children);
        assert!(!by("a/c/d").has_children);
    }

    #[test]
    fn a_folder_that_is_both_leaf_and_parent() {
        // 「연도별」에 사진이 직접 있으면서 하위 폴더도 있는 경우
        let t = build(vec![leaf(1, "연도별", 3), leaf(2, "연도별/2001", 7)], "", 7);
        let by = |p: &str| t.iter().find(|n| n.path == p).unwrap();
        assert_eq!(by("연도별").id, Some(1), "실제 폴더 행이 있다");
        assert!(by("연도별").has_children);
        assert_eq!(by("연도별").file_count, 10, "자기 것 + 아래 것");
    }

    #[test]
    fn photos_at_the_library_root_make_no_node() {
        // 루트 직속 사진은 사이드바의 "전체"가 맡는다
        let t = build(vec![leaf(1, "", 5), leaf(2, "a", 1)], "", 7);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].path, "a");
    }

    #[test]
    fn empty_input() {
        assert!(build(vec![], "", 7).is_empty());
    }

    /// 라이브러리를 안 고르면 라이브러리 자체가 트리의 뿌리다.
    /// 예전에는 여기서 빈 목록을 줘서 「앨범」이 통째로 비어 있었다.
    #[test]
    fn library_becomes_the_root_of_its_tree() {
        let t = under_root(
            build(vec![leaf(1, "연도별/2001", 10), leaf(2, "행사", 5)], "", 3),
            3,
            "PHOTO 1",
            15,
        );
        assert_eq!(t[0].name, "PHOTO 1");
        assert_eq!(t[0].depth, 0);
        assert_eq!(t[0].path, "#3");
        assert_eq!(t[0].file_count, 15);
        assert!(t[0].has_children);
        assert!(t[0].is_library);

        // 아래 것들은 한 칸씩 밀리고 경로 앞에 라이브러리가 붙는다
        let by = |p: &str| t.iter().find(|n| n.path == p).unwrap();
        assert_eq!(by("#3/연도별").depth, 1);
        assert_eq!(by("#3/연도별/2001").depth, 2);
        assert!(!by("#3/행사").is_library, "진짜 폴더는 라이브러리가 아니다");
        // 볼륨 기준 경로는 그대로 — 필터가 쓰는 건 이쪽이다
        assert_eq!(by("#3/연도별/2001").rel_path, "연도별/2001");
        // 어느 라이브러리 것인지 모든 줄이 안다
        assert!(t.iter().all(|n| n.library_id == 3));
    }

    /// 접기 열쇠에 라이브러리를 붙이는 이유: 두 라이브러리에 같은 이름의
    /// 폴더가 있어도 한쪽을 펴면 다른 쪽까지 펴지면 안 된다.
    #[test]
    fn same_folder_name_in_two_libraries_stays_apart() {
        let mut all = under_root(build(vec![leaf(1, "행사/졸업", 1)], "", 1), 1, "가", 1);
        all.extend(under_root(
            build(vec![leaf(2, "행사/졸업", 1)], "", 2),
            2,
            "나",
            1,
        ));

        let paths: Vec<&str> = all.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "#1",
                "#1/행사",
                "#1/행사/졸업",
                "#2",
                "#2/행사",
                "#2/행사/졸업"
            ]
        );
        // 경로가 다르니 하나를 펴도 다른 쪽은 접힌 채로 남는다
        assert_eq!(paths.iter().filter(|p| **p == "#1/행사").count(), 1);
    }

    /// 실제로 `#0_사진백업-NAS…`처럼 `#`으로 시작하는 폴더가 있다.
    /// 경로 모양으로 라이브러리를 가려내면 이런 폴더를 라이브러리로 본다.
    #[test]
    fn a_folder_whose_name_starts_with_hash_is_not_a_library() {
        let t = under_root(
            build(
                vec![leaf(1, "#0_사진백업-NAS 자료와 동기화/2003", 3)],
                "",
                2,
            ),
            2,
            "PHOTO 1",
            3,
        );
        let f = t
            .iter()
            .find(|n| n.name == "#0_사진백업-NAS 자료와 동기화")
            .unwrap();
        assert!(!f.is_library);
        assert_eq!(f.depth, 1);
        assert!(t[0].is_library);
    }

    #[test]
    fn a_library_with_no_folders_is_still_shown() {
        // 스캔 전이라 폴더가 없어도 이름은 보여야 한다 — 없으면 등록한
        // 라이브러리가 어디로 갔는지 알 수 없다.
        let t = under_root(vec![], 5, "빈 것", 0);
        assert_eq!(t.len(), 1);
        assert!(!t[0].has_children);
    }
}
