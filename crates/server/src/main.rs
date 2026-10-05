//! `rosaclef` — the Rosaclef studio server and command line tools.

mod folder;
mod library;
mod progress;
mod server;
mod terminal;

#[cfg(feature = "device-audio")]
mod device;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use folder::Folder;
use rosaclef_core::{format, validate, Device, PROJECT_FILE};
use rosaclef_engine::render::{self, RenderScope};
use rosaclef_studio::library::Library;
use rosaclef_studio::render::render_project_with;
use rosaclef_studio::{decode, guide, slug};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "rosaclef",
    version,
    about = "Rosaclef — an opulent music studio for the AI era"
)]
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
    /// Lint the song against production rules of thumb (harmony, melody,
    /// rhythm, arrangement, low end, mix, effects, master): what to fix, and
    /// one-click fixes. Mechanical checks, no AI. See docs/critic.md.
    Critic {
        /// Project folder or project.json (default: current folder).
        path: Option<PathBuf>,
        /// Print the findings as JSON (with each fix's JSON Patch operations).
        #[arg(long)]
        json: bool,
        /// Apply fixes and save: a finding's key, a rule id, or `all`
        /// (repeatable). Suppressed findings are left alone unless named by key.
        #[arg(long)]
        fix: Vec<String>,
        /// Suppress a finding (by key) or turn a check off (by rule id), in
        /// project.json (repeatable).
        #[arg(long)]
        suppress: Vec<String>,
        /// Bring back a suppressed finding or a check turned off (repeatable).
        #[arg(long)]
        unsuppress: Vec<String>,
        /// Turn a check on, by rule id: the classical theory checks (keys,
        /// counterpoint, singable melodies) are off by default (repeatable).
        #[arg(long)]
        enable: Vec<String>,
        /// Turn a check off, by rule id (repeatable).
        #[arg(long)]
        disable: Vec<String>,
        /// List the suppressed findings too.
        #[arg(long)]
        suppressed: bool,
        /// List the checks (and which are off).
        #[arg(long)]
        rules: bool,
        /// Also run the audio checks ("Mix check": overload, pumping, masking,
        /// clashes, low end, phase, build): renders the song once (cached),
        /// like `rosaclef mixcheck`.
        #[arg(long)]
        audio: bool,
    },
    /// Mix diagnostics from one render: loudness, the limiter, masking and
    /// audibility, gain reduction, clashes, spectrum and stereo — over any
    /// range — with findings and JSON Patch fixes. Exit status: 0 no
    /// warnings, 1 warnings, 2 error. See docs/mixcheck.md.
    Mixcheck(MixcheckArgs),
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
    /// List the drum grooves a project's drum part can play.
    Grooves,
    /// Write the project's drum part (`drums` in project.json) into drum
    /// patterns and clips on a Drums track.
    Drums {
        /// Project folder or project.json (default: current folder).
        path: Option<PathBuf>,
        /// Use this groove (starts a drum part if there is none).
        #[arg(long)]
        groove: Option<String>,
        /// Use this kit: a General MIDI drum kit or Ebony.
        #[arg(long)]
        kit: Option<String>,
        /// Guess the sections from the playlist (also when the part has none).
        #[arg(long)]
        guess: bool,
        /// Forget the hand edits (kept patterns and changed grooves) first.
        #[arg(long)]
        reset_edits: bool,
        /// Instead: make drum pattern ID again from its recipe
        /// (`patterns[].drums`, the studio's Drums tab).
        #[arg(long, value_name = "ID")]
        pattern: Option<String>,
    },
    /// List factory presets (optionally for one instrument type), or print one as JSON.
    Presets {
        /// Instrument type (e.g. additive) or a preset name.
        filter: Option<String>,
    },
    /// Write/refresh AGENTS.md, CLAUDE.md and GEMINI.md in a project folder.
    Guide { dir: Option<PathBuf> },
    /// Import an LMMS project (.mmp or .mmpz) as a new project in the library.
    ImportLmms(ImportArgs),
    /// Import a Standard MIDI File (.mid) as a new project in the library.
    ImportMidi(ImportArgs),
}

