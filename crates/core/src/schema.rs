//! JSON Schema (draft 2020-12) for project documents, generated from the model
//! and the device catalog.

use crate::automation;
use crate::catalog::{self, Category, DeviceSpec};
use crate::model::{FORMAT, SCORE_CLEFS, SCORE_KEYS};
use serde_json::{json, Map, Value};

pub const SCHEMA_ID: &str = "https://rosaclef.dev/schema/project-v1.json";

fn num(min: f64, max: f64, desc: &str) -> Value {
    json!({"type": "number", "minimum": min, "maximum": max, "description": desc})
}

fn device_schema(spec: &DeviceSpec) -> Value {
    let mut params = Map::new();
    for p in spec.params {
        let unit = if p.unit.is_empty() {
            String::new()
        } else {
            format!(" [{}]", p.unit)
        };
        let mut s = json!({
            "type": if p.integer { "integer" } else { "number" },
            "minimum": p.min,
            "maximum": p.max,
            "default": p.default,
            "description": format!("{}{} — {}", p.label, unit, p.doc),
        });
        if p.integer {
            s["type"] = json!("integer");
        }
        params.insert(p.key.into(), s);
    }
    let mut options = Map::new();
    for o in spec.options {
        let mut s = json!({"type": "string", "default": o.default, "description": o.doc});
        if !o.choices.is_empty() {
            s["enum"] = json!(o.choices);
        }
        options.insert(o.key.into(), s);
    }
    let params_schema = if spec.open_params {
        json!({"type": "object", "additionalProperties": {"type": "number"}, "description": "Plugin parameter id -> plain value."})
    } else {
        json!({"type": "object", "properties": params, "additionalProperties": false})
    };
    json!({
        "if": {"properties": {"type": {"const": spec.kind}}},
        "then": {
            "description": format!("{} — {}", spec.label, spec.doc),
            "properties": {
                "params": params_schema,
                "options": {"type": "object", "properties": options, "additionalProperties": false}
            }
        }
    })
}

fn device_def(category: Category) -> Value {
    let specs: Vec<&DeviceSpec> = catalog::DEVICES
        .iter()
        .filter(|d| d.category == category)
        .collect();
    let kinds: Vec<&str> = specs.iter().map(|d| d.kind).collect();
    let all_of: Vec<Value> = specs.iter().map(|d| device_schema(d)).collect();
    let mut props = json!({
        "type": {"enum": kinds, "description": "Device type; see the catalog in AGENTS.md."},
        "params": {"type": "object", "description": "Numeric settings. Omitted keys use their defaults."},
        "options": {"type": "object", "description": "Textual settings. Omitted keys use their defaults."}
    });
    if category == Category::Effect {
        props["enabled"] = json!({"type": "boolean", "default": true, "description": "Bypass the effect when false."});
    }
    json!({
        "type": "object",
        "required": ["type"],
        "additionalProperties": false,
        "properties": props,
        "allOf": all_of
    })
}

