//! General MIDI: program names and the drum kits of the built-in soundfont.
//!
//! The `soundfont` instrument's `program` option is one of these names: the
//! 128 GM melodic programs (bank 0) or a drum kit (bank 128).

/// The General MIDI program names, by program number.
pub const PROGRAMS: [&str; 128] = [
    "Acoustic Grand Piano",
    "Bright Acoustic Piano",
    "Electric Grand Piano",
    "Honky-tonk Piano",
    "Electric Piano 1",
    "Electric Piano 2",
    "Harpsichord",
    "Clavinet",
    "Celesta",
    "Glockenspiel",
    "Music Box",
    "Vibraphone",
    "Marimba",
    "Xylophone",
    "Tubular Bells",
    "Dulcimer",
    "Drawbar Organ",
    "Percussive Organ",
    "Rock Organ",
    "Church Organ",
    "Reed Organ",
    "Accordion",
    "Harmonica",
    "Tango Accordion",
    "Acoustic Guitar (nylon)",
    "Acoustic Guitar (steel)",
    "Electric Guitar (jazz)",
    "Electric Guitar (clean)",
    "Electric Guitar (muted)",
    "Overdriven Guitar",
    "Distortion Guitar",
    "Guitar Harmonics",
    "Acoustic Bass",
    "Electric Bass (finger)",
    "Electric Bass (pick)",
    "Fretless Bass",
    "Slap Bass 1",
    "Slap Bass 2",
    "Synth Bass 1",
    "Synth Bass 2",
    "Violin",
    "Viola",
    "Cello",
    "Contrabass",
    "Tremolo Strings",
    "Pizzicato Strings",
    "Orchestral Harp",
    "Timpani",
    "String Ensemble 1",
    "String Ensemble 2",
    "Synth Strings 1",
    "Synth Strings 2",
    "Choir Aahs",
    "Voice Oohs",
    "Synth Voice",
    "Orchestra Hit",
    "Trumpet",
    "Trombone",
    "Tuba",
    "Muted Trumpet",
    "French Horn",
    "Brass Section",
    "Synth Brass 1",
    "Synth Brass 2",
    "Soprano Sax",
    "Alto Sax",
    "Tenor Sax",
    "Baritone Sax",
    "Oboe",
    "English Horn",
    "Bassoon",
    "Clarinet",
    "Piccolo",
    "Flute",
    "Recorder",
    "Pan Flute",
    "Blown Bottle",
    "Shakuhachi",
    "Whistle",
    "Ocarina",
    "Lead 1 (square)",
    "Lead 2 (sawtooth)",
    "Lead 3 (calliope)",
    "Lead 4 (chiff)",
    "Lead 5 (charang)",
    "Lead 6 (voice)",
    "Lead 7 (fifths)",
    "Lead 8 (bass + lead)",
    "Pad 1 (new age)",
    "Pad 2 (warm)",
    "Pad 3 (polysynth)",
    "Pad 4 (choir)",
    "Pad 5 (bowed)",
    "Pad 6 (metallic)",
    "Pad 7 (halo)",
    "Pad 8 (sweep)",
    "FX 1 (rain)",
    "FX 2 (soundtrack)",
    "FX 3 (crystal)",
    "FX 4 (atmosphere)",
    "FX 5 (brightness)",
    "FX 6 (goblins)",
    "FX 7 (echoes)",
    "FX 8 (sci-fi)",
    "Sitar",
    "Banjo",
    "Shamisen",
    "Koto",
    "Kalimba",
    "Bagpipe",
    "Fiddle",
    "Shanai",
    "Tinkle Bell",
    "Agogo",
    "Steel Drums",
    "Woodblock",
    "Taiko Drum",
    "Melodic Tom",
    "Synth Drum",
    "Reverse Cymbal",
    "Guitar Fret Noise",
    "Breath Noise",
    "Seashore",
    "Bird Tweet",
    "Telephone Ring",
    "Helicopter",
    "Applause",
    "Gunshot",
];

/// Drum kits (bank 128): name and program number. Notes play the General
/// MIDI drum map (36 kick, 38 snare, 42 closed hat, ...).
pub const KITS: [(&str, u8); 8] = [
    ("Standard Kit", 0),
    ("Room Kit", 8),
    ("Power Kit", 16),
    ("Electronic Kit", 24),
    ("TR-808 Kit", 25),
    ("Jazz Kit", 32),
    ("Brush Kit", 40),
    ("Orchestra Kit", 48),
];

/// The drum bank.
pub const DRUM_BANK: u16 = 128;

/// Every legal value of the `program` option: the programs, then the kits.
pub const CHOICES: [&str; 136] = {
    let mut out = [""; 136];
    let mut i = 0;
    while i < 128 {
        out[i] = PROGRAMS[i];
        i += 1;
    }
    while i < 136 {
        out[i] = KITS[i - 128].0;
        i += 1;
    }
    out
};

/// Bank and program of a `program` option value.
pub fn lookup(name: &str) -> Option<(u16, u8)> {
    if let Some(p) = PROGRAMS.iter().position(|n| *n == name) {
        return Some((0, p as u8));
    }
    KITS.iter().find(|k| k.0 == name).map(|k| (DRUM_BANK, k.1))
}

/// The kit for a program change on the MIDI drum channel (the nearest kit
/// at or below the program, as GM2 synthesizers do).
pub fn kit(program: u8) -> &'static str {
    KITS.iter()
        .rev()
        .find(|k| k.1 <= program)
        .map(|k| k.0)
        .unwrap_or(KITS[0].0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_resolve() {
        assert_eq!(lookup("Acoustic Grand Piano"), Some((0, 0)));
        assert_eq!(lookup("Gunshot"), Some((0, 127)));
        assert_eq!(lookup("Brush Kit"), Some((128, 40)));
        assert_eq!(lookup("nope"), None);
        assert!(CHOICES.iter().all(|c| lookup(c).is_some()));
        assert_eq!(kit(26), "TR-808 Kit");
    }
}
