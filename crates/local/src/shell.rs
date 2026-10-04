//! The terminal of the browser build: a small `rosaclef` shell.
//!
//! The native studio runs the producer's coding agent in a real terminal.
//! In the browser there are no processes, so the agent panel runs this shell
//! instead: the `rosaclef` command line (validate, summary, render, note,
//! catalog, presets, schema — the commands AGENTS.md documents) plus a few
//! commands to look at and edit the song directly (`get`, `set`, `del`),
//! browse the project's files and switch projects. Edits land in the studio
//! live, like an agent's, and Ctrl+Z undoes them.

use crate::host::Host;
use anyhow::{anyhow, bail, Result};
use rosaclef_core::{validate, Device};
use rosaclef_engine::render::{encode_wav, render_note_with};
use rosaclef_studio::library;
use rosaclef_studio::render::levels_db;
use serde_json::{json, Value};

const GOLD: &str = "\x1b[38;2;227;196;122m";
const DIM: &str = "\x1b[38;2;163;151;128m";
const RED: &str = "\x1b[38;2;217;112;126m";
const GREEN: &str = "\x1b[38;2;143;191;154m";
const RESET: &str = "\x1b[0m";

const HELP: &str = "\
The rosaclef shell — the studio's command line, running in your browser.

Song
  summary                     overview of channels, patterns, arrangement, mixer
  validate                    check project.json (errors name the JSON path)
  get [/json/pointer]         print (part of) the song, e.g. get /transport/bpm
  set /json/pointer VALUE     change the song, e.g. set /transport/bpm 128
                              (VALUE is JSON; bare words are strings)
  del /json/pointer           remove a key or an array element
  fmt                         rewrite project.json in canonical form
  context                     what you are looking at in the studio
  critic [all]                lint the song: what to fix, and fixes (all: with
                              the suppressed findings too)
  critic fix KEY|RULE|all     apply fixes
  critic suppress KEY|RULE    suppress a finding, or turn a check off
  critic unsuppress KEY|RULE  bring it back       critic rules   list the checks

Sound
  render [--pattern ID] [--loops N] [--bits 16|24|32] [--out renders/x.wav]
                              offline mixdown to WAV (then: Projects → Files)
  note (--channel ID | --instrument JSON) [--pitch 60] [--velocity 0.9]
       [--seconds 2] --out samples/x.wav
                              synthesize one note into a sample

Reference
  catalog                     every instrument and effect, with parameters
  presets [TYPE|NAME]         factory presets; a name prints its JSON
  schema                      the JSON schema of project.json

Files and projects
  ls [DIR]    cat FILE    rm FILE (to .trash/)
  projects    open NAME

  clear       help        exit

`rosaclef <command>` works too, as in AGENTS.md. Up/Down browse the history.";

const COMMANDS: &[&str] = &[
    "help", "summary", "validate", "get", "set", "del", "fmt", "context", "critic", "render",
    "note", "catalog", "presets", "schema", "ls", "cat", "rm", "projects", "open", "clear", "exit",
];

/// Line editing state of one terminal.
#[derive(Clone, Debug, Default)]
pub struct Shell {
    line: Vec<char>,
    cursor: usize,
    history: Vec<String>,
    /// Position while browsing the history (`history.len()`: the new line).
    hist: usize,
    /// An escape sequence split across inputs.
    esc: String,
}

/// Terminal text: `\n` → `\r\n`.
fn crlf(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\n', "\r\n")
}