#[derive(clap::Args, Default)]
struct MixcheckArgs {
    /// Project folder or project.json (default: current folder).
    path: Option<PathBuf>,
    /// Bars as the producer counts them, from 1, both included: 52:59.
    #[arg(long, value_name = "BAR:BAR")]
    range: Option<String>,
    /// Song beats (written, as clip starts): 196:228.
    #[arg(long, value_name = "B:B")]
    beats: Option<String>,
    /// A section: a score mark's label or a drum part section's name.
    #[arg(long, value_name = "NAME")]
    section: Option<String>,
    /// Rows: bar, section, or N-beats (e.g. 8-beats).
    #[arg(long, default_value = "bar")]
    by: String,
    /// Channel ids, insert indices or names, "master" (comma-separated).
    #[arg(long, value_name = "ID,…")]
    focus: Option<String>,
    /// levels,audibility,masking,dynamics,gainreduction,clashes,spectrum,stereo (default: all).
    #[arg(long, value_name = "LIST")]
    checks: Option<String>,
    /// RFC 6902 operations applied in memory (never written): JSON or @file.json.
    #[arg(long = "what-if", value_name = "JSONPATCH|@FILE")]
    what_if: Option<String>,
    /// Compare two project files (or folders): the report of B, with what changed from A.
    #[arg(long, num_args = 2, value_names = ["A", "B"])]
    compare: Vec<PathBuf>,
    /// JSON output (the default).
    #[arg(long)]
    json: bool,
    /// A summary of at most 40 lines instead of JSON.
    #[arg(long)]
    text: bool,
    #[arg(long, default_value_t = 10)]
    max_findings: usize,
    /// strict | normal | loose.
    #[arg(long, default_value = "normal")]
    threshold: String,
    /// Re-measure each suggestion under its patch (one render each).
    #[arg(long)]
    verify: bool,
    /// The master's loudness, true peak and limiter over time (every 200 ms).
    #[arg(long)]
    history: bool,
    /// A delivery target: spotify, apple, youtube, amazon, tidal, ebu-r128, atsc-a85.
    #[arg(long)]
    target: Option<String>,
    /// A reference recording to compare with, level-matched.
    #[arg(long, value_name = "FILE")]
    reference: Option<String>,
    /// Pre-roll in beats (default: 8, and at least 3 seconds).
    #[arg(long)]
    preroll: Option<f64>,
    #[arg(long, default_value_t = 48000)]
    sample_rate: u32,
    /// Render again even when the cache has it.
    #[arg(long)]
    no_cache: bool,
    /// Print the JSON Schema of the report.
    #[arg(long)]
    schema: bool,
}

