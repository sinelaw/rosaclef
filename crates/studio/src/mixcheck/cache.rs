//! Renders are cached on a hash of what makes the sound: the project
//! without its cosmetic parts (title, score, film, the Critic's settings,
//! names and colors), the samples it plays (path, size, date), the stretch
//! rendered and the pre-roll, the sample rate and this version. A repeated
//! question about the same song is answered without rendering.
//!
//! The analysis is kept in memory (the last few) and on disk in the project
//! folder (`.rosaclef/mixcheck/<key>.bin`, deflated; the oldest are removed).

use super::analyze::{self, Analysis, GrSeries, Pos, StreamKey, FR};
use super::timeline::Segment;
use crate::folder::Folder;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use serde_json::Value;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

/// Bumped whenever the measurements change.
const FORMAT: u32 = 3;
const MAGIC: &[u8; 8] = b"RCMIXCK\0";
/// Memory the remembered analyses may hold (the browser's worker less).
#[cfg(not(target_arch = "wasm32"))]
const MEMORY_BYTES: usize = 384 << 20;
#[cfg(target_arch = "wasm32")]
const MEMORY_BYTES: usize = 64 << 20;
const DISK: usize = 24;

/// 128-bit FNV-1a (two lanes): stable across builds and platforms.
#[derive(Clone, Copy)]
pub struct Hasher(u64, u64);

impl Default for Hasher {
    fn default() -> Self {
        Hasher(0xcbf29ce484222325, 0x6c62272e07bb0142)
    }
}

impl Hasher {
    pub fn bytes(&mut self, b: &[u8]) {
        for x in b {
            self.0 = (self.0 ^ *x as u64).wrapping_mul(0x100000001b3);
            // An odd multiplier, so every byte keeps mattering.
            self.1 = (self.1 ^ *x as u64).wrapping_mul(0x9e3779b97f4a7c15);
        }
        // Lengths separate fields.
        self.0 = self
            .0
            .wrapping_add(b.len() as u64)
            .wrapping_mul(0x100000001b3);
    }
    pub fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    pub fn hex(&self) -> String {
        format!("{:016x}{:016x}", self.0, self.1)
    }
}

/// The parts of a project that make its sound (as JSON).
pub fn sound_of(project: &rosaclef_core::Project) -> Value {
    let mut v = serde_json::to_value(project).unwrap_or(Value::Null);
    if let Some(m) = v.as_object_mut() {
        for k in ["$schema", "meta", "score", "animation", "critic", "drums"] {
            m.remove(k);
        }
        let strip = |x: &mut Value, keys: &[&str]| {
            if let Some(arr) = x.as_array_mut() {
                for e in arr {
                    if let Some(o) = e.as_object_mut() {
                        for k in keys {
                            o.remove(*k);
                        }
                    }
                }
            }
        };
        if let Some(c) = m.get_mut("channels") {
            strip(c, &["name", "color"]);
        }
        if let Some(c) = m.get_mut("patterns") {
            strip(c, &["name", "color", "drums"]);
        }
        if let Some(c) = m.get_mut("automation") {
            strip(c, &["name", "color"]);
        }
        if let Some(pl) = m.get_mut("playlist").and_then(|p| p.as_object_mut()) {
            if let Some(t) = pl.get_mut("tracks") {
                strip(t, &["name"]);
            }
        }
        if let Some(mx) = m.get_mut("mixer").and_then(|p| p.as_object_mut()) {
            if let Some(t) = mx.get_mut("inserts") {
                strip(t, &["name"]);
            }
        }
    }
    v
}

/// What makes the instruments' sound: [`sound_of`] without the mixer, the
/// channels' volume, pan, mute and routing, and the automation lanes of
/// those — all of which act after the instrument (the mix check's fixes
/// change only these, but for an instrument's filter).
pub fn instruments_of(project: &rosaclef_core::Project) -> Value {
    let mut v = sound_of(project);
    if let Some(m) = v.as_object_mut() {
        m.remove("mixer");
        if let Some(chs) = m.get_mut("channels").and_then(|c| c.as_array_mut()) {
            for c in chs.iter_mut().filter_map(|c| c.as_object_mut()) {
                for k in ["volume", "pan", "mute", "mixer"] {
                    c.remove(k);
                }
            }
        }
        if let Some(lanes) = m.get_mut("automation").and_then(|a| a.as_array_mut()) {
            lanes.retain(|l| {
                let t = l.get("target").and_then(|t| t.as_str()).unwrap_or("");
                let parts: Vec<&str> = t.split('/').collect();
                !(parts[0] == "insert"
                    || parts[0] == "channel"
                        && parts.len() == 3
                        && matches!(parts[2], "volume" | "pan"))
            });
        }
    }
    v
}

