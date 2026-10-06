//! The studio's portable back end.
//!
//! Everything the Rosaclef server does that does not need an operating
//! system: project folders, the project library and file manager, the agent
//! guides, audio decoding and offline rendering. It is written against
//! [`rosaclef_fs::Fs`], so the same code runs on the disk (the native server,
//! `crates/server`) and in the browser on an in-memory tree persisted to
//! IndexedDB (`crates/local`, compiled to WebAssembly).

pub mod archive;
pub mod decode;
pub mod folder;
pub mod fonts;
pub mod guide;
pub mod jobs;
pub mod library;
pub mod mixcheck;
pub mod render;
pub mod transcribe;

pub use folder::Folder;
pub use library::Library;
pub use rosaclef_fs as fs;

/// A file-name friendly form of a title (`My Song!` → `my-song`).
pub fn slug(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s
        .split('-')
        .filter(|x| !x.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if s.is_empty() {
        "untitled".into()
    } else {
        s
    }
}

/// RFC 3339 UTC time (`2026-09-30T12:00:00Z`) from milliseconds since the epoch.
pub fn rfc3339(ms: f64) -> String {
    let secs = (ms / 1000.0).floor() as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

#[cfg(test)]
mod tests {
    #[test]
    fn helpers() {
        assert_eq!(super::slug("My Song!"), "my-song");
        assert_eq!(super::slug("!!"), "untitled");
        assert_eq!(super::rfc3339(0.0), "1970-01-01T00:00:00Z");
        assert_eq!(super::rfc3339(1_790_000_000_000.0), "2026-09-21T14:13:20Z");
    }
}