/// Split a command line into words (quotes and backslashes as in a shell).
pub fn words(line: &str) -> Result<Vec<String>> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut has = false;
    let mut chars = line.chars();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => cur.push(chars.next().unwrap_or('\\')),
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                has = true;
            }
            (None, '\\') => {
                cur.push(chars.next().unwrap_or('\\'));
                has = true;
            }
            (None, c) if c.is_whitespace() => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                has = false;
            }
            (None, c) => cur.push(c),
        }
    }
    if quote.is_some() {
        bail!("unterminated quote");
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

/// `--flag value` options and positional arguments.
struct Args {
    pos: Vec<String>,
    opts: Vec<(String, String)>,
}

impl Args {
    fn parse(words: &[String]) -> Result<Args> {
        let mut pos = vec![];
        let mut opts = vec![];
        let mut it = words.iter();
        while let Some(w) = it.next() {
            if let Some(name) = w.strip_prefix("--") {
                let (k, v) = match name.split_once('=') {
                    Some((k, v)) => (k.to_string(), v.to_string()),
                    None => (
                        name.to_string(),
                        it.next()
                            .cloned()
                            .ok_or_else(|| anyhow!("--{name} needs a value"))?,
                    ),
                };
                opts.push((k.replace('_', "-"), v));
            } else {
                pos.push(w.clone());
            }
        }
        Ok(Args { pos, opts })
    }

    fn opt(&self, k: &str) -> Option<&str> {
        self.opts
            .iter()
            .rev()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }

    fn num<T: std::str::FromStr>(&self, k: &str, default: T) -> Result<T> {
        match self.opt(k) {
            Some(v) => v.parse().map_err(|_| anyhow!("--{k}: not a number: {v}")),
            None => Ok(default),
        }
    }
}

/// Parse a JSON pointer's parent and last segment (`/a/b/0` → (`/a/b`, `0`)).
fn split_pointer(p: &str) -> Result<(String, String)> {
    if !p.starts_with('/') || p.len() < 2 {
        bail!("a JSON pointer starts with / (e.g. /transport/bpm)");
    }
    let i = p.rfind('/').unwrap_or(0);
    Ok((
        p[..i].to_string(),
        p[i + 1..].replace("~1", "/").replace("~0", "~"),
    ))
}

/// A command-line value: JSON when it parses, else a string.
fn value_of(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.to_string()))
}

impl Host {
    fn prompt(&self) -> String {
        format!(
            "{GOLD}rosaclef{RESET} {DIM}{}{RESET} ❯ ",
            self.folder.name()
        )
    }

    fn status(&mut self, client: u64, running: bool, exit: Option<i32>) {
        self.term_json(client, json!({"t": "status", "running": running, "agent": if running { "shell" } else { "" }, "name": "Rosaclef shell", "exitCode": exit}));
    }

    pub fn term_open(&mut self, client: u64) {
        let running = self.shells.contains_key(&client);
        self.status(client, running, None);
    }

    /// A message from the agent panel (`start`, `input`, `resize`, `stop`).
    pub fn term_message(&mut self, client: u64, text: &str) {
        let Ok(v) = serde_json::from_str::<Value>(text) else {
            return;
        };
        match v.get("t").and_then(|t| t.as_str()).unwrap_or("") {
            "start" => {
                if v.get("agent").and_then(|a| a.as_str()) != Some("shell") {
                    self.term_json(client, json!({"t": "error", "message": "Only the Rosaclef shell runs in the browser. Coding agents need the native studio (cargo run -- serve)."}));
                    return;
                }
                self.shells.insert(client, Shell::default());
                self.status(client, true, None);
                let banner = format!(
                    "{GOLD}✦ Rosaclef shell{RESET} {DIM}— the studio's command line, in your browser.{RESET}\n\
                     {DIM}Type {RESET}help{DIM} for the commands, e.g. {RESET}summary{DIM}, {RESET}set /transport/bpm 128{DIM}, {RESET}render{DIM}.{RESET}\n\
                     {DIM}Coding agents (Claude Code, Codex, ...) run in the native studio: see AGENTS.md.{RESET}\n\n"
                );
                let p = self.prompt();
                self.term_out(client, crlf(&banner) + &p);
            }
            "input" => {
                let data = v
                    .get("data")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                self.term_input(client, &data);
            }
            "stop" => {
                if self.shells.remove(&client).is_some() {
                    self.term_out(client, "\r\n".into());
                    self.status(client, false, Some(0));
                }
            }
            _ => {}
        }
    }

