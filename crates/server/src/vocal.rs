//! `rosaclef export` and `rosaclef align`: the song in vocal, notation and
//! karaoke formats, and a lyric line's phoneme timing from an aligned
//! recording.

use crate::{folder, load_project, project_file};
use anyhow::{anyhow, bail, Context, Result};
use rosaclef_core::{format, validate};
use rosaclef_studio::export::export;
use rosaclef_vocal::align::{self, Target};
use rosaclef_vocal::FORMATS;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(clap::Args)]
pub struct ExportArgs {
    /// Project folder or project.json (default: current folder).
    path: Option<PathBuf>,
    /// The format's id (see --list).
    #[arg(short, long)]
    format: Option<String>,
    /// List the formats.
    #[arg(long)]
    list: bool,
    /// Export this pattern, played once, instead of the song.
    #[arg(long)]
    pattern: Option<String>,
    /// The verse the pattern sings.
    #[arg(long, default_value_t = 1)]
    verse: u32,
    /// Output file, `-` for standard output (default: renders/ in the project).
    #[arg(short, long)]
    out: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct AlignArgs {
    /// The project folder or project.json (default: the current folder),
    /// then the aligner's output: a Praat .TextGrid (MFA, SOFA) or a
    /// WhisperX .json.
    #[arg(value_name = "[DIR] FILE", required = true, num_args = 1..=2)]
    paths: Vec<PathBuf>,
    /// The pattern the recording sings.
    #[arg(long)]
    pattern: String,
    /// The channel of its lyric line.
    #[arg(long)]
    channel: String,
    /// The verse sung.
    #[arg(long, default_value_t = 1)]
    verse: u32,
    /// Seconds into the recording where the pattern starts.
    #[arg(long)]
    at: f64,
}

pub fn export_cli(a: ExportArgs) -> Result<()> {
    if a.list {
        for f in FORMATS {
            println!("{:<10} .{:<9} {}", f.id, f.extension, f.description);
        }
        return Ok(());
    }
    let Some(id) = a.format else {
        bail!("pass --format ID (rosaclef export --list shows them)");
    };
    let file = project_file(a.path)?;
    let project = load_project(&file)?;
    let made = export(&project, &id, a.pattern.as_deref(), a.verse)?;
    let out = a.out.unwrap_or_else(|| {
        let dir = file.parent().unwrap_or(Path::new("."));
        dir.join(folder::RENDERS_DIR).join(&made.name)
    });
    if out == Path::new("-") {
        std::io::stdout().write_all(&made.bytes)?;
        return Ok(());
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out, &made.bytes)?;
    println!("wrote {} ({} bytes)", out.display(), made.bytes.len());
    Ok(())
}

pub fn align_cli(mut a: AlignArgs) -> Result<()> {
    let aligned_file = a.paths.pop().expect("clap requires the file");
    let file = project_file(a.paths.pop())?;
    let project = load_project(&file)?;
    let text = std::fs::read_to_string(&aligned_file)
        .with_context(|| format!("reading {}", aligned_file.display()))?;
    let name = aligned_file.to_string_lossy();
    let aligned = align::read(&name, &text).map_err(|e| anyhow!(e))?;
    let target = Target {
        pattern: a.pattern,
        channel: a.channel,
        verse: a.verse,
        at: a.at,
    };
    let (timed, count) = align::apply(&project, &target, &aligned).map_err(|e| anyhow!(e))?;
    if count == 0 {
        bail!("no aligned word matched the lyrics of {:?}", target.pattern);
    }
    let issues = validate::validate(&timed);
    if issues
        .iter()
        .any(|i| i.severity == validate::Severity::Error)
    {
        let msgs: Vec<String> = issues.iter().take(3).map(|i| i.to_string()).collect();
        bail!("the timed project would be invalid:\n{}", msgs.join("\n"));
    }
    std::fs::write(&file, format::to_string(&timed))?;
    println!(
        "timed {count} syllable(s) of {} (verse {}) from {}; wrote {}",
        target.pattern,
        target.verse,
        aligned_file.display(),
        file.display()
    );
    Ok(())
}
