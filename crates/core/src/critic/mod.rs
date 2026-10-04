//! The Critic: mechanical checks ("lints") of a project against the rules of
//! thumb of composition, arrangement, sound design and mixing.
//!
//! Everything here is pure and deterministic — no AI, no audio analysis: the
//! checks read the project (notes, clips, channels, the mixer) and return
//! [`Finding`]s. A finding with a [`Fix`] is a suggestion: its fix is a list
//! of JSON Patch operations (RFC 6902 `add` / `remove`) on `project.json`,
//! applied by [`apply_fixes`] (the studio, `rosaclef critic --fix`, or an
//! agent by hand). One without is an issue to know about. The rules are
//! listed in [`RULES`]; docs/critic.md catalogues their thresholds.
//!
//! Pitches are MIDI numbers as they sound (C4 = 60): a channel's notes are
//! shifted by the song's and the instrument's transposition before the
//! register checks read them; fixes move the written notes by the same steps.

mod analysis;
mod arrangement;
mod harmony;
mod mixing;
mod rhythm;
#[cfg(test)]
mod tests;

use crate::model::Project;
use analysis::Ana;
use serde::Serialize;
use serde_json::{json, Value};

// ------------------------------------------------------------------ types

/// Where a finding points.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Where {
    /// "pattern" (`id`; `channel` and `notes`, its note indexes, when set),
    /// "channel" (`id`), "insert" (`index`), "song" (`beat`; `index` = a
    /// clip, or -1), "lane" (`index`) or "project".
    pub kind: &'static str,
    pub id: String,
    pub channel: String,
    pub index: i64,
    pub beat: f64,
    pub notes: Vec<usize>,
    /// What to call the place ("Verse · Bass", "Insert 3 · Drums", "Bar 17").
    pub label: String,
}

/// A JSON Patch operation (RFC 6902) on `project.json`.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Op {
    /// "add" (set an object member, append with `-`, or insert into an
    /// array) or "remove".
    pub op: &'static str,
    /// A JSON pointer: `/patterns/2/notes/14/pitch`.
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

/// The one-click change a suggestion offers.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Fix {
    pub label: String,
    pub ops: Vec<Op>,
}

/// A finding of the Critic.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// Stable across unrelated edits: rule and place (for ignoring it, or
    /// for `--fix`).
    pub key: String,
    pub rule: &'static str,
    pub category: &'static str,
    /// "warn" or "info".
    pub level: &'static str,
    pub title: String,
    pub detail: String,
    #[serde(rename = "where")]
    pub at: Where,
    pub fix: Option<Fix>,
    /// Suppressed in the project (`critic.suppress`): kept so it can be
    /// brought back, not reported as something to do.
    pub suppressed: bool,
}

/// A check the Critic runs: its category, its name and why it matters.
#[derive(Serialize, Clone, Copy, Debug)]
pub struct Rule {
    pub id: &'static str,
    pub category: &'static str,
    pub name: &'static str,
    pub why: &'static str,
}

// ------------------------------------------------------------------ rules

pub const CATEGORIES: &[&str] = &[
    "Harmony",
    "Melody",
    "Rhythm",
    "Arrangement",
    "Low end",
    "Mix",
    "Stereo",
    "Effects",
    "Master",
    "Project",
];

const fn r(
    id: &'static str,
    category: &'static str,
    name: &'static str,
    why: &'static str,
) -> Rule {
    Rule {
        id,
        category,
        name,
        why,
    }
}

