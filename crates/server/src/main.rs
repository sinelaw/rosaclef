//! `rosaclef` — the Rosaclef studio server and command line tools.

mod decode;
mod folder;
mod guide;
mod server;
mod terminal;

#[cfg(feature = "device-audio")]
mod device;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use folder::Folder;
use rosaclef_core::{format, validate, Device, PROJECT_FILE};
use rosaclef_engine::render::{self, RenderScope};
use rosaclef_engine::Engine;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "rosaclef", version, about = "Rosaclef — an opulent music studio for the AI era")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Open a project folder in the studio (default command).
    Serve(ServeArgs),
    /// Create a new project folder.
    New {
        dir: PathBuf,
        /// Start from the bundled demo song instead of an empty project.
        #[arg(long)]
        demo: bool,
    },
    /// Check a project file against the schema and the semantic rules.
    Validate {
        /// project.json or a project folder (default: current folder).
        path: Option<PathBuf>,
    },
    /// Rewrite a project file in canonical formatting.
    Fmt { path: Option<PathBuf> },
    /// Print a compact overview of a project.
    Summary { path: Option<PathBuf> },
    /// Render the song (or a pattern) to a WAV file.
    Render {
        /// Project folder or project.json (default: current folder).
        path: Option<PathBuf>,
        /// Output file (default: renders/<title>.wav inside the project).
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Render this pattern instead of the song.
        #[arg(long)]
        pattern: Option<String>,
        /// Pattern repetitions.
        #[arg(long, default_value_t = 1)]
        loops: u32,
        /// Bit depth: 16, 24 or 32 (float).
        #[arg(long, default_value_t = 24)]
        bits: u16,
        #[arg(long, default_value_t = 48000)]
        sample_rate: u32,
    },
    /// Synthesize a single note to a WAV file (e.g. to create a sample).
    Note {
        /// Project folder used to resolve --channel (default: current folder).
        #[arg(long)]
        project: Option<PathBuf>,
        /// Use the instrument of this channel.
        #[arg(long)]
        channel: Option<String>,
        /// Or an instrument as JSON, e.g. '{"type":"drum","options":{"kind":"clap"}}'.
        #[arg(long)]
        instrument: Option<String>,
        #[arg(long, default_value_t = 60)]
        pitch: u8,
        #[arg(long, default_value_t = 0.9)]
        velocity: f32,
        #[arg(long, default_value_t = 2.0)]
        seconds: f32,
        #[arg(short, long)]
        out: PathBuf,
        #[arg(long, default_value_t = 48000)]
        sample_rate: u32,
    },
    /// Print the project JSON schema.
    Schema,
    /// Print the device catalog (instruments, effects and their parameters).
    Catalog,
    /// List factory presets (optionally for one instrument type), or print one as JSON.
    Presets {
        /// Instrument type (e.g. prisme) or a preset name.
        filter: Option<String>,
    },
    /// Write/refresh AGENTS.md, CLAUDE.md and GEMINI.md in a project folder.
    Guide { dir: Option<PathBuf> },
}

