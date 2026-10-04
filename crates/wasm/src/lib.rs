//! WebAssembly build of the Rosaclef engine, for use inside an AudioWorklet.
//!
//! A minimal C ABI (no wasm-bindgen) so the worklet can instantiate the
//! module without any JavaScript glue. Strings and buffers are passed through
//! linear memory: the host calls `rc_alloc`, writes bytes, and passes
//! pointer + length.
//!
//! The same module also serves the page's soundfont worker
//! (`web/engine/fonts.js`, a second instance): `rc_font_*` parse soundfont
//! indexes and decode presets there, off the audio thread; the worklet then
//! receives each preset in small steps (`rc_preset_*`), so a large preset
//! never holds up the audio.

use rosaclef_core::compat;
use rosaclef_engine::samples::{PresetKey, SampleData};
use rosaclef_engine::soundfont::{LoadedPreset, PresetBuilder, SoundFont};
use rosaclef_engine::{Engine, PlayMode};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

const OUT_FRAMES: usize = 4096;

struct State {
    engine: Engine,
    left: Vec<f32>,
    right: Vec<f32>,
    meters: Vec<f32>,
    result: Vec<u8>,
    /// A preset being received.
    building: Option<(PresetKey, PresetBuilder)>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.borrow_mut().as_mut().map(f))
}

unsafe fn text<'a>(ptr: *const u8, len: usize) -> &'a str {
    if ptr.is_null() || len == 0 {
        return "";
    }
    std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap_or("")
}

#[no_mangle]
pub extern "C" fn rc_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # Safety
/// `ptr` must come from `rc_alloc(len)`.
#[no_mangle]
pub unsafe extern "C" fn rc_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(Vec::from_raw_parts(ptr, 0, len.max(1)));
    }
}

#[no_mangle]
pub extern "C" fn rc_init(sample_rate: f32) {
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            engine: Engine::new(sample_rate),
            left: vec![0.0; OUT_FRAMES],
            right: vec![0.0; OUT_FRAMES],
            meters: vec![0.0; 1024],
            result: vec![],
            building: None,
        })
    });
}

/// Load a project from JSON. Returns 0 on success, 1 when invalid (details
/// via `rc_result_*`). Loading is forward-compatible
/// ([`rosaclef_core::compat`]): what this engine does not know is left out
/// or played on a stand-in, and listed in the result's `fallbacks`.
///
/// # Safety
/// `ptr`/`len` must describe readable memory.
#[no_mangle]
pub unsafe extern "C" fn rc_set_project(ptr: *const u8, len: usize) -> i32 {
    let loaded = compat::for_playback(text(ptr, len));
    with(|s| {
        let playable = match loaded {
            Ok(p) => p,
            Err(checked) => {
                let msgs: Vec<String> = checked.issues.iter().map(|i| i.to_string()).collect();
                s.result = msgs.join("\n").into_bytes();
                return 1;
            }
        };
        s.engine.set_project(playable.project);
        // Report which samples and soundfont presets still need to be provided.
        let samples: Vec<String> = s
            .engine
            .required_samples()
            .into_iter()
            .filter(|p| !s.engine.has_sample(p))
            .collect();
        let presets: Vec<serde_json::Value> = s
            .engine
            .required_presets()
            .into_iter()
            .filter(|k| !s.engine.has_preset(k))
            .map(|k| serde_json::json!({"font": k.font, "bank": k.bank, "program": k.program}))
            .collect();
        s.result = serde_json::to_vec(&serde_json::json!({"samples": samples, "presets": presets, "fallbacks": playable.fallbacks}))
            .unwrap_or_default();
        0
    })
    .unwrap_or(2)
}

#[no_mangle]
pub extern "C" fn rc_result_ptr() -> *const u8 {
    with(|s| s.result.as_ptr()).unwrap_or(std::ptr::null())
}

#[no_mangle]
pub extern "C" fn rc_result_len() -> usize {
    with(|s| s.result.len()).unwrap_or(0)
}

