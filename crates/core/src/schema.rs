//! JSON Schema (draft 2020-12) for project documents, generated from the model
//! and the device catalog.

use crate::automation;
use crate::catalog::{self, Category, DeviceSpec};
use crate::model::FORMAT;
use serde_json::{json, Map, Value};

pub const SCHEMA_ID: &str = "https://rosaclef.dev/schema/project-v1.json";

fn num(min: f64, max: f64, desc: &str) -> Value {
    json!({"type": "number", "minimum": min, "maximum": max, "description": desc})
}

fn device_schema(spec: &DeviceSpec) -> Value {
    let mut params = Map::new();
    for p in spec.params {
        let unit = if p.unit.is_empty() { String::new() } else { format!(" [{}]", p.unit) };
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
    let specs: Vec<&DeviceSpec> = catalog::DEVICES.iter().filter(|d| d.category == category).collect();
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
                    "swing": num(0.0, 1.0, "16th-note swing: 0 straight, 1 full triplet feel.")
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
                    "mixer": {"type": "integer", "minimum": 0, "description": "Mixer insert index (0 = master)."}
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
