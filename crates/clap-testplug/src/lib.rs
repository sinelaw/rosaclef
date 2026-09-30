//! A tiny CLAP plugin bundle used to test Rosaclef's CLAP host.
//!
//! It exports `clap_entry` with a factory holding two plugins:
//!
//! * `dev.rosaclef.test-sine`: a polyphonic sine synthesizer with a simple
//!   attack/release envelope and one parameter (id 0, "Gain", 0..1, default 0.5).
//! * `dev.rosaclef.test-gain`: a stereo gain effect with one parameter
//!   (id 0, "Gain", 0..2, default 1).
//!
//! Everything is written against the raw C ABI of `clap-sys`. The plugin
//! state is split in two allocations: the `clap_plugin` struct handed to the
//! host, and a [`State`] reached through `plugin_data`. Main-thread functions
//! only touch the atomic parameter value; audio-thread state lives in an
//! `UnsafeCell` that only `activate` (while not processing) and `process`
//! touch, as the CLAP threading rules guarantee they never run concurrently.

#![allow(clippy::missing_safety_doc)]

use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::audio_ports::*;
use clap_sys::ext::note_ports::*;
use clap_sys::ext::params::*;
use clap_sys::factory::plugin_factory::*;
use clap_sys::host::clap_host;
use clap_sys::id::{clap_id, CLAP_INVALID_ID};
use clap_sys::plugin::*;
use clap_sys::process::*;
use clap_sys::version::CLAP_VERSION;
use std::cell::UnsafeCell;
use std::ffi::{c_char, c_void, CStr};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU64, Ordering};

// ------------------------------------------------------------- descriptors

/// A null-terminated array of C strings that can live in a `static`.
#[repr(transparent)]
struct Features<const N: usize>([*const c_char; N]);
// SAFETY: the pointers refer to immutable 'static string literals.
unsafe impl<const N: usize> Sync for Features<N> {}

static SINE_FEATURES: Features<3> = Features([c"instrument".as_ptr(), c"synthesizer".as_ptr(), null()]);
static GAIN_FEATURES: Features<2> = Features([c"audio-effect".as_ptr(), null()]);

static SINE_DESC: clap_plugin_descriptor = clap_plugin_descriptor {
    clap_version: CLAP_VERSION,
    id: c"dev.rosaclef.test-sine".as_ptr(),
    name: c"Rosaclef Test Sine".as_ptr(),
    vendor: c"Rosaclef".as_ptr(),
    url: c"".as_ptr(),
    manual_url: c"".as_ptr(),
    support_url: c"".as_ptr(),
    version: c"0.1.0".as_ptr(),
    description: c"Polyphonic sine synthesizer for host tests".as_ptr(),
    features: SINE_FEATURES.0.as_ptr(),
};

static GAIN_DESC: clap_plugin_descriptor = clap_plugin_descriptor {
    clap_version: CLAP_VERSION,
    id: c"dev.rosaclef.test-gain".as_ptr(),
    name: c"Rosaclef Test Gain".as_ptr(),
    vendor: c"Rosaclef".as_ptr(),
    url: c"".as_ptr(),
    manual_url: c"".as_ptr(),
    support_url: c"".as_ptr(),
    version: c"0.1.0".as_ptr(),
    description: c"Stereo gain effect for host tests".as_ptr(),
    features: GAIN_FEATURES.0.as_ptr(),
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Sine,
    Gain,
}

impl Kind {
    fn gain_range(self) -> (f64, f64, f64) {
        match self {
            Kind::Sine => (0.0, 1.0, 0.5),
            Kind::Gain => (0.0, 2.0, 1.0),
        }
    }
}

// ------------------------------------------------------------------- entry

#[allow(non_upper_case_globals)]
#[no_mangle]
pub static clap_entry: clap_plugin_entry = clap_plugin_entry {
    clap_version: CLAP_VERSION,
    init: Some(entry_init),
    deinit: Some(entry_deinit),
    get_factory: Some(entry_get_factory),
};

unsafe extern "C" fn entry_init(_path: *const c_char) -> bool {
    true
}

unsafe extern "C" fn entry_deinit() {}