/// Provide decoded audio. `data` holds `channels` planar blocks of `frames` floats.
///
/// # Safety
/// Pointers must describe readable memory of the given sizes.
#[no_mangle]
pub unsafe extern "C" fn rc_set_sample(
    path: *const u8,
    path_len: usize,
    sample_rate: f32,
    channels: usize,
    frames: usize,
    data: *const f32,
) {
    let path = text(path, path_len).to_string();
    let all = std::slice::from_raw_parts(data, channels * frames);
    let chans: Vec<Vec<f32>> = (0..channels.clamp(1, 2))
        .map(|c| all[c * frames..(c + 1) * frames].to_vec())
        .collect();
    with(|s| {
        s.engine.set_sample(
            &path,
            SampleData {
                sample_rate,
                channels: chans,
            },
        )
    });
}

/// Render `frames` (<= 4096) frames; read them from `rc_left` / `rc_right`.
#[no_mangle]
pub extern "C" fn rc_process(frames: usize) {
    with(|s| {
        let n = frames.min(OUT_FRAMES);
        let State {
            engine,
            left,
            right,
            ..
        } = s;
        engine.process(&mut left[..n], &mut right[..n]);
    });
}

#[no_mangle]
pub extern "C" fn rc_left() -> *const f32 {
    with(|s| s.left.as_ptr()).unwrap_or(std::ptr::null())
}

#[no_mangle]
pub extern "C" fn rc_right() -> *const f32 {
    with(|s| s.right.as_ptr()).unwrap_or(std::ptr::null())
}

#[no_mangle]
pub extern "C" fn rc_play() {
    with(|s| s.engine.play());
}

/// Play after `beats` beats of count-in clicks.
#[no_mangle]
pub extern "C" fn rc_play_count_in(beats: f64) {
    with(|s| s.engine.play_count_in(beats));
}

#[no_mangle]
pub extern "C" fn rc_set_metronome(on: i32) {
    with(|s| s.engine.set_metronome(on != 0));
}

/// Pattern mode plays on past the pattern's end instead of looping.
#[no_mangle]
pub extern "C" fn rc_set_open_ended(on: i32) {
    with(|s| s.engine.set_open_ended(on != 0));
}

#[no_mangle]
pub extern "C" fn rc_pause() {
    with(|s| s.engine.pause());
}

#[no_mangle]
pub extern "C" fn rc_stop() {
    with(|s| s.engine.stop());
}

#[no_mangle]
pub extern "C" fn rc_is_playing() -> i32 {
    with(|s| s.engine.is_playing() as i32).unwrap_or(0)
}

/// Pattern mode when `len > 0`, song mode otherwise.
///
/// # Safety
/// `ptr`/`len` must describe readable memory.
#[no_mangle]
pub unsafe extern "C" fn rc_set_mode(ptr: *const u8, len: usize) {
    let id = text(ptr, len).to_string();
    with(|s| {
        s.engine.set_mode(if id.is_empty() {
            PlayMode::Song
        } else {
            PlayMode::Pattern(id)
        })
    });
}

#[no_mangle]
pub extern "C" fn rc_seek(beat: f64) {
    with(|s| s.engine.seek(beat));
}

