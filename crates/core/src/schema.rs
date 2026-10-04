//! JSON Schema (draft 2020-12) for project documents, generated from the model
//! and the device catalog.

use crate::automation;
use crate::catalog::{self, Category, DeviceSpec};
use crate::model::{
    FILM_EASES, FILM_EFFECTS, FILM_FRAMES, FILM_MODES, FILM_ROLES, FILM_SURFACES, FILM_TRANSITIONS,
    FILM_VIEWS, FORMAT, SCORE_CLEFS, SCORE_KEYS,
};
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
            },
            "animation": {
                "type": "object",
                "additionalProperties": false,
                "description": "The film of the song (the Score view's Film mode): the score's pages lie on a desk and a camera above them follows the music, zooming, tilting and turning to frame some staves. auto: the camera directs itself (it frames the lead, the rhythm section or the background, whichever carries the music). manual: it plays `shots`, and directs itself where no shot covers the song. Changes nothing that plays.",
                "properties": {
                    "mode": {"enum": FILM_MODES, "default": "auto"},
                    "view": {"enum": FILM_VIEWS, "default": "score", "description": "What is filmed."},
                    "surface": {"enum": FILM_SURFACES, "default": "walnut", "description": "The desk the pages lie on."},
                    "energy": num(0.0, 1.0, "How much the self-directed camera moves: 0 calm, 1 restless (default 0.5)."),
                    "effects": {"type": "array", "items": {"$ref": "#/$defs/filmEffect"}, "description": "Effects over the whole film; unlisted ones keep their defaults (vignette 0.5, focus 0, spotlight 0, glow 0)."},
                    "shots": {"type": "array", "items": {"$ref": "#/$defs/shot"}}
                }
            }
        },
        "$defs": {
            "shot": {
                "type": "object",
                "required": ["start", "end"],
                "additionalProperties": false,
                "description": "A shot of the film, in song beats: the camera frames the staves of `focus` (or of a `role`, or all) at a `frame` size, following the playhead (or looking `at` one beat), and moves into the shot over `glide` beats.",
                "properties": {
                    "start": {"type": "number", "minimum": 0},
                    "end": {"type": "number", "description": "After start."},
                    "label": {"type": "string", "description": "Shown on the film's timeline."},
                    "focus": {"type": "array", "items": {"type": "string"}, "description": "Channel ids whose staves are framed. Empty: role, or every staff."},
                    "role": {"enum": FILM_ROLES, "description": "The part of the band to frame when focus is empty: lead (melody), rhythm (drums and bass), background (chords, pads), all."},
                    "frame": {"enum": FILM_FRAMES, "default": "close", "description": "How much the picture holds: desk (every page), page, system (the whole line), medium (~2 bars), close (~1 bar), detail (~a beat, close enough to see the ink)."},
                    "zoom": num(0.1, 10.0, "Closer (> 1) or farther (< 1) than the frame (default 1)."),
                    "tilt": num(0.0, 75.0, "Degrees the camera leans from looking straight down."),
                    "turn": num(-180.0, 180.0, "Degrees the camera turns about the vertical: the music runs diagonally across the picture."),
                    "offset": {"type": "array", "items": {"type": "number", "minimum": -2, "maximum": 2}, "minItems": 2, "maxItems": 2, "description": "Shift of the framed point, [x, y] as fractions of the frame."},
                    "at": {"type": "number", "minimum": 0, "description": "A song beat to look at for the whole shot instead of following the playhead."},
                    "to": {"$ref": "#/$defs/cameraMove"},
                    "transition": {"enum": FILM_TRANSITIONS, "default": "glide", "description": "How the camera comes into the shot: glide (a smooth move), cut, swoop (rises away from the desk and comes down again), whip (fast, blurred)."},
                    "glide": num(0.0, 64.0, "Beats the move into the shot takes (default: up to a bar)."),
                    "ease": {"enum": FILM_EASES, "default": "smooth"},
                    "effects": {"type": "array", "items": {"$ref": "#/$defs/filmEffect"}, "description": "Effects during the shot; they override the film's by type."}
                }
            },
            "cameraMove": {
                "type": "object",
                "additionalProperties": false,
                "description": "Where the camera drifts to by the end of the shot (a slow push in, a turn); values left out stay as at the start.",
                "properties": {
                    "zoom": num(0.1, 10.0, "Zoom at the end of the shot."),
                    "tilt": num(0.0, 75.0, "Tilt at the end of the shot."),
                    "turn": num(-180.0, 180.0, "Turn at the end of the shot."),
                    "offset": {"type": "array", "items": {"type": "number", "minimum": -2, "maximum": 2}, "minItems": 2, "maxItems": 2}
                }
            },
            "filmEffect": {
                "type": "object",
                "required": ["type"],
                "additionalProperties": false,
                "properties": {
                    "type": {"enum": FILM_EFFECTS, "description": "vignette (dark edges), spotlight (a pool of light on the framed staves), focus (depth of field: what is far from the framed point blurs), glow (notes glow as they play)."},
                    "amount": num(0.0, 1.0, "Strength; 0 turns the effect off (default 1).")
                }
            },
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
                    "notes": {"type": "array", "items": {"$ref": "#/$defs/note"}}
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
                    "mixer": {"type": "integer", "minimum": 0, "description": "Audio clips: mixer insert."}
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
