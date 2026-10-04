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

/// More drum kits (bank 128), variants of the kits above; they play the
/// General MIDI drum map too.
pub const MORE_KITS: [(&str, u8); 23] = [
    ("Standard Kit 1", 1),
    ("Standard Kit 2", 2),
    ("Standard Kit 3", 3),
    ("Standard Kit 4", 4),
    ("Standard Kit 5", 5),
    ("Standard Kit 6", 6),
    ("Standard Kit 7", 7),
    ("Room Kit 1", 9),
    ("Room Kit 2", 10),
    ("Room Kit 3", 11),
    ("Room Kit 4", 12),
    ("Room Kit 5", 13),
    ("Room Kit 6", 14),
    ("Room Kit 7", 15),
    ("Power Kit 1", 17),
    ("Power Kit 2", 18),
    ("Power Kit 3", 19),
    ("Jazz Kit 1", 33),
    ("Jazz Kit 2", 34),
    ("Jazz Kit 3", 35),
    ("Jazz Kit 4", 36),
    ("Brush Kit 1", 41),
    ("Brush Kit 2", 42),
];

/// The soundfont's other instruments: variations of the General MIDI
/// programs on higher banks, orchestral sections and marching percussion
/// (bank 128, not on the drum map). Name, bank, program.
pub const VARIATIONS: [(&str, u16, u8); 58] = [
    ("Mellow Grand Piano", 8, 0),
    ("Detuned Tine EP", 8, 4),
    ("Detuned FM EP", 8, 5),
    ("Coupled Harpsichord", 8, 6),
    ("Church Bell", 8, 14),
    ("Detuned Organ 1", 8, 16),
    ("Detuned Organ 2", 8, 17),
    ("Church Organ 2", 8, 19),
    ("Italian Accordion", 8, 21),
    ("Ukulele", 8, 24),
    ("12-String Guitar", 8, 25),
    ("Mandolin", 16, 25),
    ("Hawaiian Guitar", 8, 26),
    ("Funk Guitar", 8, 28),
    ("Feedback Guitar", 8, 30),
    ("Guitar Feedback", 8, 31),
    ("Synth Bass 3", 8, 38),
    ("Synth Bass 4", 8, 39),
    ("Slow Violin", 8, 40),
    ("Violins Tremolo", 20, 44),
    ("Violins Pizzicato", 20, 45),
    ("Violins Fast", 20, 48),
    ("Violins Slow", 20, 49),
    ("Violins 2 Tremolo", 25, 44),
    ("Violins 2 Pizzicato", 25, 45),
    ("Violins 2 Fast", 25, 48),
    ("Violins 2 Slow", 25, 49),
    ("Violas Tremolo", 30, 44),
    ("Violas Pizzicato", 30, 45),
    ("Violas Fast", 30, 48),
    ("Violas Slow", 30, 49),
    ("Celli Tremolo", 40, 44),
    ("Celli Pizzicato", 40, 45),
    ("Celli Fast", 40, 48),
    ("Celli Slow", 40, 49),
    ("Basses Tremolo", 50, 44),
    ("Basses Pizzicato", 50, 45),
    ("Basses Fast", 50, 48),
    ("Basses Slow", 50, 49),
    ("Orchestral Pad", 8, 48),
    ("Synth Strings 3", 8, 50),
    ("Brass 2", 8, 61),
    ("Synth Brass 3", 8, 62),
    ("Synth Brass 4", 8, 63),
    ("Sine Wave", 8, 80),
    ("Detuned Saw", 20, 81),
    ("Taisho Koto", 8, 107),
    ("Temple Blocks", 1, 115),
    ("Castanets", 8, 115),
    ("Concert Bass Drum", 8, 116),
    ("Melo Tom 2", 8, 117),
    ("808 Tom", 8, 118),
    ("Marching Snare", 128, 56),
    ("Marching Bass", 128, 59),
    ("Old Marching Bass", 128, 57),
    ("Marching Cymbals", 128, 58),
    ("Marching Tenor", 128, 96),
    ("Old Marching Tenor", 128, 95),
];

/// The drum bank.
pub const DRUM_BANK: u16 = 128;

/// How many values the `program` option has.
pub const N_CHOICES: usize = 128 + 8 + 23 + 58;

/// Every legal value of the `program` option: the General MIDI programs,
/// the kits, the other kits, then the variations.
pub const CHOICES: [&str; N_CHOICES] = {
    let mut out = [""; N_CHOICES];
    let mut i = 0;
    while i < 128 {
        out[i] = PROGRAMS[i];
        i += 1;
    }
    let mut k = 0;
    while k < KITS.len() {
        out[i] = KITS[k].0;
        i += 1;
        k += 1;
    }
    k = 0;
    while k < MORE_KITS.len() {
        out[i] = MORE_KITS[k].0;
        i += 1;
        k += 1;
    }
    k = 0;
    while k < VARIATIONS.len() {
        out[i] = VARIATIONS[k].0;
        i += 1;
        k += 1;
    }
    out
};

/// Bank and program of a `program` option value.
pub fn lookup(name: &str) -> Option<(u16, u8)> {
    if let Some(p) = PROGRAMS.iter().position(|n| *n == name) {
        return Some((0, p as u8));
    }
    if let Some(k) = KITS.iter().chain(MORE_KITS.iter()).find(|k| k.0 == name) {
        return Some((DRUM_BANK, k.1));
    }
    VARIATIONS.iter().find(|v| v.0 == name).map(|v| (v.1, v.2))
}