#[derive(clap::Args)]
struct ImportArgs {
    file: PathBuf,
    /// Name of the new project folder (default: the file name).
    #[arg(long)]
    name: Option<String>,
    /// Library folder to create the project in (default: the current folder).
    #[arg(long)]
    library: Option<PathBuf>,
    /// MIDI: play the file with Rosaclef's synthesizers instead of the
    /// sampled General MIDI instruments.
    #[arg(long)]
    synth: bool,
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
    /// Project library shown in the Projects window (default: the folder
    /// containing the project).
    #[arg(long)]
    library: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Serve(ServeArgs::default())) {
        Command::Serve(a) => serve(a),
        Command::New { dir, demo } => {
            let f = Folder::on_disk(&dir);
            if f.project_path().exists() {
                bail!("{} already contains a {PROJECT_FILE}", dir.display());
            }
            f.init(demo)?;
            guide::write(&f, &exe())?;
            println!("Created {}", f.dir.display());
            println!("Open it with: rosaclef serve {}", dir.display());
            Ok(())
        }
        Command::Validate { path } => {
            let file = project_file(path)?;
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("reading {}", file.display()))?;
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
        Command::Critic {
            path,
            json,
            fix,
            suppress,
            unsuppress,
            enable,
            disable,
            suppressed,
            rules,
            audio,
        } => {
            use rosaclef_core::{compat, critic};
            let file = project_file(path)?;
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("reading {}", file.display()))?;
            let raw: serde_json::Value = serde_json::from_str(&text)
                .with_context(|| format!("{} is not JSON", file.display()))?;
            // Forward-compatibly, as the engine plays it: what this version
            // doesn't know is reported, and kept when the file is saved.
            let playable = compat::value_for_playback(raw.clone()).map_err(|checked| {
                let msgs: Vec<String> = checked.issues.iter().map(|i| i.to_string()).collect();
                anyhow!("{} is invalid:\n{}", file.display(), msgs.join("\n"))
            })?;
            let lossy = compat::lossy(&playable);
            let fallbacks = playable.fallbacks;
            let mut p = playable.project;
            if rules {
                print!("{}", critic::rules_text(&p));
                return Ok(());
            }
            let mut changed = false;
            // The audio findings (measured on a render) when asked for.
            let audio_now = |p: &rosaclef_core::Project| -> Result<Vec<critic::Finding>> {
                if audio {
                    audio_findings(&file, p)
                } else {
                    Ok(vec![])
                }
            };
            let heard = audio_now(&p)?;
            for w in &suppress {
                if heard.iter().any(|f| f.key == *w) {
                    if !p.critic.suppress.contains(w) {
                        p.critic.suppress.push(w.clone());
                    }
                    println!("suppressed: {w}");
                } else {
                    println!(
                        "{}",
                        critic::suppress(&mut p, w, &fallbacks).map_err(|e| anyhow!(e))?
                    );
                }
                changed = true;
            }
            for (ids, on) in [(&enable, true), (&disable, false)] {
                for id in ids {
                    println!(
                        "{}",
                        critic::set_enabled(&mut p, id, on).map_err(|e| anyhow!(e))?
                    );
                    changed = true;
                }
            }
            for w in &unsuppress {
                println!("{}", critic::unsuppress(&mut p, w).map_err(|e| anyhow!(e))?);
                changed = true;
            }
            if !fix.is_empty() && lossy {
                bail!(
                    "{} holds content this version of Rosaclef doesn't know (run `rosaclef critic`); fixes are off so that nothing of it is lost",
                    file.display()
                );
            }
            if !fix.is_empty() {
                // Audio fixes first (they were measured on this song), then the rest.
                let mut done_audio = vec![];
                let wanted = |f: &critic::Finding| {
                    fix.iter().any(|k| {
                        *k == f.key
                            || (!p.critic.suppress.contains(&f.key) && (k == "all" || k == f.rule))
                    })
                };
                let mut doc = serde_json::to_value(&p)?;
                // Each was measured on the song as it is: one that touches what an
                // earlier one changed (a fader, an effect chain) waits for a new
                // measurement rather than overwriting it.
                let mut touched: Vec<String> = vec![];
                let place = |path: &str| match path.find("/effects/") {
                    Some(i) => path[..i + 8].to_string(),
                    None => path.to_string(),
                };
                for f in heard.iter().filter(|f| wanted(f)) {
                    let Some(fx) = &f.fix else { continue };
                    let places: Vec<String> = fx.ops.iter().map(|o| place(&o.path)).collect();
                    if places.iter().any(|x| touched.contains(x)) {
                        println!(
                            "skipped: {} (it changes what an earlier fix changed; run `rosaclef critic --audio` again to re-measure)",
                            fx.label
                        );
                        continue;
                    }
                    critic::apply_ops(&mut doc, &fx.ops).map_err(|e| anyhow!("{}: {e}", f.key))?;
                    touched.extend(places);
                    done_audio.push(fx.label.clone());
                }
                if !done_audio.is_empty() {
                    p = serde_json::from_value(doc)?;
                }
                let rest: Vec<String> = fix
                    .iter()
                    .filter(|k| {
                        !heard.iter().any(|f| f.key == **k)
                            && !critic::rule(k).is_some_and(|r| r.category == "Mix check")
                    })
                    .cloned()
                    .collect();
                let (fixed, mut done) = if rest.is_empty() {
                    (p.clone(), vec![])
                } else {
                    critic::apply_fixes(&p, &rest, &[]).map_err(|e| anyhow!(e))?
                };
                done.splice(0..0, done_audio);
                let errors: Vec<String> = validate::validate(&fixed)
                    .into_iter()
                    .filter(|i| i.severity == validate::Severity::Error)
                    .map(|i| i.to_string())
                    .collect();
                if !errors.is_empty() {
                    bail!(
                        "the fixes would make the project invalid:\n{}",
                        errors.join("\n")
                    );
                }
                for d in &done {
                    println!("fixed: {d}");
                }
                if done.is_empty() {
                    println!("nothing to fix for {}", fix.join(", "));
                }
                changed |= !done.is_empty();
                p = fixed;
            }
            if changed {
                if lossy {
                    // Only the Critic's settings changed: write them into the
                    // document as it is, so nothing else is lost.
                    let mut doc = raw;
                    let settings = serde_json::to_value(&p.critic)?;
                    match doc.as_object_mut() {
                        Some(m) if p.critic.is_empty() => {
                            m.remove("critic");
                        }
                        Some(m) => {
                            m.insert("critic".into(), settings);
                        }
                        None => bail!("{} is not a JSON object", file.display()),
                    }
                    std::fs::write(&file, format::to_string(&doc))?;
                } else {
                    std::fs::write(&file, format::to_string(&p))?;
                }
                println!("saved {}", file.display());
                if json {
                    return Ok(());
                }
                println!();
            }
            let mut c = critic::critique_with(&p, &[], &fallbacks);
            if audio {
                let off = |id: &str| critic::rule(id).is_some_and(|r| !critic::enabled(&p, r));
                c.findings
                    .extend(audio_now(&p)?.into_iter().filter(|f| !off(f.rule)));
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&c)?);
            } else if !changed || fix.is_empty() {
                print!("{}", critic::report_text(&c, suppressed));
            } else {
                let left = c.findings.iter().filter(|f| !f.suppressed).count();
                println!("{left} findings left (run `rosaclef critic` to see them)");
            }
            Ok(())
        }
        Command::Mixcheck(a) => match mixcheck_cli(a) {
            Ok(code) => std::process::exit(code),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(2);
            }
        },
        Command::Render {
            path,
            out,
            pattern,
            loops,
            bits,
            sample_rate,
        } => {
            let file = project_file(path)?;
            let dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
            let folder = Folder::on_disk(&dir);
            let project = load_playable(&file)?;
            if ![16, 24, 32].contains(&bits) {
                bail!("--bits must be 16, 24 or 32");
            }
            let scope = match &pattern {
                Some(id) => {
                    if project.pattern(id).is_none() {
                        bail!("no pattern with id {id:?}");
                    }
                    RenderScope::Pattern {
                        id: id.clone(),
                        loops,
                    }
                }
                None => RenderScope::Song,
            };
            let out = out.unwrap_or_else(|| {
                let name = pattern.clone().unwrap_or_else(|| slug(&project.meta.title));
                dir.join(folder::RENDERS_DIR).join(format!("{name}.wav"))
            });
            let t0 = std::time::Instant::now();
            let mut bar = progress::Bar::new();
            let (audio, warnings) = render_project_with(
                &folder,
                project,
                &scope,
                sample_rate as f32,
                &server::fonts(),
                server::install_plugin_host,
                |p| bar.render(p),
            );
            bar.clear();
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
        Command::Note {
            project,
            channel,
            instrument,
            pitch,
            velocity,
            seconds,
            out,
            sample_rate,
        } => {
            let device: Device = match (channel, instrument) {
                (Some(id), None) => {
                    let p = load_project(&project_file(project)?)?;
                    p.channel(&id)
                        .ok_or_else(|| anyhow!("no channel with id {id:?}"))?
                        .instrument
                        .clone()
                }
                (None, Some(json)) => {
                    serde_json::from_str(&json).context("parsing --instrument")?
                }
                _ => bail!("pass exactly one of --channel or --instrument"),
            };
            let fonts = server::fonts();
            let audio = render::render_note_with(
                &device,
                pitch,
                velocity,
                seconds,
                sample_rate as f32,
                |e| {
                    for w in fonts.provide(e) {
                        eprintln!("warning: {w}");
                    }
                },
            );
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
        Command::Grooves => {
            print!("{}", rosaclef_core::drums::grooves_text());
            Ok(())
        }
        Command::Drums {
            path,
            groove,
            kit,
            guess,
            reset_edits,
            pattern,
        } => {
            use rosaclef_core::drums;
            let file = project_file(path)?;
            let mut p = load_project(&file)?;
            if let Some(id) = pattern {
                drums::render_pattern(&mut p, &id).map_err(anyhow::Error::msg)?;
                let checked = validate::validate(&p);
                if let Some(e) = checked
                    .iter()
                    .find(|i| i.severity == validate::Severity::Error)
                {
                    bail!("the made pattern is invalid: {e}");
                }
                std::fs::write(&file, format::to_string(&p))?;
                let pat = p.pattern(&id).expect("made pattern");
                println!(
                    "made {} ({} notes, {} beats)",
                    pat.name,
                    pat.notes.len(),
                    format::format_f64(pat.length)
                );
                return Ok(());
            }
            let Some(first) = groove
                .clone()
                .or_else(|| p.drums.as_ref().map(|d| d.groove.clone()))
            else {
                bail!("the project has no drum part: pass --groove (see `rosaclef grooves`)");
            };
            let part = p.drums.get_or_insert_with(|| drums::DrumPart::new(&first));
            if let Some(g) = groove {
                part.groove = g;
            }
            if let Some(k) = kit {
                part.kit = k;
            }
            if guess || part.sections.is_empty() {
                let (start, sections) = drums::guess_sections(&p);
                let part = p.drums.as_mut().expect("drum part");
                part.start = start;
                part.sections = sections;
            }
            if reset_edits {
                drums::reset_edits(&mut p);
            }
            let report = drums::write(&mut p).map_err(anyhow::Error::msg)?;
            let checked = validate::validate(&p);
            if let Some(e) = checked
                .iter()
                .find(|i| i.severity == validate::Severity::Error)
            {
                bail!("the written project is invalid: {e}");
            }
            std::fs::write(&file, format::to_string(&p))?;
            println!(
                "wrote {} drum patterns in {} clips on track {} ({}){}{}",
                report.patterns,
                report.clips,
                report.track + 1,
                p.playlist.tracks[report.track].name,
                if report.kept > 0 {
                    format!(
                        "; kept {} edited by hand (--reset-edits forgets them)",
                        report.kept
                    )
                } else {
                    String::new()
                },
                if report.left > 0 {
                    format!(
                        "; left {} bar{} to the song's own drum patterns",
                        report.left,
                        if report.left == 1 { "" } else { "s" }
                    )
                } else {
                    String::new()
                }
            );
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
            let f = Folder::on_disk(dir.unwrap_or_else(|| PathBuf::from(".")));
            guide::write(&f, &exe())?;
            println!("wrote agent guides in {}", f.dir.display());
            Ok(())
        }
        Command::ImportLmms(a) => import_cli(a, |bytes, name, file| {
            let mut opts = rosaclef_import::lmms::Options::new(name);
            opts.source_dir = file.parent().map(|p| p.to_path_buf());
            rosaclef_import::lmms::import(bytes, &opts)
        }),
        Command::ImportMidi(a) => {
            let soundfont = !a.synth;
            import_cli(a, move |bytes, name, _| {
                let opts = rosaclef_import::midi::Options {
                    soundfont,
                    ..rosaclef_import::midi::Options::new(name)
                };
                rosaclef_import::midi::import(bytes, &opts)
            })
        }
    }
}

