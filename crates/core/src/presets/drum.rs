//! Factory presets for the `drum` engine (Atelier).

use super::Preset;

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "House Kick",
        kind: "drum",
        tags: "drums, kick",
        doc: "Punchy club kick.",
        params: &[
            ("tone", 0.35),
            ("snap", 0.55),
            ("decay", 1.1),
            ("tune", -1.0),
        ],
        options: &[("kind", "kick")],
    },
    Preset {
        name: "Satin Clap",
        kind: "drum",
        tags: "drums, clap",
        doc: "Wide layered clap.",
        params: &[("tone", 0.45), ("decay", 1.2)],
        options: &[("kind", "clap")],
    },
    Preset {
        name: "Champagne Hat",
        kind: "drum",
        tags: "drums, hat",
        doc: "Crisp closed hi-hat.",
        params: &[("tone", 0.62), ("decay", 0.9)],
        options: &[("kind", "hat")],
    },
];