/// The cache key of a render.
pub fn key(
    folder: Option<&Folder>,
    project: &rosaclef_core::Project,
    samples: &[String],
    segments: &[Segment],
    preroll: f64,
    sr: f32,
) -> String {
    key_of(folder, &sound_of(project), samples, segments, preroll, sr)
}

/// The key of a render's instrument outputs ([`instruments_of`]).
pub fn dry_key(
    folder: Option<&Folder>,
    project: &rosaclef_core::Project,
    samples: &[String],
    segments: &[Segment],
    preroll: f64,
    sr: f32,
) -> String {
    let mut v = instruments_of(project);
    if let Some(m) = v.as_object_mut() {
        m.insert("dry".into(), Value::Bool(true));
    }
    key_of(folder, &v, samples, segments, preroll, sr)
}

fn key_of(
    folder: Option<&Folder>,
    sound: &Value,
    samples: &[String],
    segments: &[Segment],
    preroll: f64,
    sr: f32,
) -> String {
    let mut h = Hasher::default();
    h.str(&format!(
        "{FORMAT}|{}|{sr}|{preroll}",
        env!("CARGO_PKG_VERSION")
    ));
    h.str(&serde_json::to_string(sound).unwrap_or_default());
    for s in segments {
        h.str(&format!("{}:{}", s.from, s.to));
    }
    for s in samples {
        h.str(s);
        if let Some(meta) = folder
            .and_then(|f| f.resolve(s))
            .and_then(|p| folder.and_then(|f| f.fs.metadata(&p).ok()))
        {
            h.str(&format!("{}|{}", meta.len, meta.modified));
        }
    }
    h.hex()
}

// ------------------------------------------------------------ memory

static MEMO: Mutex<Vec<(String, Arc<Analysis>)>> = Mutex::new(Vec::new());

pub fn remembered(key: &str) -> Option<Arc<Analysis>> {
    let mut m = MEMO.lock().unwrap_or_else(|e| e.into_inner());
    let i = m.iter().position(|(k, _)| k == key)?;
    let hit = m.remove(i);
    let a = hit.1.clone();
    m.push(hit);
    Some(a)
}

pub fn remember(key: &str, a: Arc<Analysis>) {
    let mut m = MEMO.lock().unwrap_or_else(|e| e.into_inner());
    m.retain(|(k, _)| k != key);
    m.push((key.to_string(), a));
    // The oldest go first; the newest stays even if it alone is over.
    while m.len() > 1 && m.iter().map(|(_, a)| bytes(a)).sum::<usize>() > MEMORY_BYTES {
        m.remove(0);
    }
}

// ------------------------------------------------------- instrument outputs

/// The instruments' outputs of a render, kept on disk only — never in
/// memory: `.rosaclef/mixcheck/dry-<key>/`, a file per segment and channel,
/// written as the render goes and read as a later render of the same notes
/// plays (see [`instruments_of`]). A file is the output's chunks in order:
/// a byte, 0 for a silent chunk or 1 for one whose frames follow (left, then
/// right; f32, little-endian). A recording counts once its render has ended
/// (its folder renamed into place); the newest two are kept.
#[cfg(not(target_arch = "wasm32"))]
pub mod dry {
    use crate::folder::Folder;
    use rosaclef_engine::{DryRead, DryWrite, DRY_CHUNK};
    use std::fs::File;
    use std::io::{BufReader, BufWriter, Read, Write};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// Renders' outputs kept.
    const KEEP: usize = 2;

    /// A render's instrument outputs: played from the disk when kept, else
    /// kept there as it goes.
    pub struct DryFiles {
        dir: PathBuf,
        part: PathBuf,
        replay: bool,
        failed: Arc<AtomicBool>,
    }

