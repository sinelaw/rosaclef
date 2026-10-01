//! Voices: singing and speech engines installed on this computer, which
//! render the phrases of channels whose voice has engine "render" (see
//! [`rosaclef_core::phrase`]).
//!
//! They are set up per user in `~/.config/rosaclef/voices.json` (or the file
//! `$ROSACLEF_VOICES` names), never in the project: a project only names a
//! voice, so opening someone's project cannot run a command.
//!
//! ```json
//! {
//!   "alto":    { "input": "ds", "command": ["python3", "infer.py", "{input}", "--out", "{output}"] },
//!   "narrator": { "input": "ssml", "command": ["my-tts", "--ssml", "{input}", "--wav", "{output}"] },
//!   "by-hand": { "input": "ustx" }
//! }
//! ```
//!
//! For each phrase not rendered yet, its input (any export format, for the
//! phrase alone, timed from 0.3 s before its first note) is written next to
//! where the render goes, `renders/voice/<voice>/<key>.<ext>`, and the
//! command runs with `{input}` and `{output}` replaced by absolute paths. It
//! must write a WAV at `{output}`, which then becomes `<key>.wav`. A voice
//! without a command only gets the input files: sing them in any program and
//! save `<key>.wav` beside each. The file watcher picks renders up either way.

use crate::folder::Folder;
use anyhow::{anyhow, bail, Context, Result};
use parking_lot::Mutex;
use rosaclef_core::Project;
use rosaclef_vocal::formats::FORMATS;
use rosaclef_vocal::phrase::{self, Job};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;

/// How to run one voice.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Voice {
    /// The export format the engine reads (e.g. "ds", "ssml", "musicxml").
    pub input: String,
    /// The command and its arguments; none: input files only.
    #[serde(default)]
    pub command: Vec<String>,
}

/// The voices file: where it is and what it says.
fn settings() -> Result<BTreeMap<String, Voice>> {
    let path = match std::env::var_os("ROSACLEF_VOICES") {
        Some(p) => PathBuf::from(p),
        None => match std::env::var_os("HOME") {
            Some(home) => Path::new(&home).join(".config/rosaclef/voices.json"),
            None => return Ok(BTreeMap::new()),
        },
    };
    if !path.is_file() {
        return Ok(BTreeMap::new());
    }
    let text = std::fs::read_to_string(&path)?;
    serde_json::from_str(&text).with_context(|| format!("reading {}", path.display()))
}

/// A phrase handed to the worker: run `argv`, then move `part` to `done`.
struct Run {
    argv: Vec<String>,
    dir: PathBuf,
    part: PathBuf,
    done: PathBuf,
}

/// Renders phrases in the background, one at a time.
pub struct Renderer {
    work: Mutex<Sender<Run>>,
    /// Renders asked for and not finished (or failed) in this session.
    asked: Arc<Mutex<HashSet<PathBuf>>>,
}

impl Renderer {
    pub fn new() -> Renderer {
        let (tx, rx) = channel::<Run>();
        let asked: Arc<Mutex<HashSet<PathBuf>>> = Arc::default();
        std::thread::spawn(move || {
            for run in rx {
                if let Err(e) = execute(&run) {
                    eprintln!("  ✗ voice render {}: {e:#}", run.done.display());
                }
            }
        });
        Renderer {
            work: Mutex::new(tx),
            asked,
        }
    }

    /// Ask for every phrase of the project not rendered yet.
    pub fn update(&self, project: &Project, folder: &Folder) {
        let jobs = phrase::jobs(project);
        if jobs.is_empty() {
            return;
        }
        let voices = match settings() {
            Ok(v) => v,
            Err(e) => return eprintln!("  ✗ voices: {e:#}"),
        };
        for job in jobs {
            if let Err(e) = self.ask(&job, &voices, folder) {
                eprintln!("  ✗ voice {}: {e:#}", job.voice);
            }
        }
    }

    fn ask(&self, job: &Job, voices: &BTreeMap<String, Voice>, folder: &Folder) -> Result<()> {
        let done = folder
            .resolve(&job.path)
            .ok_or_else(|| anyhow!("bad path"))?;
        if done.is_file() || !self.asked.lock().insert(done.clone()) {
            return Ok(());
        }
        let Some(voice) = voices.get(&job.voice) else {
            bail!("no such voice in the voices settings (see AGENTS.md)");
        };
        let format = FORMATS
            .iter()
            .find(|f| f.id == voice.input)
            .ok_or_else(|| anyhow!("unknown input format {:?}", voice.input))?;
        let dir = done
            .parent()
            .ok_or_else(|| anyhow!("bad path"))?
            .to_path_buf();
        std::fs::create_dir_all(&dir)?;
        let input = done.with_extension(format.extension);
        std::fs::write(&input, format.write(&job.song))?;
        if voice.command.is_empty() {
            return Ok(());
        }
        let part = done.with_extension("part.wav");
        let fill = |a: &String| {
            a.replace("{input}", &input.display().to_string())
                .replace("{output}", &part.display().to_string())
        };
        let run = Run {
            argv: voice.command.iter().map(fill).collect(),
            dir,
            part,
            done,
        };
        self.work
            .lock()
            .send(run)
            .map_err(|_| anyhow!("the render worker stopped"))
    }
}

/// Run a voice's command and keep its WAV.
fn execute(run: &Run) -> Result<()> {
    let (program, args) = run
        .argv
        .split_first()
        .ok_or_else(|| anyhow!("empty command"))?;
    let out = std::process::Command::new(program)
        .args(args)
        .current_dir(&run.dir)
        .output()
        .with_context(|| format!("running {program}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("{program} failed ({}): {}", out.status, err.trim());
    }
    if !run.part.is_file() {
        bail!("{program} wrote no audio at {}", run.part.display());
    }
    std::fs::rename(&run.part, &run.done)?;
    println!("  ♪ rendered {}", run.done.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_read_voices() {
        let v: BTreeMap<String, Voice> = serde_json::from_str(
            r#"{"alto": {"input": "ds", "command": ["sing", "{input}", "{output}"]}, "hand": {"input": "ustx"}}"#,
        )
        .unwrap();
        assert_eq!(v["alto"].command.len(), 3);
        assert!(v["hand"].command.is_empty());
        assert!(serde_json::from_str::<BTreeMap<String, Voice>>(
            r#"{"x": {"input": "ds", "cmd": []}}"#
        )
        .is_err());
    }
}
