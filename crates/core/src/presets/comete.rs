//! Factory presets for the `comete` engine (transition effects).

use super::Preset;

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Ascension d'Or",
        kind: "comete",
        tags: "riser, build, fx",
        doc: "Two-bar riser: noise and a detuned tone climbing two octaves as the filter opens into the drop.",
        params: &[("intensity", 0.7), ("noise", 0.55), ("tone", 0.6)],
        options: &[],
    },
    Preset {
        name: "Voie Lactée",
        kind: "comete",
        tags: "riser, long, cinematic",
        doc: "Four-bar ascent: a slow, wide climb of three octaves with a long shimmering tail.",
        params: &[("length", 16.0), ("intensity", 0.85), ("pitch", 36.0), ("noise", 0.5), ("space", 0.65), ("drive", 0.25)],
        options: &[],
    },
    Preset {
        name: "Météore de Basalte",
        kind: "comete",
        tags: "impact, boom, cinematic",
        doc: "Impact: a deep sub boom, a burst of noise and a metallic strike ringing into a long dark tail.",
        params: &[("intensity", 0.8), ("noise", 0.5), ("tone", 0.4), ("space", 0.7), ("drive", 0.35)],
        options: &[("kind", "impact")],
    },
    Preset {
        name: "Déclin de Lune",
        kind: "comete",
        tags: "downlifter, fx",
        doc: "One-bar downlifter: a bright wash that falls two octaves and fades out after the drop.",
        params: &[("length", 4.0), ("noise", 0.7), ("space", 0.6)],
        options: &[("kind", "downlifter")],
    },
    Preset {
        name: "Vent Solaire",
        kind: "comete",
        tags: "sweep, noise, fx",
        doc: "White-noise sweep that opens up and closes again while drifting across the stereo field.",
        params: &[("intensity", 0.7), ("noise", 1.0), ("tone", 0.7)],
        options: &[("kind", "sweep")],
    },
    Preset {
        name: "Abîme d'Encre",
        kind: "comete",
        tags: "subdrop, sub, fx",
        doc: "One-bar sub drop: a warm sine falling two octaves into the floor.",
        params: &[("length", 4.0), ("noise", 0.1), ("tone", 0.3), ("space", 0.2), ("drive", 0.3)],
        options: &[("kind", "subdrop")],
    },
];
