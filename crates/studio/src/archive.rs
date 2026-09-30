//! Projects as zip archives: a backup of a song and its files, and the way
//! songs move between the browser build (whose files live in the browser's
//! storage) and a project folder on a machine.
//!
//! A small, dependency-light zip implementation: writing uses deflate for
//! text and stores audio as is; reading supports stored and deflated entries
//! (what every common tool writes). No zip64: archives stay under 4 GB.

use crate::folder::{self, Folder};
use crate::library::{self, Library, TRASH_DIR};
use anyhow::{anyhow, bail, Context, Result};
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::{Compression, Crc};
use rosaclef_core::PROJECT_FILE;
use std::io::{Read, Write};
use std::path::Path;

/// Largest total size of the files unpacked from one archive.
const MAX_UNPACKED: u64 = 2 << 30;

// ------------------------------------------------------------------ writing

/// MS-DOS date and time (the zip format's timestamps) from ms since the epoch.
fn dos_time(ms: f64) -> (u16, u16) {
    let t = crate::rfc3339(ms); // YYYY-MM-DDTHH:MM:SSZ
    let n = |a: usize, b: usize| t.get(a..b).and_then(|x| x.parse::<u16>().ok()).unwrap_or(0);
    let year = n(0, 4).clamp(1980, 2107);
    let date = ((year - 1980) << 9) | (n(5, 7) << 5) | n(8, 10);
    let time = (n(11, 13) << 11) | (n(14, 16) << 5) | (n(17, 19) / 2);
    (time, date)
}

struct Entry {
    name: String,
    crc: u32,
    method: u16,
    compressed: u32,
    size: u32,
    time: u16,
    date: u16,
    offset: u32,
}

/// A zip archive being written into memory.
#[derive(Default)]
pub struct ZipWriter {
    out: Vec<u8>,
    entries: Vec<Entry>,
}

fn u32_of(n: usize, what: &str) -> Result<u32> {
    u32::try_from(n).map_err(|_| anyhow!("{what} is too large for a zip archive (4 GB)"))
}

impl ZipWriter {
    pub fn new() -> ZipWriter {
        ZipWriter::default()
    }

    /// Add a file. `compress` deflates it (worth it for text, not for audio).
    pub fn add(&mut self, name: &str, data: &[u8], modified_ms: f64, compress: bool) -> Result<()> {
        let mut crc = Crc::new();
        crc.update(data);
        let deflated;
        let (method, body): (u16, &[u8]) = if compress {
            let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
            enc.write_all(data)?;
            deflated = enc.finish()?;
            if deflated.len() < data.len() {
                (8, &deflated)
            } else {
                (0, data)
            }
        } else {
            (0, data)
        };
        let (time, date) = dos_time(modified_ms);
        let e = Entry {
            name: name.to_string(),
            crc: crc.sum(),
            method,
            compressed: u32_of(body.len(), name)?,
            size: u32_of(data.len(), name)?,
            time,
            date,
            offset: u32_of(self.out.len(), "the archive")?,
        };
        let o = &mut self.out;
        o.extend_from_slice(&0x04034b50u32.to_le_bytes());
        o.extend_from_slice(&20u16.to_le_bytes()); // version needed
        o.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
        o.extend_from_slice(&e.method.to_le_bytes());
        o.extend_from_slice(&e.time.to_le_bytes());
        o.extend_from_slice(&e.date.to_le_bytes());
        o.extend_from_slice(&e.crc.to_le_bytes());
        o.extend_from_slice(&e.compressed.to_le_bytes());
        o.extend_from_slice(&e.size.to_le_bytes());
        o.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        o.extend_from_slice(&0u16.to_le_bytes());
        o.extend_from_slice(e.name.as_bytes());
        o.extend_from_slice(body);
        self.entries.push(e);
        Ok(())
    }