pub static RULES: &[Rule] = &[
    // Harmony
    r("low-interval", "Harmony", "Low interval limits", "Close intervals voiced low (a third under C3, a second under E3) turn to mud: their partials beat against each other."),
    r("chord-too-low", "Harmony", "Chords crowd the bass", "Chord notes under C3 fight the bass line; voice the harmony above it (or rootless) and leave the low end to the bass."),
    r("voice-leading", "Harmony", "Jumpy voice leading", "Chords whose voices leap instead of moving to the nearest notes sound disjointed; inversions keep the voicing compact."),
    r("parallel-fifths", "Harmony", "Parallel fifths and octaves", "In acoustic parts, voices moving in parallel perfect fifths or octaves fuse into one: great for power chords and stabs, a loss when the voices should stay independent."),
    r("wide-spacing", "Harmony", "Gaps in the upper voices", "Upper voices more than an octave apart leave a hole in the chord; keep adjacent upper voices within an octave."),
    r("out-of-key", "Harmony", "Notes outside the key", "A few notes outside the song's key are often slips of the mouse rather than deliberate chromaticism."),
    r("key-signature", "Harmony", "Key signature disagrees", "The score's key should be the key the notes are in, or every note is spelled with accidentals."),
    r("semitone-clash", "Harmony", "Sustained semitone clashes", "Two parts holding notes a minor second or minor ninth apart grind against each other."),
    // Melody
    r("melody-range", "Melody", "Melody range", "A melody wider than an octave and a half is hard to sing and to follow."),
    r("large-leap", "Melody", "Leaps over an octave", "Leaps wider than an octave break a line into two; few melodies need them."),
    r("leap-recovery", "Melody", "Leaps that do not recover", "A big leap is balanced by a step back the other way (melodic fluency); leaps that keep going sound aimless."),
    r("no-rests", "Melody", "A melody that never breathes", "Rests give the listener time to take in a phrase; a line that never stops tires the ear."),
    r("monotone", "Melody", "Monotone melody", "A lead that keeps to one or two notes for bars on end has no contour to remember."),
    r("instrument-range", "Melody", "Beyond the instrument's range", "A real instrument cannot play these notes: the sample stretches unnaturally and players would refuse the part."),
    // Rhythm
    r("flat-velocity", "Rhythm", "Robotic velocities", "Every note at the same velocity sounds like a machine gun; real players accent the beat and play ghost notes softly."),
    r("max-velocity", "Rhythm", "Everything at full velocity", "Notes all at the top leave no room for accents: velocity is the part's dynamics."),
    r("machine-gun", "Rhythm", "Machine-gun runs", "Fast runs of one drum at one velocity sound mechanical; alternate strong and weak hits."),
    r("no-ghost-notes", "Rhythm", "No ghost notes", "Snares and hats with no soft hits between the backbeats lack groove."),
    r("rigid-timing", "Rhythm", "Live instrument on the grid", "A real instrument played exactly on the grid sounds programmed; a few milliseconds of push and pull bring it to life."),
    r("sloppy-timing", "Rhythm", "Notes just off the grid", "In a part otherwise on the grid, notes a hair off it are usually slips."),
    r("same-pitch-overlap", "Rhythm", "Overlapping notes", "Two notes of one pitch overlapping retrigger or cut each other off."),
    r("duplicate-notes", "Rhythm", "Duplicate notes", "Stacked identical notes double the level and phase against each other."),
    r("silent-notes", "Rhythm", "Silent notes", "Notes at zero velocity make no sound."),
    r("tiny-notes", "Rhythm", "Very short notes", "Pitched notes shorter than a 64th are usually accidental clicks."),
    r("past-end", "Rhythm", "Notes after the pattern ends", "Notes that start after the pattern's end never play."),
    r("swing-unused", "Rhythm", "Swing with nothing to swing", "Swing delays the off-beat 16ths; with no notes there it does nothing."),
    // Arrangement
    r("empty-playlist", "Arrangement", "Nothing on the playlist", "Patterns play in the song only once they are placed on the playlist."),
    r("short-song", "Arrangement", "The song is one loop", "An eight- or sixteen-bar loop is a sketch, not a song: it needs sections."),
    r("loopitis", "Arrangement", "Loopitis", "The same bars repeating unchanged for a long stretch fatigue the ear; change something every 4 to 8 bars."),
    r("no-contrast", "Arrangement", "No contrast between sections", "A chorus feels big because the verse before it was sparse (subtractive arrangement): density should rise and fall."),
    r("too-many-elements", "Arrangement", "Too many elements at once", "Listeners follow about three things at once (rhythm, melody, a wildcard); more competing parts become clutter."),
    r("full-intro", "Arrangement", "Everything enters at once", "An intro that starts with every part has nowhere left to build to."),
    r("abrupt-ending", "Arrangement", "Abrupt ending", "A song that stops at full density sounds cut off; thin it out or fade it."),
    r("clip-off-bar", "Arrangement", "Clips just off the bar", "Sections start on the bar; a clip a little off it is usually a slip of the mouse."),
    r("clip-overlap", "Arrangement", "Overlapping clips", "Clips overlapping on one track play twice over each other."),
    r("unused-pattern", "Arrangement", "Unused patterns", "Patterns that are never placed are not heard."),
    r("empty-clips", "Arrangement", "Clips of empty patterns", "Clips of patterns with no notes play nothing."),
    r("identical-patterns", "Arrangement", "Identical patterns", "Duplicates of a pattern have to be edited twice; use one."),
    r("no-movement", "Arrangement", "No automation", "Filter sweeps, volume rides and risers carry the energy from one section into the next."),
    // Low end
    r("sub-too-low", "Low end", "Bass below E1", "Under E1 (41 Hz) most speakers reproduce nothing: the note is felt as rumble, if at all."),
    r("low-crowding", "Low end", "Two parts in the sub", "Only one part should own the low end (under G2, about 100 Hz) at a time, or they mask each other."),
    r("kick-bass", "Low end", "Kick and bass collide", "Bass notes ringing through every kick fight it for the same frequencies; leave room for the kick or duck the bass."),
    r("lowend-panned", "Low end", "Low end off center", "Kick and bass belong in the center: clubs sum the low end to mono and it carries the mix."),
    r("lowend-wide", "Low end", "Stereo width on the bass", "Unison spread and chorus on a bass smear it and cancel in mono."),
    r("reverb-on-bass", "Low end", "Reverb on the low end", "Reverb on a kick or bass muddies the whole low end."),
    r("no-highpass", "Low end", "No low cut", "Parts that live above the bass still carry low-frequency rumble; a high-pass leaves the low end to the bass and kick."),
    r("fm-bass", "Low end", "Inharmonic FM bass", "Non-integer FM ratios make inharmonic partials: on a bass the pitch turns vague; keep a clean 1:1 carrier for the sub."),
    // Mix
    r("hot-faders", "Mix", "Faders above unity", "Gain staging: pull other parts down rather than pushing faders up, to keep headroom."),
    r("unmixed", "Mix", "Every channel at the same level", "Identical volumes mean the balance has not been set yet."),
    r("not-routed", "Mix", "Channels straight to the master", "Each instrument on its own mixer insert can be EQ'd, compressed and leveled on its own."),
    r("unused-insert", "Mix", "Inserts with effects but no input", "Effects on an insert nothing plays into are clutter."),
    r("solo", "Mix", "Solo left on", "A soloed insert silences everything else."),
    r("muted", "Mix", "Muted parts", "Muted channels, tracks and inserts are left out of the song and the render."),
    r("flat-automation", "Mix", "Automation that never moves", "A lane whose points all have one value does nothing a fixed setting would not."),
    // Stereo
    r("all-center", "Stereo", "Everything in the center", "A mix with every part in the middle is narrow and crowded; spread the supporting parts."),
    r("lopsided", "Stereo", "Lopsided stereo image", "Parts panned mostly to one side tip the mix over."),
    // Effects
    r("fx-order", "Effects", "Reverb before dynamics", "The usual chain is EQ → compression → saturation → delay and reverb; compressing a reverb tail pumps it."),
    r("delay-sync", "Effects", "Delay off the beat", "Delays timed to a note value (1/8, dotted 1/8, ...) lock into the groove."),
    r("delay-feedback", "Effects", "Runaway delay", "Feedback near 1 makes the repeats build up instead of dying away."),
    r("reverb-wet", "Effects", "Washed-out reverb", "A reverb mostly wet pushes the part far back and blurs it."),
    r("double-reverb", "Effects", "Two reverbs on one insert", "Two reverbs in a row are rarely better than one."),
    r("crushing-compressor", "Effects", "Crushing compression", "A high ratio with a low threshold flattens the part's micro-dynamics (its punch)."),
    r("drum-attack", "Effects", "Compressor eats the drum transients", "An attack under 3 ms clamps the drum's initial crack; 10–30 ms lets it through."),
    r("eq-boost", "Effects", "Big EQ boosts", "Boosts over 9 dB usually mean something else should be cut instead."),
    r("resonance", "Effects", "Screaming resonance", "Filter resonance near its maximum whistles and can self-oscillate."),
    r("disabled", "Effects", "Disabled effects", "Bypassed effects are clutter in the chain."),
    // Master
    r("master-hot", "Master", "Master fader above 0 dB", "Leave the master at unity or below: headroom for mastering, no clipping."),
    r("no-limiter", "Master", "No limiter on the master", "Nothing stops the render from clipping."),
    r("limiter-last", "Master", "Limiter not last", "The brickwall limiter goes last on the master, or what comes after it can clip."),
    r("limiter-ceiling", "Master", "No true-peak headroom", "A ceiling of −1 dB keeps inter-sample peaks and lossy encoding from clipping."),
    r("limiter-drive", "Master", "Over-limited master", "Streaming normalizes loudness (about −14 LUFS): an over-limited master is turned down and only loses its punch."),
    r("master-chain", "Master", "Heavy master chain", "Many devices or several limiters on the master usually fix in mastering what belongs in the mix."),
    // Project
    r("tempo", "Project", "Tempo", "A fractional tempo is usually accidental; an extreme one may really be half or double time."),
    r("pattern-bars", "Project", "Patterns of odd lengths", "Patterns in whole bars, and phrases of 2, 4 or 8 bars, line up with the song's form."),
    r("unused-channel", "Project", "Unused channels", "Channels with no notes are clutter."),
    r("names", "Project", "Default names", "Names like \"Pattern 3\" say nothing when you come back to the project."),
    r("unknown-content", "Project", "Content this version doesn't know", "The song names sections, instruments, effects or settings this version of Rosaclef doesn't know (made with a newer one, or a typo): they are left out, or played on a stand-in, until it is updated."),
];

