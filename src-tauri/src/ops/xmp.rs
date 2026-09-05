//! XMP 사이드카 — 평점·태그·판정을 파일 옆 `.xmp`에 적는다.
//!
//! nas_photo와 값만 맞추기 위해서다. 같은 사진을 두 앱이 다른 목적으로 보니
//! DB를 공유하지 않고, 파일 옆의 작은 XML 하나로 평점(xmp:Rating)과
//! 태그(dc:subject)를 나른다. 이름은 `IMG_1234.jpg.xmp` — 원본 이름을
//! 통째로 앞에 둬 어느 파일의 것인지 헷갈리지 않는다 (digiKam 방식).
//!
//! 우리가 쓴 것만 다시 쓴다. 남이 만든 사이드카(우리 표식이 없는 것)는
//! 건드리지 않는다.

use crate::db::conn::{Db, Result};
use crate::db::libraries;
use std::path::{Path, PathBuf};

const MARK: &str = "acut:Sidecar=\"1\"";

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct XmpResult {
    pub written: usize,
    pub skipped: usize,
    pub failed: usize,
}

pub struct Meta<'a> {
    pub rating: i32,
    /// 0 미판정 · 1 남김 · 2 제외
    pub culling_flag: i32,
    pub favorite: bool,
    pub tags: &'a [String],
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 사이드카 본문
pub fn render(m: &Meta) -> String {
    let tags = if m.tags.is_empty() {
        String::new()
    } else {
        let items: String = m
            .tags
            .iter()
            .map(|t| format!("     <rdf:li>{}</rdf:li>\n", esc(t)))
            .collect();
        format!("    <dc:subject>\n     <rdf:Bag>\n{items}     </rdf:Bag>\n    </dc:subject>\n")
    };
    let flag = match m.culling_flag {
        1 => "pick",
        2 => "reject",
        _ => "none",
    };
    format!(
        "<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
 <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
  <rdf:Description rdf:about=\"\"\n\
    xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"\n\
    xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n\
    xmlns:acut=\"https://acut.media/ns/1.0/\"\n\
    xmp:Rating=\"{rating}\"\n\
    acut:Flag=\"{flag}\"\n\
    acut:Favorite=\"{fav}\"\n\
    {MARK}>\n\
{tags}\
  </rdf:Description>\n\
 </rdf:RDF>\n\
</x:xmpmeta>\n\
<?xpacket end=\"w\"?>\n",
        rating = m.rating.clamp(0, 5),
        fav = if m.favorite { "true" } else { "false" },
    )
}

pub fn sidecar_path(photo: &Path) -> PathBuf {
    let mut p = photo.as_os_str().to_owned();
    p.push(".xmp");
    PathBuf::from(p)
}

/// 쓴다. 남의 사이드카가 있으면 건너뛴다(false). 내용이 같으면 안 쓴다.
pub fn write(photo: &Path, m: &Meta) -> std::io::Result<bool> {
    let p = sidecar_path(photo);
    let body = render(m);
    if let Ok(old) = std::fs::read_to_string(&p) {
        if !old.contains(MARK) {
            return Ok(false);
        }
        if old == body {
            return Ok(true);
        }
    }
    std::fs::write(&p, body)?;
    Ok(true)
}

struct Row {
    id: i64,
    vol: String,
    vol_rel: String,
    rating: i32,
    flag: i32,
    favorite: bool,
}

/// 평점·태그·판정이 있는 사진 전부(또는 한 라이브러리)의 사이드카를 쓴다.
pub fn export(
    db: &Db,
    library_id: Option<i64>,
    on_progress: impl Fn(usize, usize),
) -> Result<XmpResult> {
    let rows: Vec<Row> = db.read(|c| {
        let mut st = c.prepare(
            "SELECT fi.id, fo.volume_uuid, fo.rel_path, fi.name, fi.rating, fi.culling_flag, fi.favorite
               FROM files fi JOIN folders fo ON fo.id = fi.folder_id
              WHERE fi.trashed_at IS NULL AND (?1 IS NULL OR fo.library_id = ?1)
                AND (fi.rating > 0 OR fi.culling_flag <> 0 OR fi.favorite = 1
                     OR EXISTS (SELECT 1 FROM file_tags t WHERE t.file_id = fi.id))",
        )?;
        let it = st.query_map([library_id], |r| {
            let dir: String = r.get(2)?;
            let name: String = r.get(3)?;
            Ok(Row {
                id: r.get(0)?,
                vol: r.get(1)?,
                vol_rel: crate::media::cache::rel_path(&dir, &name),
                rating: r.get(4)?,
                flag: r.get(5)?,
                favorite: r.get::<_, i64>(6)? != 0,
            })
        })?;
        it.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    let mut out = XmpResult::default();
    let total = rows.len();
    let mounts: std::collections::HashMap<String, Option<PathBuf>> = rows
        .iter()
        .map(|r| r.vol.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .map(|u| (u.clone(), crate::db::volumes::find_mount(&u)))
        .collect();
    let _ = libraries::list(db);
    for (i, r) in rows.iter().enumerate() {
        let tags: Vec<String> = db.read(|c| {
            let mut st = c.prepare("SELECT t.name FROM file_tags ft JOIN tags t ON t.id = ft.tag_id WHERE ft.file_id = ?1 ORDER BY t.name")?;
            let it = st.query_map([r.id], |x| x.get(0))?;
            it.collect()
        })?;
        let Some(Some(mount)) = mounts.get(&r.vol) else {
            out.skipped += 1;
            continue;
        };
        let photo = mount.join(&r.vol_rel);
        let m = Meta {
            rating: r.rating,
            culling_flag: r.flag,
            favorite: r.favorite,
            tags: &tags,
        };
        match write(&photo, &m) {
            Ok(true) => out.written += 1,
            Ok(false) => out.skipped += 1,
            Err(_) => out.failed += 1,
        }
        if i % 50 == 0 {
            on_progress(i, total);
        }
    }
    on_progress(total, total);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_rating_flag_and_tags() {
        let tags = vec!["가족".to_string(), "a<b".to_string()];
        let s = render(&Meta {
            rating: 4,
            culling_flag: 1,
            favorite: true,
            tags: &tags,
        });
        assert!(s.contains("xmp:Rating=\"4\""));
        assert!(s.contains("acut:Flag=\"pick\""));
        assert!(s.contains("<rdf:li>가족</rdf:li>"));
        assert!(s.contains("<rdf:li>a&lt;b</rdf:li>"));
        assert!(s.contains(MARK));
    }

    #[test]
    fn writes_next_to_the_photo_and_leaves_foreign_sidecars_alone() {
        let d = tempfile::tempdir().unwrap();
        let photo = d.path().join("IMG_1.jpg");
        std::fs::write(&photo, b"x").unwrap();
        let m = Meta {
            rating: 3,
            culling_flag: 0,
            favorite: false,
            tags: &[],
        };
        assert!(write(&photo, &m).unwrap());
        assert!(d.path().join("IMG_1.jpg.xmp").exists());
        // 남이 만든 사이드카
        let other = d.path().join("IMG_2.jpg");
        std::fs::write(&other, b"y").unwrap();
        std::fs::write(d.path().join("IMG_2.jpg.xmp"), "<x:xmpmeta/>").unwrap();
        assert!(!write(&other, &m).unwrap());
        assert_eq!(
            std::fs::read_to_string(d.path().join("IMG_2.jpg.xmp")).unwrap(),
            "<x:xmpmeta/>"
        );
    }
}