#[no_mangle]
pub extern "C" fn rc_position() -> f64 {
    with(|s| s.engine.position()).unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn rc_loop_length() -> f64 {
    with(|s| s.engine.loop_length()).unwrap_or(0.0)
}

/// # Safety
/// `ptr`/`len` must describe readable memory.
#[no_mangle]
pub unsafe extern "C" fn rc_note(ptr: *const u8, len: usize, key: u32, velocity: f32, on: i32) {
    let ch = text(ptr, len).to_string();
    with(|s| {
        if on != 0 {
            s.engine.note_on(&ch, key.min(127) as u8, velocity)
        } else {
            s.engine.note_off(&ch, key.min(127) as u8)
        }
    });
}

/// Collect meters: `[insertCount, channelCount, insert L/R pairs..., channel peaks...]`.
/// Returns the number of floats written, readable from `rc_meters_ptr`.
#[no_mangle]
pub extern "C" fn rc_meters() -> usize {
    with(|s| {
        let m = s.engine.take_meters();
        s.meters.clear();
        s.meters.push(m.inserts.len() as f32);
        s.meters.push(m.channels.len() as f32);
        for p in &m.inserts {
            s.meters.push(p[0]);
            s.meters.push(p[1]);
        }
        s.meters.extend_from_slice(&m.channels);
        s.meters.len()
    })
    .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn rc_meters_ptr() -> *const f32 {
    with(|s| s.meters.as_ptr()).unwrap_or(std::ptr::null())
}

// ------------------------------------------------------------------ presets (worklet)

/// Start receiving a soundfont preset: `ptr`/`len` hold its header
/// ([`LoadedPreset::header`]). Returns 0, or 1 when the header is corrupt.
///
/// # Safety
/// Pointers must describe readable memory of the given sizes.
#[no_mangle]
pub unsafe extern "C" fn rc_preset_begin(
    font: *const u8,
    font_len: usize,
    bank: u32,
    program: u32,
    ptr: *const u8,
    len: usize,
) -> i32 {
    let key = PresetKey {
        font: text(font, font_len).to_string(),
        bank: bank.min(u16::MAX as u32) as u16,
        program: program.min(127) as u8,
    };
    let header = std::slice::from_raw_parts(ptr, len);
    with(|s| match PresetBuilder::from_header(header) {
        Ok((b, _)) => {
            s.building = Some((key, b));
            0
        }
        Err(_) => {
            s.building = None;
            1
        }
    })
    .unwrap_or(2)
}

/// The buffer of sample `i` of the preset being received (16-bit frames,
/// as many as its header says), or null.
#[no_mangle]
pub extern "C" fn rc_preset_sample(i: usize) -> *mut i16 {
    with(|s| {
        s.building
            .as_mut()
            .and_then(|(_, b)| b.sample_mut(i))
            .map(|d| d.as_mut_ptr())
            .unwrap_or(std::ptr::null_mut())
    })
    .unwrap_or(std::ptr::null_mut())
}

/// Finish the preset being received and give it to the engine.
#[no_mangle]
pub extern "C" fn rc_preset_end() {
    with(|s| {
        if let Some((key, b)) = s.building.take() {
            s.engine.set_preset(key, Arc::new(b.finish()));
        }
    });
}

// ------------------------------------------------------------------ soundfonts (font worker)

#[derive(Default)]
struct Fonts {
    fonts: HashMap<String, SoundFont>,
    pieces: HashMap<String, HashMap<usize, Vec<u8>>>,
    loaded: Option<LoadedPreset>,
    result: Vec<u8>,
}

thread_local! {
    static FONTS: RefCell<Fonts> = RefCell::new(Fonts::default());
}

/// Parse a soundfont index (`<font>/index.sf2`). Returns 0, or 1 (error in
/// `rc_font_result_*`).
///
/// # Safety
/// Pointers must describe readable memory of the given sizes.
#[no_mangle]
pub unsafe extern "C" fn rc_font_index(
    font: *const u8,
    font_len: usize,
    ptr: *const u8,
    len: usize,
) -> i32 {
    let name = text(font, font_len).to_string();
    let bytes = std::slice::from_raw_parts(ptr, len);
    FONTS.with(|f| {
        let mut f = f.borrow_mut();
        match SoundFont::parse(bytes) {
            Ok(sf) => {
                f.fonts.insert(name, sf);
                0
            }
            Err(e) => {
                f.result = e.into_bytes();
                1
            }
        }
    })
}

/// The pieces a preset needs that are not stored yet, as a JSON array in
/// `rc_font_result_*`. Returns 0, or 1 when the index is not loaded.
///
/// # Safety
/// Pointers must describe readable memory of the given sizes.
#[no_mangle]
pub unsafe extern "C" fn rc_font_pieces(
    font: *const u8,
    font_len: usize,
    bank: u32,
    program: u32,
) -> i32 {
    let name = text(font, font_len).to_string();
    FONTS.with(|f| {
        let mut f = f.borrow_mut();
        let Some(sf) = f.fonts.get(&name) else {
            return 1;
        };
        let want: Vec<usize> = match sf.find(bank as u16, program as u16) {
            Some(p) if sf.is_split() => sf.pieces(p),
            _ => vec![],
        };
        let have = f.pieces.get(&name);
        let missing: Vec<usize> = want
            .into_iter()
            .filter(|k| !have.is_some_and(|h| h.contains_key(k)))
            .collect();
        f.result = serde_json::to_vec(&missing).unwrap_or_default();
        0
    })
}

/// Store piece `k` of a split soundfont.
///
/// # Safety
/// Pointers must describe readable memory of the given sizes.
#[no_mangle]
pub unsafe extern "C" fn rc_font_piece(
    font: *const u8,
    font_len: usize,
    k: usize,
    ptr: *const u8,
    len: usize,
) {
    let name = text(font, font_len).to_string();
    let bytes = std::slice::from_raw_parts(ptr, len).to_vec();
    FONTS.with(|f| {
        f.borrow_mut()
            .pieces
            .entry(name)
            .or_default()
            .insert(k, bytes);
    });
}

/// Decode a preset. Returns 0 with its header in `rc_font_result_*` and
/// its samples in `rc_font_sample_*`, or 1 with an error message.
///
/// # Safety
/// Pointers must describe readable memory of the given sizes.
#[no_mangle]
pub unsafe extern "C" fn rc_font_load(
    font: *const u8,
    font_len: usize,
    bank: u32,
    program: u32,
) -> i32 {
    let name = text(font, font_len).to_string();
    FONTS.with(|f| {
        let mut f = f.borrow_mut();
        let f = &mut *f;
        let result = match f.fonts.get(&name) {
            None => Err(format!("soundfont {name} is not loaded")),
            Some(sf) => match sf.find(bank as u16, program as u16) {
                None => Err(format!("soundfont {name} has no presets")),
                Some(p) => {
                    let none = HashMap::new();
                    let pieces = f.pieces.get(&name).unwrap_or(&none);
                    sf.load(p, pieces, &mut HashMap::new())
                }
            },
        };
        match result {
            Ok(p) => {
                f.result = p.header();
                f.loaded = Some(p);
                0
            }
            Err(e) => {
                f.result = e.into_bytes();
                1
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn rc_font_result_ptr() -> *const u8 {
    FONTS.with(|f| f.borrow().result.as_ptr())
}

#[no_mangle]
pub extern "C" fn rc_font_result_len() -> usize {
    FONTS.with(|f| f.borrow().result.len())
}

/// Number of samples of the decoded preset.
#[no_mangle]
pub extern "C" fn rc_font_sample_count() -> usize {
    FONTS.with(|f| {
        f.borrow()
            .loaded
            .as_ref()
            .map(|p| p.samples.len())
            .unwrap_or(0)
    })
}

/// The data of sample `i` of the decoded preset (16-bit frames).
#[no_mangle]
pub extern "C" fn rc_font_sample_ptr(i: usize) -> *const i16 {
    FONTS.with(|f| {
        f.borrow()
            .loaded
            .as_ref()
            .and_then(|p| p.samples.get(i))
            .map(|s| s.data.as_ptr())
            .unwrap_or(std::ptr::null())
    })
}

#[no_mangle]
pub extern "C" fn rc_font_sample_len(i: usize) -> usize {
    FONTS.with(|f| {
        f.borrow()
            .loaded
            .as_ref()
            .and_then(|p| p.samples.get(i))
            .map(|s| s.data.len())
            .unwrap_or(0)
    })
}

/// Forget the decoded preset and the stored pieces (the indexes stay).
#[no_mangle]
pub extern "C" fn rc_font_clear() {
    FONTS.with(|f| {
        let mut f = f.borrow_mut();
        f.loaded = None;
        f.pieces.clear();
        f.result = vec![];
    });
}