/// The Critic's audio findings for a song: a whole-song mix check (cached),
/// with the project's suppressed findings kept (marked as such).
fn audio_findings(
    file: &Path,
    p: &rosaclef_core::Project,
) -> Result<Vec<rosaclef_core::critic::Finding>> {
    use rosaclef_studio::mixcheck::{self, Env, Options};
    let folder = Folder::on_disk(file.parent().unwrap_or(Path::new(".")));
    let fonts = server::fonts();
    let env = Env {
        folder: &folder,
        fonts: &fonts,
        setup: &server::install_plugin_host,
        progress: &|_| {},
        disk_cache: true,
        any_file: true,
    };
    let mut all = p.clone();
    all.critic.suppress.clear();
    let o = Options {
        max_findings: 50,
        ..Options::default()
    };
    let r = mixcheck::run(&env, &all, &o).map_err(|e| anyhow!(e.0))?;
    Ok(mixcheck::critic::findings(&r, p))
}

/// The request a mixcheck command line makes (the same JSON the HTTP
/// endpoint and the studio's panel send).
fn mixcheck_request(a: &MixcheckArgs) -> Result<serde_json::Value> {
    use serde_json::json;
    let mut req = json!({
        "by": a.by,
        "threshold": a.threshold,
        "maxFindings": a.max_findings,
        "verify": a.verify,
        "history": a.history,
        "sampleRate": a.sample_rate,
        "cache": !a.no_cache,
    });
    let m = req.as_object_mut().expect("object");
    for (k, v) in [
        ("range", &a.range),
        ("beats", &a.beats),
        ("section", &a.section),
        ("focus", &a.focus),
        ("checks", &a.checks),
        ("target", &a.target),
        ("reference", &a.reference),
    ] {
        if let Some(v) = v {
            m.insert(k.into(), json!(v));
        }
    }
    if let Some(p) = a.preroll {
        m.insert("prerollBeats".into(), json!(p));
    }
    if let Some(w) = &a.what_if {
        let ops = rosaclef_studio::mixcheck::patch::parse_ops(w, |f| {
            std::fs::read_to_string(f).map_err(|e| format!("what-if: reading {f}: {e}"))
        })
        .map_err(anyhow::Error::msg)?;
        m.insert("whatIf".into(), json!(ops));
    }
    Ok(req)
}

