//! Rosaclef core: the project document model shared by every part of the
//! system (native server, WebAssembly engine, command line tools).

pub mod catalog;
pub mod presets;
pub mod format;
pub mod model;
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
    let _ = writeln!(s, "\"{}\" — {} BPM, {}/4, swing {}", p.meta.title, format::format_f64(t.bpm), t.beats_per_bar, format::format_f64(t.swing));
    let _ = writeln!(s, "\nChannels ({}):", p.channels.len());
    for (i, c) in p.channels.iter().enumerate() {
        let detail = match c.instrument.kind.as_str() {
            "drum" => format!("drum/{}", c.instrument.option("kind")),
            "sampler" => format!("sampler {}", c.instrument.option("sample")),
            "plugin" => format!("plugin {}", c.instrument.option("path")),
            k => k.to_string(),
        };
        let _ = writeln!(s, "  [{i}] {:<14} {:<22} → insert {}{}", c.id, detail, c.mixer, if c.mute { " (muted)" } else { "" });
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
        let _ = writeln!(s, "  {:<14} \"{}\" {} beats, {} notes [{}]", pat.id, pat.name, format::format_f64(pat.length), pat.notes.len(), per.join(" "));
    }
    let _ = writeln!(s, "\nPlaylist: {} tracks, {} clips, song length {} beats", p.playlist.tracks.len(), p.playlist.clips.len(), format::format_f64(p.song_length()));
    for (ti, tr) in p.playlist.tracks.iter().enumerate() {
        let clips: Vec<String> = p
            .playlist
            .clips
            .iter()
            .filter(|c| c.track.index() == ti)
            .map(|c| {
                let what = if c.pattern.is_empty() { c.sample.clone() } else { c.pattern.clone() };
                format!("{what}@{}+{}", format::format_f64(c.start), format::format_f64(c.length))
            })
            .collect();
        if !clips.is_empty() {
            let _ = writeln!(s, "  [{ti}] {:<12} {}", tr.name, clips.join("  "));
        }
    }
    let _ = writeln!(s, "\nMixer ({} inserts):", p.mixer.inserts.len());
    for (i, ins) in p.mixer.inserts.iter().enumerate() {
        let used = i == 0 || p.channels.iter().any(|c| c.mixer.index() == i) || !ins.effects.is_empty();
        if !used {
            continue;
        }
        let fx: Vec<&str> = ins.effects.iter().map(|e| e.kind.as_str()).collect();
        let _ = writeln!(s, "  [{i}] {:<12} vol {} pan {} fx [{}]", ins.name, format::format_f64(ins.volume), format::format_f64(ins.pan), fx.join(", "));
    }
    s
}

/// Markdown reference of every device and parameter (embedded in the agent guide).
pub fn catalog_markdown() -> String {
    use std::fmt::Write;
    let mut s = String::new();
    for (title, cat) in [("Instruments", catalog::Category::Instrument), ("Effects", catalog::Category::Effect)] {
        let _ = writeln!(s, "### {title}\n");
        for d in catalog::DEVICES.iter().filter(|d| d.category == cat) {
            let _ = writeln!(s, "#### `{}` — {}\n\n{}\n", d.kind, d.label, d.doc);
            if !d.options.is_empty() {
                let _ = writeln!(s, "| option | values | default | |\n|---|---|---|---|");
                for o in d.options {
                    let values = if o.choices.is_empty() { "text".to_string() } else { o.choices.join(" \\| ") };
                    let _ = writeln!(s, "| `{}` | {} | `{}` | {} |", o.key, values, o.default, o.doc);
                }
                s.push('\n');
            }
            if !d.params.is_empty() {
                let _ = writeln!(s, "| param | range | default | |\n|---|---|---|---|");
                for p in d.params {
                    let unit = if p.unit.is_empty() { String::new() } else { format!(" {}", p.unit) };
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
            instrument: Device { kind: "synth".into(), enabled: true, params: [("cutoff".into(), 99999.0)].into(), options: Default::default() },
            volume: 0.8,
            pan: 0.0,
            mute: false,
            mixer: InsertIx(99),
        });
        let issues = validate::validate(&p);
        let paths: Vec<&str> = issues.iter().map(|i| i.path.as_str()).collect();
        assert!(paths.contains(&"channels[0].mixer"));
        assert!(paths.contains(&"channels[0].instrument.params.cutoff"));
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