/// The rule with this id.
pub fn rule(id: &str) -> Option<&'static Rule> {
    RULES.iter().find(|r| r.id == id)
}

// ------------------------------------------------------------------ entry

/// Everything the Critic has to say about a project.
#[derive(Serialize, Clone, Debug)]
pub struct Critique {
    /// The key it hears the song in ("F minor"; "" without pitched notes).
    pub key: String,
    pub findings: Vec<Finding>,
}

/// Run every check on a project. Rules turned off in the project
/// (`critic.off`) or in `off` are not reported; findings it suppresses
/// (`critic.suppress`) come marked as suppressed.
pub fn critique(p: &Project, off: &[String]) -> Critique {
    critique_with(p, off, &[])
}

/// [`critique`] of a project loaded for playback, its fallbacks
/// ([`crate::compat`]: what was left out or replaced) reported too.
pub fn critique_with(p: &Project, off: &[String], fallbacks: &[String]) -> Critique {
    let mut a = Ana::new(p);
    for f in fallbacks {
        let (path, what) = f.split_once(": ").unwrap_or(("", f));
        let mut at = analysis::at_project(path);
        at.id = path.into();
        let mut chars = what.chars();
        let title = chars
            .next()
            .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
            .unwrap_or_default();
        a.add(
            "unknown-content",
            "warn",
            title,
            format!("{path} in project.json is not something this version of Rosaclef knows. Update Rosaclef, or fix the name if it is a typo (`rosaclef validate` shows it)."),
            at,
            None,
        );
    }
    harmony::find_key(&mut a);
    harmony::check_harmony(&mut a);
    harmony::check_key(&mut a);
    harmony::check_clashes(&mut a);
    harmony::check_melody(&mut a);
    rhythm::check_rhythm(&mut a);
    arrangement::check_arrangement(&mut a);
    mixing::check_low_end(&mut a);
    mixing::check_mix(&mut a);
    mixing::check_stereo(&mut a);
    mixing::check_effects(&mut a);
    mixing::check_master(&mut a);
    arrangement::check_project(&mut a);
    let mut out: Vec<Finding> = a
        .out
        .into_iter()
        .filter(|f| !off.iter().chain(&p.critic.off).any(|o| o == f.rule))
        .map(|mut f| {
            f.suppressed = p.critic.suppress.contains(&f.key);
            f
        })
        .collect();
    let rank = |f: &Finding| {
        CATEGORIES
            .iter()
            .position(|c| *c == f.category)
            .unwrap_or(99)
            * 4
            + if f.level == "warn" { 0 } else { 2 }
            + if f.fix.is_some() { 0 } else { 1 }
            + if f.suppressed { 1000 } else { 0 }
    };
    // A stable sort keeps the checks' own order within a rank.
    out.sort_by_key(rank);
    Critique {
        key: a.key_label,
        findings: out,
    }
}