    impl DryFiles {
        /// The outputs under `key` in `folder` (none for a folder not on
        /// disk).
        pub fn open(folder: &Folder, key: &str) -> Option<DryFiles> {
            let base = super::dir(folder);
            let dir = base.join(format!("dry-{key}"));
            let part = base.join(format!("dry-{key}.part"));
            let replay = dir.is_dir();
            if replay {
                // In use: the newest.
                let _ = std::fs::write(dir.join("used"), b"");
            } else {
                let _ = std::fs::remove_dir_all(&part);
                std::fs::create_dir_all(&part).ok()?;
                let ignore = base.join(".gitignore");
                if !ignore.exists() {
                    let _ = std::fs::write(&ignore, b"*\n");
                }
            }
            Some(DryFiles {
                dir,
                part,
                replay,
                failed: Arc::new(AtomicBool::new(false)),
            })
        }

        fn file(&self, at: &std::path::Path, seg: usize, ch: usize) -> PathBuf {
            at.join(format!("s{seg}-c{ch}.bin"))
        }

        /// The recording's end: kept when every write went through, and the
        /// oldest let go.
        pub fn finish(self) {
            if self.replay {
                return;
            }
            if self.failed.load(Ordering::Relaxed)
                || std::fs::rename(&self.part, &self.dir).is_err()
            {
                let _ = std::fs::remove_dir_all(&self.part);
                return;
            }
            let _ = std::fs::write(self.dir.join("used"), b"");
            let Some(base) = self.dir.parent() else {
                return;
            };
            let Ok(entries) = std::fs::read_dir(base) else {
                return;
            };
            let mut kept: Vec<(f64, PathBuf)> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_dir()
                        && p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.starts_with("dry-") && !n.ends_with(".part"))
                })
                .map(|p| {
                    let t = std::fs::metadata(p.join("used"))
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map_or(0.0, |d| d.as_secs_f64());
                    (t, p)
                })
                .collect();
            kept.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (_, p) in kept.iter().skip(KEEP) {
                let _ = std::fs::remove_dir_all(p);
            }
        }
    }

    impl super::super::analyze::DryStore for DryFiles {
        fn replay(&mut self, seg: usize, channels: usize) -> Option<Vec<Box<dyn DryRead>>> {
            if !self.replay {
                return None;
            }
            (0..channels)
                .map(|c| {
                    File::open(self.file(&self.dir, seg, c)).ok().map(|f| {
                        Box::new(FileIn {
                            r: Some(BufReader::with_capacity(1 << 16, f)),
                            next: 0,
                            bytes: vec![0; DRY_CHUNK * 8],
                        }) as Box<dyn DryRead>
                    })
                })
                .collect()
        }

        fn record(&mut self, seg: usize, channels: usize) -> Option<Vec<Box<dyn DryWrite>>> {
            if self.replay {
                return None;
            }
            let outs: Option<Vec<Box<dyn DryWrite>>> = (0..channels)
                .map(|c| {
                    File::create(self.file(&self.part, seg, c)).ok().map(|f| {
                        Box::new(FileOut {
                            w: Some(BufWriter::with_capacity(1 << 16, f)),
                            bytes: Vec::with_capacity(DRY_CHUNK * 8 + 1),
                            failed: self.failed.clone(),
                        }) as Box<dyn DryWrite>
                    })
                })
                .collect();
            if outs.is_none() {
                self.failed.store(true, Ordering::Relaxed);
            }
            outs
        }
    }

    struct FileOut {
        w: Option<BufWriter<File>>,
        bytes: Vec<u8>,
        failed: Arc<AtomicBool>,
    }

    impl DryWrite for FileOut {
        fn chunk(&mut self, frames: Option<&[f32]>) {
            let Some(w) = &mut self.w else { return };
            self.bytes.clear();
            match frames {
                None => self.bytes.push(0),
                Some(x) => {
                    self.bytes.push(1);
                    for v in x {
                        self.bytes.extend_from_slice(&v.to_le_bytes());
                    }
                }
            }
            if w.write_all(&self.bytes).is_err() {
                self.w = None;
                self.failed.store(true, Ordering::Relaxed);
            }
        }
    }

    impl Drop for FileOut {
        fn drop(&mut self) {
            if let Some(w) = &mut self.w {
                if w.flush().is_err() {
                    self.failed.store(true, Ordering::Relaxed);
                }
            }
        }
    }

    struct FileIn {
        r: Option<BufReader<File>>,
        /// The next chunk in the file.
        next: usize,
        bytes: Vec<u8>,
    }

    impl DryRead for FileIn {
        fn chunk(&mut self, i: usize, out: &mut [f32]) -> bool {
            let Some(r) = &mut self.r else { return false };
            while self.next <= i {
                let mut flag = [0u8];
                if r.read_exact(&mut flag).is_err()
                    || flag[0] == 1 && r.read_exact(&mut self.bytes).is_err()
                {
                    self.r = None;
                    return false;
                }
                let here = self.next;
                self.next += 1;
                if here == i {
                    if flag[0] == 0 {
                        return false;
                    }
                    for (k, b) in self.bytes.chunks_exact(4).enumerate() {
                        out[k] = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                    }
                    return true;
                }
            }
            false
        }
    }
}

