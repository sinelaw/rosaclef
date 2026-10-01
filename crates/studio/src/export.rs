//! The song in the vocal, notation and karaoke formats
//! ([`rosaclef_vocal::formats`]): what `GET /api/export` serves, natively
//! and in the browser, and `rosaclef export` writes.

use crate::slug;
use anyhow::{anyhow, Result};
use rosaclef_core::Project;
use rosaclef_vocal::{formats, Song};
use serde_json::{json, Value};

/// A file made from the song.
#[derive(Debug)]
pub struct Export {
    /// The file name to save it as.
    pub name: String,
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

/// The song (or, with `pattern`, that pattern played once singing `verse`)
/// in the format with id `format`.
pub fn export(p: &Project, format: &str, pattern: Option<&str>, verse: u32) -> Result<Export> {
    let f = formats::find(format).ok_or_else(|| {
        let ids: Vec<&str> = formats::FORMATS.iter().map(|f| f.id).collect();
        anyhow!("unknown format {format:?} (one of: {})", ids.join(", "))
    })?;
    let (song, name) = match pattern {
        Some(id) => {
            let song =
                Song::of_pattern(p, id, verse).ok_or_else(|| anyhow!("no pattern {id:?}"))?;
            (
                song,
                format!("{}-{}-v{verse}", slug(&p.meta.title), slug(id)),
            )
        }
        None => (Song::of_project(p), slug(&p.meta.title)),
    };
    Ok(Export {
        name: format!("{name}{}.{}", suffix(f), f.extension),
        mime: f.mime,
        bytes: f.write(&song),
    })
}

/// `-id` for a format whose extension another format has too (UltraStar
/// and tagged lyrics are both `.txt`), so their files do not collide.
fn suffix(f: &formats::Format) -> String {
    let shared = formats::FORMATS
        .iter()
        .any(|g| g.id != f.id && g.extension == f.extension);
    if shared {
        format!("-{}", f.id)
    } else {
        String::new()
    }
}

/// The formats, for menus: `[{id, extension, label, description}]`.
pub fn formats() -> Value {
    let list: Vec<Value> = formats::FORMATS
        .iter()
        .map(|f| {
            json!({"id": f.id, "extension": f.extension, "label": f.label, "description": f.description})
        })
        .collect();
    json!({ "formats": list })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_file_after_the_song_or_pattern() {
        let p = Project::empty("My Song");
        let song = export(&p, "lrc", None, 1).unwrap();
        assert_eq!(
            (song.name.as_str(), song.mime),
            ("my-song.lrc", "text/plain; charset=utf-8")
        );
        let part = export(&p, "midi", Some("pattern-1"), 2).unwrap();
        assert_eq!(part.name, "my-song-pattern-1-v2.mid");
        assert!(part.bytes.starts_with(b"MThd"));
        assert_eq!(
            export(&p, "tagged", None, 1).unwrap().name,
            "my-song-tagged.txt"
        );
        assert!(export(&p, "nope", None, 1).is_err());
        assert!(export(&p, "lrc", Some("nope"), 1).is_err());
        assert_eq!(formats()["formats"][0]["id"], "midi");
    }
}
