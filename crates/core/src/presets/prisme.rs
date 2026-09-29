//! Factory presets for the `prisme` engine.

use super::Preset;

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Opaline Veil",
        kind: "prisme",
        tags: "pad, pristine",
        doc: "Soft, slowly breathing saw spectrum in three detuned layers; a clean, luminous bed.",
        params: &[("partials", 48.0), ("brightness", 0.45), ("cutoff", 3200.0), ("resonance", 0.1), ("filterEnv", 0.1), ("filterDecay", 3.0), ("spectralDecay", 0.1), ("shimmer", 0.45), ("unison", 3.0), ("detune", 12.0), ("attack", 1.2), ("decay", 2.0), ("sustain", 0.85), ("release", 2.5), ("gain", 0.7)],
        options: &[],
    },
    Preset {
        name: "Rosée de Cristal",
        kind: "prisme",
        tags: "bell, glass",
        doc: "Sparse, stretched glass partials that ring like water-struck crystal and fade from the top down.",
        params: &[("partials", 24.0), ("brightness", 0.9), ("cutoff", 14000.0), ("resonance", 0.0), ("filterEnv", 0.0), ("stretch", 0.25), ("spectralDecay", 0.7), ("shimmer", 0.1), ("attack", 0.002), ("decay", 3.0), ("sustain", 0.0), ("release", 2.0), ("gain", 0.75)],
        options: &[("spectrum", "glass")],
    },
    Preset {
        name: "Séraphine",
        kind: "prisme",
        tags: "choir, pad",
        doc: "Formant-shaped \"aah\" spectrum with shimmering partials and a gentle ensemble spread.",
        params: &[("partials", 64.0), ("brightness", 0.7), ("cutoff", 6000.0), ("resonance", 0.0), ("filterEnv", 0.0), ("spectralDecay", 0.0), ("shimmer", 0.55), ("unison", 3.0), ("detune", 14.0), ("attack", 0.7), ("decay", 1.5), ("sustain", 0.9), ("release", 1.8), ("gain", 0.75)],
        options: &[("spectrum", "choir")],
    },
    Preset {
        name: "Nef d'Ivoire",
        kind: "prisme",
        tags: "organ, keys",
        doc: "Drawbar-style organ registration, steady and bright, with a whisper of chorus.",
        params: &[("partials", 16.0), ("brightness", 0.85), ("cutoff", 12000.0), ("resonance", 0.0), ("filterEnv", 0.0), ("spectralDecay", 0.0), ("shimmer", 0.05), ("unison", 2.0), ("detune", 4.0), ("attack", 0.006), ("decay", 0.3), ("sustain", 1.0), ("release", 0.12), ("gain", 0.6)],
        options: &[("spectrum", "organ")],
    },
    Preset {
        name: "Harpe de Saphir",
        kind: "prisme",
        tags: "pluck, harp",
        doc: "Plucked string: bright attack whose upper partials die away first, leaving a round core.",
        params: &[("partials", 40.0), ("brightness", 0.6), ("cutoff", 2600.0), ("resonance", 0.05), ("filterEnv", 0.35), ("filterDecay", 0.3), ("oddEven", -0.2), ("stretch", 0.05), ("spectralDecay", 0.8), ("shimmer", 0.0), ("attack", 0.0015), ("decay", 2.2), ("sustain", 0.0), ("release", 1.2), ("gain", 0.75)],
        options: &[],
    },
    Preset {
        name: "Aurore Spectrale",
        kind: "prisme",
        tags: "pad, evolving, cinematic",
        doc: "A resonant partial filter slowly rises from darkness to light over eight seconds while the partials shimmer.",
        params: &[("partials", 64.0), ("brightness", 0.75), ("cutoff", 6000.0), ("resonance", 0.7), ("filterEnv", -0.7), ("filterDecay", 8.0), ("oddEven", -0.3), ("spectralDecay", 0.0), ("shimmer", 0.7), ("unison", 2.0), ("detune", 10.0), ("attack", 1.5), ("decay", 3.0), ("sustain", 0.9), ("release", 3.0), ("gain", 0.6)],
        options: &[],
    },
];