/// Forget the remembered analyses (the disk keeps its own).
pub fn forget() {
    MEMO.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// About how much memory an analysis holds.
fn bytes(a: &Analysis) -> usize {
    let floats: usize = a.frames.iter().map(Vec::len).sum::<usize>()
        + a.kms.iter().map(Vec::len).sum::<usize>()
        + a.gr.iter().map(|g| g.max.len() * 2).sum::<usize>();
    floats * 4 + (a.hops.len() + a.lblocks.len()) * std::mem::size_of::<super::analyze::Pos>()
}

// -------------------------------------------------------------- disk

fn dir(folder: &Folder) -> std::path::PathBuf {
    folder.state_path("mixcheck")
}

pub fn load(folder: &Folder, key: &str) -> Option<Analysis> {
    let bytes = folder
        .fs
        .read(&dir(folder).join(format!("{key}.bin")))
        .ok()?;
    decode(&bytes)
}

pub fn store(folder: &Folder, key: &str, a: &Analysis) {
    let d = dir(folder);
    if folder.fs.create_dir_all(&d).is_err() {
        return;
    }
    // Renders are no source: kept out of git wherever the project lives.
    let ignore = d.join(".gitignore");
    if !folder.fs.exists(&ignore) {
        let _ = folder.fs.write(&ignore, b"*\n");
    }
    let _ = folder.fs.write(&d.join(format!("{key}.bin")), &encode(a));
    // Keep the newest few.
    if let Ok(files) = folder.fs.read_dir(&d) {
        let mut files: Vec<_> = files
            .into_iter()
            .filter(|f| f.extension().is_some_and(|x| x == "bin"))
            .collect();
        if files.len() > DISK {
            files.sort_by(|a, b| {
                let m =
                    |p: &std::path::Path| folder.fs.metadata(p).map(|m| m.modified).unwrap_or(0.0);
                m(a).total_cmp(&m(b))
            });
            for f in &files[..files.len() - DISK] {
                let _ = folder.fs.remove_file(f);
            }
        }
    }
}

// ------------------------------------------------------------ format

struct W(Vec<u8>);

impl W {
    fn u8(&mut self, x: u8) {
        self.0.push(x);
    }
    fn u16(&mut self, x: u16) {
        self.0.extend_from_slice(&x.to_le_bytes());
    }
    fn u32(&mut self, x: u32) {
        self.0.extend_from_slice(&x.to_le_bytes());
    }
    fn f64(&mut self, x: f64) {
        self.0.extend_from_slice(&x.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
    }
    fn pos(&mut self, p: &Pos) {
        self.u32(p.seg);
        self.u32(p.span);
        self.f64(p.beat);
        self.f64(p.perf);
        self.u8(p.inside as u8);
    }
}

struct R<'a>(&'a [u8], usize);

impl R<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.0.get(self.1..self.1 + n)?;
        self.1 += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
    fn pos(&mut self) -> Option<Pos> {
        Some(Pos {
            seg: self.u32()?,
            span: self.u32()?,
            beat: self.f64()?,
            perf: self.f64()?,
            inside: self.u8()? != 0,
        })
    }
}