// ------------------------------------------------------------------ fixes

/// Apply JSON Patch operations to a JSON document. `add` sets an object
/// member (creating missing parent objects), appends to an array with `-`
/// or inserts into it at an index; `remove` deletes a member or element.
pub fn apply_ops(doc: &mut Value, ops: &[Op]) -> Result<(), String> {
    for op in ops {
        let parts: Vec<String> = op
            .path
            .split('/')
            .skip(1)
            .map(|s| s.replace("~1", "/").replace("~0", "~"))
            .collect();
        let Some((last, parents)) = parts.split_last() else {
            return Err(format!("{}: an empty path", op.op));
        };
        let mut at = &mut *doc;
        for seg in parents {
            at = match at {
                Value::Object(m) => m.entry(seg.clone()).or_insert_with(|| json!({})),
                Value::Array(a) => {
                    let i: usize = seg
                        .parse()
                        .map_err(|_| format!("{}: not an index", op.path))?;
                    a.get_mut(i)
                        .ok_or_else(|| format!("{}: no element {i}", op.path))?
                }
                _ => return Err(format!("{}: not a container", op.path)),
            };
        }
        match (op.op, at) {
            ("add", Value::Object(m)) => {
                m.insert(last.clone(), op.value.clone().unwrap_or(Value::Null));
            }
            ("add", Value::Array(a)) => {
                let v = op.value.clone().unwrap_or(Value::Null);
                if last == "-" {
                    a.push(v);
                } else {
                    let i: usize = last
                        .parse()
                        .map_err(|_| format!("{}: not an index", op.path))?;
                    if i > a.len() {
                        return Err(format!("{}: index out of range", op.path));
                    }
                    a.insert(i, v);
                }
            }
            ("remove", Value::Object(m)) => {
                m.remove(last.as_str());
            }
            ("remove", Value::Array(a)) => {
                let i: usize = last
                    .parse()
                    .map_err(|_| format!("{}: not an index", op.path))?;
                if i >= a.len() {
                    return Err(format!("{}: no element {i}", op.path));
                }
                a.remove(i);
            }
            (o, _) => return Err(format!("{}: cannot {o} here", op.path)),
        }
    }
    Ok(())
}