/// `rosaclef mixcheck`: the exit status (0 no warnings, 1 warnings).
fn mixcheck_cli(a: MixcheckArgs) -> Result<i32> {
    use rosaclef_studio::mixcheck::{self, Env, Options};
    if a.schema {
        print!("{}", mixcheck::SCHEMA);
        return Ok(0);
    }
    let o = Options::from_json(&mixcheck_request(&a)?).map_err(anyhow::Error::msg)?;
    let first = match a.compare.first() {
        Some(p) => p.clone(),
        None => a.path.clone().unwrap_or_else(|| PathBuf::from(".")),
    };
    let file = project_file(Some(first))?;
    let folder = Folder::on_disk(file.parent().unwrap_or(Path::new(".")));
    let fonts = server::fonts();
    let tty = std::io::IsTerminal::is_terminal(&std::io::stderr());
    let last = std::sync::Mutex::new(-1i64);
    let progress = |x: f64| {
        let pct = (x * 100.0) as i64;
        let mut l = last.lock().unwrap_or_else(|e| e.into_inner());
        if tty && pct != *l {
            *l = pct;
            eprint!("\rmixcheck: rendering {pct:>3}%");
        }
    };
    let env = Env {
        folder: &folder,
        fonts: &fonts,
        setup: &server::install_plugin_host,
        progress: &progress,
        disk_cache: true,
        any_file: true,
    };
    let load = |f: &Path| -> Result<rosaclef_core::Project> {
        let text =
            std::fs::read_to_string(f).with_context(|| format!("reading {}", f.display()))?;
        rosaclef_core::compat::for_playback(&text)
            .map(|p| p.project)
            .map_err(|checked| {
                let msgs: Vec<String> = checked.errors().map(|i| i.to_string()).collect();
                anyhow!("{} is invalid:\n{}", f.display(), msgs.join("\n"))
            })
    };
    let result = if a.compare.len() == 2 {
        let fb = project_file(Some(a.compare[1].clone()))?;
        let (pa, pb) = (load(&file)?, load(&fb)?);
        let la = file.display().to_string();
        let lb = fb.display().to_string();
        // B plays its own folder's samples (and caches there).
        let folder_b = Folder::on_disk(fb.parent().unwrap_or(Path::new(".")));
        let env_b = Env {
            folder: &folder_b,
            ..env
        };
        mixcheck::compare((&env, &pa), (&env_b, &pb), (&la, &lb), &o)
    } else {
        mixcheck::run(&env, &load(&file)?, &o)
    };
    if tty && *last.lock().unwrap_or_else(|e| e.into_inner()) >= 0 {
        eprint!("\r\x1b[K");
    }
    let r = result.map_err(|e| anyhow!(e.0))?;
    if a.text && !a.json {
        print!("{}", mixcheck::text::summary(&r));
    } else {
        println!("{}", serde_json::to_string_pretty(&r)?);
    }
    Ok(if mixcheck::has_warnings(&r) { 1 } else { 0 })
}

