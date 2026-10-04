//! Factory presets for the `granular` engine.

use super::Preset;

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Amber Ember Drone",
        kind: "granular",
        tags: "keys, drone, dark, cinematic",
        doc: "Smouldering ember hum and crackle, scattered into a dark, glowing drone that answers every key.",
        params: &[("position", 0.45), ("spray", 0.3), ("grainSize", 0.2), ("density", 24.0), ("drift", 0.4), ("reverse", 0.2), ("cutoff", 3500.0), ("attack", 0.03), ("release", 3.0), ("gain", 0.9)],
        options: &[("source", "ember")],
    },
    Preset {
        name: "Singing Bowl Grotto",
        kind: "granular",
        tags: "pad, bowls, meditative",
        doc: "Singing-bowl partials smeared into a cavernous, slowly beating cloud.",
        params: &[("position", 0.15), ("spray", 0.35), ("grainSize", 0.3), ("density", 14.0), ("drift", 0.3), ("width", 0.9), ("cutoff", 7000.0), ("attack", 0.6), ("release", 5.0)],
        options: &[("source", "bowl")],
    },
    Preset {
        name: "Veiled Choir",
        kind: "granular",
        tags: "choir, pad, wash",
        doc: "A wide, drifting wash of \"aah\" voices built from long overlapping grains.",
        params: &[("spray", 0.4), ("grainSize", 0.25), ("density", 30.0), ("drift", 0.5), ("width", 0.9), ("cutoff", 6000.0), ("attack", 1.2), ("release", 4.0)],
        options: &[("source", "choir")],
    },
    Preset {
        name: "Opal Breath Texture",
        kind: "granular",
        tags: "texture, air, breathing",
        doc: "Breathing, faintly pitched air that swells and recedes like a slow tide.",
        params: &[("position", 0.5), ("spray", 0.6), ("grainSize", 0.18), ("density", 36.0), ("drift", 0.6), ("width", 1.0), ("cutoff", 8000.0), ("attack", 1.5), ("release", 3.0), ("gain", 0.9)],
        options: &[("source", "air")],
    },
    Preset {
        name: "Frosted Strings",
        kind: "granular",
        tags: "strings, pad, frozen",
        doc: "A string ensemble frozen in place: long, nearly still grains with a touch of reversal.",
        params: &[("position", 0.5), ("spray", 0.05), ("grainSize", 0.35), ("density", 30.0), ("drift", 0.1), ("reverse", 0.3), ("width", 0.8), ("cutoff", 5000.0), ("attack", 1.0), ("release", 4.0)],
        options: &[("source", "strings")],
    },
    Preset {
        name: "Stardust Shimmer Pad",
        kind: "granular",
        tags: "pad, shimmer, glitter",
        doc: "Tiny, dense bowl grains flickering forwards and backwards into a glittering halo.",
        params: &[("position", 0.25), ("spray", 0.8), ("grainSize", 0.05), ("density", 70.0), ("scatter", 0.2), ("drift", 0.5), ("width", 1.0), ("reverse", 0.5), ("cutoff", 14000.0), ("attack", 0.8), ("release", 3.5)],
        options: &[("source", "bowl")],
    },
];