    fn term_input(&mut self, client: u64, data: &str) {
        let Some(mut sh) = self.shells.remove(&client) else {
            return;
        };
        let mut echo = String::new();
        let mut input: Vec<char> = std::mem::take(&mut sh.esc)
            .chars()
            .chain(data.chars())
            .collect();
        let mut i = 0;
        while i < input.len() {
            let c = input[i];
            i += 1;
            match c {
                '\x1b' => {
                    // CSI / SS3 sequences: arrows, home/end, delete.
                    let rest = &input[i..];
                    if rest.is_empty() {
                        sh.esc = "\x1b".into();
                        continue;
                    }
                    if rest[0] != '[' && rest[0] != 'O' {
                        continue; // Alt+key: ignored
                    }
                    let Some(k) = rest[1..].iter().position(|c| ('@'..='~').contains(c)) else {
                        // Incomplete: wait for the rest.
                        sh.esc = std::iter::once('\x1b')
                            .chain(rest.iter().copied())
                            .collect();
                        i = input.len();
                        continue;
                    };
                    let seq: String = rest[..k + 2].iter().collect();
                    i += k + 2;
                    match seq.as_str() {
                        "[A" | "OA" => history(&mut sh, -1, &mut echo),
                        "[B" | "OB" => history(&mut sh, 1, &mut echo),
                        "[C" | "OC" if sh.cursor < sh.line.len() => {
                            sh.cursor += 1;
                            echo.push_str("\x1b[C");
                        }
                        "[D" | "OD" if sh.cursor > 0 => {
                            sh.cursor -= 1;
                            echo.push_str("\x1b[D");
                        }
                        "[H" | "OH" | "[1~" => {
                            if sh.cursor > 0 {
                                echo.push_str(&format!("\x1b[{}D", sh.cursor));
                            }
                            sh.cursor = 0;
                        }
                        "[F" | "OF" | "[4~" => {
                            if sh.line.len() > sh.cursor {
                                echo.push_str(&format!("\x1b[{}C", sh.line.len() - sh.cursor));
                            }
                            sh.cursor = sh.line.len();
                        }
                        "[3~" if sh.cursor < sh.line.len() => {
                            sh.line.remove(sh.cursor);
                            redraw_tail(&sh, &mut echo, 1);
                        }
                        _ => {}
                    }
                }
                '\r' | '\n' => {
                    if c == '\n' && i >= 2 && input[i - 2] == '\r' {
                        continue;
                    }
                    let line: String = sh.line.iter().collect();
                    sh.line.clear();
                    sh.cursor = 0;
                    echo.push_str("\r\n");
                    let trimmed = line.trim().to_string();
                    if !trimmed.is_empty() && sh.history.last() != Some(&trimmed) {
                        sh.history.push(trimmed.clone());
                    }
                    sh.hist = sh.history.len();
                    self.term_out(client, std::mem::take(&mut echo));
                    let (output, exit) = self.run(client, &trimmed);
                    if !output.is_empty() {
                        let mut o = crlf(&output);
                        if !o.ends_with('\n') {
                            o.push_str("\r\n");
                        }
                        self.term_out(client, o);
                    }
                    if exit {
                        self.status(client, false, Some(0));
                        return;
                    }
                    echo.push_str(&self.prompt());
                }
                '\x7f' | '\x08' => {
                    if sh.cursor > 0 {
                        sh.cursor -= 1;
                        sh.line.remove(sh.cursor);
                        echo.push('\x08');
                        redraw_tail(&sh, &mut echo, 1);
                    }
                }
                '\x03' => {
                    sh.line.clear();
                    sh.cursor = 0;
                    sh.hist = sh.history.len();
                    echo.push_str("^C\r\n");
                    echo.push_str(&self.prompt());
                }
                '\x15' => {
                    // Ctrl+U: clear the line.
                    if sh.cursor > 0 {
                        echo.push_str(&format!("\x1b[{}D", sh.cursor));
                    }
                    echo.push_str("\x1b[K");
                    sh.line.clear();
                    sh.cursor = 0;
                }
                '\x0c' => {
                    echo.push_str("\x1b[2J\x1b[H");
                    echo.push_str(&self.prompt());
                    echo.push_str(&sh.line.iter().collect::<String>());
                    sh.cursor = sh.line.len();
                }
                '\t' => {
                    let typed: String = sh.line.iter().collect();
                    if !typed.contains(' ') {
                        let matches: Vec<&str> = COMMANDS
                            .iter()
                            .copied()
                            .filter(|c| c.starts_with(typed.as_str()))
                            .collect();
                        if matches.len() == 1 {
                            let add: String = matches[0][typed.len()..].to_string() + " ";
                            sh.line.extend(add.chars());
                            sh.cursor = sh.line.len();
                            echo.push_str(&add);
                        } else if matches.len() > 1 {
                            echo.push_str(&format!(
                                "\r\n{}\r\n{}{typed}",
                                matches.join("  "),
                                self.prompt()
                            ));
                        }
                    }
                }
                c if !c.is_control() => {
                    sh.line.insert(sh.cursor, c);
                    sh.cursor += 1;
                    echo.push(c);
                    redraw_tail(&sh, &mut echo, 0);
                }
                _ => {}
            }
        }
        input.clear();
        if !echo.is_empty() {
            self.term_out(client, echo);
        }
        self.shells.insert(client, sh);
    }

