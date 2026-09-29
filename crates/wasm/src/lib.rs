//! WebAssembly build of the Rosaclef engine, for use inside an AudioWorklet.
//!
//! A minimal C ABI (no wasm-bindgen) so the worklet can instantiate the
//! module without any JavaScript glue. Strings and buffers are passed through
//! linear memory: the host calls `rc_alloc`, writes bytes, and passes
//! pointer + length.

use rosaclef_core::validate;
use rosaclef_engine::samples::SampleData;
use rosaclef_engine::{Engine, PlayMode};
use std::cell::RefCell;

const OUT_FRAMES: usize = 4096;

struct State {
    engine: Engine,
    left: Vec<f32>,
    right: Vec<f32>,
    meters: Vec<f32>,
    result: Vec<u8>,
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
        })
    });
}

/// Load a project from JSON. Returns 0 on success, 1 when invalid (details
/// via `rc_result_*`).
///
/// # Safety
/// `ptr`/`len` must describe readable memory.
#[no_mangle]
pub unsafe extern "C" fn rc_set_project(ptr: *const u8, len: usize) -> i32 {
    let checked = validate::parse_and_validate(text(ptr, len));
    with(|s| {
        if !checked.is_ok() {
            let msgs: Vec<String> = checked.issues.iter().map(|i| i.to_string()).collect();
            s.result = msgs.join("\n").into_bytes();
            return 1;
        }
        s.engine.set_project(checked.project.unwrap());
        // Report which samples still need to be provided.
        let missing: Vec<String> = s.engine.required_samples().into_iter().filter(|p| !s.engine.has_sample(p)).collect();
        s.result = serde_json::to_vec(&missing).unwrap_or_default();
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
pub unsafe extern "C" fn rc_set_sample(path: *const u8, path_len: usize, sample_rate: f32, channels: usize, frames: usize, data: *const f32) {
    let path = text(path, path_len).to_string();
    let all = std::slice::from_raw_parts(data, channels * frames);
    let chans: Vec<Vec<f32>> = (0..channels.clamp(1, 2)).map(|c| all[c * frames..(c + 1) * frames].to_vec()).collect();
    with(|s| s.engine.set_sample(&path, SampleData { sample_rate, channels: chans }));
}

/// Render `frames` (<= 4096) frames; read them from `rc_left` / `rc_right`.
#[no_mangle]
pub extern "C" fn rc_process(frames: usize) {
    with(|s| {
        let n = frames.min(OUT_FRAMES);
        let State { engine, left, right, .. } = s;
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
    with(|s| s.engine.set_mode(if id.is_empty() { PlayMode::Song } else { PlayMode::Pattern(id) }));
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