pub fn schema() -> Value {
    let id = json!({"type": "string", "pattern": "^[A-Za-z0-9_.-]{1,64}$"});
    let color = json!({"type": "string", "pattern": "^#[0-9a-fA-F]{6}$", "description": "#rrggbb"});
    let arp_chords: Vec<&str> = crate::arp::CHORDS.iter().map(|c| c.0).collect();
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": SCHEMA_ID,
        "title": "Rosaclef project",
        "description": "A Rosaclef song. All times are in beats (quarter notes). Pitches are MIDI note numbers (60 = C4).",
        "type": "object",
        "required": ["format", "meta", "transport", "mixer"],
        "additionalProperties": false,
        "properties": {
            "$schema": {"type": "string"},
            "format": {"const": FORMAT},
            "meta": {
                "type": "object",
                "required": ["title"],
                "additionalProperties": false,
                "properties": {
                    "title": {"type": "string"},
                    "author": {"type": "string"},
                    "description": {"type": "string"}
                }
            },
            "transport": {
                "type": "object",
                "required": ["bpm"],
                "additionalProperties": false,
                "properties": {
                    "bpm": num(20.0, 999.0, "Tempo in beats per minute."),
                    "beatsPerBar": {"type": "integer", "minimum": 1, "maximum": 32, "default": 4},
                    "swing": num(0.0, 1.0, "16th-note swing: 0 straight, 1 full triplet feel."),
                    "transpose": {"type": "integer", "minimum": -12, "maximum": 12, "default": 0, "description": "Semitones every pitched instrument is shifted by when it plays (to match a singer's range). Notes stay as written; drums and audio clips are not shifted."},
                    "meters": {
                        "type": "array",
                        "description": "Time-signature changes, sorted by bar. Each holds from its bar until the next; bars before the first have beatsPerBar beats. A bar lasts 4 × numerator / denominator beats.",
                        "maxItems": 4096,
                        "items": {
                            "type": "object",
                            "required": ["bar", "numerator", "denominator"],
                            "additionalProperties": false,
                            "properties": {
                                "bar": {"type": "integer", "minimum": 1, "description": "First bar in the new meter, counted from 1."},
                                "numerator": {"type": "integer", "minimum": 1, "maximum": 64},
                                "denominator": {"enum": [1, 2, 4, 8, 16, 32]}
                            }
                        }
                    }
                }
            },
            "channels": {
                "type": "array",
                "description": "The channel rack: one instrument per channel.",
                "items": {"$ref": "#/$defs/channel"}
            },
            "patterns": {
                "type": "array",
                "description": "Patterns hold notes for any number of channels.",
                "items": {"$ref": "#/$defs/pattern"}
            },
            "playlist": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "tracks": {"type": "array", "items": {"$ref": "#/$defs/track"}},
                    "clips": {"type": "array", "items": {"$ref": "#/$defs/clip"}}
                }
            },
            "mixer": {
                "type": "object",
                "required": ["inserts"],
                "additionalProperties": false,
                "properties": {
                    "inserts": {
                        "type": "array",
                        "minItems": 1,
                        "description": "Insert 0 is the master bus. All other inserts feed the master.",
                        "items": {"$ref": "#/$defs/insert"}
                    }
                }
            },
            "automation": {
                "type": "array",
                "description": "Automation lanes: breakpoint curves that drive one value each (tempo, a mix control or a device parameter) over song time. Applied while the song plays in song mode and in renders.",
                "items": {"$ref": "#/$defs/automationLane"}
            },
            "repeats": {
                "type": "array",
                "description": "Repeated passages of the arrangement, as repeat signs in a score: the song plays from start to end `times` times in all, then goes on. Endings (voltas) play only on the listed passes; the last pass's ending usually follows the end. Times are song beats, usually on bar lines.",
                "items": {"$ref": "#/$defs/repeat"}
            },
            "score": {
                "type": "object",
                "additionalProperties": false,
                "description": "How the song reads as sheet music in the studio's Score view. Changes nothing that plays.",
                "properties": {
                    "key": {"type": "string", "enum": SCORE_KEYS, "default": "auto", "description": "Key signature; auto guesses it from the notes. Minor keys end in m (Am, F#m)."},
                    "hidden": {"type": "array", "items": {"type": "string"}, "description": "Channel ids whose staves are hidden."},
                    "hiddenTracks": {"type": "array", "items": {"type": "integer", "minimum": 0}, "description": "Playlist tracks left out of the song's score."},
                    "clefs": {
                        "type": "object",
                        "additionalProperties": {"type": "string", "enum": SCORE_CLEFS},
                        "description": "Clef per channel id (grand = a piano's treble and bass staves). Unlisted channels pick one from their range."
                    },
                    "marks": {"type": "array", "items": {"$ref": "#/$defs/scoreMark"}}
                }
            }
        },
        "$defs": {
            "channel": {
                "type": "object",
                "required": ["id", "name", "instrument"],
                "additionalProperties": false,
                "properties": {
                    "id": id,
                    "name": {"type": "string"},
                    "color": color,
                    "instrument": {"$ref": "#/$defs/instrument"},
                    "volume": num(0.0, 1.5, "Channel gain (linear)."),
                    "pan": num(-1.0, 1.0, "-1 left, 0 centre, 1 right."),
                    "mute": {"type": "boolean"},
                    "mixer": {"type": "integer", "minimum": 0, "description": "Mixer insert index (0 = master)."},
                    "arp": {"$ref": "#/$defs/arp"}
                }
            },
            "arp": {
                "type": "object",
                "description": "The channel's arpeggiator (absent = off). While a note is held it plays the notes of `chord` above it over `octaves` octaves, one every `rate` beats, each `gate` × rate long. The notes stay as written; only playback arpeggiates.",
                "required": ["rate"],
                "additionalProperties": false,
                "properties": {
                    "chord": {"enum": arp_chords, "default": "octave", "description": "Chord the notes cycle through; \"octave\" plays the note itself in each octave."},
                    "octaves": {"type": "integer", "minimum": 1, "maximum": crate::arp::OCTAVES_MAX, "default": 1},
                    "rate": num(crate::arp::RATE_MIN, crate::arp::RATE_MAX, "Beats from one note to the next (0.25 = sixteenths)."),
                    "direction": {"enum": crate::arp::DIRECTIONS, "default": "up"},
                    "gate": num(crate::arp::GATE_MIN, crate::arp::GATE_MAX, "Length of each note as a fraction of the rate (default 1)."),
                    "mode": {"enum": crate::arp::MODES, "default": "free", "description": "free: every held note runs its own arpeggio; sort: notes struck together take turns, lowest first."}
                }
            },
            "instrument": device_def(Category::Instrument),
            "effect": device_def(Category::Effect),
            "pattern": {
                "type": "object",
                "required": ["id", "name", "length"],
                "additionalProperties": false,
                "properties": {
                    "id": id,
                    "name": {"type": "string"},
                    "color": color,
                    "length": {"type": "number", "exclusiveMinimum": 0, "maximum": 4096, "description": "Pattern length in beats."},
                    "notes": {"type": "array", "items": {"$ref": "#/$defs/note"}},
                    "uses": {"type": "array", "items": {"$ref": "#/$defs/use"}, "description": "Other patterns played inside this one by reference (not copied): editing them changes every place that uses them."},
                    "lyrics": {"type": "array", "items": {"$ref": "#/$defs/lyrics"}, "description": "Words sung on this pattern's notes, one line per vocal channel."}
                }
            },
            "use": {
                "type": "object",
                "required": ["pattern", "start"],
                "additionalProperties": false,
                "properties": {
                    "pattern": {"type": "string", "description": "Id of the pattern played (it cannot contain this one)."},
                    "start": {"type": "number", "minimum": 0, "description": "Where it starts in this pattern, in beats."},
                    "from": {"type": "number", "minimum": 0, "default": 0, "description": "Start of the range played, in the used pattern's beats."},
                    "to": {"type": "number", "description": "End of the range played (default: the used pattern's end). Notes are cut here."},
                    "transpose": {"type": "integer", "minimum": -48, "maximum": 48, "default": 0, "description": "Semitones (drum channels are not moved)."},
                    "channel": {"type": "string", "description": "Channel id to play every note on instead of its own."},
                    "velocity": {"type": "number", "minimum": 0, "maximum": 2, "default": 1, "description": "Factor on the notes' velocities."},
                    "verse": {"type": "integer", "minimum": 1, "description": "Verse its lyrics sing (default: the verse this pattern sings)."}
                }
            },
            "lyrics": {
                "type": "object",
                "required": ["channel", "verses"],
                "additionalProperties": false,
                "description": "Words for one vocal channel. Syllables fall on the channel's notes in time order (notes that used patterns already gave words are skipped). Notation: a space ends a word, `-` splits syllables (Hel-lo), `_` holds the previous syllable over a note, `/` ends a line and `//` a paragraph, `word[w ɜ d]` gives a pronunciation in IPA, `(br)` is a breath; `\\` escapes a character.",
                "properties": {
                    "channel": {"type": "string", "description": "The vocal channel."},
                    "lang": {"type": "string", "description": "BCP 47 language tag (en, en-US, ja, es...). Default: en."},
                    "mode": {"enum": ["sing", "rap", "speak"], "default": "sing", "description": "sing on the pitches, rap (pitch ignored), speak."},
                    "verses": {
                        "type": "object",
                        "minProperties": 1,
                        "propertyNames": {"pattern": "^[1-9][0-9]*$"},
                        "additionalProperties": {"type": "string"},
                        "description": "Verse number (from 1) → text. A verse a line does not have sings its lowest-numbered one."
                    },
                    "timing": {"type": "array", "items": {"$ref": "#/$defs/syllableTiming"}}
                }
            },
            "syllableTiming": {
                "type": "object",
                "required": ["verse", "at", "phonemes"],
                "additionalProperties": false,
                "description": "Fixed phoneme timing for one syllable (from aligning a recording, or by hand).",
                "properties": {
                    "verse": {"type": "integer", "minimum": 1},
                    "at": {"type": "number", "minimum": 0, "description": "Beat (in this pattern) of the note the syllable is sung on."},
                    "phonemes": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "required": ["p", "offset"],
                            "additionalProperties": false,
                            "properties": {
                                "p": {"type": "string", "description": "IPA phoneme."},
                                "offset": {"type": "number", "description": "Seconds from the note's start (negative: before it)."}
                            }
                        }
                    }
                }
            },
            "note": {
                "type": "object",
                "required": ["channel", "pitch", "start", "length"],
                "additionalProperties": false,
                "properties": {
                    "channel": {"type": "string", "description": "Channel id."},
                    "pitch": {"type": "integer", "minimum": 0, "maximum": 127, "description": "MIDI note (60 = C4)."},
                    "start": {"type": "number", "minimum": 0, "description": "Beats from pattern start."},
                    "length": {"type": "number", "exclusiveMinimum": 0, "description": "Duration in beats."},
                    "velocity": {"type": "number", "minimum": 0, "maximum": 1, "default": 0.8}
                }
            },
            "track": {
                "type": "object",
                "required": ["name"],
                "additionalProperties": false,
                "properties": {"name": {"type": "string"}, "mute": {"type": "boolean"}}
            },
            "clip": {
                "type": "object",
                "required": ["track", "start", "length"],
                "additionalProperties": false,
                "description": "Exactly one of pattern / sample.",
                "properties": {
                    "pattern": {"type": "string", "description": "Pattern id; the pattern loops to fill the clip."},
                    "sample": {"type": "string", "description": "Project-relative audio file path."},
                    "track": {"type": "integer", "minimum": 0},
                    "start": {"type": "number", "minimum": 0, "description": "Timeline position in beats."},
                    "length": {"type": "number", "exclusiveMinimum": 0, "description": "Length in beats."},
                    "offset": {"type": "number", "minimum": 0, "description": "Start offset into the pattern/sample, in beats."},
                    "gain": {"type": "number", "minimum": 0, "maximum": 4, "default": 1},
                    "mixer": {"type": "integer", "minimum": 0, "description": "Audio clips: mixer insert."},
                    "verse": {"type": "integer", "minimum": 1, "description": "Pattern clips: the verse its lyrics sing (default: the pass of the repeat it plays in, else 1)."}
                }
            },
            "insert": {
                "type": "object",
                "required": ["name"],
                "additionalProperties": false,
                "properties": {
                    "name": {"type": "string"},
                    "volume": num(0.0, 2.0, "Fader gain (linear, 1 = 0 dB)."),
                    "pan": num(-1.0, 1.0, "Balance."),
                    "mute": {"type": "boolean"},
                    "solo": {"type": "boolean"},
                    "effects": {"type": "array", "items": {"$ref": "#/$defs/effect"}}
                }
            },
            "automationLane": {
                "type": "object",
                "required": ["id", "target", "points"],
                "additionalProperties": false,
                "properties": {
                    "id": id,
                    "name": {"type": "string"},
                    "target": {
                        "type": "string",
                        "pattern": automation::TARGET_PATTERN,
                        "description": format!(
                            "What the lane drives: {}. Values use the target's natural units and range: tempo in BPM (20..999), swing 0..1, channel volume 0..1.5 and insert volume 0..2 (linear gain), pan -1..1, device parameters as in the catalog (plugin parameters: any number). At most one lane per target.",
                            automation::TARGET_GRAMMAR
                        )
                    },
                    "color": color,
                    "mute": {"type": "boolean", "default": false, "description": "A muted lane is ignored."},
                    "points": {
                        "type": "array",
                        "minItems": 1,
                        "description": "Breakpoints sorted by beat. Before the first point the lane holds its first value, after the last point its last value. Two points on the same beat make a step.",
                        "items": {"$ref": "#/$defs/automationPoint"}
                    }
                }
            },
            "repeat": {
                "type": "object",
                "required": ["start", "end"],
                "additionalProperties": false,
                "properties": {
                    "start": {"type": "number", "minimum": 0, "description": "Start repeat sign, in song beats."},
                    "end": {"type": "number", "description": "End repeat sign, in song beats: playing returns to start here."},
                    "times": {"type": "integer", "minimum": 1, "maximum": 99, "default": 2, "description": "How many times the passage plays in all."},
                    "endings": {"type": "array", "items": {"$ref": "#/$defs/ending"}}
                }
            },
            "ending": {
                "type": "object",
                "required": ["start", "end", "passes"],
                "additionalProperties": false,
                "description": "A volta: music played only on some passes. Inside the repeat it is skipped on the other passes; after the end sign (directly) it is where the listed passes go on.",
                "properties": {
                    "start": {"type": "number", "minimum": 0},
                    "end": {"type": "number"},
                    "passes": {"type": "array", "minItems": 1, "items": {"type": "integer", "minimum": 1}, "description": "Passes that play it, counted from 1."}
                }
            },
            "scoreMark": {
                "type": "object",
                "required": ["start", "end", "color"],
                "additionalProperties": false,
                "description": "A colored passage of the score.",
                "properties": {
                    "start": {"type": "number", "minimum": 0, "description": "Start in beats: song time, or pattern time when pattern is set."},
                    "end": {"type": "number", "description": "End in beats (after start)."},
                    "color": color,
                    "label": {"type": "string", "description": "Shown above the passage."},
                    "pattern": {"type": "string", "description": "Pattern id: the passage is in that pattern's time and is colored wherever the pattern plays."},
                    "channels": {"type": "array", "items": {"type": "string"}, "description": "Channel ids to color; empty or missing = every staff."}
                }
            },
            "automationPoint": {
                "type": "object",
                "required": ["beat", "value"],
                "additionalProperties": false,
                "properties": {
                    "beat": {"type": "number", "minimum": 0, "description": "Absolute song time in beats."},
                    "value": {"type": "number", "description": "Target value in its natural units."},
                    "curve": num(-1.0, 1.0, "Shape of the segment ending at this point: 0 linear, > 0 slow start then fast (exponential rise), < 0 fast start then settling.")
                }
            }
        }
    })
}

/// The schema as pretty-printed text.
pub fn schema_text() -> String {
    serde_json::to_string_pretty(&schema()).unwrap() + "\n"
}