/// Apply the fixes of the findings named by `keys` (finding keys or rule
/// ids; "all" = every suggestion), one after the other. Each is looked up
/// again on the project as the previous fixes left it (they can move the
/// notes a later one points at). Returns the fixed project and the fixes
/// applied.
pub fn apply_fixes(
    p: &Project,
    keys: &[String],
    off: &[String],
) -> Result<(Project, Vec<String>), String> {
    let mut p = p.clone();
    let mut done = vec![];
    // A finding by its key; by rule or "all", only those not suppressed.
    let wanted = |f: &Finding| {
        keys.iter()
            .any(|k| *k == f.key || (!f.suppressed && (k == "all" || k == f.rule)))
    };
    // The keys to fix, in order (a key the fixes make go away is skipped).
    let order: Vec<String> = critique(&p, off)
        .findings
        .iter()
        .filter(|f| f.fix.is_some() && wanted(f))
        .map(|f| f.key.clone())
        .collect();
    for key in order {
        let Some(f) = critique(&p, off)
            .findings
            .into_iter()
            .find(|f| f.key == key)
        else {
            continue;
        };
        let Some(fix) = f.fix else { continue };
        let mut doc = serde_json::to_value(&p).map_err(|e| e.to_string())?;
        apply_ops(&mut doc, &fix.ops).map_err(|e| format!("{key}: {e}"))?;
        p = serde_json::from_value(doc).map_err(|e| format!("{key}: {e}"))?;
        done.push(fix.label);
    }
    Ok((p, done))
}

/// The studio's endpoint (POST /api/critic): `{"project": …, "fix": [keys
/// or rule ids]}` in; the key and findings out — and, when `fix` names
/// any, the fixed project and the fixes applied.
pub fn api(text: &str) -> Result<Value, String> {
    #[derive(serde::Deserialize)]
    struct Req {
        project: Value,
        #[serde(default)]
        fix: Vec<String>,
    }
    let req: Req = serde_json::from_str(text).map_err(|e| e.to_string())?;
    // Forward-compatibly, as the engine plays it.
    let playable = crate::compat::value_for_playback(req.project).map_err(|checked| {
        let msgs: Vec<String> = checked.errors().map(|i| i.to_string()).collect();
        format!("the project is invalid: {}", msgs.join("; "))
    })?;
    if req.fix.is_empty() {
        let c = critique_with(&playable.project, &[], &playable.fallbacks);
        return Ok(json!({"key": c.key, "findings": c.findings}));
    }
    if crate::compat::lossy(&playable) {
        return Err("the song holds content this version of Rosaclef doesn't know (see the Critic); fixes are off so that nothing of it is lost".into());
    }
    let (fixed, done) = apply_fixes(&playable.project, &req.fix, &[])?;
    if let Some(e) = crate::validate::validate(&fixed)
        .into_iter()
        .find(|i| i.severity == crate::validate::Severity::Error)
    {
        return Err(format!("the fixed project is invalid: {e}"));
    }
    let c = critique(&fixed, &[]);
    Ok(json!({"key": c.key, "findings": c.findings, "applied": done, "project": fixed}))
}