/// Shared driver of `import-lmms` / `import-midi`.
fn import_cli(
    a: ImportArgs,
    run: impl FnOnce(&[u8], &str, &Path) -> Result<rosaclef_import::Imported>,
) -> Result<()> {
    let bytes = std::fs::read(&a.file).with_context(|| format!("reading {}", a.file.display()))?;
    let stem = a
        .file
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Imported".into());
    let name = rosaclef_studio::library::sanitize_name(a.name.as_deref().unwrap_or(&stem));
    let file = a.file.canonicalize().unwrap_or(a.file.clone());
    let imported = run(&bytes, &name, &file)?;
    let library = a.library.unwrap_or_else(|| PathBuf::from("."));
    let (name, warnings) =
        Library::new(rosaclef_fs::disk(), &library, &exe()).save_imported(&name, &imported)?;
    for w in &warnings {
        eprintln!("warning: {w}");
    }
    let p = &imported.project;
    let notes: usize = p.patterns.iter().map(|x| x.notes.len()).sum();
    let dir = library.join(&name);
    println!(
        "Imported {} → {} — {} BPM, {} channels, {} patterns ({notes} notes), {} clips, {} samples copied, {} warning(s)",
        a.file.display(),
        dir.display(),
        rosaclef_core::format::format_f64(p.transport.bpm),
        p.channels.len(),
        p.patterns.len(),
        p.playlist.clips.len(),
        imported.samples.len(),
        warnings.len()
    );
    println!("Open it with: rosaclef serve {}", dir.display());
    Ok(())
}