#[derive(clap::Args, Default)]
struct ServeArgs {
    /// Project folder (created if missing).
    dir: Option<PathBuf>,
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(long, default_value_t = 7470)]
    port: u16,
    /// Directory with the web UI (default: the repository's web/ folder).
    #[arg(long)]
    web: Option<PathBuf>,
    /// Seed new projects with the demo song.
    #[arg(long)]
    demo: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Serve(ServeArgs::default())) {
        Command::Serve(a) => serve(a),
        Command::New { dir, demo } => {
            let f = Folder::new(&dir);
            if f.project_path().exists() {
                bail!("{} already contains a {PROJECT_FILE}", dir.display());
            }
            f.init(demo)?;
            guide::write(&f)?;
            println!("Created {}", f.dir.display());
            println!("Open it with: rosaclef serve {}", dir.display());
            Ok(())
        }
        Command::Validate { path } => {
            let file = project_file(path)?;
            let text = std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
            let checked = validate::parse_and_validate(&text);
            for i in &checked.issues {
                println!("{i}");
            }
            if checked.is_ok() {
                let p = checked.project.unwrap();
                let notes: usize = p.patterns.iter().map(|x| x.notes.len()).sum();
                println!(
                    "ok: {} — {} channels, {} patterns ({notes} notes), {} clips, {} inserts",
                    file.display(),
                    p.channels.len(),
                    p.patterns.len(),
                    p.playlist.clips.len(),
                    p.mixer.inserts.len()
                );
                Ok(())
            } else {
                std::process::exit(1);
            }
        }
        Command::Fmt { path } => {
            let file = project_file(path)?;
            let p = load_project(&file)?;
            std::fs::write(&file, format::to_string(&p))?;
            println!("formatted {}", file.display());
            Ok(())
        }
        Command::Summary { path } => {
            let p = load_project(&project_file(path)?)?;
            print!("{}", rosaclef_core::summary(&p));
            Ok(())
        }
        Command::Render { path, out, pattern, loops, bits, sample_rate } => {
            let file = project_file(path)?;
            let dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
            let folder = Folder::new(&dir);
            let project = load_project(&file)?;
            if ![16, 24, 32].contains(&bits) {
                bail!("--bits must be 16, 24 or 32");
            }
            let scope = match &pattern {
                Some(id) => {
                    if project.pattern(id).is_none() {
                        bail!("no pattern with id {id:?}");
                    }
                    RenderScope::Pattern { id: id.clone(), loops }
                }
                None => RenderScope::Song,
            };
            let out = out.unwrap_or_else(|| {
                let name = pattern.clone().unwrap_or_else(|| slug(&project.meta.title));
                dir.join(folder::RENDERS_DIR).join(format!("{name}.wav"))
            });
            let t0 = std::time::Instant::now();
            let (audio, warnings) = render_project(&folder, project, &scope, sample_rate as f32);
            for w in &warnings {
                eprintln!("warning: {w}");
            }
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out, render::encode_wav(&audio, bits))?;
            println!(
                "rendered {} ({:.1}s, peak {:.1} dBFS, rms {:.1} dBFS) in {:.2}s",
                out.display(),
                audio.duration(),
                20.0 * audio.peak().max(1e-9).log10(),
                20.0 * audio.rms().max(1e-9).log10(),
                t0.elapsed().as_secs_f32()
            );
            Ok(())
        }
        Command::Note { project, channel, instrument, pitch, velocity, seconds, out, sample_rate } => {
            let device: Device = match (channel, instrument) {
                (Some(id), None) => {
                    let p = load_project(&project_file(project)?)?;
                    p.channel(&id).ok_or_else(|| anyhow!("no channel with id {id:?}"))?.instrument.clone()
                }
                (None, Some(json)) => serde_json::from_str(&json).context("parsing --instrument")?,
                _ => bail!("pass exactly one of --channel or --instrument"),
            };
            let audio = render::render_note(&device, pitch, velocity, seconds, sample_rate as f32);
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out, render::encode_wav(&audio, 24))?;
            println!("wrote {} ({seconds}s)", out.display());
            Ok(())
        }
        Command::Schema => {
            print!("{}", rosaclef_core::schema::schema_text());
            Ok(())
        }
        Command::Catalog => {
            print!("{}", rosaclef_core::catalog_markdown());
            Ok(())
        }
        Command::Presets { filter } => {
            if let Some(p) = filter.as_deref().and_then(rosaclef_core::presets::find) {
                println!("{}", serde_json::to_string_pretty(&p.device())?);
                return Ok(());
            }
            for p in rosaclef_core::presets::all() {
                if filter.as_deref().map(|f| f == p.kind).unwrap_or(true) {
                    println!("{:<10} {:<26} {:<24} {}", p.kind, p.name, p.tags, p.doc);
                }
            }
            Ok(())
        }
        Command::Guide { dir } => {
            let f = Folder::new(dir.unwrap_or_else(|| PathBuf::from(".")));
            guide::write(&f)?;
            println!("wrote agent guides in {}", f.dir.display());
            Ok(())
        }
    }
}

/// Render a project offline, loading its samples and plugins.
pub fn render_project(folder: &Folder, project: rosaclef_core::Project, scope: &RenderScope, sample_rate: f32) -> (render::Audio, Vec<String>) {
    let mut engine = Engine::new(sample_rate);
    server::install_plugin_host(&mut engine);
    engine.set_project(project);
    let mut warnings = engine.device_errors.clone();
    for path in engine.required_samples() {
        match folder.resolve(&path).ok_or_else(|| anyhow!("invalid path")).and_then(|p| decode::decode_file(&p)) {
            Ok(data) => engine.set_sample(&path, data),
            Err(e) => warnings.push(format!("sample {path}: {e}")),
        }
    }
    (render::render(&mut engine, scope), warnings)
}

fn project_file(path: Option<PathBuf>) -> Result<PathBuf> {
    let p = path.unwrap_or_else(|| PathBuf::from("."));
    let file = if p.is_dir() { p.join(PROJECT_FILE) } else { p };
    if !file.exists() {
        bail!("{} not found (pass a project folder or project.json)", file.display());
    }
    Ok(file)
}

fn load_project(file: &Path) -> Result<rosaclef_core::Project> {
    let text = std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let checked = validate::parse_and_validate(&text);
    if !checked.is_ok() {
        let msgs: Vec<String> = checked.issues.iter().map(|i| i.to_string()).collect();
        bail!("{} is invalid:\n{}", file.display(), msgs.join("\n"));
    }
    Ok(checked.project.unwrap())
}

pub fn slug(s: &str) -> String {
    let s: String = s.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.split('-').filter(|x| !x.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() {
        "untitled".into()
    } else {
        s
    }
}

fn serve(a: ServeArgs) -> Result<()> {
    let dir = a.dir.unwrap_or_else(|| PathBuf::from("."));
    let folder = Folder::new(&dir);
    let fresh = !folder.project_path().exists();
    folder.init(a.demo || fresh && is_empty_dir(&folder.dir))?;
    let folder = Folder::new(&dir);
    guide::write(&folder)?;
    let web = a.web.unwrap_or_else(default_web_dir);
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(server::run(server::Config { folder, host: a.host, port: a.port, web }))
}

fn is_empty_dir(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().all(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.starts_with('.') || [folder::SAMPLES_DIR, folder::RENDERS_DIR].contains(&n.as_ref())
        }))
        .unwrap_or(true)
}

fn default_web_dir() -> PathBuf {
    if let Ok(p) = std::env::var("ROSACLEF_WEB") {
        return PathBuf::from(p);
    }
    // Next to the executable (installed layout), else the source tree.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            let cand = d.join("web");
            if cand.join("index.html").exists() {
                return cand;
            }
        }
    }
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../web"))
}
