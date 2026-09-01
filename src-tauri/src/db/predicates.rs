//! 여러 곳이 함께 써야 하는 판정 — SQL 과 러스트가 같은 뜻이어야 한다.
//!
//! 좌표 조건이 `query.rs`(지도 격자·통계)와 `geo.rs`(지명 대상·파일 갱신)에
//! 따로 있었다. 한쪽만 고치면 «지도가 세는 사진»과 «지명을 붙일 사진»이 조용히
//! 달라진다 — 그러면 처리할 수 없는 행이 영원히 «남은 것»으로 남는다 (2026-09-01).

/// 쓸 수 있는 좌표인가 — SQL 조건.
///
/// `alias` 는 파일 표의 별칭이다. **내부 상수만 넘긴다** — 사용자 입력이 여기로
/// 오면 안 된다. 빈 문자열이면 별칭 없이(`gps_lat`) 쓴다.
///
/// 두 값이 **모두 정확히 0** 인 것만 «좌표 없음» 센티널로 본다. 한쪽만 0 인
/// 좌표(적도·본초자오선 위)는 정상이다. 지구 범위 밖은 잘못된 값으로 친다.
pub(crate) fn valid_gps_sql(alias: &str) -> String {
    let p = if alias.is_empty() { String::new() } else { format!("{alias}.") };
    format!(
        "{p}gps_lat IS NOT NULL AND {p}gps_lon IS NOT NULL
         AND {p}gps_lat BETWEEN -90.0 AND 90.0
         AND {p}gps_lon BETWEEN -180.0 AND 180.0
         AND NOT ({p}gps_lat = 0.0 AND {p}gps_lon = 0.0)"
    )
}

/// 같은 판정을 러스트에서. SQL 과 결과가 반드시 같아야 한다.
pub(crate) fn is_valid_gps(lat: Option<f64>, lon: Option<f64>) -> bool {
    let (Some(lat), Some(lon)) = (lat, lon) else { return false };
    (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) && !(lat == 0.0 && lon == 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::conn::Db;

    #[test]
    fn boundaries_and_sentinels() {
        // 경계값은 유효하다
        assert!(is_valid_gps(Some(90.0), Some(180.0)));
        assert!(is_valid_gps(Some(-90.0), Some(-180.0)));
        // 한쪽만 0 인 좌표는 정상이다 — 적도·본초자오선
        assert!(is_valid_gps(Some(0.0), Some(127.0)));
        assert!(is_valid_gps(Some(37.5), Some(0.0)));
        // 두 값이 모두 0 이면 «좌표 없음» 센티널
        assert!(!is_valid_gps(Some(0.0), Some(0.0)));
        // 없거나 범위 밖
        assert!(!is_valid_gps(None, Some(127.0)));
        assert!(!is_valid_gps(Some(37.5), None));
        assert!(!is_valid_gps(Some(90.1), Some(127.0)));
        assert!(!is_valid_gps(Some(37.5), Some(180.1)));
    }

    /// SQL 과 러스트가 같은 답을 내야 한다 — 어긋나면 세는 것과 처리하는 것이 달라진다
    #[test]
    fn the_sql_and_the_rust_check_agree() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute_batch(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library');
                 INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1);",
            )
        })
        .unwrap();

        let cases: &[(Option<f64>, Option<f64>)] = &[
            (Some(37.5), Some(127.0)),
            (Some(90.0), Some(180.0)),
            (Some(-90.0), Some(-180.0)),
            (Some(0.0), Some(127.0)),
            (Some(37.5), Some(0.0)),
            (Some(0.0), Some(0.0)),
            (None, Some(127.0)),
            (Some(37.5), None),
            (Some(90.1), Some(127.0)),
            (Some(37.5), Some(180.1)),
            (None, None),
        ];
        for (i, (lat, lon)) in cases.iter().enumerate() {
            let id = i as i64 + 1;
            db.write(|c| {
                c.execute(
                    "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at,gps_lat,gps_lon)
                     VALUES(?1,1,?2,1,0,1,0,0,?3,?4)",
                    rusqlite::params![id, format!("f{id}.jpg"), lat, lon],
                )
            })
            .unwrap();
            let by_sql: bool = db
                .read(|c| {
                    c.query_row(
                        &format!("SELECT EXISTS(SELECT 1 FROM files fi WHERE fi.id = ?1 AND ({}))", valid_gps_sql("fi")),
                        [id],
                        |r| r.get(0),
                    )
                })
                .unwrap();
            assert_eq!(by_sql, is_valid_gps(*lat, *lon), "{lat:?},{lon:?} 에서 SQL 과 러스트가 다르다");
        }
    }

    /// 별칭이 있든 없든 같은 뜻이어야 한다
    #[test]
    fn the_alias_only_prefixes_the_columns() {
        let with = valid_gps_sql("fi");
        let bare = valid_gps_sql("");
        assert!(with.contains("fi.gps_lat IS NOT NULL") && with.contains("fi.gps_lon BETWEEN"));
        assert!(bare.starts_with("gps_lat IS NOT NULL"));
        // 별칭이 없으면 칸 이름 앞에 접두사가 붙지 않는다 (숫자 리터럴의 점은 논외)
        assert!(!bare.contains("fi."));
        assert_eq!(with.matches("gps_lat").count(), bare.matches("gps_lat").count());
        assert_eq!(with.matches("fi.").count(), 6, "칸 참조 여섯 곳에 모두 별칭이 붙는다");
    }
}
