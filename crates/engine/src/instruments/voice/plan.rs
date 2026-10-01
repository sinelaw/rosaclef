//! A syllable as the voice performs it: the consonants that start it, the
//! vowel held for the note, and what ends it when the note ends (a
//! diphthong's glide, then the closing consonants).

use super::phones::{self, Noise, Target};
use crate::instruments::Sung;
use rosaclef_core::lyrics::Sung as Token;
use rosaclef_core::LyricMode;
use rosaclef_phonetics::ipa::{self, Class};

/// A timed part of a consonant: a target held for `secs`, at level `gate`
/// (0 for a stop's closure).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    pub target: Target,
    pub secs: f32,
    pub gate: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    Syllable {
        onset: Vec<Step>,
        vowel: Target,
        coda: Vec<Step>,
        /// Rap or speech: the notes give the timing, not the pitch.
        spoken: bool,
    },
    /// Hold the previous syllable's vowel at the new pitch.
    Hold,
}

/// A breath: aspiration through an open throat, no voice.
const BREATH: Target = Target {
    formants: [600.0, 1300.0, 2400.0],
    voicing: 0.0,
    noise: Noise {
        amp: 0.2,
        freq: 1400.0,
        bw: 3000.0,
    },
};

impl Plan {
    pub fn of(s: &Sung) -> Plan {
        let spoken = s.mode != LyricMode::Sing;
        match s.token.as_ref().map(|t| &t.sung) {
            None => Plan::vocalise(),
            Some(Token::Hold) => Plan::Hold,
            Some(Token::Breath) => Plan::Syllable {
                onset: vec![],
                vowel: BREATH,
                coda: vec![],
                spoken,
            },
            Some(Token::Syllable { .. }) => syllable(&s.phonemes, spoken),
        }
    }

    /// The vowel sung on notes without words.
    pub fn vocalise() -> Plan {
        Plan::Syllable {
            onset: vec![],
            vowel: phones::target("ɑ"),
            coda: vec![],
            spoken: false,
        }
    }
}

/// A syllable's plan from its phonemes: the first vowel is the nucleus. A
/// syllable without a vowel holds its last voiced sound (m, n, l...).
fn syllable(phonemes: &[String], spoken: bool) -> Plan {
    let nucleus = phonemes.iter().position(|p| ipa::is_vowel(p)).or_else(|| {
        phonemes
            .iter()
            .rposition(|p| matches!(ipa::class(p), Class::Nasal | Class::Liquid | Class::Glide))
    });
    let Some(v) = nucleus else {
        return Plan::Syllable {
            onset: steps(phonemes),
            vowel: phones::SCHWA,
            coda: vec![],
            spoken,
        };
    };
    let glide = phones::offglide(&phonemes[v]).map(|t| step(t, 0.08));
    Plan::Syllable {
        onset: steps(&phonemes[..v]),
        vowel: phones::target(&phonemes[v]),
        coda: glide.into_iter().chain(steps(&phonemes[v + 1..])).collect(),
        spoken,
    }
}

fn steps(phonemes: &[String]) -> Vec<Step> {
    phonemes.iter().flat_map(|p| consonant(p)).collect()
}

/// The steps of one consonant (or a vowel met outside the nucleus).
fn consonant(p: &str) -> Vec<Step> {
    let voiced = ipa::voiced(p);
    match ipa::class(p) {
        Class::Stop => stop(p, voiced, None),
        Class::Affricate => {
            let tail: String = p.chars().skip(1).collect();
            stop(p, voiced, Some(phones::target(&tail)))
        }
        Class::Fricative => vec![step(phones::target(p), if voiced { 0.07 } else { 0.09 })],
        Class::Nasal => vec![step(phones::target(p), 0.06)],
        Class::Liquid | Class::Glide => vec![step(phones::target(p), 0.05)],
        Class::Vowel => vec![step(phones::target(p), 0.12)],
        Class::Other => vec![Step {
            target: phones::SCHWA,
            secs: 0.04,
            gate: 0.0,
        }],
    }
}

/// A closure (silent, or a low voice bar when voiced), then the release: a
/// burst, or the affricate's fricative.
fn stop(p: &str, voiced: bool, frication: Option<Target>) -> Vec<Step> {
    let locus = phones::stop_locus(p);
    let closure = Target {
        voicing: if voiced { 0.15 } else { 0.0 },
        ..locus
    };
    let release = match frication {
        Some(f) => step(f, 0.07),
        None => step(
            Target {
                noise: Noise {
                    amp: if voiced { 0.3 } else { 0.5 },
                    ..phones::burst(p)
                },
                ..locus
            },
            0.02,
        ),
    };
    vec![
        Step {
            target: closure,
            secs: 0.045,
            gate: if voiced { 1.0 } else { 0.0 },
        },
        release,
    ]
}

fn step(target: Target, secs: f32) -> Step {
    Step {
        target,
        secs,
        gate: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosaclef_core::lyrics::parse;

    fn sung(text: &str, phonemes: &str) -> Sung {
        Sung {
            token: Some(parse(text).unwrap().remove(0)),
            lang: "en".into(),
            mode: LyricMode::Sing,
            phonemes: phonemes.split_whitespace().map(str::to_string).collect(),
            phrase: None,
        }
    }

    #[test]
    fn a_syllable_has_onset_vowel_and_coda() {
        let Plan::Syllable {
            onset, vowel, coda, ..
        } = Plan::of(&sung("friend", "f ɹ ɛ n d"))
        else {
            panic!("a syllable");
        };
        assert_eq!(onset.len(), 2);
        assert_eq!(vowel, phones::target("ɛ"));
        assert_eq!(coda.len(), 3, "n, then d's closure and burst");
    }

    #[test]
    fn diphthongs_holds_breaths_and_hums() {
        let Plan::Syllable { vowel, coda, .. } = Plan::of(&sung("my", "m aɪ")) else {
            panic!()
        };
        assert_eq!(vowel, phones::target("a"));
        assert_eq!(
            coda[0].target,
            phones::target("ɪ"),
            "the glide ends the note"
        );
        assert_eq!(Plan::of(&sung("_", "")), Plan::Hold);
        let Plan::Syllable { vowel, .. } = Plan::of(&sung("(br)", "")) else {
            panic!()
        };
        assert_eq!(vowel.voicing, 0.0);
        let Plan::Syllable { vowel, .. } = Plan::of(&sung("hmm", "h m")) else {
            panic!()
        };
        assert_eq!(vowel, phones::target("m"));
    }
}