    /// Run one command line. Returns its output and whether the shell exits.
    fn run(&mut self, client: u64, line: &str) -> (String, bool) {
        let words = match words(line) {
            Ok(w) => w,
            Err(e) => return (format!("{RED}{e}{RESET}"), false),
        };
        let mut w: &[String] = &words;
        if w.first().map(|s| s == "rosaclef").unwrap_or(false) {
            w = &w[1..];
        }
        let Some(cmd) = w.first() else {
            return (String::new(), false);
        };
        if cmd == "exit" || cmd == "quit" {
            return ("bye".into(), true);
        }
        let _ = client;
        match self.command(cmd, &w[1..]) {
            Ok(s) => (s, false),
            Err(e) if e.is::<crate::host::NeedContent>() => (String::new(), false),
            Err(e) => (format!("{RED}{e:#}{RESET}"), false),
        }
    }

    fn command(&mut self, cmd: &str, args: &[String]) -> Result<String> {
        let a = Args::parse(args)?;
        let arg = |i: usize| a.pos.get(i).cloned().unwrap_or_default();
        Ok(match cmd {
            "help" => HELP.to_string(),
            "clear" => "\x1b[2J\x1b[H".into(),
            "summary" => rosaclef_core::summary(self.project()),
            "validate" => {
                let text = self.folder.read_text()?;
                let checked = validate::parse_and_validate(&text);
                let mut out: Vec<String> = checked.issues.iter().map(|i| i.to_string()).collect();
                if checked.is_ok() {
                    let p = checked.project.unwrap();
                    let notes: usize = p.patterns.iter().map(|x| x.notes.len()).sum();
                    out.push(format!(
                        "{GREEN}ok{RESET}: project.json — {} channels, {} patterns ({notes} notes), {} clips, {} inserts",
                        p.channels.len(),
                        p.patterns.len(),
                        p.playlist.clips.len(),
                        p.mixer.inserts.len()
                    ));
                }
                out.join("\n")
            }
            "get" => {
                let v = serde_json::to_value(self.project())?;
                let p = arg(0);
                let at = if p.is_empty() || p == "/" {
                    Some(&v)
                } else {
                    v.pointer(&p)
                };
                let at = at.ok_or_else(|| anyhow!("nothing at {p}"))?;
                serde_json::to_string_pretty(at)?
            }
            "set" | "del" => {
                let p = arg(0);
                let mut v = serde_json::to_value(self.project())?;
                let (parent, last) = split_pointer(&p)?;
                let target = if parent.is_empty() {
                    Some(&mut v)
                } else {
                    v.pointer_mut(&parent)
                };
                let target = target.ok_or_else(|| anyhow!("nothing at {parent}"))?;
                if cmd == "set" {
                    if a.pos.len() < 2 {
                        bail!("usage: set /json/pointer VALUE");
                    }
                    let value = value_of(&a.pos[1..].join(" "));
                    match target {
                        Value::Object(m) => {
                            m.insert(last, value);
                        }
                        Value::Array(items) if last == "-" => items.push(value),
                        Value::Array(items) => {
                            let i: usize = last
                                .parse()
                                .map_err(|_| anyhow!("{last}: not an array index"))?;
                            *items
                                .get_mut(i)
                                .ok_or_else(|| anyhow!("{p}: index out of range"))? = value;
                        }
                        _ => bail!("{parent} is not an object or an array"),
                    }
                } else {
                    match target {
                        Value::Object(m) => {
                            m.remove(&last).ok_or_else(|| anyhow!("nothing at {p}"))?;
                        }
                        Value::Array(items) => {
                            let i: usize = last
                                .parse()
                                .map_err(|_| anyhow!("{last}: not an array index"))?;
                            if i >= items.len() {
                                bail!("{p}: index out of range");
                            }
                            items.remove(i);
                        }
                        _ => bail!("{parent} is not an object or an array"),
                    }
                }
                let checked = validate::value_and_validate(v);
                if !checked.is_ok() {
                    let msgs: Vec<String> = checked
                        .issues
                        .iter()
                        .take(5)
                        .map(|i| i.to_string())
                        .collect();
                    bail!("the change was not applied:\n{}", msgs.join("\n"));
                }
                let rev = self.apply(checked.project.unwrap(), checked.issues, "disk", 0)?;
                format!("{GREEN}ok{RESET} {DIM}(rev {rev} — Ctrl+Z in the studio undoes it){RESET}")
            }
            "critic" => {
                use rosaclef_core::critic;
                let mut p = self.project().clone();
                let what = arg(1);
                let done = match arg(0).as_str() {
                    "" | "all" => return Ok(critic::report_text(&critic::critique(&p, &[]), arg(0) == "all")),
                    "rules" => return Ok(critic::rules_text(&p)),
                    "fix" => {
                        let (fixed, applied) = critic::apply_fixes(&p, std::slice::from_ref(&what), &[]).map_err(|e| anyhow!(e))?;
                        if applied.is_empty() {
                            bail!("nothing to fix for {what:?}");
                        }
                        p = fixed;
                        applied.iter().map(|d| format!("fixed: {d}")).collect::<Vec<_>>().join("\n")
                    }
                    "suppress" => critic::suppress(&mut p, &what, &[]).map_err(|e| anyhow!(e))?,
                    "unsuppress" => critic::unsuppress(&mut p, &what).map_err(|e| anyhow!(e))?,
                    other => bail!("critic {other}: use critic, critic all, critic rules, critic fix|suppress|unsuppress KEY|RULE"),
                };
                let issues = validate::validate(&p);
                if let Some(e) = issues
                    .iter()
                    .find(|i| i.severity == validate::Severity::Error)
                {
                    bail!("the change was not applied: {e}");
                }
                let rev = self.apply(p, issues, "disk", 0)?;
                format!("{done}\n{GREEN}ok{RESET} {DIM}(rev {rev} — Ctrl+Z in the studio undoes it){RESET}")
            }
            "fmt" => {
                let p = self.project().clone();
                self.folder.write_project(&p)?;
                "formatted project.json".into()
            }
            "context" => {
                if self.context.is_null() {
                    "(nothing yet — select something in the studio)".into()
                } else {
                    serde_json::to_string_pretty(&self.context)?
                }
            }
            "render" => {
                let out = a.opt("out").map(|s| s.to_string());
                let r = self.render(
                    a.opt("pattern").unwrap_or(""),
                    a.num("loops", 1)?,
                    a.num("bits", 24)?,
                    a.num("sample-rate", 48000)?,
                    out.as_deref(),
                )?;
                let mut s = format!(
                    "rendered {} ({:.1}s, peak {:.1} dBFS, rms {:.1} dBFS)",
                    r["path"].as_str().unwrap_or(""),
                    r["duration"].as_f64().unwrap_or(0.0),
                    r["peakDb"].as_f64().unwrap_or(0.0),
                    r["rmsDb"].as_f64().unwrap_or(0.0)
                );
                for w in r["warnings"].as_array().into_iter().flatten() {
                    s.push_str(&format!(
                        "\n{RED}warning{RESET}: {}",
                        w.as_str().unwrap_or("")
                    ));
                }
                s
            }
            "note" => {
                let device: Device = match (a.opt("channel"), a.opt("instrument")) {
                    (Some(id), None) => self
                        .project()
                        .channel(id)
                        .ok_or_else(|| anyhow!("no channel with id {id:?}"))?
                        .instrument
                        .clone(),
                    (None, Some(j)) => {
                        serde_json::from_str(j).map_err(|e| anyhow!("--instrument: {e}"))?
                    }
                    _ => bail!("pass exactly one of --channel or --instrument"),
                };
                let out = a
                    .opt("out")
                    .ok_or_else(|| anyhow!("--out samples/NAME.wav is required"))?
                    .to_string();
                let seconds: f32 = a.num("seconds", 2.0)?;
                if device.kind == "soundfont" {
                    let key = rosaclef_engine::instruments::SoundFontInst::preset_key(&device);
                    self.fonts.ensure(&[key])?;
                }
                let fonts = self.fonts.clone();
                let audio = render_note_with(
                    &device,
                    a.num::<u8>("pitch", 60)?.min(127),
                    a.num("velocity", 0.9)?,
                    seconds.clamp(0.05, 60.0),
                    a.num::<u32>("sample-rate", 48000)? as f32,
                    |e| {
                        fonts.provide(e);
                    },
                );
                self.folder.write(&out, &encode_wav(&audio, 24))?;
                let (peak, _) = levels_db(&audio);
                format!("wrote {out} ({seconds}s, peak {peak:.1} dBFS)")
            }
            "catalog" => rosaclef_core::catalog_markdown(),
            "schema" => rosaclef_core::schema::schema_text(),
            "presets" => {
                let filter = a.pos.first().cloned();
                if let Some(p) = filter.as_deref().and_then(rosaclef_core::presets::find) {
                    return Ok(serde_json::to_string_pretty(&p.device())?);
                }
                let mut out = vec![];
                for p in rosaclef_core::presets::all() {
                    if filter.as_deref().map(|f| f == p.kind).unwrap_or(true) {
                        out.push(format!(
                            "{:<10} {:<26} {:<24} {}",
                            p.kind, p.name, p.tags, p.doc
                        ));
                    }
                }
                out.join("\n")
            }
            "ls" => {
                let dir = arg(0);
                let root = if dir.is_empty() || dir == "." {
                    self.folder.dir.clone()
                } else {
                    self.folder
                        .resolve(dir.trim_end_matches('/'))
                        .ok_or_else(|| anyhow!("invalid path {dir:?}"))?
                };
                let fs = self.folder.fs.clone();
                let mut out = vec![];
                for p in fs.read_dir(&root)? {
                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let m = fs.metadata(&p)?;
                    if m.is_dir {
                        out.push(format!("{GOLD}{name}/{RESET}"));
                    } else {
                        out.push(format!("{name}  {DIM}{}{RESET}", human(m.len)));
                    }
                }
                out.join("\n")
            }
            "cat" => {
                let p = self
                    .folder
                    .resolve(&arg(0))
                    .ok_or_else(|| anyhow!("usage: cat FILE"))?;
                if rosaclef_studio::decode::is_audio_file(&p) {
                    bail!("{} is audio — listen to it in Projects → Files", arg(0));
                }
                self.folder.fs.read_to_string(&p)?
            }
            "rm" => format!("moved to {}", library::trash_path(&self.folder, &arg(0))?),
            "projects" => {
                let list = self.library.list(&self.folder.dir);
                list.iter()
                    .map(|p| {
                        format!(
                            "{} {:<28} {DIM}{} · {} BPM{}{RESET}",
                            if p.current { "▶" } else { " " },
                            p.name,
                            p.title,
                            p.bpm,
                            if p.invalid { " · needs repair" } else { "" }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            "open" => {
                let name = a.pos.join(" ");
                if self.switch_to(&name)? {
                    format!("opened {name}")
                } else {
                    format!("{name} is already open")
                }
            }
            other => bail!("{other}: unknown command (type help)"),
        })
    }
}

fn human(n: u64) -> String {
    match n {
        n if n < 1024 => format!("{n} B"),
        n if n < 1 << 20 => format!("{:.0} KB", n as f64 / 1024.0),
        n => format!("{:.1} MB", n as f64 / (1 << 20) as f64),
    }
}

/// Redraw the line from the cursor to the end (after an insert or a delete).
fn redraw_tail(sh: &Shell, echo: &mut String, erased: usize) {
    let tail: String = sh.line[sh.cursor..].iter().collect();
    if tail.is_empty() && erased == 0 {
        return;
    }
    echo.push_str(&tail);
    echo.push_str(&" ".repeat(erased));
    let back = tail.chars().count() + erased;
    if back > 0 {
        echo.push_str(&format!("\x1b[{back}D"));
    }
}

fn history(sh: &mut Shell, dir: i32, echo: &mut String) {
    if sh.history.is_empty() {
        return;
    }
    let next = (sh.hist as i64 + dir as i64).clamp(0, sh.history.len() as i64) as usize;
    if next == sh.hist {
        return;
    }
    sh.hist = next;
    let text = sh.history.get(next).cloned().unwrap_or_default();
    if sh.cursor > 0 {
        echo.push_str(&format!("\x1b[{}D", sh.cursor));
    }
    echo.push_str("\x1b[K");
    echo.push_str(&text);
    sh.line = text.chars().collect();
    sh.cursor = sh.line.len();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitting() {
        assert_eq!(
            words("set /meta/title 'My song'").unwrap(),
            vec!["set", "/meta/title", "My song"]
        );
        assert_eq!(
            words(r#"note --instrument "{\"type\":\"drum\"}" x\ y"#).unwrap(),
            vec!["note", "--instrument", r#"{"type":"drum"}"#, "x y"]
        );
        assert_eq!(words("a ''").unwrap(), vec!["a", ""]);
        assert!(words("a 'b").is_err());
        assert_eq!(
            split_pointer("/a/b~1c").unwrap(),
            ("/a".to_string(), "b/c".to_string())
        );
        assert_eq!(value_of("128"), json!(128));
        assert_eq!(value_of("hello"), json!("hello"));
    }
}
