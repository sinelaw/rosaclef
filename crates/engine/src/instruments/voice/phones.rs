//! What each phoneme sounds like to the formant voice: where its formants
//! sit, how much the vocal folds sound, and its noise (frication, bursts).
//! Values are an adult average; the voice's `gender` shifts them.

use rosaclef_phonetics::ipa::{self, Class};

/// Noise shaped by a band-pass filter (frication, bursts, aspiration).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Noise {
    pub amp: f32,
    pub freq: f32,
    pub bw: f32,
}

impl Noise {
    pub const NONE: Noise = Noise {
        amp: 0.0,
        freq: 1000.0,
        bw: 1000.0,
    };
}

/// The sound a phoneme aims for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    /// F1, F2, F3 in Hz.
    pub formants: [f32; 3],
    /// Level of the voiced (glottal) sound, 0..1.
    pub voicing: f32,
    pub noise: Noise,
}

const fn vowel(f1: f32, f2: f32, f3: f32) -> Target {
    Target {
        formants: [f1, f2, f3],
        voicing: 1.0,
        noise: Noise::NONE,
    }
}

const fn sonorant(f1: f32, f2: f32, f3: f32, voicing: f32) -> Target {
    Target {
        formants: [f1, f2, f3],
        voicing,
        noise: Noise::NONE,
    }
}

const fn fricative(freq: f32, bw: f32, amp: f32, voicing: f32) -> Target {
    Target {
        formants: [300.0, 1500.0, 2500.0],
        voicing,
        noise: Noise { amp, freq, bw },
    }
}

/// The neutral vowel, for anything unknown.
pub const SCHWA: Target = vowel(500.0, 1500.0, 2500.0);

const VOWELS: &[(&str, Target)] = &[
    ("i", vowel(270.0, 2290.0, 3010.0)),
    ("ɪ", vowel(390.0, 1990.0, 2550.0)),
    ("e", vowel(400.0, 2100.0, 2700.0)),
    ("ɛ", vowel(530.0, 1840.0, 2480.0)),
    ("æ", vowel(660.0, 1720.0, 2410.0)),
    ("a", vowel(750.0, 1250.0, 2500.0)),
    ("ɑ", vowel(730.0, 1090.0, 2440.0)),
    ("ɒ", vowel(650.0, 950.0, 2400.0)),
    ("ɔ", vowel(570.0, 840.0, 2410.0)),
    ("o", vowel(450.0, 800.0, 2400.0)),
    ("ʊ", vowel(440.0, 1020.0, 2240.0)),
    ("u", vowel(300.0, 870.0, 2240.0)),
    ("ɯ", vowel(350.0, 1250.0, 2300.0)),
    ("ʌ", vowel(640.0, 1190.0, 2390.0)),
    ("ə", SCHWA),
    ("ɝ", vowel(490.0, 1350.0, 1690.0)),
    ("ɚ", vowel(490.0, 1350.0, 1690.0)),
    ("y", vowel(270.0, 1900.0, 2300.0)),
    ("ø", vowel(390.0, 1600.0, 2300.0)),
    ("œ", vowel(550.0, 1500.0, 2400.0)),
    ("ɨ", vowel(320.0, 1650.0, 2400.0)),
];

/// Diphthongs: where they start and where they glide to.
const DIPHTHONGS: &[(&str, &str, &str)] = &[
    ("aɪ", "a", "ɪ"),
    ("aʊ", "a", "ʊ"),
    ("eɪ", "e", "ɪ"),
    ("oʊ", "o", "ʊ"),
    ("ɔɪ", "ɔ", "ɪ"),
];