pub fn encode(a: &Analysis) -> Vec<u8> {
    let mut w = W(vec![]);
    w.u32(FORMAT);
    w.f64(a.sr as f64);
    w.u32(a.lblock as u32);
    w.f64(a.preroll);
    w.u32(a.segments.len() as u32);
    for s in &a.segments {
        w.f64(s.from);
        w.f64(s.to);
    }
    w.u32(a.streams.len() as u32);
    for s in &a.streams {
        let (k, i) = match s {
            StreamKey::Channel(i) => (0, *i),
            StreamKey::Insert(i) => (1, *i),
            StreamKey::MasterPre => (2, 0),
            StreamKey::MasterLimiter => (3, 0),
            StreamKey::MasterOut => (4, 0),
        };
        w.u8(k);
        w.u32(i as u32);
    }
    w.u32(a.hops.len() as u32);
    a.hops.iter().for_each(|p| w.pos(p));
    w.u32(a.lblocks.len() as u32);
    a.lblocks.iter().for_each(|p| w.pos(p));
    for (f, k) in a.frames.iter().zip(&a.kms) {
        w.u8(!f.is_empty() as u8);
        for (i, v) in f.iter().enumerate() {
            if i % FR == analyze::F_LR {
                w.u8((*v < 0.0) as u8);
                w.u16(analyze::enc_pow(v.abs()));
            } else {
                w.u16(analyze::enc_pow(*v));
            }
        }
        w.u32(k.len() as u32);
        k.iter().for_each(|v| w.u16(analyze::enc_pow(*v)));
    }
    w.u32(a.gr.len() as u32);
    for g in &a.gr {
        w.u32(g.insert as u32);
        w.u32(g.fx as u32);
        w.str(&g.kind);
        w.u32(g.max.len() as u32);
        g.max.iter().for_each(|v| w.u16(analyze::enc_gr(*v)));
        g.mean.iter().for_each(|v| w.u16(analyze::enc_gr(*v)));
    }
    w.u32(a.warnings.len() as u32);
    a.warnings.iter().for_each(|s| w.str(s));
    let mut out = MAGIC.to_vec();
    let mut z = DeflateEncoder::new(&mut out, Compression::fast());
    let _ = z.write_all(&w.0);
    let _ = z.finish();
    out
}

pub fn decode(bytes: &[u8]) -> Option<Analysis> {
    let body = bytes.strip_prefix(MAGIC.as_slice())?;
    let mut raw = vec![];
    DeflateDecoder::new(body).read_to_end(&mut raw).ok()?;
    let mut r = R(&raw, 0);
    if r.u32()? != FORMAT {
        return None;
    }
    let sr = r.f64()? as f32;
    let lblock = r.u32()? as usize;
    let preroll = r.f64()?;
    let segments = (0..r.u32()?)
        .map(|_| {
            Some(Segment {
                from: r.f64()?,
                to: r.f64()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let streams = (0..r.u32()?)
        .map(|_| {
            let k = r.u8()?;
            let i = r.u32()? as usize;
            Some(match k {
                0 => StreamKey::Channel(i),
                1 => StreamKey::Insert(i),
                2 => StreamKey::MasterPre,
                3 => StreamKey::MasterLimiter,
                _ => StreamKey::MasterOut,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let hops = (0..r.u32()?).map(|_| r.pos()).collect::<Option<Vec<_>>>()?;
    let lblocks = (0..r.u32()?).map(|_| r.pos()).collect::<Option<Vec<_>>>()?;
    let mut frames = vec![];
    let mut kms = vec![];
    for _ in 0..streams.len() {
        let any = r.u8()? != 0;
        let mut f = vec![];
        if any {
            f.reserve(hops.len() * FR);
            for i in 0..hops.len() * FR {
                if i % FR == analyze::F_LR {
                    let neg = r.u8()? != 0;
                    let v = analyze::dec_pow(r.u16()?);
                    f.push(if neg { -v } else { v });
                } else {
                    f.push(analyze::dec_pow(r.u16()?));
                }
            }
        }
        frames.push(f);
        let n = r.u32()? as usize;
        kms.push(
            (0..n)
                .map(|_| r.u16().map(analyze::dec_pow))
                .collect::<Option<Vec<_>>>()?,
        );
    }
    let gr = (0..r.u32()?)
        .map(|_| {
            let insert = r.u32()? as usize;
            let fx = r.u32()? as usize;
            let kind = r.str()?;
            let n = r.u32()? as usize;
            let max = (0..n)
                .map(|_| r.u16().map(analyze::dec_gr))
                .collect::<Option<Vec<_>>>()?;
            let mean = (0..n)
                .map(|_| r.u16().map(analyze::dec_gr))
                .collect::<Option<Vec<_>>>()?;
            Some(GrSeries {
                insert,
                fx,
                kind,
                max,
                mean,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let warnings = (0..r.u32()?).map(|_| r.str()).collect::<Option<Vec<_>>>()?;
    Some(Analysis {
        sr,
        lblock,
        streams,
        hops,
        lblocks,
        frames,
        kms,
        gr,
        segments,
        preroll,
        warnings,
    })
}