    /// Write the central directory and return the archive.
    pub fn finish(mut self) -> Result<Vec<u8>> {
        let start = u32_of(self.out.len(), "the archive")?;
        for e in &self.entries {
            let o = &mut self.out;
            o.extend_from_slice(&0x02014b50u32.to_le_bytes());
            o.extend_from_slice(&0x031eu16.to_le_bytes()); // made by: Unix, spec 3.0
            o.extend_from_slice(&20u16.to_le_bytes());
            o.extend_from_slice(&0x0800u16.to_le_bytes());
            o.extend_from_slice(&e.method.to_le_bytes());
            o.extend_from_slice(&e.time.to_le_bytes());
            o.extend_from_slice(&e.date.to_le_bytes());
            o.extend_from_slice(&e.crc.to_le_bytes());
            o.extend_from_slice(&e.compressed.to_le_bytes());
            o.extend_from_slice(&e.size.to_le_bytes());
            o.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            o.extend_from_slice(&[0; 8]); // extra, comment, disk, internal attributes
            o.extend_from_slice(&(0o100644u32 << 16).to_le_bytes()); // rw-r--r--
            o.extend_from_slice(&e.offset.to_le_bytes());
            o.extend_from_slice(e.name.as_bytes());
        }
        let size = u32_of(self.out.len(), "the archive")? - start;
        let n = u16::try_from(self.entries.len())
            .map_err(|_| anyhow!("too many files for a zip archive"))?;
        let o = &mut self.out;
        o.extend_from_slice(&0x06054b50u32.to_le_bytes());
        o.extend_from_slice(&[0; 4]);
        o.extend_from_slice(&n.to_le_bytes());
        o.extend_from_slice(&n.to_le_bytes());
        o.extend_from_slice(&size.to_le_bytes());
        o.extend_from_slice(&start.to_le_bytes());
        o.extend_from_slice(&0u16.to_le_bytes());
        Ok(self.out)
    }
}