unsafe extern "C" fn entry_get_factory(id: *const c_char) -> *const c_void {
    if !id.is_null() && CStr::from_ptr(id) == CLAP_PLUGIN_FACTORY_ID {
        return &FACTORY as *const clap_plugin_factory as *const c_void;
    }
    null()
}

static FACTORY: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(factory_count),
    get_plugin_descriptor: Some(factory_descriptor),
    create_plugin: Some(factory_create),
};

unsafe extern "C" fn factory_count(_f: *const clap_plugin_factory) -> u32 {
    2
}

unsafe extern "C" fn factory_descriptor(_f: *const clap_plugin_factory, index: u32) -> *const clap_plugin_descriptor {
    match index {
        0 => &SINE_DESC,
        1 => &GAIN_DESC,
        _ => null(),
    }
}

unsafe extern "C" fn factory_create(_f: *const clap_plugin_factory, host: *const clap_host, id: *const c_char) -> *const clap_plugin {
    if id.is_null() || host.is_null() {
        return null();
    }
    let id = CStr::from_ptr(id);
    let (kind, desc): (Kind, &'static clap_plugin_descriptor) = if id == CStr::from_ptr(SINE_DESC.id) {
        (Kind::Sine, &SINE_DESC)
    } else if id == CStr::from_ptr(GAIN_DESC.id) {
        (Kind::Gain, &GAIN_DESC)
    } else {
        return null();
    };
    let state = Box::into_raw(Box::new(State {
        kind,
        gain: AtomicU64::new(kind.gain_range().2.to_bits()),
        audio: UnsafeCell::new(Audio { sr: 48000.0, voices: [Voice::default(); VOICES] }),
    }));
    let plugin = Box::new(clap_plugin {
        desc,
        plugin_data: state as *mut c_void,
        init: Some(plugin_init),
        destroy: Some(plugin_destroy),
        activate: Some(plugin_activate),
        deactivate: Some(plugin_deactivate),
        start_processing: Some(plugin_start_processing),
        stop_processing: Some(plugin_stop_processing),
        reset: Some(plugin_reset),
        process: Some(plugin_process),
        get_extension: Some(plugin_get_extension),
        on_main_thread: Some(plugin_on_main_thread),
    });
    Box::into_raw(plugin)
}

// ------------------------------------------------------------------- state

const VOICES: usize = 16;
const ATTACK_S: f32 = 0.005;
const RELEASE_S: f32 = 0.05;

#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,
    releasing: bool,
    key: i16,
    velocity: f32,
    phase: f32,
    freq: f32,
    env: f32,
    age: u64,
}

struct Audio {
    sr: f32,
    voices: [Voice; VOICES],
}

struct State {
    kind: Kind,
    /// Plain parameter value, as f64 bits.
    gain: AtomicU64,
    /// Audio-thread state (see the module docs for the access rules).
    audio: UnsafeCell<Audio>,
}

impl State {
    fn gain(&self) -> f64 {
        f64::from_bits(self.gain.load(Ordering::Relaxed))
    }
    fn set_gain(&self, v: f64) {
        let (lo, hi, _) = self.kind.gain_range();
        if v.is_finite() {
            self.gain.store(v.clamp(lo, hi).to_bits(), Ordering::Relaxed);
        }
    }
}

/// # Safety
/// `plugin` must be a pointer created by `factory_create` and not destroyed.
unsafe fn state<'a>(plugin: *const clap_plugin) -> &'a State {
    &*((*plugin).plugin_data as *const State)
}

// ------------------------------------------------------------------ plugin

unsafe extern "C" fn plugin_init(_p: *const clap_plugin) -> bool {
    true
}

unsafe extern "C" fn plugin_destroy(p: *const clap_plugin) {
    if p.is_null() {
        return;
    }
    let plugin = Box::from_raw(p as *mut clap_plugin);
    drop(Box::from_raw(plugin.plugin_data as *mut State));
}

unsafe extern "C" fn plugin_activate(p: *const clap_plugin, sr: f64, _min: u32, _max: u32) -> bool {
    let audio = &mut *state(p).audio.get();
    audio.sr = sr as f32;
    audio.voices = [Voice::default(); VOICES];
    true
}

