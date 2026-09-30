//! Factory presets for the `dedale` engine (generative sequencer).

use super::Preset;

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Rouages de Platine",
        kind: "dedale",
        tags: "arp, pluck, clockwork",
        doc: "Clockwork arpeggio: a dense, precise euclidean pattern of bright plucks that barely changes from bar to bar.",
        params: &[("pulses", 12.0), ("density", 1.0), ("range", 1.0), ("variation", 0.1), ("gate", 0.35), ("accent", 0.55), ("seed", 12.0), ("tone", 0.65), ("decay", 0.25)],
        options: &[("scale", "dorian")],
    },
    Preset {
        name: "Pluie de Perles",
        kind: "dedale",
        tags: "bells, generative, pentatonic",
        doc: "A pentatonic rain of FM bells falling across two octaves, never quite the same twice.",
        params: &[("density", 0.7), ("range", 2.0), ("variation", 0.45), ("gate", 1.0), ("accent", 0.3), ("seed", 31.0), ("tone", 0.6), ("decay", 1.4), ("gain", 0.5)],
        options: &[("scale", "pentatonic"), ("voice", "bell")],
    },
    Preset {
        name: "Transe de Jais",
        kind: "dedale",
        tags: "bass, hypnotic, sequence",
        doc: "Hypnotic minor bassline: a tight nine-over-sixteen pattern that stays close to the root.",
        params: &[("pulses", 9.0), ("density", 0.95), ("range", 0.6), ("variation", 0.15), ("gate", 0.6), ("accent", 0.6), ("seed", 3.0), ("tone", 0.35), ("decay", 0.3), ("gain", 0.7)],
        options: &[("voice", "bass")],
    },
    Preset {
        name: "Rituel de Malachite",
        kind: "dedale",
        tags: "percussion, ritual, sparse",
        doc: "Sparse percussion ritual: five pitched, metallic hits spread over the bar, slowly shifting.",
        params: &[("pulses", 5.0), ("density", 0.8), ("range", 1.0), ("variation", 0.4), ("gate", 0.3), ("accent", 0.7), ("seed", 77.0), ("tone", 0.45), ("decay", 0.5), ("gain", 0.8)],
        options: &[("scale", "phrygian"), ("voice", "perc")],
    },
    Preset {
        name: "Crypte de Grenat",
        kind: "dedale",
        tags: "sequence, dark, phrygian",
        doc: "Dark phrygian sequence: muted plucks in a twelve-step cycle, circling the root with a menacing half step.",
        params: &[("steps", 12.0), ("range", 1.2), ("gate", 0.6), ("accent", 0.5), ("seed", 13.0), ("tone", 0.25), ("decay", 0.45)],
        options: &[("scale", "phrygian")],
    },
    Preset {
        name: "Étincelles de Diamant",
        kind: "dedale",
        tags: "sparkle, bright, major",
        doc: "Bright major-key sparkle: quick glassy bells scattered high over a wide range.",
        params: &[("rate", 0.125), ("pulses", 9.0), ("density", 0.75), ("range", 2.5), ("variation", 0.5), ("accent", 0.3), ("seed", 101.0), ("tone", 0.9), ("decay", 0.5), ("gain", 0.45)],
        options: &[("scale", "major"), ("voice", "bell")],
    },
];