/// Suppress a finding by its key, or a whole check by its rule id (turned
/// off). Returns what it did.
/// `fallbacks`: what loading for playback left out (their findings can be
/// suppressed too).
pub fn suppress(p: &mut Project, what: &str, fallbacks: &[String]) -> Result<String, String> {
    if let Some(r) = rule(what) {
        if !p.critic.off.iter().any(|x| x == what) {
            p.critic.off.push(what.into());
        }
        return Ok(format!("turned off: {} ({})", r.name, r.id));
    }
    let known = critique_with(p, &[], fallbacks)
        .findings
        .iter()
        .any(|f| f.key == what);
    if !known && !p.critic.suppress.iter().any(|x| x == what) {
        return Err(format!("no finding or check {what:?} (run `rosaclef critic` for the keys, `--rules` for the checks)"));
    }
    if !p.critic.suppress.iter().any(|x| x == what) {
        p.critic.suppress.push(what.into());
    }
    Ok(format!("suppressed: {what}"))
}

/// Bring back a suppressed finding (by key) or a check turned off (by rule id).
pub fn unsuppress(p: &mut Project, what: &str) -> Result<String, String> {
    let (off, sup) = (p.critic.off.len(), p.critic.suppress.len());
    p.critic.off.retain(|x| x != what);
    p.critic.suppress.retain(|x| x != what);
    if p.critic.off.len() == off && p.critic.suppress.len() == sup {
        return Err(format!("{what:?} is neither suppressed nor turned off"));
    }
    Ok(format!("back on: {what}"))
}

/// The checks as text (`rosaclef critic --rules`).
pub fn rules_text(p: &Project) -> String {
    let mut out = String::new();
    for cat in CATEGORIES {
        out.push_str(&format!("\n{cat}\n"));
        for r in RULES.iter().filter(|r| r.category == *cat) {
            let off = p.critic.off.iter().any(|x| x == r.id);
            out.push_str(&format!(
                "  {:<20} {}{}\n",
                r.id,
                r.name,
                if off { "  (off)" } else { "" }
            ));
        }
    }
    out
}

/// The rules and categories (GET /api/critic).
pub fn catalog() -> Value {
    json!({"categories": CATEGORIES, "rules": RULES})
}

/// The findings as text, for the command line and the shell (`suppressed`:
/// list the suppressed findings too).
pub fn report_text(c: &Critique, suppressed: bool) -> String {
    let mut out = String::new();
    if !c.key.is_empty() {
        out.push_str(&format!("key: {}\n", c.key));
    }
    let mut category = "";
    for f in c.findings.iter().filter(|f| suppressed || !f.suppressed) {
        if f.category != category {
            category = f.category;
            out.push_str(&format!("\n{category}\n"));
        }
        let level = if f.suppressed { "off" } else { f.level };
        out.push_str(&format!("  {level:<4} {} — {}\n", f.title, f.at.label));
        out.push_str(&format!("       {}\n", f.detail));
        match &f.fix {
            Some(fix) => out.push_str(&format!(
                "       fix: {}   (--fix '{}')\n",
                fix.label, f.key
            )),
            None => out.push_str(&format!("       key: {}\n", f.key)),
        }
    }
    let shown: Vec<&Finding> = c.findings.iter().filter(|f| !f.suppressed).collect();
    let fixes = shown.iter().filter(|f| f.fix.is_some()).count();
    let hidden = c.findings.len() - shown.len();
    out.push_str(&format!(
        "\n{} ({fixes} with a fix: --fix KEY, --fix RULE or --fix all){}\n",
        crate::critic::analysis::plural(shown.len(), "finding", "findings"),
        if hidden > 0 {
            format!("; {hidden} suppressed (--suppressed shows them)")
        } else {
            String::new()
        }
    ));
    out
}