unsafe extern "C" fn plugin_deactivate(_p: *const clap_plugin) {}

unsafe extern "C" fn plugin_start_processing(_p: *const clap_plugin) -> bool {
    true
}

unsafe extern "C" fn plugin_stop_processing(_p: *const clap_plugin) {}

unsafe extern "C" fn plugin_reset(p: *const clap_plugin) {
    let audio = &mut *state(p).audio.get();
    audio.voices = [Voice::default(); VOICES];
}

unsafe extern "C" fn plugin_on_main_thread(_p: *const clap_plugin) {}

unsafe extern "C" fn plugin_get_extension(p: *const clap_plugin, id: *const c_char) -> *const c_void {
    if id.is_null() {
        return null();
    }
    let id = CStr::from_ptr(id);
    if id == CLAP_EXT_PARAMS {
        return &PARAMS as *const _ as *const c_void;
    }
    if id == CLAP_EXT_AUDIO_PORTS {
        return &AUDIO_PORTS as *const _ as *const c_void;
    }
    if id == CLAP_EXT_NOTE_PORTS && state(p).kind == Kind::Sine {
        return &NOTE_PORTS as *const _ as *const c_void;
    }
    null()
}

/// Apply one input event that is not audio-rate (parameter change, note).
unsafe fn handle_event(st: &State, audio: Option<&mut Audio>, ev: *const clap_event_header) {
    if ev.is_null() || (*ev).space_id != CLAP_CORE_EVENT_SPACE_ID {
        return;
    }
    match (*ev).type_ {
        CLAP_EVENT_PARAM_VALUE => {
            let e = &*(ev as *const clap_event_param_value);
            if e.param_id == 0 {
                st.set_gain(e.value);
            }
        }
        CLAP_EVENT_NOTE_ON if st.kind == Kind::Sine => {
            let Some(audio) = audio else { return };
            let e = &*(ev as *const clap_event_note);
            let sr = audio.sr;
            let age = audio.voices.iter().map(|v| v.age).max().unwrap_or(0) + 1;
            // Prefer a free voice, then the oldest one.
            let idx = audio
                .voices
                .iter()
                .position(|v| !v.active)
                .unwrap_or_else(|| audio.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap_or(0));
            let freq = 440.0 * 2f32.powf((e.key as f32 - 69.0) / 12.0);
            audio.voices[idx] = Voice {
                active: true,
                releasing: false,
                key: e.key,
                velocity: e.velocity.clamp(0.0, 1.0) as f32,
                phase: 0.0,
                freq: freq.min(sr * 0.45),
                env: 0.0,
                age,
            };
        }
        CLAP_EVENT_NOTE_OFF if st.kind == Kind::Sine => {
            let Some(audio) = audio else { return };
            let e = &*(ev as *const clap_event_note);
            for v in audio.voices.iter_mut().filter(|v| v.active && (e.key < 0 || v.key == e.key)) {
                v.releasing = true;
            }
        }
        _ => {}
    }
}

/// Render the synth voices into `l`/`r` for frames `from..to`.
fn render_sine(audio: &mut Audio, gain: f32, l: &mut [f32], mut r: Option<&mut [f32]>, from: usize, to: usize) {
    let sr = audio.sr.max(1.0);
    let att = 1.0 / (ATTACK_S * sr);
    let rel = 1.0 / (RELEASE_S * sr);
    for i in from..to {
        let mut s = 0.0;
        for v in audio.voices.iter_mut().filter(|v| v.active) {
            if v.releasing {
                v.env -= rel;
                if v.env <= 0.0 {
                    v.active = false;
                    continue;
                }
            } else if v.env < 1.0 {
                v.env = (v.env + att).min(1.0);
            }
            s += (v.phase * std::f32::consts::TAU).sin() * v.env * v.velocity;
            v.phase = (v.phase + v.freq / sr).fract();
        }
        let s = s * 0.3 * gain;
        l[i] = s;
        if let Some(r) = r.as_deref_mut() {
            r[i] = s;
        }
    }
}

