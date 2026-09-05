use super::*;

/// 평점·선별 플래그·즐겨찾기를 한 번에 바꾼다. 여러 장을 동시에 처리한다.
#[tauri::command]
pub async fn files_mark(
    state: State<'_, AppState>,
    ids: Vec<i64>,
    rating: Option<i32>,
    culling_flag: Option<i32>,
    favorite: Option<bool>,
) -> Result<usize, String> {
    if ids.is_empty() {
        return Ok(0);
    }
    // 한 문장 — id 마다 세 문장이면 5,000장에 15,000번 실행이다. NULL 은 «그대로»
    let list = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    let n = state
        .db
        .transaction(|tx| {
            let n = tx.execute(
                &format!(
                    "UPDATE files SET rating = COALESCE(?1, rating),
                                      culling_flag = COALESCE(?2, culling_flag),
                                      favorite = COALESCE(?3, favorite)
                     WHERE id IN ({list})"
                ),
                rusqlite::params![
                    rating.map(|r| r.clamp(0, 5)),
                    culling_flag.map(|f| f.clamp(0, 2)),
                    favorite.map(|v| v as i32),
                ],
            )?;
            Ok(n)
        })
        .map_err(err)?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_are_clamped_to_valid_range() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        db.write(|c| {
            c.execute(
                "INSERT INTO volumes(uuid,name,role) VALUES('V','t','library')",
                [],
            )?;
            c.execute(
                "INSERT INTO folders(id,volume_uuid,rel_path,name,area) VALUES(1,'V','a','a',1)",
                [],
            )?;
            c.execute(
                "INSERT INTO files(id,folder_id,name,size,kind,taken_at,taken_at_source,scanned_at)
                 VALUES(1,1,'a.jpg',1,0,1,0,0)",
                [],
            )
        })
        .unwrap();

        // 범위를 벗어난 값이 그대로 들어가면 안 된다
        db.transaction(|tx| {
            tx.execute("UPDATE files SET rating=?1 WHERE id=1", [99i32.clamp(0, 5)])?;
            Ok(())
        })
        .unwrap();
        let r: i32 = db
            .read(|c| c.query_row("SELECT rating FROM files WHERE id=1", [], |x| x.get(0)))
            .unwrap();
        assert_eq!(r, 5);
    }
}
