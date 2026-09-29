//! The device catalog: every built-in instrument and effect, with the legal
//! keys, ranges and defaults of its parameters.
//!
//! This table is the single source of truth for validation, the generated JSON
//! schema, the agent guide and the knobs rendered by the UI.

use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Instrument,
    Effect,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Curve {
    /// Evenly spaced values.
    Linear,
    /// Logarithmic knob travel (frequencies, times).
    Exp,
}

#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ParamSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub unit: &'static str,
    pub curve: Curve,
    /// Whole numbers only.
    pub integer: bool,
    pub doc: &'static str,
}

#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OptionSpec {
    pub key: &'static str,
    pub label: &'static str,
    /// Allowed values. Empty means free text (e.g. a file path).
    pub choices: &'static [&'static str],
    pub default: &'static str,
    pub doc: &'static str,
}

#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSpec {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub label: &'static str,
    pub category: Category,
    pub doc: &'static str,
    pub params: &'static [ParamSpec],
    pub options: &'static [OptionSpec],
    /// Parameters are free-form (plugin parameter ids) and are not validated
    /// against this spec.
    pub open_params: bool,
}

impl DeviceSpec {
    pub fn param(&self, key: &str) -> Option<&'static ParamSpec> {
        self.params.iter().find(|p| p.key == key)
    }
    pub fn option(&self, key: &str) -> Option<&'static OptionSpec> {
        self.options.iter().find(|o| o.key == key)
    }
}

const fn p(key: &'static str, label: &'static str, min: f64, max: f64, default: f64, unit: &'static str, doc: &'static str) -> ParamSpec {
    ParamSpec { key, label, min, max, default, unit, curve: Curve::Linear, integer: false, doc }
}
const fn pe(key: &'static str, label: &'static str, min: f64, max: f64, default: f64, unit: &'static str, doc: &'static str) -> ParamSpec {
    ParamSpec { key, label, min, max, default, unit, curve: Curve::Exp, integer: false, doc }
}
const fn pi(key: &'static str, label: &'static str, min: f64, max: f64, default: f64, unit: &'static str, doc: &'static str) -> ParamSpec {
    ParamSpec { key, label, min, max, default, unit, curve: Curve::Linear, integer: true, doc }
}
const fn o(key: &'static str, label: &'static str, choices: &'static [&'static str], default: &'static str, doc: &'static str) -> OptionSpec {
    OptionSpec { key, label, choices, default, doc }
}

pub const WAVES: &[&str] = &["sine", "triangle", "saw", "square", "noise"];
pub const FILTER_MODES: &[&str] = &["lowpass", "highpass", "bandpass"];
pub const DRUM_KINDS: &[&str] = &["kick", "snare", "clap", "hat", "openhat", "tom", "rim", "cowbell", "shaker"];
pub const PLUGIN_FORMATS: &[&str] = &["clap", "vst3", "lv2"];

const ENV: [ParamSpec; 4] = [
    pe("attack", "Attack", 0.001, 8.0, 0.005, "s", "Amplitude envelope attack time."),
    pe("decay", "Decay", 0.001, 8.0, 0.3, "s", "Amplitude envelope decay time."),
    p("sustain", "Sustain", 0.0, 1.0, 0.7, "", "Amplitude envelope sustain level."),
    pe("release", "Release", 0.001, 10.0, 0.25, "s", "Amplitude envelope release time."),
];