unsafe extern "C" fn plugin_process(p: *const clap_plugin, process: *const clap_process) -> clap_process_status {
    if p.is_null() || process.is_null() {
        return CLAP_PROCESS_ERROR;
    }
    let st = state(p);
    let audio = &mut *st.audio.get();
    let pr = &*process;
    let n = pr.frames_count as usize;
    if pr.audio_outputs_count < 1 || pr.audio_outputs.is_null() {
        return CLAP_PROCESS_ERROR;
    }
    let out = &mut *pr.audio_outputs;
    if out.data32.is_null() || out.channel_count < 1 {
        return CLAP_PROCESS_ERROR;
    }
    out.constant_mask = 0;

    // Events, sorted by time as the host must deliver them.
    let (ev_count, ev_get) = match pr.in_events.as_ref() {
        Some(list) => match (list.size, list.get) {
            (Some(size), Some(get)) => (size(list), Some(get)),
            _ => (0, None),
        },
        None => (0, None),
    };
    let mut next_ev = 0u32;

    match st.kind {
        Kind::Sine => {
            let out_l = std::slice::from_raw_parts_mut(*out.data32, n);
            let mut out_r = if out.channel_count >= 2 { Some(std::slice::from_raw_parts_mut(*out.data32.add(1), n)) } else { None };
            let mut pos = 0usize;
            while pos < n {
                // Apply every event due at `pos`, then render up to the next one.
                let mut until = n;
                while next_ev < ev_count {
                    let ev = ev_get.map(|g| g(pr.in_events, next_ev)).unwrap_or(null());
                    let t = if ev.is_null() { 0 } else { (*ev).time as usize };
                    if t <= pos {
                        handle_event(st, Some(audio), ev);
                        next_ev += 1;
                    } else {
                        until = t.min(n);
                        break;
                    }
                }
                let gain = st.gain() as f32;
                render_sine(audio, gain, out_l, out_r.as_deref_mut(), pos, until);
                pos = until;
            }
            // Events at or past the end of the block.
            while next_ev < ev_count {
                let ev = ev_get.map(|g| g(pr.in_events, next_ev)).unwrap_or(null());
                handle_event(st, Some(audio), ev);
                next_ev += 1;
            }
            CLAP_PROCESS_CONTINUE
        }
        Kind::Gain => {
            // Parameter changes are applied at block start (good enough for tests).
            while next_ev < ev_count {
                let ev = ev_get.map(|g| g(pr.in_events, next_ev)).unwrap_or(null());
                handle_event(st, None, ev);
                next_ev += 1;
            }
            let gain = st.gain() as f32;
            // Input channel pointers (empty when there is no usable input).
            let inputs: &[*mut f32] = match pr.audio_inputs.as_ref() {
                Some(b) if pr.audio_inputs_count >= 1 && !b.data32.is_null() => std::slice::from_raw_parts(b.data32, b.channel_count as usize),
                _ => &[],
            };
            let out_ch = out.channel_count.min(2) as usize;
            for c in 0..out_ch {
                let dst = *out.data32.add(c);
                // Read then write each sample so in-place buffers work too.
                let src: *const f32 = if inputs.is_empty() { null() } else { inputs[c.min(inputs.len() - 1)] };
                for i in 0..n {
                    let x = if src.is_null() { 0.0 } else { *src.add(i) };
                    *dst.add(i) = x * gain;
                }
            }
            CLAP_PROCESS_CONTINUE
        }
    }
}

// ------------------------------------------------------------------ params

static PARAMS: clap_plugin_params = clap_plugin_params {
    count: Some(params_count),
    get_info: Some(params_get_info),
    get_value: Some(params_get_value),
    value_to_text: Some(params_value_to_text),
    text_to_value: Some(params_text_to_value),
    flush: Some(params_flush),
};

unsafe extern "C" fn params_count(_p: *const clap_plugin) -> u32 {
    1
}

/// Copy `s` into a fixed-size C string buffer (truncating, always terminated).
unsafe fn write_cstr(dst: *mut c_char, cap: usize, s: &str) {
    if dst.is_null() || cap == 0 {
        return;
    }
    let n = s.len().min(cap - 1);
    for (i, b) in s.as_bytes()[..n].iter().enumerate() {
        *dst.add(i) = *b as c_char;
    }
    *dst.add(n) = 0;
}