/// Pack a project folder: every file except the studio's live state and the
/// trash, under a top-level folder named after the project.
pub fn export(f: &Folder) -> Result<Vec<u8>> {
    let mut zip = ZipWriter::new();
    let root = f.name();
    fn walk(f: &Folder, dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<()> {
        for p in f.fs.read_dir(dir)? {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name == folder::STATE_DIR || name == TRASH_DIR || name.ends_with(".tmp") {
                continue;
            }
            if f.fs.is_dir(&p) {
                walk(f, &p, out)?;
            } else {
                out.push(p);
            }
        }
        Ok(())
    }
    let mut paths = vec![];
    walk(f, &f.dir, &mut paths)?;
    for p in paths {
        let rel = p
            .strip_prefix(&f.dir)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let data = f.fs.read(&p)?;
        let modified = f.fs.metadata(&p).map(|m| m.modified).unwrap_or(0.0);
        zip.add(
            &format!("{root}/{rel}"),
            &data,
            modified,
            !crate::decode::is_audio_file(&p),
        )?;
    }
    zip.finish()
}

// ------------------------------------------------------------------ reading

fn u16_at(b: &[u8], i: usize) -> Result<u16> {
    b.get(i..i + 2)
        .map(|x| u16::from_le_bytes([x[0], x[1]]))
        .ok_or_else(|| anyhow!("truncated zip archive"))
}

fn u32_at(b: &[u8], i: usize) -> Result<u32> {
    b.get(i..i + 4)
        .map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
        .ok_or_else(|| anyhow!("truncated zip archive"))
}

/// The files of a zip archive: `(name, content)`, folders left out.
pub fn read_zip(b: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    // The end-of-central-directory record is within the last 64 KiB + 22 bytes.
    let lowest = b.len().saturating_sub(22 + 65535);
    let eocd = (lowest..b.len().saturating_sub(21))
        .rev()
        .find(|&i| u32_at(b, i).ok() == Some(0x06054b50))
        .ok_or_else(|| anyhow!("not a zip archive"))?;
    let count = u16_at(b, eocd + 10)? as usize;
    let directory = u32_at(b, eocd + 16)?;
    if count == 0xffff || directory == 0xffff_ffff {
        bail!("zip64 archives are not supported");
    }
    let mut at = directory as usize;
    let mut out = vec![];
    let mut total = 0u64;
    for _ in 0..count {
        if u32_at(b, at)? != 0x02014b50 {
            bail!("corrupt zip archive (central directory)");
        }
        let flags = u16_at(b, at + 8)?;
        let method = u16_at(b, at + 10)?;
        let crc = u32_at(b, at + 16)?;
        let csize = u32_at(b, at + 20)? as usize;
        let size = u32_at(b, at + 24)? as u64;
        let (nlen, xlen, clen) = (
            u16_at(b, at + 28)? as usize,
            u16_at(b, at + 30)? as usize,
            u16_at(b, at + 32)? as usize,
        );
        let local = u32_at(b, at + 42)? as usize;
        let name_bytes = b
            .get(at + 46..at + 46 + nlen)
            .ok_or_else(|| anyhow!("truncated zip archive"))?;
        let name = String::from_utf8_lossy(name_bytes).replace('\\', "/");
        at += 46 + nlen + xlen + clen;
        if name.ends_with('/') {
            continue;
        }
        if flags & 1 != 0 {
            bail!("{name} is encrypted");
        }
        total += size;
        if total > MAX_UNPACKED {
            bail!("the archive unpacks to more than {} GB", MAX_UNPACKED >> 30);
        }
        if u32_at(b, local)? != 0x04034b50 {
            bail!("corrupt zip archive ({name})");
        }
        let data_at =
            local + 30 + u16_at(b, local + 26)? as usize + u16_at(b, local + 28)? as usize;
        let raw = b
            .get(data_at..data_at + csize)
            .ok_or_else(|| anyhow!("truncated zip archive ({name})"))?;
        let data = match method {
            0 => raw.to_vec(),
            8 => {
                let mut v = Vec::with_capacity(size as usize);
                DeflateDecoder::new(raw)
                    .take(size + 1)
                    .read_to_end(&mut v)
                    .with_context(|| format!("inflating {name}"))?;
                v
            }
            m => bail!("{name}: unsupported compression method {m}"),
        };
        let mut c = Crc::new();
        c.update(&data);
        if c.sum() != crc || data.len() as u64 != size {
            bail!("{name} is damaged (checksum mismatch)");
        }
        out.push((name, data));
    }
    Ok(out)
}

/// Unpack an archive holding a project folder as a new library project.
/// `name` is the wanted project name (made valid and unique). Returns the
/// project's name and notes on what was skipped.
pub fn import(lib: &Library, name: &str, zip: &[u8]) -> Result<(String, Vec<String>)> {
    let files = read_zip(zip)?;
    // The project is where the shallowest project.json is.
    let root = files
        .iter()
        .map(|(n, _)| n.as_str())
        .filter(|n| *n == PROJECT_FILE || n.ends_with(&format!("/{PROJECT_FILE}")))
        .filter(|n| !n.starts_with("__MACOSX/"))
        .min_by_key(|n| n.matches('/').count())
        .map(|n| n[..n.len() - PROJECT_FILE.len()].to_string())
        .ok_or_else(|| anyhow!("the archive holds no {PROJECT_FILE}"))?;
    let base = if name.trim().is_empty() {
        root.trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_string()
    } else {
        name.to_string()
    };
    let name = lib.unique_name(&library::sanitize_name(if base.is_empty() {
        "Imported"
    } else {
        &base
    }));
    let f = lib.folder(&name);
    lib.fs.create_dir_all(&f.dir)?;
    let mut notes = vec![];
    for (path, data) in files {
        let Some(rel) = path.strip_prefix(&root) else {
            continue;
        };
        let hidden = rel
            .split('/')
            .any(|s| s.starts_with('.') || s == "__MACOSX");
        if hidden {
            continue;
        }
        match f.resolve(rel) {
            Some(dst) => lib.fs.write(&dst, &data)?,
            None => notes.push(format!("skipped {path}: the path leaves the project")),
        }
    }
    f.init(false)?;
    crate::guide::write(&f, &lib.exe)?;
    if !f.load().map(|c| c.is_ok()).unwrap_or(false) {
        notes.push(format!(
            "{PROJECT_FILE} has errors: open the project to see them, or fix it in the Files tab"
        ));
    }
    Ok((name, notes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosaclef_fs::MemFs;
    use std::sync::Arc;

    #[test]
    fn round_trip() {
        let mem = MemFs::new();
        mem.set_now(1_790_000_000_000.0);
        let lib = Library::new(Arc::new(mem), "/library", "rosaclef");
        let f = lib.create("Song", true).unwrap();
        f.write("samples/kick.wav", b"RIFF....").unwrap();
        f.write(".rosaclef/status.json", b"{}").unwrap();
        let zip = export(&f).unwrap();
        let names: Vec<String> = read_zip(&zip)
            .unwrap()
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert!(names.contains(&"Song/project.json".to_string()));
        assert!(names.contains(&"Song/samples/kick.wav".to_string()));
        assert!(
            !names.iter().any(|n| n.contains(".rosaclef")),
            "live state stays behind"
        );

        let (name, notes) = import(&lib, "", &zip).unwrap();
        assert_eq!(name, "Song 2");
        assert!(notes.is_empty(), "{notes:?}");
        let copy = lib.folder(&name);
        assert_eq!(
            copy.fs.read(&copy.dir.join("samples/kick.wav")).unwrap(),
            b"RIFF...."
        );
        assert_eq!(
            copy.load().unwrap().project.unwrap(),
            f.load().unwrap().project.unwrap()
        );
        assert!(import(&lib, "x", b"not a zip").is_err());
    }

    #[test]
    fn dos_times() {
        // 2026-09-21 14:13:20
        assert_eq!(
            dos_time(1_790_000_000_000.0),
            ((14 << 11) | (13 << 5) | 10, (46 << 9) | (9 << 5) | 21)
        );
    }
}