pub static DEVICES: &[DeviceSpec] = &[
    // ---------------------------------------------------------------- instruments
    DeviceSpec {
        kind: "synth",
        label: "Aurum",
        category: Category::Instrument,
        doc: "Two-oscillator subtractive synthesizer with unison, sub oscillator, resonant filter and envelopes. Basses, leads, pads, stabs.",
        params: &[
            pi("osc2Semi", "Osc 2 Semi", -24.0, 24.0, 0.0, "st", "Oscillator 2 transpose in semitones."),
            p("osc2Detune", "Osc 2 Fine", -100.0, 100.0, 7.0, "ct", "Oscillator 2 fine tune in cents."),
            p("osc2Mix", "Osc 2 Mix", 0.0, 1.0, 0.5, "", "Oscillator 2 level relative to oscillator 1."),
            p("sub", "Sub", 0.0, 1.0, 0.0, "", "Sine sub-oscillator one octave down."),
            pi("unison", "Unison", 1.0, 7.0, 1.0, "", "Voices stacked per note."),
            p("spread", "Spread", 0.0, 100.0, 12.0, "ct", "Unison detune spread in cents."),
            pe("cutoff", "Cutoff", 20.0, 20000.0, 2400.0, "Hz", "Filter cutoff frequency."),
            p("resonance", "Reso", 0.0, 1.0, 0.2, "", "Filter resonance."),
            p("filterEnv", "Env Amt", -1.0, 1.0, 0.35, "", "Filter envelope depth (±1 = ±6 octaves)."),
            pe("filterDecay", "Env Decay", 0.001, 8.0, 0.35, "s", "Filter envelope decay time."),
            pe("glide", "Glide", 0.0001, 2.0, 0.0001, "s", "Portamento time between successive notes."),
            ENV[0], ENV[1], ENV[2], ENV[3],
            p("gain", "Gain", 0.0, 1.5, 0.6, "", "Output level."),
        ],
        options: &[
            o("wave1", "Osc 1", WAVES, "saw", "Oscillator 1 waveform."),
            o("wave2", "Osc 2", WAVES, "saw", "Oscillator 2 waveform."),
            o("filter", "Filter", FILTER_MODES, "lowpass", "Filter mode."),
        ],
        open_params: false,
    },
    DeviceSpec {
        kind: "fm",
        label: "Lumière",
        category: Category::Instrument,
        doc: "Two-operator FM synthesizer. Electric pianos, bells, mallets, glassy keys and plucked basses.",
        params: &[
            p("ratio", "Ratio", 0.25, 16.0, 2.0, "", "Modulator frequency ratio (integers sound harmonic)."),
            p("index", "Index", 0.0, 20.0, 3.0, "", "Modulation depth / brightness."),
            pe("indexDecay", "Idx Decay", 0.01, 8.0, 0.6, "s", "How quickly the brightness fades."),
            p("feedback", "Feedback", 0.0, 1.0, 0.0, "", "Modulator self-feedback (adds grit)."),
            p("velocity", "Vel Sens", 0.0, 1.0, 0.6, "", "How much velocity affects brightness."),
            p("detune", "Detune", 0.0, 30.0, 4.0, "ct", "Chorus-like detune of a second carrier."),
            ENV[0], ENV[1], ENV[2], ENV[3],
            p("gain", "Gain", 0.0, 1.5, 0.6, "", "Output level."),
        ],
        options: &[],
        open_params: false,
    },
    DeviceSpec {
        kind: "drum",
        label: "Atelier",
        category: Category::Instrument,
        doc: "Synthesized drum voice. Choose the sound with options.kind. The note pitch shifts the tune around 60 (C4); drum notes are one-shots, so their length does not matter.",
        params: &[
            p("tune", "Tune", -24.0, 24.0, 0.0, "st", "Pitch offset in semitones."),
            pe("decay", "Decay", 0.1, 4.0, 1.0, "×", "Decay time multiplier."),
            p("tone", "Tone", 0.0, 1.0, 0.5, "", "Darker (0) to brighter (1)."),
            p("snap", "Snap", 0.0, 1.0, 0.5, "", "Transient / click amount."),
            p("drive", "Drive", 0.0, 1.0, 0.0, "", "Saturation."),
            p("gain", "Gain", 0.0, 1.5, 0.8, "", "Output level."),
        ],
        options: &[o("kind", "Kind", DRUM_KINDS, "kick", "Which drum to synthesize.")],
        open_params: false,
    },
    DeviceSpec {
        kind: "sampler",
        label: "Vault",
        category: Category::Instrument,
        doc: "Sample player. options.sample is a project-relative audio file (wav, flac, mp3, ogg). In 'pitched' mode the note pitch transposes relative to 'root'; in 'oneshot' mode every note plays the sample at its original pitch to the end.",
        params: &[
            pi("root", "Root", 0.0, 127.0, 60.0, "", "MIDI note that plays the sample at original pitch."),
            p("tune", "Tune", -24.0, 24.0, 0.0, "st", "Transpose in semitones."),
            p("start", "Start", 0.0, 1.0, 0.0, "", "Playback start, as a fraction of the sample."),
            p("end", "End", 0.0, 1.0, 1.0, "", "Playback end, as a fraction of the sample."),
            pe("attack", "Attack", 0.001, 8.0, 0.001, "s", "Fade-in time."),
            pe("release", "Release", 0.001, 10.0, 0.05, "s", "Fade-out time after note end."),
            p("gain", "Gain", 0.0, 2.0, 0.8, "", "Output level."),
        ],
        options: &[
            o("sample", "Sample", &[], "", "Project-relative path of the audio file, e.g. samples/vox.wav."),
            o("mode", "Mode", &["pitched", "oneshot", "loop"], "pitched", "Playback mode."),
        ],
        open_params: false,
    },
    DeviceSpec {
        kind: "plugin",
        label: "Plugin",
        category: Category::Instrument,
        doc: "An external audio plugin (CLAP today; VST3/LV2 reserved). options.path is the plugin bundle, options.id the plugin id inside it. params maps plugin parameter ids (as strings) to plain values. Only playable by the native engine.",
        params: &[],
        options: &[
            o("format", "Format", PLUGIN_FORMATS, "clap", "Plugin format."),
            o("path", "Path", &[], "", "Absolute path of the plugin bundle/binary."),
            o("id", "Plugin id", &[], "", "Plugin identifier inside the bundle."),
        ],
        open_params: true,
    },
    // ---------------------------------------------------------------- effects
    DeviceSpec {
        kind: "eq",
        label: "Vernis EQ",
        category: Category::Effect,
        doc: "Three-band equalizer: low shelf, bell, high shelf.",
        params: &[
            p("low", "Low", -18.0, 18.0, 0.0, "dB", "Low shelf gain."),
            pe("lowFreq", "Low Freq", 30.0, 600.0, 120.0, "Hz", "Low shelf frequency."),
            p("mid", "Mid", -18.0, 18.0, 0.0, "dB", "Bell gain."),
            pe("midFreq", "Mid Freq", 150.0, 10000.0, 1000.0, "Hz", "Bell centre frequency."),
            pe("midQ", "Mid Q", 0.3, 8.0, 0.9, "", "Bell width (higher = narrower)."),
            p("high", "High", -18.0, 18.0, 0.0, "dB", "High shelf gain."),
            pe("highFreq", "High Freq", 1500.0, 18000.0, 6000.0, "Hz", "High shelf frequency."),
        ],
        options: &[],
        open_params: false,
    },
    DeviceSpec {
        kind: "filter",
        label: "Soie Filter",
        category: Category::Effect,
        doc: "Resonant state-variable filter.",
        params: &[
            pe("cutoff", "Cutoff", 20.0, 20000.0, 8000.0, "Hz", "Cutoff frequency."),
            p("resonance", "Reso", 0.0, 1.0, 0.2, "", "Resonance."),
            p("mix", "Mix", 0.0, 1.0, 1.0, "", "Dry/wet."),
        ],
        options: &[o("mode", "Mode", FILTER_MODES, "lowpass", "Filter mode.")],
        open_params: false,
    },
    DeviceSpec {
        kind: "delay",
        label: "Écho",
        category: Category::Effect,
        doc: "Tempo-synced stereo delay. time is in beats (0.75 = dotted eighth).",
        params: &[
            pe("time", "Time", 0.0625, 4.0, 0.75, "beats", "Delay time in beats."),
            p("feedback", "Feedback", 0.0, 0.95, 0.35, "", "Repeats."),
            pe("tone", "Tone", 300.0, 18000.0, 5000.0, "Hz", "Low-pass on the repeats."),
            p("mix", "Mix", 0.0, 1.0, 0.25, "", "Dry/wet."),
        ],
        options: &[o("mode", "Mode", &["stereo", "pingpong"], "pingpong", "Stereo or ping-pong repeats.")],
        open_params: false,
    },
    DeviceSpec {
        kind: "reverb",
        label: "Cathédrale",
        category: Category::Effect,
        doc: "Lush algorithmic reverb.",
        params: &[
            p("size", "Size", 0.0, 1.0, 0.7, "", "Room size / decay."),
            p("damping", "Damping", 0.0, 1.0, 0.4, "", "High-frequency absorption."),
            p("width", "Width", 0.0, 1.0, 1.0, "", "Stereo width."),
            pe("predelay", "Pre-delay", 0.0001, 0.25, 0.012, "s", "Gap before the reverb tail."),
            p("mix", "Mix", 0.0, 1.0, 0.25, "", "Dry/wet."),
        ],
        options: &[],
        open_params: false,
    },
    DeviceSpec {
        kind: "chorus",
        label: "Chœur",
        category: Category::Effect,
        doc: "Stereo chorus for width and shimmer.",
        params: &[
            pe("rate", "Rate", 0.05, 8.0, 0.6, "Hz", "Modulation speed."),
            p("depth", "Depth", 0.0, 1.0, 0.5, "", "Modulation depth."),
            p("mix", "Mix", 0.0, 1.0, 0.4, "", "Dry/wet."),
        ],
        options: &[],
        open_params: false,
    },
    DeviceSpec {
        kind: "drive",
        label: "Velours Drive",
        category: Category::Effect,
        doc: "Warm tube-style saturation.",
        params: &[
            p("amount", "Drive", 0.0, 1.0, 0.3, "", "Saturation amount."),
            pe("tone", "Tone", 300.0, 18000.0, 9000.0, "Hz", "Post-drive low-pass."),
            p("mix", "Mix", 0.0, 1.0, 1.0, "", "Dry/wet."),
            p("output", "Output", 0.0, 1.5, 0.8, "", "Output level."),
        ],
        options: &[],
        open_params: false,
    },
    DeviceSpec {
        kind: "compressor",
        label: "Couronne Comp",
        category: Category::Effect,
        doc: "Feed-forward compressor.",
        params: &[
            p("threshold", "Threshold", -60.0, 0.0, -18.0, "dB", "Level where compression starts."),
            pe("ratio", "Ratio", 1.0, 20.0, 4.0, ":1", "Compression ratio."),
            pe("attack", "Attack", 0.1, 200.0, 10.0, "ms", "Attack time."),
            pe("release", "Release", 5.0, 2000.0, 120.0, "ms", "Release time."),
            p("makeup", "Makeup", 0.0, 24.0, 3.0, "dB", "Output gain."),
        ],
        options: &[],
        open_params: false,
    },
    DeviceSpec {
        kind: "limiter",
        label: "Sceptre Limiter",
        category: Category::Effect,
        doc: "Brickwall-style peak limiter; keep one on the master.",
        params: &[
            p("gain", "Input", 0.0, 24.0, 0.0, "dB", "Input gain into the limiter."),
            p("ceiling", "Ceiling", -12.0, 0.0, -0.3, "dB", "Maximum output level."),
            pe("release", "Release", 5.0, 1000.0, 80.0, "ms", "Release time."),
        ],
        options: &[],
        open_params: false,
    },
    DeviceSpec {
        kind: "plugin",
        label: "Plugin",
        category: Category::Effect,
        doc: "An external audio effect plugin (see the instrument 'plugin' type).",
        params: &[],
        options: &[
            o("format", "Format", PLUGIN_FORMATS, "clap", "Plugin format."),
            o("path", "Path", &[], "", "Absolute path of the plugin bundle/binary."),
            o("id", "Plugin id", &[], "", "Plugin identifier inside the bundle."),
        ],
        open_params: true,
    },
];

/// Look up a device (instrument or effect) by type.
pub fn device(kind: &str) -> Option<&'static DeviceSpec> {
    DEVICES.iter().find(|d| d.kind == kind)
}

/// Look up a device of a given category.
pub fn device_in(kind: &str, category: Category) -> Option<&'static DeviceSpec> {
    DEVICES.iter().find(|d| d.kind == kind && d.category == category)
}

pub fn instruments() -> impl Iterator<Item = &'static DeviceSpec> {
    DEVICES.iter().filter(|d| d.category == Category::Instrument)
}

pub fn effects() -> impl Iterator<Item = &'static DeviceSpec> {
    DEVICES.iter().filter(|d| d.category == Category::Effect)
}