/// Is `name` a drum kit on the General MIDI drum map?
pub fn is_kit(name: &str) -> bool {
    KITS.iter().chain(MORE_KITS.iter()).any(|k| k.0 == name)
}

/// Every drum kit on the General MIDI drum map, by name.
pub fn kit_names() -> impl Iterator<Item = &'static str> {
    KITS.iter().chain(MORE_KITS.iter()).map(|k| k.0)
}

/// The kit for a program change on the MIDI drum channel: the kit with that
/// program, else the nearest General MIDI kit below it (as GM2 synthesizers
/// do).
pub fn kit(program: u8) -> &'static str {
    if let Some(k) = KITS.iter().chain(MORE_KITS.iter()).find(|k| k.1 == program) {
        return k.0;
    }
    KITS.iter()
        .rev()
        .find(|k| k.1 <= program)
        .map(|k| k.0)
        .unwrap_or(KITS[0].0)
}

/// The sample collection the `soundfont` instrument plays: where it comes
/// from and its license, for the studio's credits (see
/// `web/soundfonts/gm/LICENSE.md`).
pub fn collection() -> serde_json::Value {
    serde_json::json!({
        "id": "musescore-general",
        "name": "MuseScore General",
        "version": "0.2 (13 May 2020)",
        "license": "MIT",
        "authors": "S. Christian Collins (MuseScore General); Frank Wen (FluidR3); Michael Cowgill (FluidR3Mono); Ethan Winer (Temple Blocks); Michael Schorsch (Drumline Cymbals)",
        "summary": "General MIDI instruments and drum kits from FluidR3, rebuilt with newer samples for MuseScore by S. Christian Collins. Shared under the MIT license; the acknowledgements and copyright notices must be included in any derivative work.",
        "source": "https://ftp.osuosl.org/pub/musescore/soundfont/MuseScore_General/",
        "licenseFile": "soundfonts/gm/LICENSE.md",
        "readmeFile": "soundfonts/gm/README.md",
        "sourcesFile": "soundfonts/gm/SOURCES.csv",
        "instrument": "soundfont",
        "presets": CHOICES.iter().map(|n| {
            let (bank, program) = lookup(n).unwrap_or((0, 0));
            serde_json::json!([n, bank, program])
        }).collect::<Vec<_>>(),
    })
}

/// The sounding range (MIDI pitches) of the real instrument a General MIDI
/// program samples, for the programs that have one a player would keep to
/// (the Critic flags notes outside it). Pianos run the full keyboard.
pub fn range(program: &str) -> Option<(i32, i32)> {
    RANGES.iter().find(|r| r.0 == program).map(|r| (r.1, r.2))
}

const RANGES: &[(&str, i32, i32)] = &[
    ("Acoustic Grand Piano", 21, 108),
    ("Bright Acoustic Piano", 21, 108),
    ("Electric Grand Piano", 21, 108),
    ("Honky-tonk Piano", 21, 108),
    ("Electric Piano 1", 28, 103),
    ("Electric Piano 2", 28, 103),
    ("Harpsichord", 29, 89),
    ("Clavinet", 29, 89),
    ("Celesta", 60, 108),
    ("Glockenspiel", 79, 108),
    ("Vibraphone", 53, 89),
    ("Marimba", 45, 96),
    ("Xylophone", 65, 108),
    ("Tubular Bells", 60, 77),
    ("Harmonica", 60, 84),
    ("Acoustic Guitar (nylon)", 40, 83),
    ("Acoustic Guitar (steel)", 40, 86),
    ("Electric Guitar (jazz)", 40, 88),
    ("Electric Guitar (clean)", 40, 88),
    ("Electric Guitar (muted)", 40, 88),
    ("Overdriven Guitar", 40, 88),
    ("Distortion Guitar", 40, 88),
    ("Acoustic Bass", 28, 67),
    ("Electric Bass (finger)", 28, 67),
    ("Electric Bass (pick)", 28, 67),
    ("Fretless Bass", 28, 67),
    ("Slap Bass 1", 28, 67),
    ("Slap Bass 2", 28, 67),
    ("Violin", 55, 103),
    ("Viola", 48, 91),
    ("Cello", 36, 84),
    ("Contrabass", 28, 67),
    ("Orchestral Harp", 23, 104),
    ("Timpani", 38, 62),
    ("Choir Aahs", 40, 81),
    ("Voice Oohs", 40, 81),
    ("Trumpet", 52, 86),
    ("Muted Trumpet", 52, 82),
    ("Trombone", 40, 77),
    ("Tuba", 26, 65),
    ("French Horn", 35, 77),
    ("Soprano Sax", 56, 88),
    ("Alto Sax", 49, 81),
    ("Tenor Sax", 44, 76),
    ("Baritone Sax", 36, 69),
    ("Oboe", 58, 93),
    ("English Horn", 52, 81),
    ("Bassoon", 34, 75),
    ("Clarinet", 50, 94),
    ("Piccolo", 74, 108),
    ("Flute", 60, 96),
    ("Recorder", 72, 98),
];

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
        assert_eq!(kit(34), "Jazz Kit 2");
        assert_eq!(lookup("Celli Pizzicato"), Some((40, 45)));
        assert!(is_kit("Brush Kit 2") && !is_kit("Marching Snare"));
        let mut seen = std::collections::HashSet::new();
        assert!(
            CHOICES.iter().all(|c| !c.is_empty() && seen.insert(*c)),
            "names are unique"
        );
    }
}