const CONSONANTS: &[(&str, Target)] = &[
    ("m", sonorant(250.0, 1000.0, 2200.0, 0.55)),
    ("n", sonorant(250.0, 1450.0, 2400.0, 0.55)),
    ("ŋ", sonorant(250.0, 1900.0, 2400.0, 0.55)),
    ("ɲ", sonorant(250.0, 2000.0, 2700.0, 0.55)),
    ("ɴ", sonorant(280.0, 1100.0, 2300.0, 0.55)),
    ("l", sonorant(360.0, 1100.0, 2600.0, 0.8)),
    ("ɹ", sonorant(310.0, 1060.0, 1380.0, 0.8)),
    ("r", sonorant(400.0, 1300.0, 2200.0, 0.8)),
    ("ɾ", sonorant(350.0, 1600.0, 2500.0, 0.7)),
    ("w", sonorant(290.0, 610.0, 2150.0, 0.8)),
    ("j", sonorant(260.0, 2070.0, 3020.0, 0.8)),
    ("s", fricative(6500.0, 2500.0, 0.45, 0.0)),
    ("z", fricative(6500.0, 2500.0, 0.3, 0.45)),
    ("ʃ", fricative(3200.0, 1800.0, 0.45, 0.0)),
    ("ʒ", fricative(3200.0, 1800.0, 0.3, 0.45)),
    ("ɕ", fricative(4200.0, 2200.0, 0.4, 0.0)),
    ("ʑ", fricative(4200.0, 2200.0, 0.28, 0.45)),
    ("f", fricative(6500.0, 5000.0, 0.18, 0.0)),
    ("v", fricative(6500.0, 5000.0, 0.12, 0.5)),
    ("θ", fricative(6000.0, 5000.0, 0.15, 0.0)),
    ("ð", fricative(6000.0, 5000.0, 0.1, 0.5)),
    ("ɸ", fricative(1500.0, 3000.0, 0.15, 0.0)),
    ("β", fricative(1500.0, 3000.0, 0.1, 0.5)),
    ("x", fricative(1600.0, 900.0, 0.3, 0.0)),
    ("ɣ", fricative(1600.0, 900.0, 0.2, 0.45)),
    ("ç", fricative(4000.0, 1800.0, 0.3, 0.0)),
    ("ʝ", fricative(3500.0, 2000.0, 0.2, 0.5)),
    ("χ", fricative(1200.0, 900.0, 0.3, 0.0)),
    ("h", fricative(1600.0, 3000.0, 0.22, 0.0)),
];

/// The target of a single vowel or consonant (a diphthong's first part).
pub fn target(p: &str) -> Target {
    let base = p.trim_end_matches(['ː', 'ʰ', 'ʲ', 'ʷ']);
    if let Some((_, a, _)) = DIPHTHONGS.iter().find(|(d, _, _)| *d == base) {
        return target(a);
    }
    let found = VOWELS.iter().chain(CONSONANTS).find(|(s, _)| *s == base);
    match found {
        Some((_, t)) => *t,
        None => by_class(base),
    }
}

/// Where a diphthong glides to.
pub fn offglide(p: &str) -> Option<Target> {
    DIPHTHONGS
        .iter()
        .find(|(d, _, _)| *d == p)
        .map(|(_, _, b)| target(b))
}

/// Stops and affricates are placed by their locus: where F2 points while
/// the mouth is closed (lips low, ridge middle, palate high).
pub fn stop_locus(p: &str) -> Target {
    let f2 = match p.chars().next().unwrap_or('t') {
        'p' | 'b' => 900.0,
        'k' | 'g' | 'ɡ' | 'q' => 2000.0,
        'c' | 'ɟ' => 2300.0,
        _ => 1700.0,
    };
    sonorant(250.0, f2, 2500.0, 0.0)
}

/// The noise of a stop's release.
pub fn burst(p: &str) -> Noise {
    let (freq, bw) = match p.chars().next().unwrap_or('t') {
        'p' | 'b' => (900.0, 1500.0),
        'k' | 'g' | 'ɡ' | 'q' => (2200.0, 1200.0),
        _ => (4200.0, 2500.0),
    };
    Noise { amp: 0.5, freq, bw }
}

/// A phoneme the tables lack, by its class.
fn by_class(p: &str) -> Target {
    match ipa::class(p) {
        Class::Vowel => SCHWA,
        Class::Nasal => sonorant(250.0, 1400.0, 2400.0, 0.55),
        Class::Liquid | Class::Glide => sonorant(350.0, 1300.0, 2400.0, 0.8),
        Class::Fricative => fricative(
            4000.0,
            3000.0,
            0.25,
            if ipa::voiced(p) { 0.45 } else { 0.0 },
        ),
        _ => stop_locus(p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets() {
        assert_eq!(target("ɑ").formants[0], 730.0);
        assert_eq!(target("aɪ"), target("a"));
        assert_eq!(offglide("aɪ"), Some(target("ɪ")));
        assert_eq!(offglide("ɑ"), None);
        assert!(target("s").noise.amp > 0.0 && target("s").voicing == 0.0);
        assert_eq!(target("iː"), target("i"));
        assert_eq!(
            target("ʁ").voicing,
            0.8,
            "unknown liquids get a liquid's target"
        );
    }
}