/// How agents reach this executable (written into the agent guides).
pub fn exe() -> String {
    std::env::current_exe()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "rosaclef".into())
}

fn project_file(path: Option<PathBuf>) -> Result<PathBuf> {
    let p = path.unwrap_or_else(|| PathBuf::from("."));
    let file = if p.is_dir() { p.join(PROJECT_FILE) } else { p };
    if !file.exists() {
        bail!(
            "{} not found (pass a project folder or project.json)",
            file.display()
        );
    }
    Ok(file)
}

/// Load a project to play it (forward-compatibly: what this version does
/// not know is left out or played on a stand-in, with a warning for each).
fn load_playable(file: &Path) -> Result<rosaclef_core::Project> {
    let text =
        std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    match rosaclef_core::compat::for_playback(&text) {
        Ok(p) => {
            for f in &p.fallbacks {
                eprintln!("warning: {f}");
            }
            Ok(p.project)
        }
        Err(checked) => {
            let msgs: Vec<String> = checked.issues.iter().map(|i| i.to_string()).collect();
            bail!("{} is invalid:\n{}", file.display(), msgs.join("\n"));
        }
    }
}

fn load_project(file: &Path) -> Result<rosaclef_core::Project> {
    let text =
        std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let checked = validate::parse_and_validate(&text);
    if !checked.is_ok() {
        let msgs: Vec<String> = checked.issues.iter().map(|i| i.to_string()).collect();
        bail!("{} is invalid:\n{}", file.display(), msgs.join("\n"));
    }
    Ok(checked.project.unwrap())
}

fn serve(a: ServeArgs) -> Result<()> {
    let dir = a.dir.unwrap_or_else(|| PathBuf::from("."));
    let folder = Folder::on_disk(&dir);
    let fresh = !folder.project_path().exists();
    folder.init(a.demo || fresh && is_empty_dir(&folder.dir))?;
    let folder = Folder::on_disk(&dir);
    guide::write(&folder, &exe())?;
    let web = a.web.unwrap_or_else(default_web_dir);
    server::init_fonts(&web);
    let library = match a.library {
        Some(l) => {
            std::fs::create_dir_all(&l)
                .with_context(|| format!("creating the library {}", l.display()))?;
            l.canonicalize()?
        }
        None => folder
            .dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| folder.dir.clone()),
    };
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(server::run(server::Config {
        folder,
        library,
        host: a.host,
        port: a.port,
        web,
    }))
}

fn is_empty_dir(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten().all(|e| {
                let n = e.file_name();
                let n = n.to_string_lossy();
                n.starts_with('.')
                    || [folder::SAMPLES_DIR, folder::RENDERS_DIR].contains(&n.as_ref())
            })
        })
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
