//! The producer's live UI context, written to `.rosaclef/context.json` so a
//! coding agent can resolve "this", "here", "the selected notes", "what I
//! just did". The UI sends it; the server normalizes it through these types
//! (unknown fields are dropped, missing ones defaulted) and stamps it.

use crate::model::{AutomationPoint, Clip, Note};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct UiContext {
    /// Increases with every write.
    pub seq: u64,
    /// RFC 3339 UTC timestamp of this write.
    pub updated_at: String,
    /// Panel the producer last interacted with.
    pub focus: String,
    /// Editor shown in the bottom dock: "channel rack", "piano roll" or "mixer".
    pub dock: String,
    pub transport: TransportContext,
    pub selection: Selection,
    pub visible: Visible,
    /// Most recent manual edits by the producer, newest last.
    pub recent_edits: Vec<Edit>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct TransportContext {
    pub playing: bool,
    /// "song" (arrangement) or "pattern" (loops the selected pattern).
    pub mode: String,
    /// Playhead in beats (within the pattern in pattern mode).
    pub position_beats: f64,
    /// Playhead as "bar:beat:tick" (1-based bar and beat, 96 ticks per beat).
    pub position: String,
    pub bpm: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Selection {
    pub pattern: Option<PatternRef>,
    pub channel: Option<ChannelRef>,
    pub insert: Option<IndexRef>,
    pub track: Option<IndexRef>,
    /// Selected notes of the selected pattern, with their index in `patterns[..].notes`.
    pub notes: Vec<SelectedNote>,
    /// Selected playlist clips, with their index in `playlist.clips`.
    pub clips: Vec<SelectedClip>,
    /// Selected automation lane (and points), if any.
    pub automation: Option<AutomationRef>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AutomationRef {
    /// Index in `automation`.
    pub index: u32,
    pub id: String,
    pub name: String,
    pub target: String,
    pub point_count: u32,
    /// Selected points, with their index in the lane's `points`.
    pub points: Vec<SelectedPoint>,
    /// The lane's value at the playhead.
    pub value_at_playhead: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedPoint {
    pub index: u32,
    pub point: AutomationPoint,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PatternRef {
    pub id: String,
    pub name: String,
    pub length: f64,
    pub note_count: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ChannelRef {
    pub id: String,
    pub name: String,
    /// Instrument type (catalog device type).
    pub instrument: String,
    /// Mixer insert index.
    pub mixer: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct IndexRef {
    pub index: u32,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedNote {
    pub index: u32,
    pub note: Note,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedClip {
    pub index: u32,
    pub clip: Clip,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Visible {
    pub playlist: Option<PlaylistView>,
    pub piano_roll: Option<PianoRollView>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PlaylistView {
    pub start_beat: f64,
    pub end_beat: f64,
    pub first_track: u32,
    pub last_track: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PianoRollView {
    pub pattern: String,
    pub channel: String,
    pub start_beat: f64,
    pub end_beat: f64,
    pub low_pitch: i32,
    pub high_pitch: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Edit {
    /// RFC 3339 UTC timestamp.
    pub at: String,
    pub summary: String,
}

/// Parse a context sent by the UI, keeping only known fields.
pub fn normalize(v: Value) -> UiContext {
    serde_json::from_value(v).unwrap_or_default()
}

pub fn schema() -> Value {
    let beats = json!({"type": "number", "minimum": 0});
    let index_ref = json!({
        "type": ["object", "null"],
        "properties": {"index": {"type": "integer"}, "name": {"type": "string"}}
    });
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://rosaclef.dev/schema/context-v1.json",
        "title": "Rosaclef UI context",
        "description": "What the producer is doing in the studio right now. Written by the studio to .rosaclef/context.json; read-only for agents. Indexes refer to arrays in project.json at the time of writing.",
        "type": "object",
        "properties": {
            "seq": {"type": "integer", "description": "Increases with every write."},
            "updatedAt": {"type": "string", "format": "date-time"},
            "focus": {"enum": ["playlist", "channel rack", "piano roll", "mixer", "browser", "agent", ""], "description": "Panel the producer last interacted with."},
            "dock": {"enum": ["channel rack", "piano roll", "mixer", ""], "description": "Editor visible in the bottom dock."},
            "transport": {
                "type": "object",
                "properties": {
                    "playing": {"type": "boolean"},
                    "mode": {"enum": ["song", "pattern", ""]},
                    "positionBeats": beats,
                    "position": {"type": "string", "description": "bar:beat:tick, 1-based, 96 ticks per beat."},
                    "bpm": {"type": "number"}
                }
            },
            "selection": {
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": ["object", "null"],
                        "properties": {"id": {"type": "string"}, "name": {"type": "string"}, "length": beats, "noteCount": {"type": "integer"}}
                    },
                    "channel": {
                        "type": ["object", "null"],
                        "properties": {"id": {"type": "string"}, "name": {"type": "string"}, "instrument": {"type": "string"}, "mixer": {"type": "integer"}}
                    },
                    "insert": index_ref,
                    "track": index_ref,
                    "notes": {
                        "type": "array",
                        "description": "Selected notes of the selected pattern; index is into patterns[<selected>].notes.",
                        "items": {"type": "object", "properties": {"index": {"type": "integer"}, "note": {"$ref": "project.schema.json#/$defs/note"}}}
                    },
                    "clips": {
                        "type": "array",
                        "description": "Selected playlist clips; index is into playlist.clips.",
                        "items": {"type": "object", "properties": {"index": {"type": "integer"}, "clip": {"$ref": "project.schema.json#/$defs/clip"}}}
                    },
                    "automation": {
                        "type": ["object", "null"],
                        "description": "The selected automation lane; index is into automation, point indexes into automation[<index>].points.",
                        "properties": {
                            "index": {"type": "integer"},
                            "id": {"type": "string"},
                            "name": {"type": "string"},
                            "target": {"type": "string"},
                            "pointCount": {"type": "integer"},
                            "points": {
                                "type": "array",
                                "items": {"type": "object", "properties": {"index": {"type": "integer"}, "point": {"$ref": "project.schema.json#/$defs/automationPoint"}}}
                            },
                            "valueAtPlayhead": {"type": ["number", "null"]}
                        }
                    }
                }
            },
            "visible": {
                "type": "object",
                "description": "The part of the song on screen.",
                "properties": {
                    "playlist": {
                        "type": ["object", "null"],
                        "properties": {"startBeat": beats, "endBeat": beats, "firstTrack": {"type": "integer"}, "lastTrack": {"type": "integer"}}
                    },
                    "pianoRoll": {
                        "type": ["object", "null"],
                        "properties": {
                            "pattern": {"type": "string"}, "channel": {"type": "string"},
                            "startBeat": beats, "endBeat": beats,
                            "lowPitch": {"type": "integer"}, "highPitch": {"type": "integer"}
                        }
                    }
                }
            },
            "recentEdits": {
                "type": "array",
                "description": "The producer's latest manual edits, newest last (edits made through project.json by an agent are not listed).",
                "items": {"type": "object", "properties": {"at": {"type": "string", "format": "date-time"}, "summary": {"type": "string"}}}
            }
        }
    })
}

pub fn schema_text() -> String {
    serde_json::to_string_pretty(&schema()).unwrap() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_fields_are_dropped_and_missing_defaulted() {
        let c = normalize(json!({"focus": "piano roll", "bogus": 1, "transport": {"playing": true}}));
        assert_eq!(c.focus, "piano roll");
        assert!(c.transport.playing);
        assert!(c.selection.notes.is_empty());
        let text = serde_json::to_string(&c).unwrap();
        assert!(!text.contains("bogus"));
        assert!(c.selection.automation.is_none());
    }

    #[test]
    fn automation_selection_is_kept() {
        let c = normalize(json!({"selection": {"automation": {
            "index": 1, "id": "pad-cutoff", "target": "channel/pad/cutoff", "pointCount": 2,
            "points": [{"index": 1, "point": {"beat": 32, "value": 6000, "curve": 0.4}}], "valueAtPlayhead": 1200.5
        }}}));
        let a = c.selection.automation.expect("automation selection");
        assert_eq!(a.id, "pad-cutoff");
        assert_eq!(a.points[0].point.value, 6000.0);
        assert_eq!(a.value_at_playhead, Some(1200.5));
    }
}
