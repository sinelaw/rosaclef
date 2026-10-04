//! Rosaclef core: the project document model shared by every part of the
//! system (native server, WebAssembly engine, command line tools).

pub mod arp;
pub mod automation;
pub mod catalog;
pub mod context;
pub mod form;
pub mod format;
pub mod gm;
pub mod model;
pub mod presets;
pub mod schema;
pub mod validate;

pub use model::*;

/// Name of the project document inside a project folder.
pub const PROJECT_FILE: &str = "project.json";

/// A short human-readable overview of a project (used by `rosaclef summary`).
pub fn summary(p: &Project) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let t = &p.transport;
    let _ = writeln!(
        s,
        "\"{}\" — {} BPM, {}/4, swing {}",
        p.meta.title,
        format::format_f64(t.bpm),
        t.beats_per_bar,
        format::format_f64(t.swing)
    );
    if !t.meters.is_empty() {
        let list: Vec<String> = t
            .meters
            .iter()
            .take(12)
            .map(|m| format!("bar {}: {}/{}", m.bar, m.numerator, m.denominator))
            .collect();
        let more = if t.meters.len() > 12 { ", ..." } else { "" };
        let _ = writeln!(s, "Meter changes: {}{more}", list.join(", "));
    }
    let _ = writeln!(s, "\nChannels ({}):", p.channels.len());
    for (i, c) in p.channels.iter().enumerate() {
        let detail = match c.instrument.kind.as_str() {
            "drum" => format!("drum/{}", c.instrument.option("kind")),
            "sampler" => format!("sampler {}", c.instrument.option("sample")),
            "plugin" => format!("plugin {}", c.instrument.option("path")),
            k => k.to_string(),
        };
        let layer = match &c.layer_of {
            Some(of) => format!(" (layer of {of})"),
            None => String::new(),
        };
        let _ = writeln!(
            s,
            "  [{i}] {:<14} {:<22} → insert {}{}{layer}",
            c.id,
            detail,
            c.mixer,
            if c.mute { " (muted)" } else { "" }
        );
    }
    let _ = writeln!(s, "\nPatterns ({}):", p.patterns.len());
    for pat in &p.patterns {
        let mut per: Vec<(String, usize)> = vec![];
        for n in &pat.notes {
            match per.iter_mut().find(|(c, _)| *c == n.channel) {
                Some(e) => e.1 += 1,
                None => per.push((n.channel.clone(), 1)),
            }
        }
        let per: Vec<String> = per.iter().map(|(c, n)| format!("{c}:{n}")).collect();
        let _ = writeln!(
            s,
            "  {:<14} \"{}\" {} beats, {} notes [{}]",
            pat.id,
            pat.name,
            format::format_f64(pat.length),
            pat.notes.len(),
            per.join(" ")
        );
    }
    let secs = form::performance_seconds(p, &automation::TempoMap::new(p));
    let _ = writeln!(
        s,
        "\nPlaylist: {} tracks, {} clips, song length {} beats ({}:{:04.1})",
        p.playlist.tracks.len(),
        p.playlist.clips.len(),
        format::format_f64(p.song_length()),
        (secs / 60.0).floor(),
        secs % 60.0
    );
    for (ti, tr) in p.playlist.tracks.iter().enumerate() {
        let clips: Vec<String> = p
            .playlist
            .clips
            .iter()
            .filter(|c| c.track.index() == ti)
            .map(|c| {
                let what = if c.pattern.is_empty() {
                    c.sample.clone()
                } else {
                    c.pattern.clone()
                };
                format!(
                    "{what}@{}+{}",
                    format::format_f64(c.start),
                    format::format_f64(c.length)
                )
            })
            .collect();
        if !clips.is_empty() {
            let _ = writeln!(s, "  [{ti}] {:<12} {}", tr.name, clips.join("  "));
        }
    }
    let _ = writeln!(s, "\nMixer ({} inserts):", p.mixer.inserts.len());
    for (i, ins) in p.mixer.inserts.iter().enumerate() {
        let used =
            i == 0 || p.channels.iter().any(|c| c.mixer.index() == i) || !ins.effects.is_empty();
        if !used {
            continue;
        }
        let fx: Vec<&str> = ins.effects.iter().map(|e| e.kind.as_str()).collect();
        let _ = writeln!(
            s,
            "  [{i}] {:<12} vol {} pan {} fx [{}]",
            ins.name,
            format::format_f64(ins.volume),
            format::format_f64(ins.pan),
            fx.join(", ")
        );
    }
    if !p.automation.is_empty() {
        let _ = writeln!(s, "\nAutomation ({} lanes):", p.automation.len());
        for lane in &p.automation {
            let (lo, hi) = lane
                .points
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), pt| {
                    (lo.min(pt.value), hi.max(pt.value))
                });
            let span = match (lane.points.first(), lane.points.last()) {
                (Some(a), Some(b)) => format!(
                    "beats {}..{}",
                    format::format_f64(a.beat),
                    format::format_f64(b.beat)
                ),
                _ => "no points".into(),
            };
            let range = if lane.points.is_empty() {
                String::new()
            } else {
                format!(
                    ", values {}..{}",
                    format::format_f64(lo),
                    format::format_f64(hi)
                )
            };
            let _ = writeln!(
                s,
                "  {:<14} {:<28} {} points, {span}{range}{}",
                lane.id,
                lane.target,
                lane.points.len(),
                if lane.mute { " (muted)" } else { "" }
            );
        }
    }
    s
}