unsafe extern "C" fn params_get_info(p: *const clap_plugin, index: u32, info: *mut clap_param_info) -> bool {
    if index != 0 || info.is_null() {
        return false;
    }
    let (min, max, def) = state(p).kind.gain_range();
    let info = &mut *info;
    info.id = 0;
    info.flags = CLAP_PARAM_IS_AUTOMATABLE;
    info.cookie = null_mut();
    write_cstr(info.name.as_mut_ptr(), info.name.len(), "Gain");
    write_cstr(info.module.as_mut_ptr(), info.module.len(), "");
    info.min_value = min;
    info.max_value = max;
    info.default_value = def;
    true
}

unsafe extern "C" fn params_get_value(p: *const clap_plugin, id: clap_id, out: *mut f64) -> bool {
    if id != 0 || out.is_null() {
        return false;
    }
    *out = state(p).gain();
    true
}

unsafe extern "C" fn params_value_to_text(_p: *const clap_plugin, id: clap_id, value: f64, buf: *mut c_char, cap: u32) -> bool {
    if id != 0 {
        return false;
    }
    write_cstr(buf, cap as usize, &format!("{value:.2}"));
    true
}

unsafe extern "C" fn params_text_to_value(_p: *const clap_plugin, id: clap_id, text: *const c_char, out: *mut f64) -> bool {
    if id != 0 || text.is_null() || out.is_null() {
        return false;
    }
    match CStr::from_ptr(text).to_str().ok().and_then(|s| s.trim().parse::<f64>().ok()) {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

unsafe extern "C" fn params_flush(p: *const clap_plugin, inp: *const clap_input_events, _out: *const clap_output_events) {
    let Some(list) = inp.as_ref() else { return };
    let (Some(size), Some(get)) = (list.size, list.get) else { return };
    let st = state(p);
    for i in 0..size(list) {
        handle_event(st, None, get(list, i));
    }
}

// ------------------------------------------------------------- audio ports

static AUDIO_PORTS: clap_plugin_audio_ports = clap_plugin_audio_ports { count: Some(audio_ports_count), get: Some(audio_ports_get) };

unsafe extern "C" fn audio_ports_count(p: *const clap_plugin, is_input: bool) -> u32 {
    match (state(p).kind, is_input) {
        (Kind::Sine, true) => 0,
        _ => 1,
    }
}

unsafe extern "C" fn audio_ports_get(p: *const clap_plugin, index: u32, is_input: bool, info: *mut clap_audio_port_info) -> bool {
    if index >= audio_ports_count(p, is_input) || info.is_null() {
        return false;
    }
    let info = &mut *info;
    info.id = 0;
    write_cstr(info.name.as_mut_ptr(), info.name.len(), if is_input { "Input" } else { "Output" });
    info.flags = CLAP_AUDIO_PORT_IS_MAIN;
    info.channel_count = 2;
    info.port_type = CLAP_PORT_STEREO.as_ptr();
    info.in_place_pair = if state(p).kind == Kind::Gain { 0 } else { CLAP_INVALID_ID };
    true
}

// -------------------------------------------------------------- note ports

static NOTE_PORTS: clap_plugin_note_ports = clap_plugin_note_ports { count: Some(note_ports_count), get: Some(note_ports_get) };

unsafe extern "C" fn note_ports_count(_p: *const clap_plugin, is_input: bool) -> u32 {
    u32::from(is_input)
}

unsafe extern "C" fn note_ports_get(_p: *const clap_plugin, index: u32, is_input: bool, info: *mut clap_note_port_info) -> bool {
    if !is_input || index != 0 || info.is_null() {
        return false;
    }
    let info = &mut *info;
    info.id = 0;
    info.supported_dialects = CLAP_NOTE_DIALECT_CLAP;
    info.preferred_dialect = CLAP_NOTE_DIALECT_CLAP;
    write_cstr(info.name.as_mut_ptr(), info.name.len(), "Notes");
    true
}
