//! Synthesizer V Studio projects (`.svp`, JSON): a track per singing
//! channel, notes in blicks (705 600 000 per quarter note), one tempo and
//! one meter.
//!
//! Lyrics as Synthesizer V reads them ([`sung`]): an English word on its
//! first note and `+` on the notes of its later syllables (other languages
//! a syllable per note), `-` on held notes, `br` for breaths, `la` on notes
//! without words. A pronunciation given in the lyrics is the note's
//! `phonemes` (ARPAbet for English, romaji for Japanese, X-SAMPA
//! otherwise). Each note carries its language as `languageOverride`; rap
//! and spoken lines are `musicalType: "rap"`.

use super::sung::{self, Marks};
use super::{monophonic, phones};
use crate::line::{Line, Performed, Song};
use rosaclef_core::LyricMode;
use rosaclef_phonetics::alphabet;
use serde_json::{json, Value};

/// Blicks per quarter note.
const BLICKS: f64 = 705_600_000.0;

const MARKS: Marks = Marks {
    next: "+",
    hold: "-",
    breath: "br",
};

pub fn write(song: &Song) -> String {
    let tracks: Vec<Value> = song.lines.iter().enumerate().map(track).collect();
    let doc = json!({
        "version": 153,
        "time": {
            "meter": [{"index": 0, "numerator": song.beats_per_bar.max(1), "denominator": 4}],
            "tempo": [{"position": 0, "bpm": song.bpm}],
        },
        "library": [],
        "tracks": tracks,
        "renderConfig": {
            "destination": "", "filename": song.title, "numChannels": 1,
            "aspirationFormat": "noAspiration", "bitDepth": 16, "sampleRate": 44100,
            "exportMixDown": true,
        },
    });
    serde_json::to_string_pretty(&doc).expect("JSON values serialize") + "\n"
}

fn track((index, line): (usize, &Line)) -> Value {
    let uuid = format!("00000000-0000-4000-8000-{:012}", index + 1);
    let lang = line
        .notes
        .iter()
        .find_map(|n| n.syllable.as_ref())
        .map_or("english", |s| language(&s.lang));
    json!({
        "name": line.name, "dispColor": "ff7db235", "dispOrder": index, "renderEnabled": false,
        "mixer": {"gainDecibel": 0.0, "pan": 0.0, "mute": false, "solo": false, "display": true},
        "mainGroup": {
            "name": "main", "uuid": uuid, "parameters": parameters(), "vocalModes": {},
            "notes": notes(line),
        },
        "mainRef": {
            "groupID": uuid, "blickOffset": 0, "pitchOffset": 0, "isInstrumental": false,
            "systemPitchDelta": curve(),
            "database": {
                "name": "", "language": lang, "phoneset": phoneset(lang),
                "languageOverride": "", "phonesetOverride": "", "backendType": "",
            },
            "dictionary": "",
            "voice": {"vocalModeInherited": true, "vocalModePreset": "", "vocalModeParams": {}},
        },
        "groups": [],
    })
}

fn notes(line: &Line) -> Vec<Value> {
    let notes = monophonic(&line.notes);
    let lyrics = sung::lyrics(&notes, &MARKS);
    notes
        .iter()
        .zip(lyrics)
        .map(|(n, lyric)| note(n, lyric.as_deref().unwrap_or("la")))
        .collect()
}

fn note(n: &Performed, lyric: &str) -> Value {
    let onset = (n.start.beat * BLICKS).round() as i64;
    let duration = ((n.end.beat - n.start.beat) * BLICKS).round().max(1.0) as i64;
    let syllable = n.syllable.as_ref();
    let rap = syllable.is_some_and(|s| s.mode != LyricMode::Sing);
    json!({
        "onset": onset, "duration": duration, "lyrics": lyric, "phonemes": phonemes(n),
        "pitch": n.pitch, "detune": 0, "attributes": {},
        "musicalType": if rap { "rap" } else { "singing" },
        "languageOverride": syllable.map_or("", |s| language(&s.lang)),
        "instantMode": false,
    })
}

/// The pronunciation the lyrics give for the note's syllable, if any.
fn phonemes(n: &Performed) -> String {
    let Some(s) = n.syllable.as_ref().filter(|_| sung::written(n)) else {
        return String::new();
    };
    let spelled: Vec<String> = s
        .phonemes
        .iter()
        .map(|p| match phones::primary(&s.lang).as_str() {
            "en" | "ja" => phones::for_engine(&s.lang, p),
            _ => alphabet::xsampa(p),
        })
        .collect();
    spelled.join(" ")
}

/// Synthesizer V's name for a language.
fn language(lang: &str) -> &'static str {
    match phones::primary(lang).as_str() {
        "ja" => "japanese",
        "zh" => "mandarin",
        "yue" => "cantonese",
        "es" => "spanish",
        "ko" => "korean",
        _ => "english",
    }
}

fn phoneset(language: &str) -> &'static str {
    match language {
        "english" => "arpabet",
        "japanese" => "romaji",
        _ => "xsampa",
    }
}

fn parameters() -> Value {
    let names = [
        "pitchDelta",
        "vibratoEnv",
        "loudness",
        "tension",
        "breathiness",
        "voicing",
        "gender",
        "toneShift",
    ];
    Value::Object(names.iter().map(|n| (n.to_string(), curve())).collect())
}

fn curve() -> Value {
    json!({"mode": "cubic", "points": []})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::song;

    #[test]
    fn notes_in_blicks_with_lyrics_and_phonemes() {
        let v: Value = serde_json::from_str(&write(&song())).unwrap();
        assert_eq!(v["time"]["tempo"][0]["bpm"], 120.0);
        let track = &v["tracks"][0];
        assert_eq!(track["name"], "Lead");
        assert_eq!(track["mainRef"]["database"]["language"], "english");
        assert_eq!(track["mainRef"]["groupID"], track["mainGroup"]["uuid"]);
        let notes = track["mainGroup"]["notes"].as_array().unwrap();
        assert_eq!(notes.len(), 18);
        let lyric = |i: usize| notes[i]["lyrics"].as_str().unwrap();
        let shown: Vec<&str> = (0..7).map(lyric).collect();
        assert_eq!(shown, ["Hello", "+", "dark", "friend", "-", "br", "Hi"]);
        assert_eq!(notes[3]["phonemes"], "f r eh n d");
        assert_eq!(notes[2]["phonemes"], "");
        assert_eq!(notes[1]["onset"], 705_600_000_i64);
        assert_eq!(notes[3]["duration"], 352_800_000_i64);
        assert_eq!(notes[0]["languageOverride"], "english");
    }
}