/// Markdown table of the factory presets (embedded in the agent guide).
pub fn presets_markdown() -> String {
    use std::fmt::Write;
    let mut s = String::from("| preset | type | tags | |\n|---|---|---|---|\n");
    for p in presets::all() {
        let _ = writeln!(s, "| {} | `{}` | {} | {} |", p.name, p.kind, p.tags, p.doc);
    }
    s
}

/// Markdown reference of every device and parameter (embedded in the agent guide).
pub fn catalog_markdown() -> String {
    use std::fmt::Write;
    let mut s = String::new();
    // Which instrument for which part: the quick answer before the details.
    let _ = writeln!(
        s,
        "### Choosing an instrument\n\n| for | `type` | instrument | presets |\n|---|---|---|---|"
    );
    for d in catalog::instruments().filter(|d| !d.best_for.is_empty()) {
        let presets: Vec<String> = crate::presets::all()
            .into_iter()
            .filter(|p| p.kind == d.kind)
            .map(|p| p.name.to_string())
            .collect();
        let presets = if presets.is_empty() {
            "—".to_string()
        } else {
            presets.join(", ")
        };
        let _ = writeln!(
            s,
            "| {} | `{}` | {} | {} |",
            d.best_for, d.kind, d.label, presets
        );
    }
    s.push('\n');
    for (title, cat) in [
        ("Instruments", catalog::Category::Instrument),
        ("Effects", catalog::Category::Effect),
    ] {
        let _ = writeln!(s, "### {title}\n");
        for d in catalog::DEVICES.iter().filter(|d| d.category == cat) {
            let _ = writeln!(s, "#### `{}` — {}\n\n{}\n", d.kind, d.label, d.full_doc());
            if !d.options.is_empty() {
                let _ = writeln!(s, "| option | values | default | |\n|---|---|---|---|");
                for o in d.options {
                    let values = if o.choices.is_empty() {
                        "text".to_string()
                    } else {
                        o.choices.join(" \\| ")
                    };
                    let _ = writeln!(
                        s,
                        "| `{}` | {} | `{}` | {} |",
                        o.key,
                        values,
                        o.default,
                        o.full_doc()
                    );
                }
                s.push('\n');
            }
            if !d.params.is_empty() {
                let _ = writeln!(s, "| param | range | default | |\n|---|---|---|---|");
                for p in d.params {
                    let unit = if p.unit.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", p.unit)
                    };
                    let int = if p.integer { " (integer)" } else { "" };
                    let _ = writeln!(
                        s,
                        "| `{}` | {} … {}{unit}{int} | {} | {} |",
                        p.key,
                        format::format_f64(p.min),
                        format::format_f64(p.max),
                        format::format_f64(p.default),
                        p.doc
                    );
                }
                s.push('\n');
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_project_round_trips_and_validates() {
        let p = Project::empty("Test");
        let text = format::to_string(&p);
        let checked = validate::parse_and_validate(&text);
        assert!(checked.is_ok(), "{:?}", checked.issues);
        assert_eq!(checked.project.unwrap(), p);
    }

    #[test]
    fn errors_have_paths() {
        let mut p = Project::empty("Test");
        p.channels.push(Channel {
            id: "lead".into(),
            name: "Lead".into(),
            color: "#ffffff".into(),
            instrument: Device {
                kind: "analog".into(),
                enabled: true,
                params: [("cutoff".into(), 99999.0)].into(),
                options: Default::default(),
            },
            volume: 0.8,
            pan: 0.0,
            mute: false,
            mixer: InsertIx(99),
            arp: None,
            layer_of: None,
        });
        let issues = validate::validate(&p);
        let paths: Vec<&str> = issues.iter().map(|i| i.path.as_str()).collect();
        assert!(paths.contains(&"channels[0].mixer"));
        assert!(paths.contains(&"channels[0].instrument.params.cutoff"));
    }

    #[test]
    fn layers_must_point_at_a_plain_channel() {
        let mut p = with_pad();
        let mut layer = p.channels[0].clone();
        layer.id = "air".into();
        layer.layer_of = Some("pad".into());
        p.channels.push(layer);
        assert!(validate::validate(&p).is_empty());
        let bad = |of: &str| {
            let mut q = p.clone();
            let mut c = q.channels[0].clone();
            c.id = "x".into();
            c.layer_of = Some(of.into());
            q.channels.push(c);
            validate::validate(&q)
                .iter()
                .map(|i| i.path.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(bad("nope"), ["channels[2].layerOf"], "unknown channel");
        assert_eq!(bad("x"), ["channels[2].layerOf"], "itself");
        assert_eq!(bad("air"), ["channels[2].layerOf"], "a layer of a layer");
        let text = format::to_string(&p);
        assert!(text.contains(r#""layerOf": "pad""#), "{text}");
        assert!(summary(&p).contains("(layer of pad)"));
    }

    #[test]
    fn meters_map_bars_to_beats() {
        let mut p = Project::empty("Test");
        p.transport.beats_per_bar = 3;
        p.transport.meters = vec![
            Meter {
                bar: 3,
                numerator: 7,
                denominator: 8,
            },
            Meter {
                bar: 5,
                numerator: 4,
                denominator: 4,
            },
        ];
        assert!(validate::validate(&p).is_empty());
        let t = &p.transport;
        assert_eq!(t.bar_start(2), 6.0);
        assert_eq!(t.bar_start(4), 13.0);
        assert_eq!(t.bar_start(6), 21.0);
        assert_eq!(t.bar_at(12.9), (3, 9.5, 3.5));
        assert_eq!(t.bar_at(13.0), (4, 13.0, 4.0));

        p.transport.meters.push(Meter {
            bar: 5,
            numerator: 5,
            denominator: 6,
        });
        let issues = validate::validate(&p);
        let paths: Vec<&str> = issues.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(
            paths,
            ["transport.meters[2].bar", "transport.meters[2].denominator"]
        );
    }

    fn lane(id: &str, target: &str, points: &[(f64, f64, f64)]) -> AutomationLane {
        AutomationLane {
            id: id.into(),
            name: String::new(),
            target: target.into(),
            color: "#8a6bb0".into(),
            mute: false,
            points: points
                .iter()
                .map(|&(beat, value, curve)| AutomationPoint { beat, value, curve })
                .collect(),
        }
    }

    fn with_pad() -> Project {
        let mut p = Project::empty("Test");
        p.channels.push(Channel {
            id: "pad".into(),
            name: "Pad".into(),
            color: "#ffffff".into(),
            instrument: Device::new("analog"),
            volume: 0.8,
            pan: 0.0,
            mute: false,
            mixer: InsertIx(1),
            arp: None,
            layer_of: None,
        });
        p
    }

    #[test]
    fn automation_round_trips_and_validates() {
        let mut p = with_pad();
        p.automation.push(lane(
            "pad-cutoff",
            "channel/pad/cutoff",
            &[(0.0, 400.0, 0.0), (32.0, 6000.0, 0.4)],
        ));
        p.automation.push(lane(
            "tempo",
            "tempo",
            &[(0.0, 120.0, 0.0), (16.0, 128.0, 0.0)],
        ));
        p.automation.push(lane(
            "fade",
            "insert/0/volume",
            &[(0.0, 1.0, 0.0), (8.0, 0.0, -0.5)],
        ));
        p.automation.push(lane(
            "lim",
            "insert/0/effect/0/ceiling",
            &[(0.0, -1.0, 0.0)],
        ));
        let text = format::to_string(&p);
        // Points stay one per line.
        assert!(
            text.contains("\n        { \"beat\": 32, \"value\": 6000, \"curve\": 0.4 }"),
            "{text}"
        );
        let checked = validate::parse_and_validate(&text);
        assert!(checked.is_ok(), "{:?}", checked.issues);
        assert_eq!(checked.project.unwrap(), p);
        assert!(summary(&p).contains("pad-cutoff"));
        // Projects without automation serialize without the key.
        assert!(!format::to_string(&with_pad()).contains("automation"));
    }

    #[test]
    fn automation_errors_have_paths() {
        let mut p = with_pad();
        p.automation.push(lane(
            "a",
            "channel/pad/cutoff",
            &[(4.0, 400.0, 0.0), (2.0, 99999.0, 3.0)],
        ));
        p.automation
            .push(lane("a", "channel/lead/volume", &[(0.0, 0.5, 0.0)]));
        p.automation.push(lane("b", "insert/0/effect/3/mix", &[]));
        p.automation.push(lane("c", "bpm", &[(0.0, 120.0, 0.0)]));
        p.automation
            .push(lane("d", "channel/pad/cutoff", &[(-1.0, 400.0, 0.0)]));
        p.automation.push(lane("e", "tempo", &[(0.0, 10.0, 0.0)]));
        p.automation
            .push(lane("f", "channel/pad/nope", &[(0.0, 1.0, 0.0)]));
        let issues = validate::validate(&p);
        let has = |path: &str, needle: &str| {
            issues
                .iter()
                .any(|i| i.path == path && i.message.contains(needle))
        };
        assert!(has("automation[0].points[1].beat", "sorted"), "{issues:?}");
        assert!(has("automation[0].points[1].value", "range"));
        assert!(has("automation[0].points[1].curve", "range"));
        assert!(has("automation[1].id", "duplicate"));
        assert!(has("automation[1].target", "unknown channel"));
        assert!(has("automation[2].target", "no effect 3"));
        assert!(has("automation[2].points", "at least one"));
        assert!(has("automation[3].target", "invalid automation target"));
        assert!(has("automation[4].target", "already automates"));
        assert!(has("automation[4].points[0].beat", ">= 0"));
        assert!(has("automation[5].points[0].value", "range"));
        assert!(has("automation[6].target", "no parameter"));
    }

    #[test]
    fn score_round_trips_and_validates() {
        let mut p = with_pad();
        p.score.key = "Eb".into();
        p.score.hidden.push("pad".into());
        p.score.hidden_tracks.push(model::TrackIx(0));
        p.score.clefs.insert("pad".into(), "grand".into());
        p.score.marks.push(model::ScoreMark {
            start: 4.0,
            end: 8.0,
            color: "#c97b84".into(),
            label: "Chorus".into(),
            pattern: String::new(),
            channels: vec!["pad".into()],
        });
        let text = format::to_string(&p);
        let checked = validate::parse_and_validate(&text);
        assert!(checked.is_ok(), "{:?}", checked.issues);
        assert_eq!(checked.project.unwrap(), p);
        // Projects without score settings serialize without the key.
        assert!(!format::to_string(&with_pad()).contains("score"));
    }

    #[test]
    fn score_errors_have_paths() {
        let mut p = with_pad();
        p.score.key = "H".into();
        p.score.clefs.insert("pad".into(), "soprano".into());
        p.score.marks.push(model::ScoreMark {
            start: 8.0,
            end: 4.0,
            color: "red".into(),
            label: String::new(),
            pattern: "nope".into(),
            channels: vec!["ghost".into()],
        });
        let issues = validate::validate(&p);
        let has = |path: &str, needle: &str| {
            issues
                .iter()
                .any(|i| i.path == path && i.message.contains(needle))
        };
        assert!(has("score.key", "unknown key"), "{issues:?}");
        assert!(has("score.clefs.pad", "unknown clef"));
        assert!(has("score.marks[0].end", "after start"));
        assert!(has("score.marks[0].color", "hex"));
        assert!(has("score.marks[0].pattern", "does not exist"));
        assert!(has("score.marks[0].channels[0]", "does not exist"));
    }

    #[test]
    fn typos_are_reported_with_path() {
        let text = r#"{"format":"rosaclef/1","meta":{"title":"x"},"transport":{"bpm":120},
            "patterns":[{"id":"a","name":"A","length":4,"notes":[{"channel":"c","pitch":60,"startTime":0,"length":1}]}],
            "mixer":{"inserts":[{"name":"Master"}]}}"#;
        let checked = validate::parse_and_validate(text);
        assert!(!checked.is_ok());
        assert_eq!(checked.issues[0].path, "patterns[0].notes[0].startTime");
        assert!(checked.issues[0].message.contains("startTime"));
    }
}
