// The Critic: mechanical checks ("lints") of a project against the rules of
// thumb of composition, arrangement, sound design and mixing.
//
// Everything here is pure and deterministic — no AI, no audio analysis: the
// checks read the project document (notes, clips, channels, the mixer) and
// return findings. A finding with a `fix` is a suggestion the producer can
// apply with one click (`apply` mutates the project it is given, as one
// undoable step); one without is an issue to know about. The rules are
// listed in RULES (with why they matter) and catalogued in docs/critic.md.
//
// Pitches are MIDI numbers as they sound (C4 = 60): a channel's notes are
// shifted by the song's and the instrument's transposition before the
// register checks read them; fixes move the written notes by the same steps.

import { optionValue, setParam, setOption, barLines, noteName, newDevice } from "./model.js";
import { MAJOR_PROFILE, MINOR_PROFILE, correlate, KEY_NAMES } from "./voice.js";
import { insertIx, insertIndex, trackIx, trackIndex } from "#brands";

// ------------------------------------------------------------------ types

/** Where a finding points: `kind` is "pattern" (`id`; `channel` and `notes`, its note indexes, when set),
 * "channel" (`id`), "insert" (`index`), "song" (`beat`; `index` = a clip, or -1), "lane" (`index`) or "project". */
/** type Where = { kind: String, id: String, channel: String, index: Int, beat: Number, notes: Int[], label: String } */

/** A finding. `level`: "warn" or "info". `fix` names the one-click change ("" = an issue only);
 * `apply` makes it on the project it is given. `key` is stable across edits (for ignoring it). */
/** type Finding = { key: String, rule: String, category: String, level: String, title: String, detail: String, where: Where, fix: String, apply: (Project) => Undefined } */

/** A check the Critic runs: its category, its name and why it matters. */
/** type Rule = { id: String, category: String, name: String, why: String } */

/** What the Critic knows of a channel. `role`: drums, bass, harmony, lead, part (other pitched), fx (transitions),
 * gen (generative), arp (arpeggiated), sample (samplers, granular, plugins) or idle (no notes).
 * `shift`: semitones its notes sound away from where they are written. `poly`: notes per onset, on average. */
/** type ChInfo = { id: String, name: String, type: String, role: String, kind: String, pitched: Boolean, shift: Int, count: Int, median: Number, low: Int, high: Int, poly: Number, insert: Int, layerOf: String, program: String, pan: Number, volume: Number, mute: Boolean } */

/** A note as it sounds in the song (beats): `pitch` is sounding; `index` is its note in `pattern`. */
/** type SNote = { pitch: Int, start: Number, end: Number, velocity: Number, channel: String, pattern: String, index: Int } */

/** One channel's notes in one pattern (`idx`: note indexes, by start). */
/** type Part = { pat: Pattern, ch: ChInfo, idx: Int[] } */

/** Notes struck together in a part: indexes and sounding pitches, both from low to high. */
/** type Chord = { start: Number, end: Number, idx: Int[], pitches: Int[] } */

/** type Ana = { p: Project, chans: ChInfo[], parts: Part[], song: SNote[], length: Number, bars: BarPos[], tonic: Int, minor: Boolean, fit: Number, keyLabel: String, keyFromScore: Boolean, out: Finding[] } */

// ------------------------------------------------------------------ rules

export const CATEGORIES = ["Harmony", "Melody", "Rhythm", "Arrangement", "Low end", "Mix", "Stereo", "Effects", "Master", "Project"];

/** const RULES: Rule[] */
export const RULES = [
  // Harmony
  {
    id: "low-interval",
    category: "Harmony",
    name: "Low interval limits",
    why: "Close intervals voiced low (a third under C3, a second under E3) turn to mud: their partials beat against each other.",
  },
  {
    id: "chord-too-low",
    category: "Harmony",
    name: "Chords crowd the bass",
    why: "Chord notes under C3 fight the bass line; voice the harmony above it (or rootless) and leave the low end to the bass.",
  },
  {
    id: "voice-leading",
    category: "Harmony",
    name: "Jumpy voice leading",
    why: "Chords whose voices leap instead of moving to the nearest notes sound disjointed; inversions keep the voicing compact.",
  },
  {
    id: "parallel-fifths",
    category: "Harmony",
    name: "Parallel fifths and octaves",
    why: "In acoustic parts, voices moving in parallel perfect fifths or octaves fuse into one: great for power chords and stabs, a loss when the voices should stay independent.",
  },
  {
    id: "wide-spacing",
    category: "Harmony",
    name: "Gaps in the upper voices",
    why: "Upper voices more than an octave apart leave a hole in the chord; keep adjacent upper voices within an octave.",
  },
  {
    id: "out-of-key",
    category: "Harmony",
    name: "Notes outside the key",
    why: "A few notes outside the song's key are often slips of the mouse rather than deliberate chromaticism.",
  },
  {
    id: "key-signature",
    category: "Harmony",
    name: "Key signature disagrees",
    why: "The score's key should be the key the notes are in, or every note is spelled with accidentals.",
  },
  {
    id: "semitone-clash",
    category: "Harmony",
    name: "Sustained semitone clashes",
    why: "Two parts holding notes a minor second or minor ninth apart grind against each other.",
  },
  // Melody
  { id: "melody-range", category: "Melody", name: "Melody range", why: "A melody wider than an octave and a half is hard to sing and to follow." },
  { id: "large-leap", category: "Melody", name: "Leaps over an octave", why: "Leaps wider than an octave break a line into two; few melodies need them." },
  {
    id: "leap-recovery",
    category: "Melody",
    name: "Leaps that do not recover",
    why: "A big leap is balanced by a step back the other way (melodic fluency); leaps that keep going sound aimless.",
  },
  {
    id: "no-rests",
    category: "Melody",
    name: "A melody that never breathes",
    why: "Rests give the listener time to take in a phrase; a line that never stops tires the ear.",
  },
  { id: "monotone", category: "Melody", name: "Monotone melody", why: "A lead that keeps to one or two notes for bars on end has no contour to remember." },
  {
    id: "instrument-range",
    category: "Melody",
    name: "Beyond the instrument's range",
    why: "A real instrument cannot play these notes: the sample stretches unnaturally and players would refuse the part.",
  },
  // Rhythm
  {
    id: "flat-velocity",
    category: "Rhythm",
    name: "Robotic velocities",
    why: "Every note at the same velocity sounds like a machine gun; real players accent the beat and play ghost notes softly.",
  },
  {
    id: "max-velocity",
    category: "Rhythm",
    name: "Everything at full velocity",
    why: "Notes all at the top leave no room for accents: velocity is the part's dynamics.",
  },
  {
    id: "machine-gun",
    category: "Rhythm",
    name: "Machine-gun runs",
    why: "Fast runs of one drum at one velocity sound mechanical; alternate strong and weak hits.",
  },
  { id: "no-ghost-notes", category: "Rhythm", name: "No ghost notes", why: "Snares and hats with no soft hits between the backbeats lack groove." },
  {
    id: "rigid-timing",
    category: "Rhythm",
    name: "Live instrument on the grid",
    why: "A real instrument played exactly on the grid sounds programmed; a few milliseconds of push and pull bring it to life.",
  },
  { id: "sloppy-timing", category: "Rhythm", name: "Notes just off the grid", why: "In a part otherwise on the grid, notes a hair off it are usually slips." },
  { id: "same-pitch-overlap", category: "Rhythm", name: "Overlapping notes", why: "Two notes of one pitch overlapping retrigger or cut each other off." },
  { id: "duplicate-notes", category: "Rhythm", name: "Duplicate notes", why: "Stacked identical notes double the level and phase against each other." },
  { id: "silent-notes", category: "Rhythm", name: "Silent notes", why: "Notes at zero velocity make no sound." },
  { id: "tiny-notes", category: "Rhythm", name: "Very short notes", why: "Pitched notes shorter than a 64th are usually accidental clicks." },
  { id: "past-end", category: "Rhythm", name: "Notes after the pattern ends", why: "Notes that start after the pattern's end never play." },
  { id: "swing-unused", category: "Rhythm", name: "Swing with nothing to swing", why: "Swing delays the off-beat 16ths; with no notes there it does nothing." },
  // Arrangement
  {
    id: "empty-playlist",
    category: "Arrangement",
    name: "Nothing on the playlist",
    why: "Patterns play in the song only once they are placed on the playlist.",
  },
  { id: "short-song", category: "Arrangement", name: "The song is one loop", why: "An eight- or sixteen-bar loop is a sketch, not a song: it needs sections." },
  {
    id: "loopitis",
    category: "Arrangement",
    name: "Loopitis",
    why: "The same bars repeating unchanged for a long stretch fatigue the ear; change something every 4 to 8 bars.",
  },
  {
    id: "no-contrast",
    category: "Arrangement",
    name: "No contrast between sections",
    why: "A chorus feels big because the verse before it was sparse (subtractive arrangement): density should rise and fall.",
  },
  {
    id: "too-many-elements",
    category: "Arrangement",
    name: "Too many elements at once",
    why: "Listeners follow about three things at once (rhythm, melody, a wildcard); more competing parts become clutter.",
  },
  { id: "full-intro", category: "Arrangement", name: "Everything enters at once", why: "An intro that starts with every part has nowhere left to build to." },
  { id: "abrupt-ending", category: "Arrangement", name: "Abrupt ending", why: "A song that stops at full density sounds cut off; thin it out or fade it." },
  {
    id: "clip-off-bar",
    category: "Arrangement",
    name: "Clips just off the bar",
    why: "Sections start on the bar; a clip a little off it is usually a slip of the mouse.",
  },
  { id: "clip-overlap", category: "Arrangement", name: "Overlapping clips", why: "Clips overlapping on one track play twice over each other." },
  { id: "unused-pattern", category: "Arrangement", name: "Unused patterns", why: "Patterns that are never placed are not heard." },
  { id: "empty-clips", category: "Arrangement", name: "Clips of empty patterns", why: "Clips of patterns with no notes play nothing." },
  { id: "identical-patterns", category: "Arrangement", name: "Identical patterns", why: "Duplicates of a pattern have to be edited twice; use one." },
  {
    id: "no-movement",
    category: "Arrangement",
    name: "No automation",
    why: "Filter sweeps, volume rides and risers carry the energy from one section into the next.",
  },
  // Low end
  {
    id: "sub-too-low",
    category: "Low end",
    name: "Bass below E1",
    why: "Under E1 (41 Hz) most speakers reproduce nothing: the note is felt as rumble, if at all.",
  },
  {
    id: "low-crowding",
    category: "Low end",
    name: "Two parts in the sub",
    why: "Only one part should own the low end (under G2, about 100 Hz) at a time, or they mask each other.",
  },
  {
    id: "kick-bass",
    category: "Low end",
    name: "Kick and bass collide",
    why: "Bass notes ringing through every kick fight it for the same frequencies; leave room for the kick or duck the bass.",
  },
  {
    id: "lowend-panned",
    category: "Low end",
    name: "Low end off center",
    why: "Kick and bass belong in the center: clubs sum the low end to mono and it carries the mix.",
  },
  { id: "lowend-wide", category: "Low end", name: "Stereo width on the bass", why: "Unison spread and chorus on a bass smear it and cancel in mono." },
  { id: "reverb-on-bass", category: "Low end", name: "Reverb on the low end", why: "Reverb on a kick or bass muddies the whole low end." },
  {
    id: "no-highpass",
    category: "Low end",
    name: "No low cut",
    why: "Parts that live above the bass still carry low-frequency rumble; a high-pass leaves the low end to the bass and kick.",
  },
  {
    id: "fm-bass",
    category: "Low end",
    name: "Inharmonic FM bass",
    why: "Non-integer FM ratios make inharmonic partials: on a bass the pitch turns vague; keep a clean 1:1 carrier for the sub.",
  },
  // Mix
  {
    id: "hot-faders",
    category: "Mix",
    name: "Faders above unity",
    why: "Gain staging: pull other parts down rather than pushing faders up, to keep headroom.",
  },
  { id: "unmixed", category: "Mix", name: "Every channel at the same level", why: "Identical volumes mean the balance has not been set yet." },
  {
    id: "not-routed",
    category: "Mix",
    name: "Channels straight to the master",
    why: "Each instrument on its own mixer insert can be EQ'd, compressed and leveled on its own.",
  },
  { id: "unused-insert", category: "Mix", name: "Inserts with effects but no input", why: "Effects on an insert nothing plays into are clutter." },
  { id: "solo", category: "Mix", name: "Solo left on", why: "A soloed insert silences everything else." },
  { id: "muted", category: "Mix", name: "Muted parts", why: "Muted channels, tracks and inserts are left out of the song and the render." },
  {
    id: "flat-automation",
    category: "Mix",
    name: "Automation that never moves",
    why: "A lane whose points all have one value does nothing a fixed setting would not.",
  },
  // Stereo
  {
    id: "all-center",
    category: "Stereo",
    name: "Everything in the center",
    why: "A mix with every part in the middle is narrow and crowded; spread the supporting parts.",
  },
  { id: "lopsided", category: "Stereo", name: "Lopsided stereo image", why: "Parts panned mostly to one side tip the mix over." },
  // Effects
  {
    id: "fx-order",
    category: "Effects",
    name: "Reverb before dynamics",
    why: "The usual chain is EQ → compression → saturation → delay and reverb; compressing a reverb tail pumps it.",
  },
  { id: "delay-sync", category: "Effects", name: "Delay off the beat", why: "Delays timed to a note value (1/8, dotted 1/8, ...) lock into the groove." },
  { id: "delay-feedback", category: "Effects", name: "Runaway delay", why: "Feedback near 1 makes the repeats build up instead of dying away." },
  { id: "reverb-wet", category: "Effects", name: "Washed-out reverb", why: "A reverb mostly wet pushes the part far back and blurs it." },
  { id: "double-reverb", category: "Effects", name: "Two reverbs on one insert", why: "Two reverbs in a row are rarely better than one." },
  {
    id: "crushing-compressor",
    category: "Effects",
    name: "Crushing compression",
    why: "A high ratio with a low threshold flattens the part's micro-dynamics (its punch).",
  },
  {
    id: "drum-attack",
    category: "Effects",
    name: "Compressor eats the drum transients",
    why: "An attack under 3 ms clamps the drum's initial crack; 10–30 ms lets it through.",
  },
  { id: "eq-boost", category: "Effects", name: "Big EQ boosts", why: "Boosts over 9 dB usually mean something else should be cut instead." },
  { id: "resonance", category: "Effects", name: "Screaming resonance", why: "Filter resonance near its maximum whistles and can self-oscillate." },
  { id: "disabled", category: "Effects", name: "Disabled effects", why: "Bypassed effects are clutter in the chain." },
  // Master
  { id: "master-hot", category: "Master", name: "Master fader above 0 dB", why: "Leave the master at unity or below: headroom for mastering, no clipping." },
  { id: "no-limiter", category: "Master", name: "No limiter on the master", why: "Nothing stops the render from clipping." },
  { id: "limiter-last", category: "Master", name: "Limiter not last", why: "The brickwall limiter goes last on the master, or what comes after it can clip." },
  {
    id: "limiter-ceiling",
    category: "Master",
    name: "No true-peak headroom",
    why: "A ceiling of −1 dB keeps inter-sample peaks and lossy encoding from clipping.",
  },
  {
    id: "limiter-drive",
    category: "Master",
    name: "Over-limited master",
    why: "Streaming normalizes loudness (about −14 LUFS): an over-limited master is turned down and only loses its punch.",
  },
  {
    id: "master-chain",
    category: "Master",
    name: "Heavy master chain",
    why: "Many devices or several limiters on the master usually fix in mastering what belongs in the mix.",
  },
  // Project
  { id: "tempo", category: "Project", name: "Tempo", why: "A fractional tempo is usually accidental; an extreme one may really be half or double time." },
  {
    id: "pattern-bars",
    category: "Project",
    name: "Patterns of odd lengths",
    why: "Patterns in whole bars, and phrases of 2, 4 or 8 bars, line up with the song's form.",
  },
  { id: "unused-channel", category: "Project", name: "Unused channels", why: "Channels with no notes are clutter." },
  { id: "names", category: "Project", name: "Default names", why: 'Names like "Pattern 3" say nothing when you come back to the project.' },
];

/** function ruleOf(id: String) => Rule */
export function ruleOf(id) {
  for (const r of RULES) if (r.id === id) return r;
  return { id: id, category: "Project", name: id, why: "" };
}

// ------------------------------------------------------------------ helpers

/** function param(d: Device, key: String, dflt: Number) => Number */
function param(d, key, dflt) {
  for (const e of d.params) if (e.key === key) return e.value;
  return dflt;
}

/** function hasParam(d: Device, key: String) => Boolean */
function hasParam(d, key) {
  return d.params.some((e) => e.key === key);
}

/** function clamp(x: Number, lo: Number, hi: Number) => Number */
function clamp(x, lo, hi) {
  return Math.max(lo, Math.min(hi, x));
}

/** function r3(x: Number) => Number */
function r3(x) {
  return Math.round(x * 1000) / 1000;
}

/** function pc(p: Int) => Int */
function pc(p) {
  return ((p % 12) + 12) % 12;
}

/** function plural(n: Int, one: String, many: String) => String */
function plural(n, one, many) {
  return `${n} ${n === 1 ? one : many}`;
}

/** function db(gain: Number) => String */
function db(gain) {
  if (gain <= 0.00001) return "−∞ dB";
  const v = Math.round(20 * Math.log10(gain) * 10) / 10;
  return `${v > 0 ? "+" : ""}${v} dB`;
}

/** function minOf(xs: Number[]) => Number */
function minOf(xs) {
  let m = Infinity;
  for (const x of xs) m = Math.min(m, x);
  return m;
}

/** function minInt(xs: Int[]) => Int */
function minInt(xs) {
  let m = xs.length > 0 ? xs[0] : 0;
  for (const x of xs) m = Math.min(m, x);
  return m;
}

/** function maxInt(xs: Int[]) => Int */
function maxInt(xs) {
  let m = xs.length > 0 ? xs[0] : 0;
  for (const x of xs) m = Math.max(m, x);
  return m;
}

/** The note indexes of chords. */
/** function chordNotes(cs: Chord[]) => Int[] */
function chordNotes(cs) {
  /** const out: Int[] */
  const out = [];
  for (const c of cs) for (const i of c.idx) out.push(i);
  return out;
}

/** function maxOf(xs: Number[]) => Number */
function maxOf(xs) {
  let m = -Infinity;
  for (const x of xs) m = Math.max(m, x);
  return m;
}

/** The frequency of a MIDI pitch (Hz). */
/** function hz(p: Number) => Number */
function hz(p) {
  return 440 * Math.pow(2, (p - 69) / 12);
}

/** A small deterministic jitter in -1..1 (the same every time the Critic runs). */
/** function jitter(i: Int, salt: Int) => Number */
function jitter(i, salt) {
  const x = Math.sin((i + 1) * 12.9898 + salt * 78.233) * 43758.5453;
  return (x - Math.floor(x)) * 2 - 1;
}

/** function noFix(p: Project) => Undefined */
function noFix(p) {}

/** function findPattern(p: Project, id: String) => Pattern? */
function findPattern(p, id) {
  return p.patterns.find((x) => x.id === id);
}

/** function findChannel(p: Project, id: String) => Channel? */
function findChannel(p, id) {
  return p.channels.find((x) => x.id === id);
}

/** Change notes of a pattern by index (when the pattern is still there). */
/** function editNotes(p: Project, patId: String, idx: Int[], fn: (Note, Int) => Undefined) => Undefined */
function editNotes(p, patId, idx, fn) {
  const pat = findPattern(p, patId);
  if (!pat) return undefined;
  for (let k = 0; k < idx.length; k++) {
    const i = idx[k];
    if (i < pat.notes.length) fn(pat.notes[i], k);
  }
}

/** Remove notes of a pattern by index. */
/** function dropNotes(p: Project, patId: String, idx: Int[]) => Undefined */
function dropNotes(p, patId, idx) {
  const pat = findPattern(p, patId);
  if (!pat) return undefined;
  pat.notes = pat.notes.filter((n, i) => !idx.includes(i));
}

// ------------------------------------------------------------------ where

/** function atPart(part: Part, notes: Int[]) => Where */
function atPart(part, notes) {
  return { kind: "pattern", id: part.pat.id, channel: part.ch.id, index: -1, beat: -1, notes: notes, label: `${part.pat.name} · ${part.ch.name}` };
}

/** function atPattern(pat: Pattern) => Where */
function atPattern(pat) {
  return { kind: "pattern", id: pat.id, channel: "", index: -1, beat: -1, notes: [], label: pat.name };
}

/** function atChannel(c: ChInfo) => Where */
function atChannel(c) {
  return { kind: "channel", id: c.id, channel: c.id, index: -1, beat: -1, notes: [], label: c.name };
}

/** function atInsert(p: Project, i: Int) => Where */
function atInsert(p, i) {
  const ins = p.mixer.inserts[i];
  return { kind: "insert", id: "", channel: "", index: i, beat: -1, notes: [], label: i === 0 ? "Master" : `Insert ${i} · ${ins ? ins.name : ""}` };
}

/** function atSong(a: Ana, beat: Number, clip: Int) => Where */
function atSong(a, beat, clip) {
  return { kind: "song", id: "", channel: "", index: clip, beat: beat, notes: [], label: `Bar ${barOf(a, beat) + 1}` };
}

/** function atLane(l: AutomationLane, i: Int) => Where */
function atLane(l, i) {
  return {
    kind: "lane",
    id: l.id,
    channel: "",
    index: i,
    beat: l.points.length > 0 ? l.points[0].beat : 0,
    notes: [],
    label: l.name !== "" ? l.name : l.target,
  };
}

/** function atProject(label: String) => Where */
function atProject(label) {
  return { kind: "project", id: "", channel: "", index: -1, beat: -1, notes: [], label: label };
}

/** function whereKey(w: Where) => String */
function whereKey(w) {
  return `${w.kind}:${w.id}:${w.channel}:${w.index}`;
}

/** Record a finding. */
/** function add(a: Ana, rule: String, level: String, title: String, detail: String, where: Where, fix: String, apply: (Project) => Undefined) => Undefined */
function add(a, rule, level, title, detail, where, fix, apply) {
  const base = `${rule}|${whereKey(where)}`;
  let key = base;
  let n = 2;
  while (a.out.some((f) => f.key === key)) {
    key = `${base}#${n}`;
    n = n + 1;
  }
  a.out.push({ key: key, rule: rule, category: ruleOf(rule).category, level: level, title: title, detail: detail, where: where, fix: fix, apply: apply });
}

/** An issue: a finding with nothing to apply. */
/** function note(a: Ana, rule: String, level: String, title: String, detail: String, where: Where) => Undefined */
function note(a, rule, level, title, detail, where) {
  add(a, rule, level, title, detail, where, "", noFix);
}

// ------------------------------------------------------------------ analysis

const PITCHED = ["analog", "fm", "wavetable", "additive", "soundfont"];

/** function drumKitOf(c: Channel) => Boolean */
function drumKitOf(c) {
  if (c.instrument.type === "drum") return true;
  return c.instrument.type === "soundfont" && optionValue(c.instrument, "program").toLowerCase().includes("kit");
}

/** Group a part's notes into chords (notes struck within a 48th of a beat). */
/** function chordsOf(a: Ana, part: Part) => Chord[] */
function chordsOf(a, part) {
  /** const out: Chord[] */
  const out = [];
  const notes = part.pat.notes;
  for (const i of part.idx) {
    const n = notes[i];
    const last = out.length > 0 ? out[out.length - 1] : undefined;
    if (last && Math.abs(n.start - last.start) < 0.021) {
      last.idx.push(i);
      last.end = Math.max(last.end, n.start + n.length);
    } else out.push({ start: n.start, end: n.start + n.length, idx: [i], pitches: [] });
  }
  for (const c of out) {
    c.idx.sort((x, y) => notes[x].pitch - notes[y].pitch);
    c.pitches = c.idx.map((i) => Math.round(notes[i].pitch) + part.ch.shift);
  }
  return out;
}

/** function analyzeChannel(p: Project, c: Channel) => ChInfo */
function analyzeChannel(p, c) {
  const drum = drumKitOf(c);
  const type = c.instrument.type;
  const pitched = !drum && PITCHED.includes(type);
  const shift = Math.round(p.transport.transpose + (type === "soundfont" ? param(c.instrument, "transpose", 0) : 0));
  /** const pitches: Int[] */
  const pitches = [];
  let onsets = 0;
  for (const pat of p.patterns) {
    for (const n of pat.notes) {
      if (n.channel !== c.id) continue;
      pitches.push(Math.round(n.pitch) + shift);
    }
    // Onsets: distinct start times (notes come in any order).
    /** const starts: Number[] */
    const starts = [];
    for (const n of pat.notes) if (n.channel === c.id && !starts.some((s) => Math.abs(s - n.start) < 0.021)) starts.push(n.start);
    onsets = onsets + starts.length;
  }
  pitches.sort((x, y) => x - y);
  const count = pitches.length;
  const median = count > 0 ? pitches[Math.floor(count / 2)] : 0;
  const poly = onsets > 0 ? count / onsets : 0;
  let role = "part";
  if (drum) role = "drums";
  else if (type === "transition") role = "fx";
  else if (type === "generative") role = "gen";
  else if (c.arp.on) role = "arp";
  else if (!pitched) role = "sample";
  else if (count === 0) role = "idle";
  else if (poly >= 1.8) role = "harmony";
  else if (median < 50) role = "bass";
  else if (median >= 55 && poly < 1.3) role = "lead";
  return {
    id: c.id,
    name: c.name,
    type: type,
    role: role,
    kind: type === "drum" ? optionValue(c.instrument, "kind") || "kick" : "",
    pitched: pitched,
    shift: shift,
    count: count,
    median: median,
    low: count > 0 ? pitches[0] : 0,
    high: count > 0 ? pitches[count - 1] : 0,
    poly: poly,
    insert: insertIndex(c.mixer),
    layerOf: c.layerOf,
    program: type === "soundfont" ? optionValue(c.instrument, "program") || "Acoustic Grand Piano" : "",
    pan: c.pan,
    volume: c.volume,
    mute: c.mute,
  };
}

/** The notes of the song as they sound: clips expanded (muted tracks and channels left out). */
/** function songNotes(p: Project, chans: ChInfo[]) => SNote[] */
function songNotes(p, chans) {
  /** const out: SNote[] */
  const out = [];
  for (const c of p.playlist.clips) {
    if (c.pattern === "") continue;
    const tr = p.playlist.tracks[trackIndex(c.track)];
    if (tr && tr.mute) continue;
    const pat = findPattern(p, c.pattern);
    if (!pat || pat.length <= 0) continue;
    const L = pat.length;
    const w0 = c.offset;
    const w1 = c.offset + c.length;
    const clipEnd = c.start + c.length;
    for (let j = Math.floor(w0 / L); j * L < w1; j++) {
      const base = j * L;
      for (let i = 0; i < pat.notes.length; i++) {
        const n = pat.notes[i];
        const t = n.start + base;
        if (t < w0 - 1e-9 || t >= w1 - 1e-9 || n.start >= L - 1e-9) continue;
        const ch = chans.find((x) => x.id === n.channel);
        if (!ch || ch.mute) continue;
        const at = c.start + t - w0;
        out.push({
          pitch: Math.round(n.pitch) + ch.shift,
          start: at,
          end: Math.min(Math.min(at + n.length, clipEnd), c.start + base + L - w0),
          velocity: n.velocity,
          channel: n.channel,
          pattern: pat.id,
          index: i,
        });
      }
    }
  }
  out.sort((x, y) => x.start - y.start || x.pitch - y.pitch);
  return out;
}

/** function chanOf(a: Ana, id: String) => ChInfo? */
function chanOf(a, id) {
  return a.chans.find((c) => c.id === id);
}

/** The bar (from 0) that holds a song beat. */
/** function barOf(a: Ana, beat: Number) => Int */
function barOf(a, beat) {
  let lo = 0;
  let hi = a.bars.length - 1;
  if (hi < 0) return 0;
  while (lo < hi) {
    const mid = Math.ceil((lo + hi) / 2);
    if (a.bars[mid].start <= beat + 1e-9) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

/** Parse a key name ("F#m", "Bb") into a tonic pitch class and mode. */
/** function parseKey(name: String) => { tonic: Int, minor: Boolean } */
function parseKey(name) {
  const letters = "C D EF G A B";
  let tonic = letters.indexOf(name.slice(0, 1).toUpperCase());
  if (tonic < 0) tonic = 0;
  let rest = name.slice(1);
  if (rest.startsWith("#")) {
    tonic = tonic + 1;
    rest = rest.slice(1);
  } else if (rest.startsWith("b")) {
    tonic = tonic + 11;
    rest = rest.slice(1);
  }
  return { tonic: pc(tonic), minor: rest === "m" };
}

/** The pitch classes a key allows (minor keys also take the raised 6th and 7th). */
/** function keySteps(tonic: Int, minor: Boolean) => Int[] */
function keySteps(tonic, minor) {
  const steps = minor ? [0, 2, 3, 5, 7, 8, 9, 10, 11] : [0, 2, 4, 5, 7, 9, 11];
  return steps.map((s) => pc(tonic + s));
}

/** function keyText(tonic: Int, minor: Boolean) => String */
function keyText(tonic, minor) {
  return `${KEY_NAMES[pc(tonic)]} ${minor ? "minor" : "major"}`;
}

/** Find the key: the score's when it names one, else the best Krumhansl–Schmuckler fit. */
/** function findKey(a: Ana) => Undefined */
function findKey(a) {
  /** const hist: Number[] */
  const hist = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
  let total = 0;
  for (const part of a.parts) {
    if (!part.ch.pitched || part.ch.role === "drums") continue;
    for (const i of part.idx) {
      const n = part.pat.notes[i];
      const k = pc(Math.round(n.pitch) + part.ch.shift);
      hist[k] = hist[k] + Math.max(0.05, Math.min(4, n.length));
      total = total + 1;
    }
  }
  if (total === 0) return undefined;
  let best = -2;
  for (let k = 0; k < 12; k++) {
    const maj = correlate(hist, MAJOR_PROFILE, k);
    if (maj > best) {
      best = maj;
      a.tonic = k;
      a.minor = false;
    }
    const min = correlate(hist, MINOR_PROFILE, k);
    if (min > best) {
      best = min;
      a.tonic = k;
      a.minor = true;
    }
  }
  a.fit = best;
  a.keyLabel = keyText(a.tonic, a.minor);
  const named = a.p.score.key;
  if (named !== "") {
    const k = parseKey(named);
    const fit = correlate(hist, k.minor ? MINOR_PROFILE : MAJOR_PROFILE, k.tonic);
    if (fit < best - 0.15) {
      const t = a.tonic;
      const m = a.minor;
      add(
        a,
        "key-signature",
        "info",
        `The notes read as ${keyText(t, m)}, not ${keyText(k.tonic, k.minor)}`,
        `The score is set to ${keyText(k.tonic, k.minor)}, but the notes fit ${keyText(t, m)} much better (key-profile correlation ${Math.round(best * 100)}% against ${Math.round(fit * 100)}%).`,
        atProject("Score key"),
        `Set the key to ${keyText(t, m)}`,
        (p) => {
          p.score.key = keyNameOf(t, m);
        }
      );
    } else {
      a.tonic = k.tonic;
      a.minor = k.minor;
      a.fit = fit;
      a.keyLabel = keyText(k.tonic, k.minor);
      a.keyFromScore = true;
    }
  }
}

// Major keys by tonic pitch class, as the score names them (F# over Gb, Db over C#).
const MAJOR_NAMES = ["C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
const MINOR_NAMES = ["Cm", "C#m", "Dm", "Ebm", "Em", "Fm", "F#m", "Gm", "G#m", "Am", "Bbm", "Bm"];

/** function keyNameOf(tonic: Int, minor: Boolean) => String */
function keyNameOf(tonic, minor) {
  return minor ? MINOR_NAMES[pc(tonic)] : MAJOR_NAMES[pc(tonic)];
}

/** function analyze(p: Project) => Ana */
function analyze(p) {
  const chans = p.channels.map((c) => analyzeChannel(p, c));
  /** const parts: Part[] */
  const parts = [];
  for (const pat of p.patterns) {
    for (const ch of chans) {
      /** const idx: Int[] */
      const idx = [];
      for (let i = 0; i < pat.notes.length; i++) if (pat.notes[i].channel === ch.id) idx.push(i);
      if (idx.length === 0) continue;
      idx.sort((x, y) => pat.notes[x].start - pat.notes[y].start || pat.notes[x].pitch - pat.notes[y].pitch);
      parts.push({ pat: pat, ch: ch, idx: idx });
    }
  }
  const song = songNotes(p, chans);
  let length = 0;
  for (const c of p.playlist.clips) length = Math.max(length, c.start + c.length);
  const bars = length > 0 ? barLines(p.transport, 0, length - 1e-6) : [];
  /** const out: Finding[] */
  const out = [];
  const a = {
    p: p,
    chans: chans,
    parts: parts,
    song: song,
    length: length,
    bars: bars,
    tonic: 0,
    minor: false,
    fit: 0,
    keyLabel: "",
    keyFromScore: false,
    out: out,
  };
  return a;
}

// ------------------------------------------------------------------ harmony

// The lowest pitch where the lower note of each interval (semitones) still sounds clear.
const LOW_LIMITS = [0, 52, 51, 48, 46, 46, 47, 34, 43, 41, 41, 41, 0, 40, 39];

/** function lowLimitHit(ps: Int[]) => Int */
function lowLimitHit(ps) {
  for (let j = 0; j + 1 < ps.length; j++) {
    const iv = ps[j + 1] - ps[j];
    if (iv > 0 && iv < LOW_LIMITS.length && LOW_LIMITS[iv] > 0 && ps[j] < LOW_LIMITS[iv]) return j;
  }
  return -1;
}

/** function intervalName(iv: Int) => String */
function intervalName(iv) {
  const names = [
    "unison",
    "minor 2nd",
    "major 2nd",
    "minor 3rd",
    "major 3rd",
    "4th",
    "tritone",
    "5th",
    "minor 6th",
    "major 6th",
    "minor 7th",
    "major 7th",
    "octave",
    "minor 9th",
    "major 9th",
  ];
  return iv >= 0 && iv < names.length ? names[iv] : `${iv} semitones`;
}

/** Choose a voicing of `pitches` near `prev`: an inversion (the k lowest notes up an octave)
 * and an octave shift. Returns the shift of each note (low to high). */
/** function nearestVoicing(pitches: Int[], prev: Int[]) => Int[] */
function nearestVoicing(pitches, prev) {
  const n = pitches.length;
  /** let best: Int[] */
  let best = pitches.map((x) => 0);
  let bestCost = Infinity;
  const prevMean = prev.reduce((s, x) => s + x, 0) / Math.max(1, prev.length);
  for (let k = 0; k < n; k++) {
    for (const s of [0, -12, 12, -24, 24]) {
      const d = pitches.map((x, j) => Math.round((j < k ? 12 : 0) + s));
      const v = pitches.map((x, j) => x + d[j]);
      const lo = minInt(v);
      const hi = maxInt(v);
      if (lo < 36 || hi > 96) continue;
      const sorted = v.slice().sort((x, y) => x - y);
      let cost = 0;
      if (sorted.length === prev.length) for (let j = 0; j < n; j++) cost = cost + Math.abs(sorted[j] - prev[j]);
      else cost = Math.abs(sorted.reduce((t, x) => t + x, 0) / n - prevMean) * n;
      cost = cost + (k === 0 && s === 0 ? 0 : 0.5);
      if (cost < bestCost - 1e-9) {
        bestCost = cost;
        best = d;
      }
    }
  }
  return best;
}

/** function movement(a: Int[], b: Int[]) => Number */
function movement(a, b) {
  if (a.length === b.length) {
    let m = 0;
    for (let j = 0; j < a.length; j++) m = m + Math.abs(a[j] - b[j]);
    return m;
  }
  const ma = a.reduce((s, x) => s + x, 0) / Math.max(1, a.length);
  const mb = b.reduce((s, x) => s + x, 0) / Math.max(1, b.length);
  return Math.abs(ma - mb) * Math.max(a.length, b.length);
}

/** Is this a part of power chords (only roots, fifths and octaves)? */
/** function powerChords(chords: Chord[]) => Boolean */
function powerChords(chords) {
  for (const c of chords) {
    for (const x of c.pitches) if (![0, 7].includes(pc(x - c.pitches[0]))) return false;
  }
  return true;
}

/** function checkHarmony(a: Ana) => Undefined */
function checkHarmony(a) {
  const hasBass = a.chans.some((c) => c.role === "bass");
  for (const part of a.parts) {
    const ch = part.ch;
    if (!ch.pitched || ch.role === "drums") continue;
    const chords = chordsOf(a, part);
    const pid = part.pat.id;

    // Chords below C3 while a bass plays: lift the low notes (leave the root to the bass).
    /** const lowChords: Chord[] */
    const lowChords = [];
    if (hasBass && ch.role !== "bass") {
      for (const c of chords) if (c.pitches.length >= 3 && c.pitches.filter((x) => x < 48).length >= 2) lowChords.push(c);
    }
    if (lowChords.length > 0) {
      const idx = chordNotes(lowChords);
      const sh = ch.shift;
      add(
        a,
        "chord-too-low",
        "warn",
        `${plural(lowChords.length, "chord sits", "chords sit")} in the bass register`,
        `${ch.name} plays chords with two or more notes under C3 (the lowest is ${noteName(minInt(lowChords.map((c) => c.pitches[0])))}), where the bass lives.`,
        atPart(part, idx),
        "Raise the notes under C3 an octave",
        (p) =>
          editNotes(p, pid, idx, (n, k) => {
            while (Math.round(n.pitch) + sh < 48) n.pitch = n.pitch + 12;
          })
      );
    }

    // Low interval limits (chords not already lifted above).
    /** const muddy: Chord[] */
    const muddy = [];
    let worst = "";
    for (const c of chords) {
      if (c.pitches.length < 2 || lowChords.includes(c)) continue;
      const j = lowLimitHit(c.pitches);
      if (j < 0) continue;
      if (c.pitches[c.pitches.length - 1] + 12 > 108) continue;
      muddy.push(c);
      if (worst === "") worst = `a ${intervalName(c.pitches[j + 1] - c.pitches[j])} on ${noteName(c.pitches[j])}`;
    }
    if (muddy.length > 0) {
      /** const upper: Int[] */
      const upper = [];
      for (const c of muddy) for (let j = 1; j < c.idx.length; j++) upper.push(c.idx[j]);
      add(
        a,
        "low-interval",
        "warn",
        `${plural(muddy.length, "chord is", "chords are")} voiced below the low interval limits`,
        `${ch.name} has close intervals too low to sound clear (first: ${worst}). Spreading the voicing keeps the bass note and lifts the rest.`,
        atPart(part, chordNotes(muddy)),
        "Open the voicing (upper notes up an octave)",
        (p) =>
          editNotes(p, pid, upper, (n, k) => {
            n.pitch = n.pitch + 12;
          })
      );
    }

    // Voice leading: jumps between consecutive chords of three or more notes.
    if (ch.role === "harmony") {
      const big = chords.filter((c) => c.pitches.length >= 3);
      let jumps = 0;
      let before = 0;
      for (let i = 1; i < big.length; i++) {
        const P = big[i - 1].pitches;
        const N = big[i].pitches;
        if (P.join() === N.join()) continue;
        before = before + movement(P, N);
        const meanShift = Math.abs(N.reduce((s, x) => s + x, 0) / N.length - P.reduce((s, x) => s + x, 0) / P.length);
        if (Math.abs(N[N.length - 1] - P[P.length - 1]) >= 9 || meanShift >= 7) jumps = jumps + 1;
      }
      if (jumps > 0) {
        // The smoothed voicing: each chord as near the previous (smoothed) one as it can be.
        /** const shifts: Int[][] */
        const shifts = [];
        let prev = big[0].pitches;
        let after = 0;
        shifts.push(big[0].pitches.map((x) => 0));
        for (let i = 1; i < big.length; i++) {
          const d = nearestVoicing(big[i].pitches, prev);
          const v = big[i].pitches.map((x, j) => x + d[j]).sort((x, y) => x - y);
          if (big[i].pitches.join() !== big[i - 1].pitches.join()) after = after + movement(prev, v);
          shifts.push(d);
          prev = v;
        }
        if (after < before * 0.75) {
          const idx = chordNotes(big);
          /** const ds: Int[] */
          const ds = [];
          for (const d of shifts) for (const x of d) ds.push(x);
          add(
            a,
            "voice-leading",
            "info",
            `${plural(jumps, "chord change jumps", "chord changes jump")} instead of moving smoothly`,
            `In ${ch.name} the voices leap between chords (${Math.round(before)} semitones of movement in all). Inversions bring it to ${Math.round(after)}.`,
            atPart(part, idx),
            "Revoice with the nearest inversions",
            (p) =>
              editNotes(p, pid, idx, (n, k) => {
                n.pitch = n.pitch + ds[k];
              })
          );
        }
      }

      // Parallel fifths and octaves: in acoustic parts (synth stacks fuse on purpose), not power chords.
      if (ch.type === "soundfont" && !powerChords(big)) {
        let par = 0;
        let first = -1;
        for (let i = 1; i < big.length; i++) {
          const P = big[i - 1].pitches;
          const N = big[i].pitches;
          if (P.length !== N.length || P.join() === N.join()) continue;
          for (let x = 0; x < P.length; x++) {
            for (let y = x + 1; y < P.length; y++) {
              const i1 = P[y] - P[x];
              const i2 = N[y] - N[x];
              const kind = pc(i1);
              if (i1 !== i2 || (kind !== 7 && !(kind === 0 && i1 > 0))) continue;
              const mx = N[x] - P[x];
              if (mx === 0 || Math.sign(mx) !== Math.sign(N[y] - P[y])) continue;
              par = par + 1;
              if (first < 0) first = i;
            }
          }
        }
        if (par > 0) {
          note(
            a,
            "parallel-fifths",
            "info",
            `${plural(par, "parallel fifth or octave", "parallel fifths and octaves")} in ${ch.name}`,
            `Voices move in parallel perfect intervals (first at beat ${r3(big[first].start)}). Fine for power chords and synth stabs; avoid it when the voices should sound independent.`,
            atPart(part, big[first].idx.concat(big[first - 1].idx))
          );
        }
      }

      // Upper voices more than an octave apart.
      /** const gappy: Chord[] */
      const gappy = [];
      for (const c of big) {
        for (let j = 1; j + 1 < c.pitches.length; j++) if (c.pitches[j + 1] - c.pitches[j] > 12) gappy.push(c);
      }
      const gaps = gappy.filter((c, i) => gappy.indexOf(c) === i);
      if (gaps.length > 0) {
        /** const moves: Int[] */
        const moves = [];
        /** const idx: Int[] */
        const idx = [];
        for (const c of gaps) {
          const v = c.pitches.slice();
          for (let j = 1; j + 1 < v.length; j++) {
            let drop = 0;
            while (v[j + 1] - drop - v[j] > 12) drop = drop + 12;
            for (let q = j + 1; q < v.length; q++) v[q] = v[q] - drop;
          }
          for (let j = 0; j < v.length; j++) {
            idx.push(c.idx[j]);
            moves.push(v[j] - c.pitches[j]);
          }
        }
        add(
          a,
          "wide-spacing",
          "info",
          `${plural(gaps.length, "chord has", "chords have")} a gap over an octave in the upper voices`,
          `${ch.name} leaves more than an octave between adjacent upper voices; close voicings blend better.`,
          atPart(part, idx),
          "Close the gaps",
          (p) =>
            editNotes(p, pid, idx, (n, k) => {
              n.pitch = n.pitch + moves[k];
            })
        );
      }
    }
  }
}

/** Notes outside the key, part by part (when the song as a whole is clearly in it). */
/** function checkKey(a: Ana) => Undefined */
function checkKey(a) {
  if (a.keyLabel === "" || a.fit < 0.6) return undefined;
  const steps = keySteps(a.tonic, a.minor);
  /** const flagged: { part: Part, idx: Int[] }[] */
  const flagged = [];
  let total = 0;
  let outside = 0;
  for (const part of a.parts) {
    const ch = part.ch;
    if (!ch.pitched || ch.role === "drums") continue;
    /** const idx: Int[] */
    const idx = [];
    for (const i of part.idx) {
      total = total + 1;
      if (!steps.includes(pc(Math.round(part.pat.notes[i].pitch) + ch.shift))) idx.push(i);
    }
    outside = outside + idx.length;
    if (idx.length > 0 && (idx.length <= 2 || idx.length / part.idx.length <= 0.05)) flagged.push({ part: part, idx: idx });
  }
  // Lots of chromatic notes: a style, not slips.
  if (total === 0 || outside / total > 0.08) return undefined;
  for (const f of flagged) {
    const part = f.part;
    const pid = part.pat.id;
    const idx = f.idx;
    const sh = part.ch.shift;
    const names = idx.slice(0, 4).map((i) => noteName(Math.round(part.pat.notes[i].pitch) + sh));
    add(
      a,
      "out-of-key",
      "warn",
      `${plural(idx.length, "note is", "notes are")} outside ${a.keyLabel}`,
      `${part.ch.name}: ${names.join(", ")}${idx.length > 4 ? ", …" : ""}. ${a.keyFromScore ? "The score is in" : "The song reads as"} ${a.keyLabel}; the rest of the song stays in it.`,
      atPart(part, idx),
      "Snap them to the nearest note of the key",
      (p) =>
        editNotes(p, pid, idx, (n, k) => {
          const s = Math.round(n.pitch) + sh;
          // The nearer neighbour in the key, the lower one on a tie.
          if (steps.includes(pc(s - 1))) n.pitch = Math.round(n.pitch) - 1;
          else if (steps.includes(pc(s + 1))) n.pitch = Math.round(n.pitch) + 1;
        })
    );
  }
}

/** The notes to compare across parts: the song's, or each pattern's when nothing is on the playlist. */
/** function scopes(a: Ana) => SNote[][] */
function scopes(a) {
  if (a.song.length > 0) return [a.song];
  /** const out: SNote[][] */
  const out = [];
  for (const pat of a.p.patterns) {
    /** const list: SNote[] */
    const list = [];
    for (let i = 0; i < pat.notes.length; i++) {
      const n = pat.notes[i];
      const ch = chanOf(a, n.channel);
      if (!ch || n.start >= pat.length) continue;
      list.push({
        pitch: Math.round(n.pitch) + ch.shift,
        start: n.start,
        end: Math.min(pat.length, n.start + n.length),
        velocity: n.velocity,
        channel: n.channel,
        pattern: pat.id,
        index: i,
      });
    }
    list.sort((x, y) => x.start - y.start);
    if (list.length > 0) out.push(list);
  }
  return out;
}

/** The element a channel belongs to for counting parts: its layer's source ("" = not counted). */
/** function groupOf(a: Ana, id: String) => String */
function groupOf(a, id) {
  const c = chanOf(a, id);
  if (!c) return "";
  if (c.layerOf !== "" && chanOf(a, c.layerOf)) return c.layerOf;
  return c.id;
}

/** type Pair = { a: String, b: String, beats: Number, at: Number } */

/** Sum the overlaps of notes that `pick` keeps, by pair of channel groups, when `clash` says they clash. */
/** function overlaps(a: Ana, notes: SNote[], pick: (SNote) => Boolean, clash: (SNote, SNote) => Boolean) => Pair[] */
function overlaps(a, notes, pick, clash) {
  /** const pairs: Pair[] */
  const pairs = [];
  /** let active: SNote[] */
  let active = [];
  for (const n of notes) {
    if (!pick(n)) continue;
    active = active.filter((m) => m.end > n.start + 1e-9);
    const g = groupOf(a, n.channel);
    for (const m of active) {
      const h = groupOf(a, m.channel);
      if (g === h || !clash(m, n)) continue;
      const ov = Math.min(m.end, n.end) - n.start;
      if (ov <= 0) continue;
      const x = g < h ? g : h;
      const y = g < h ? h : g;
      const pr = pairs.find((q) => q.a === x && q.b === y);
      if (pr) pr.beats = pr.beats + ov;
      else pairs.push({ a: x, b: y, beats: ov, at: n.start });
    }
    active.push(n);
  }
  pairs.sort((x, y) => y.beats - x.beats);
  return pairs;
}

/** function where0(a: Ana, notes: SNote[], beat: Number) => Where */
function where0(a, notes, beat) {
  if (a.song.length > 0) return atSong(a, beat, -1);
  const pat = notes.length > 0 ? findPattern(a.p, notes[0].pattern) : undefined;
  return pat ? atPattern(pat) : atProject("");
}

/** function nameOf(a: Ana, id: String) => String */
function nameOf(a, id) {
  const c = chanOf(a, id);
  return c ? c.name : id;
}

/** function checkClashes(a: Ana) => Undefined */
function checkClashes(a) {
  for (const notes of scopes(a)) {
    const melodic = (n) => {
      const c = chanOf(a, n.channel);
      return c !== undefined && c.pitched && c.role !== "drums" && c.role !== "fx";
    };
    const semis = overlaps(a, notes, melodic, (m, n) => [1, 13, 25].includes(Math.abs(m.pitch - n.pitch)) && Math.min(m.end, n.end) - n.start >= 1);
    for (const pr of semis.slice(0, 3)) {
      if (pr.beats < 2) continue;
      note(
        a,
        "semitone-clash",
        "info",
        `${nameOf(a, pr.a)} and ${nameOf(a, pr.b)} hold notes a semitone apart`,
        `For ${r3(pr.beats)} beats in all they sustain minor 2nds or 9ns against each other (first around bar ${barOf(a, pr.at) + 1}). Move one of them a semitone, or keep the clash short.`,
        where0(a, notes, pr.at)
      );
    }
    const low = (n) => {
      const c = chanOf(a, n.channel);
      return c !== undefined && c.pitched && c.role !== "drums" && c.role !== "fx" && n.pitch < 43;
    };
    const subs = overlaps(a, notes, low, (m, n) => true);
    for (const pr of subs.slice(0, 2)) {
      if (pr.beats < 4) continue;
      note(
        a,
        "low-crowding",
        "warn",
        `${nameOf(a, pr.a)} and ${nameOf(a, pr.b)} share the sub`,
        `Both play under G2 at the same time for ${r3(pr.beats)} beats (first around bar ${barOf(a, pr.at) + 1}). Give the low end to one and move the other up an octave.`,
        where0(a, notes, pr.at)
      );
    }
  }
}

// ------------------------------------------------------------------ melody

/** type Line = { pitch: Int, start: Number, end: Number, i: Int } */

/** A part's top line: its highest note at each onset. */
/** function lineOf(a: Ana, part: Part) => Line[] */
function lineOf(a, part) {
  return chordsOf(a, part).map((c) => {
    const k = c.pitches.length - 1;
    const n = part.pat.notes[c.idx[k]];
    return { pitch: c.pitches[k], start: c.start, end: n.start + n.length, i: c.idx[k] };
  });
}

// Real instruments' ranges (sounding MIDI pitches) by General MIDI program.
/** const RANGES: { name: String, lo: Int, hi: Int }[] */
const RANGES = [
  { name: "Acoustic Grand Piano", lo: 21, hi: 108 },
  { name: "Bright Acoustic Piano", lo: 21, hi: 108 },
  { name: "Electric Grand Piano", lo: 21, hi: 108 },
  { name: "Honky-tonk Piano", lo: 21, hi: 108 },
  { name: "Electric Piano 1", lo: 28, hi: 103 },
  { name: "Electric Piano 2", lo: 28, hi: 103 },
  { name: "Harpsichord", lo: 29, hi: 89 },
  { name: "Clavinet", lo: 29, hi: 89 },
  { name: "Celesta", lo: 60, hi: 108 },
  { name: "Glockenspiel", lo: 79, hi: 108 },
  { name: "Vibraphone", lo: 53, hi: 89 },
  { name: "Marimba", lo: 45, hi: 96 },
  { name: "Xylophone", lo: 65, hi: 108 },
  { name: "Tubular Bells", lo: 60, hi: 77 },
  { name: "Harmonica", lo: 60, hi: 84 },
  { name: "Acoustic Guitar (nylon)", lo: 40, hi: 83 },
  { name: "Acoustic Guitar (steel)", lo: 40, hi: 86 },
  { name: "Electric Guitar (jazz)", lo: 40, hi: 88 },
  { name: "Electric Guitar (clean)", lo: 40, hi: 88 },
  { name: "Electric Guitar (muted)", lo: 40, hi: 88 },
  { name: "Overdriven Guitar", lo: 40, hi: 88 },
  { name: "Distortion Guitar", lo: 40, hi: 88 },
  { name: "Acoustic Bass", lo: 28, hi: 67 },
  { name: "Electric Bass (finger)", lo: 28, hi: 67 },
  { name: "Electric Bass (pick)", lo: 28, hi: 67 },
  { name: "Fretless Bass", lo: 28, hi: 67 },
  { name: "Slap Bass 1", lo: 28, hi: 67 },
  { name: "Slap Bass 2", lo: 28, hi: 67 },
  { name: "Violin", lo: 55, hi: 103 },
  { name: "Viola", lo: 48, hi: 91 },
  { name: "Cello", lo: 36, hi: 84 },
  { name: "Contrabass", lo: 28, hi: 67 },
  { name: "Orchestral Harp", lo: 23, hi: 104 },
  { name: "Timpani", lo: 38, hi: 62 },
  { name: "Choir Aahs", lo: 40, hi: 81 },
  { name: "Voice Oohs", lo: 40, hi: 81 },
  { name: "Trumpet", lo: 52, hi: 86 },
  { name: "Muted Trumpet", lo: 52, hi: 82 },
  { name: "Trombone", lo: 40, hi: 77 },
  { name: "Tuba", lo: 26, hi: 65 },
  { name: "French Horn", lo: 35, hi: 77 },
  { name: "Soprano Sax", lo: 56, hi: 88 },
  { name: "Alto Sax", lo: 49, hi: 81 },
  { name: "Tenor Sax", lo: 44, hi: 76 },
  { name: "Baritone Sax", lo: 36, hi: 69 },
  { name: "Oboe", lo: 58, hi: 93 },
  { name: "English Horn", lo: 52, hi: 81 },
  { name: "Bassoon", lo: 34, hi: 75 },
  { name: "Clarinet", lo: 50, hi: 94 },
  { name: "Piccolo", lo: 74, hi: 108 },
  { name: "Flute", lo: 60, hi: 96 },
  { name: "Recorder", lo: 72, hi: 98 },
];

/** function checkMelody(a: Ana) => Undefined */
function checkMelody(a) {
  const bar = Math.max(1, a.p.transport.beatsPerBar);
  for (const part of a.parts) {
    const ch = part.ch;
    const pid = part.pat.id;

    // Real instruments' ranges.
    if (ch.program !== "") {
      const r = RANGES.find((x) => x.name === ch.program);
      if (r) {
        const idx = part.idx.filter((i) => {
          const s = Math.round(part.pat.notes[i].pitch) + ch.shift;
          return s < r.lo || s > r.hi;
        });
        if (idx.length > 0) {
          const lo = r.lo;
          const hi = r.hi;
          const sh = ch.shift;
          const outs = idx.map((i) => Math.round(part.pat.notes[i].pitch) + sh);
          const far = outs.some((x) => x < lo) ? minInt(outs) : maxInt(outs);
          add(
            a,
            "instrument-range",
            "warn",
            `${plural(idx.length, "note is", "notes are")} out of the ${ch.program.toLowerCase()}'s range`,
            `A ${ch.program.toLowerCase()} plays ${noteName(lo)}–${noteName(hi)}; ${ch.name} goes to ${noteName(far)} in ${part.pat.name}.`,
            atPart(part, idx),
            "Move them into range by octaves",
            (p) =>
              editNotes(p, pid, idx, (n, k) => {
                let s = Math.round(n.pitch) + sh;
                let d = 0;
                while (s + d < lo && s + d + 12 <= hi) d = d + 12;
                while (s + d > hi && s + d - 12 >= lo) d = d - 12;
                n.pitch = n.pitch + d;
              })
          );
        }
      }
    }

    if (ch.role !== "lead") continue;
    const line = lineOf(a, part);
    if (line.length < 4) continue;
    const ps = line.map((x) => x.pitch);
    const lo = minInt(ps);
    const hi = maxInt(ps);
    if (hi - lo > 19) {
      note(
        a,
        "melody-range",
        "info",
        `The melody spans ${hi - lo} semitones`,
        `${ch.name} runs from ${noteName(lo)} to ${noteName(hi)} — more than an octave and a half; singable melodies keep to about an octave.`,
        atPart(part, [line[ps.indexOf(lo)].i, line[ps.indexOf(hi)].i])
      );
    }
    /** const big: Int[] */
    const big = [];
    /** const loose: Int[] */
    const loose = [];
    for (let k = 1; k < line.length; k++) {
      if (line[k].start - line[k - 1].end > bar) continue;
      const iv = line[k].pitch - line[k - 1].pitch;
      if (Math.abs(iv) > 12) big.push(line[k].i);
      if (Math.abs(iv) >= 8 && k + 1 < line.length) {
        const next = line[k + 1].pitch - line[k].pitch;
        if (Math.sign(next) === Math.sign(iv) || Math.abs(next) > 4) loose.push(line[k].i);
      }
    }
    if (big.length > 0) {
      note(
        a,
        "large-leap",
        "info",
        `${plural(big.length, "leap", "leaps")} wider than an octave`,
        `${ch.name} jumps more than an octave between consecutive notes.`,
        atPart(part, big)
      );
    }
    if (loose.length >= 2) {
      note(
        a,
        "leap-recovery",
        "info",
        `${plural(loose.length, "big leap is", "big leaps are")} not balanced by a step back`,
        `In ${ch.name}, leaps of a minor 6th or more keep going (or leap again); a step back the other way rounds them off.`,
        atPart(part, loose)
      );
    }
    // The longest stretch without a rest.
    let run = 0;
    let since = line[0].start;
    for (let k = 1; k < line.length; k++) {
      if (line[k].start - line[k - 1].end > 0.124) since = line[k].start;
      run = Math.max(run, line[k].end - since);
    }
    if (run >= 8 * bar) {
      note(
        a,
        "no-rests",
        "info",
        `${ch.name} plays ${Math.round(run / bar)} bars without a rest`,
        "Phrases breathe: leave rests between them (two- and four-bar phrases are the norm).",
        atPart(part, [])
      );
    }
    const span = line[line.length - 1].end - line[0].start;
    /** const classes: Int[] */
    const classes = [];
    for (const x of ps) if (!classes.includes(pc(x))) classes.push(pc(x));
    if (line.length >= 16 && span >= 4 * bar && classes.length <= 2) {
      note(
        a,
        "monotone",
        "info",
        `${ch.name} keeps to ${plural(classes.length, "note", "notes")}`,
        `${line.length} notes over ${Math.round(span / bar)} bars use only ${plural(classes.length, "pitch class", "pitch classes")}.`,
        atPart(part, [])
      );
    }
  }
}

// ------------------------------------------------------------------ rhythm

/** Velocity with an accent for where the note falls: downbeats strongest, 16ths softest. */
/** function accented(base: Number, start: Number, bar: Number, i: Int) => Number */
function accented(base, start, bar, i) {
  const inBeat = start - Math.floor(start + 1e-6);
  const onBar = Math.abs(start / bar - Math.round(start / bar)) < 1e-4;
  let w = 0.72;
  if (inBeat < 0.01 || inBeat > 0.99) w = onBar ? 1 : 0.92;
  else if (Math.abs(inBeat - 0.5) < 0.01) w = 0.82;
  return r3(clamp(base * w + jitter(i, 3) * 0.035, 0.05, 1));
}

/** function checkRhythm(a: Ana) => Undefined */
function checkRhythm(a) {
  const bar = Math.max(1, a.p.transport.beatsPerBar);
  for (const part of a.parts) {
    const ch = part.ch;
    const notes = part.pat.notes;
    const pid = part.pat.id;
    const idx = part.idx;
    if (ch.role === "fx" || ch.role === "gen") continue;
    const vels = idx.map((i) => notes[i].velocity);
    const vmin = minOf(vels);
    const vmax = maxOf(vels);
    // Held chords and pads may keep one velocity; struck and rhythmic parts should not.
    const lens = idx.map((i) => notes[i].length).sort((x, y) => x - y);
    const rhythmic = ch.role === "drums" || lens[Math.floor(lens.length / 2)] <= 1;
    const flat = idx.length >= 8 && vmax - vmin < 0.02 && rhythmic;

    if (flat) {
      const base = vmax >= 0.95 ? 0.85 : vmax;
      add(
        a,
        "flat-velocity",
        "warn",
        `All ${idx.length} notes at velocity ${Math.round(vmax * 127)}`,
        ch.role === "drums"
          ? `${ch.name} has no dynamics: accent the downbeats and soften the off-beats (a kick on the one about 100–110, ghost notes 40–60).`
          : `${ch.name} has no dynamics: a player leans into the strong beats and plays the passing notes softer.`,
        atPart(part, idx),
        "Add dynamics (accents and a little variation)",
        (p) => {
          const pat = findPattern(p, pid);
          if (!pat) return undefined;
          for (const i of idx) if (i < pat.notes.length) pat.notes[i].velocity = accented(base, pat.notes[i].start, bar, i);
        }
      );
    } else if (idx.length >= 8 && vels.filter((v) => v >= 0.97).length >= 0.9 * idx.length) {
      add(
        a,
        "max-velocity",
        "warn",
        "Nearly every note at full velocity",
        `${ch.name} leaves no room for accents. Bring the part down and let the strong notes stand out.`,
        atPart(part, idx),
        "Scale the velocities to 80%",
        (p) =>
          editNotes(p, pid, idx, (n, k) => {
            n.velocity = r3(n.velocity * 0.8);
          })
      );
    }

    if (ch.role === "drums" && !flat) {
      // Runs of 8+ fast hits at one velocity.
      /** const runs: Int[] */
      const runs = [];
      let s = 0;
      for (let k = 1; k <= idx.length; k++) {
        const same =
          k < idx.length &&
          Math.abs(notes[idx[k]].velocity - notes[idx[s]].velocity) < 0.01 &&
          notes[idx[k]].start - notes[idx[k - 1]].start <= 0.26 &&
          notes[idx[k]].pitch === notes[idx[s]].pitch;
        if (same) continue;
        if (k - s >= 8) for (let q = s; q < k; q++) runs.push(idx[q]);
        s = k;
      }
      if (runs.length > 0) {
        add(
          a,
          "machine-gun",
          "info",
          `${runs.length} fast hits at one velocity`,
          `${ch.name} has runs of eight or more 16ths that all hit equally hard.`,
          atPart(part, runs),
          "Alternate strong and weak hits",
          (p) => {
            const pat = findPattern(p, pid);
            if (!pat) return undefined;
            for (const i of runs) if (i < pat.notes.length) pat.notes[i].velocity = accented(Math.min(0.9, pat.notes[i].velocity), pat.notes[i].start, bar, i);
          }
        );
      }
      const snare = ch.kind === "snare" || ch.kind === "hat" || (ch.type === "soundfont" && idx.some((i) => [38, 40, 42].includes(Math.round(notes[i].pitch))));
      if (snare && idx.length >= 8 && vmin >= 0.6) {
        note(
          a,
          "no-ghost-notes",
          "info",
          `No ghost notes in ${ch.name}`,
          `Every hit is at velocity ${Math.round(vmin * 127)} or more; soft in-between hits (velocity 40–60) add groove.`,
          atPart(part, [])
        );
      }
    }

    // Timing.
    const onGrid = (t) => Math.abs(t * 4 - Math.round(t * 4)) < 1e-4;
    const starts = idx.map((i) => notes[i].start);
    const gridded = starts.filter(onGrid).length;
    const live = ch.program !== "" && RANGES.some((r) => r.name === ch.program);
    if (live && idx.length >= 16 && gridded === idx.length) {
      const beatsPerMs = a.p.transport.bpm / 60000;
      add(
        a,
        "rigid-timing",
        "info",
        `${ch.name} is exactly on the grid`,
        `A ${ch.program.toLowerCase()} played by a person drifts by a few milliseconds around the beat. Humanizing nudges each note by up to ±8 ms.`,
        atPart(part, []),
        "Humanize the timing (±8 ms)",
        (p) =>
          editNotes(p, pid, idx, (n, k) => {
            const d = jitter(idx[k], 7) * 8 * beatsPerMs;
            const s = Math.max(0, n.start + d);
            n.length = Math.max(0.01, n.length - (s - n.start));
            n.start = r3(s);
          })
      );
    }
    if (idx.length >= 8 && gridded >= 0.8 * idx.length && gridded < idx.length) {
      const slips = idx.filter((i) => {
        const t = notes[i].start;
        const d = Math.abs(t * 4 - Math.round(t * 4)) / 4;
        return d > 1e-4 && d <= 0.03;
      });
      if (slips.length > 0 && slips.length + gridded === idx.length) {
        add(
          a,
          "sloppy-timing",
          "info",
          `${plural(slips.length, "note is", "notes are")} a hair off the grid`,
          `${ch.name} is otherwise quantized to 16ths; these land up to 3% of a beat away from it.`,
          atPart(part, slips),
          "Snap them to the grid",
          (p) =>
            editNotes(p, pid, slips, (n, k) => {
              n.start = Math.round(n.start * 4) / 4;
            })
        );
      }
    }

    // Hygiene: overlaps, duplicates, silent, tiny and unreachable notes.
    /** const dupes: Int[] */
    const dupes = [];
    /** const trims: Int[] */
    const trims = [];
    /** const trimTo: Number[] */
    const trimTo = [];
    const byPitch = idx.slice().sort((x, y) => notes[x].pitch - notes[y].pitch || notes[x].start - notes[y].start || notes[y].velocity - notes[x].velocity);
    for (let k = 1; k < byPitch.length; k++) {
      const m = notes[byPitch[k - 1]];
      const n = notes[byPitch[k]];
      if (m.pitch !== n.pitch) continue;
      if (Math.abs(n.start - m.start) < 0.005) dupes.push(byPitch[k]);
      else if (ch.role !== "drums" && n.start < m.start + m.length - 1e-6) {
        trims.push(byPitch[k - 1]);
        trimTo.push(n.start - m.start);
      }
    }
    if (dupes.length > 0) {
      add(
        a,
        "duplicate-notes",
        "warn",
        `${plural(dupes.length, "duplicate note", "duplicate notes")}`,
        `${ch.name} has notes stacked on top of identical ones.`,
        atPart(part, dupes),
        "Remove the duplicates",
        (p) => dropNotes(p, pid, dupes)
      );
    }
    if (trims.length > 0) {
      add(
        a,
        "same-pitch-overlap",
        "warn",
        `${plural(trims.length, "note overlaps", "notes overlap")} the next note of the same pitch`,
        `In ${ch.name} a note is still sounding when the same pitch starts again.`,
        atPart(part, trims),
        "Trim each to where the next begins",
        (p) =>
          editNotes(p, pid, trims, (n, k) => {
            n.length = trimTo[k];
          })
      );
    }
    const silent = idx.filter((i) => notes[i].velocity <= 0.01);
    if (silent.length > 0) {
      add(
        a,
        "silent-notes",
        "warn",
        `${plural(silent.length, "silent note", "silent notes")}`,
        `${ch.name} has notes at velocity 0.`,
        atPart(part, silent),
        "Remove them",
        (p) => dropNotes(p, pid, silent)
      );
    }
    if (ch.pitched) {
      const tiny = idx.filter((i) => notes[i].length < 1 / 16);
      if (tiny.length > 0) {
        add(
          a,
          "tiny-notes",
          "info",
          `${plural(tiny.length, "very short note", "very short notes")}`,
          `${ch.name} has notes shorter than a 64th — usually accidental clicks.`,
          atPart(part, tiny),
          "Lengthen them to a 16th",
          (p) =>
            editNotes(p, pid, tiny, (n, k) => {
              n.length = 0.25;
            })
        );
      }
    }
    const late = idx.filter((i) => notes[i].start >= part.pat.length - 1e-9);
    if (late.length > 0) {
      add(
        a,
        "past-end",
        "warn",
        `${plural(late.length, "note starts", "notes start")} after the pattern ends`,
        `${part.pat.name} is ${part.pat.length} beats long; these notes of ${ch.name} never play.`,
        atPart(part, late),
        "Remove them",
        (p) => dropNotes(p, pid, late)
      );
    }
  }

  // Swing with nothing on the off-beat 16ths.
  const sw = a.p.transport.swing;
  if (sw > 0.02 && a.parts.length > 0) {
    const odd = a.parts.some((part) =>
      part.idx.some((i) => {
        const f = part.pat.notes[i].start * 4;
        return Math.abs(f - Math.round(f)) < 0.05 && Math.abs(Math.round(f)) % 2 === 1;
      })
    );
    if (!odd) {
      note(
        a,
        "swing-unused",
        "info",
        "Swing has nothing to swing",
        `Swing is at ${Math.round(sw * 100)}%, but no note falls on an off-beat 16th, which is what it delays.`,
        atProject("Transport")
      );
    }
  }
}

// ------------------------------------------------------------------ arrangement

/** For each bar: the channel groups that sound in it (drums count as one; transitions not at all). */
/** function barElements(a: Ana) => String[][] */
function barElements(a) {
  /** const out: String[][] */
  const out = a.bars.map((b) => []);
  for (const n of a.song) {
    const c = chanOf(a, n.channel);
    if (!c || c.role === "fx") continue;
    const g = c.role === "drums" ? "drums" : groupOf(a, n.channel);
    const b0 = barOf(a, n.start);
    const b1 = barOf(a, Math.max(n.start, n.end - 1e-6));
    for (let b = b0; b <= b1; b++) if (!out[b].includes(g)) out[b].push(g);
  }
  return out;
}

/** A fingerprint of what plays in each bar (relative to the bar). */
/** function barPrints(a: Ana) => String[] */
function barPrints(a) {
  /** const parts: String[][] */
  const parts = a.bars.map((b) => []);
  for (const n of a.song) {
    const b = barOf(a, n.start);
    parts[b].push(`${n.channel}/${n.pitch}/${r3(n.start - a.bars[b].start)}/${r3(n.end - n.start)}`);
  }
  return parts.map((x) => x.sort().join(";"));
}

/** function checkArrangement(a: Ana) => Undefined */
function checkArrangement(a) {
  const p = a.p;
  const used = p.patterns.filter((pat) => pat.notes.length > 0);
  if (p.playlist.clips.length === 0) {
    if (used.length > 0) {
      add(
        a,
        "empty-playlist",
        "warn",
        "The playlist is empty",
        `${plural(used.length, "pattern has", "patterns have")} notes, but nothing is placed in the song, so the song plays silence.`,
        atProject("Playlist"),
        "Lay the patterns out one after another",
        (q) => {
          if (q.playlist.tracks.length === 0) q.playlist.tracks.push({ name: "Track 1", mute: false });
          let at = 0;
          for (const pat of q.patterns) {
            if (pat.notes.length === 0) continue;
            q.playlist.clips.push({ pattern: pat.id, sample: "", track: trackIx(0), start: at, length: pat.length, offset: 0, gain: 1, mixer: insertIx(0) });
            at = at + pat.length;
          }
        }
      );
    }
    return undefined;
  }

  // Patterns and clips.
  for (const pat of used) {
    if (!p.playlist.clips.some((c) => c.pattern === pat.id))
      note(a, "unused-pattern", "info", `"${pat.name}" is never placed`, "Its notes are not part of the song.", atPattern(pat));
  }
  /** const empty: Int[] */
  const empty = [];
  for (let i = 0; i < p.playlist.clips.length; i++) {
    const c = p.playlist.clips[i];
    const pat = c.pattern !== "" ? findPattern(p, c.pattern) : undefined;
    if (pat && pat.notes.length === 0) empty.push(i);
  }
  if (empty.length > 0) {
    add(
      a,
      "empty-clips",
      "info",
      `${plural(empty.length, "clip plays", "clips play")} an empty pattern`,
      "They play nothing.",
      atSong(a, p.playlist.clips[empty[0]].start, empty[0]),
      "Remove them",
      (q) => {
        q.playlist.clips = q.playlist.clips.filter((c, i) => !empty.includes(i));
      }
    );
  }
  // Identical patterns.
  /** const same: String[] */
  const same = p.patterns.map((pat) =>
    pat.notes.length === 0
      ? ""
      : `${pat.length}|` +
        pat.notes
          .map((n) => `${n.channel}/${n.pitch}/${r3(n.start)}/${r3(n.length)}/${r3(n.velocity)}`)
          .sort()
          .join(";")
  );
  for (let j = 0; j < p.patterns.length; j++) {
    if (same[j] === "") continue;
    const i = same.indexOf(same[j]);
    if (i === j) continue;
    const keep = p.patterns[i].id;
    const dup = p.patterns[j].id;
    add(
      a,
      "identical-patterns",
      "info",
      `"${p.patterns[j].name}" is a copy of "${p.patterns[i].name}"`,
      "Two patterns with the same notes have to be edited twice.",
      atPattern(p.patterns[j]),
      `Use "${p.patterns[i].name}" in its place`,
      (q) => {
        for (const c of q.playlist.clips) if (c.pattern === dup) c.pattern = keep;
        if (!q.score.marks.some((m) => m.pattern === dup)) q.patterns = q.patterns.filter((x) => x.id !== dup);
      }
    );
  }
  // Overlapping clips on one track, and clips a little off the bar.
  const clips = p.playlist.clips.map((c, i) => i).filter((i) => p.playlist.clips[i].pattern !== "");
  clips.sort((x, y) => trackIndex(p.playlist.clips[x].track) - trackIndex(p.playlist.clips[y].track) || p.playlist.clips[x].start - p.playlist.clips[y].start);
  for (let k = 1; k < clips.length; k++) {
    const m = p.playlist.clips[clips[k - 1]];
    const n = p.playlist.clips[clips[k]];
    if (m.track !== n.track || n.start >= m.start + m.length - 0.01) continue;
    const ci = clips[k - 1];
    const len = n.start - m.start;
    if (len <= 0) continue;
    add(
      a,
      "clip-overlap",
      "info",
      "Two clips overlap on one track",
      `On track ${trackIndex(m.track) + 1}, a clip of "${m.pattern}" still plays when one of "${n.pattern}" starts (for ${r3(m.start + m.length - n.start)} beats).`,
      atSong(a, n.start, clips[k]),
      "Trim the first one",
      (q) => {
        const c = q.playlist.clips[ci];
        if (c) c.length = r3(len);
      }
    );
  }
  for (let i = 0; i < p.playlist.clips.length; i++) {
    const c = p.playlist.clips[i];
    if (a.bars.length === 0) break;
    const b = a.bars[barOf(a, c.start)];
    const next = b.start + b.length;
    const target = c.start - b.start <= next - c.start ? b.start : next;
    const d = Math.abs(c.start - target);
    if (d > 0.01 && d <= 0.5) {
      add(
        a,
        "clip-off-bar",
        "info",
        `A clip starts ${r3(d)} beats off the bar`,
        `The clip of "${c.pattern !== "" ? c.pattern : c.sample}" on track ${trackIndex(c.track) + 1} is just off bar ${barOf(a, target) + 1}.`,
        atSong(a, c.start, i),
        "Snap it to the bar",
        (q) => {
          const x = q.playlist.clips[i];
          if (x) x.start = target;
        }
      );
    }
  }

  if (a.bars.length === 0 || a.song.length === 0) return undefined;
  const nb = a.bars.length;
  const els = barElements(a);
  const counts = els.map((e) => e.length);
  const max = maxInt(counts);
  if (nb <= 16 && max > 0) {
    note(
      a,
      "short-song",
      "info",
      `The song is only ${plural(nb, "bar", "bars")} long`,
      "It is a loop so far: build an intro, sections that contrast and an ending.",
      atSong(a, 0, -1)
    );
  }

  // Loopitis: the same bars repeating with a period of 1, 2, 4 or 8 bars.
  const prints = barPrints(a);
  let bestRun = 0;
  let bestAt = 0;
  let bestPeriod = 1;
  for (const k of [1, 2, 4, 8]) {
    let s = k;
    for (let i = k; i <= nb; i++) {
      const same = i < nb && prints[i] !== "" && prints[i] === prints[i - k];
      if (same) continue;
      const run = i - s + k;
      if (i > s && run > bestRun) {
        bestRun = run;
        bestAt = s - k;
        bestPeriod = k;
      }
      s = i + 1;
    }
  }
  if (bestRun >= 24) {
    note(
      a,
      "loopitis",
      bestRun >= 32 ? "warn" : "info",
      `The same ${plural(bestPeriod, "bar repeats", "bars repeat")} for ${bestRun} bars`,
      `From bar ${bestAt + 1}, ${bestPeriod === 1 ? "one bar plays" : `a ${bestPeriod}-bar loop plays`} unchanged for ${bestRun} bars. Add or take away a part every 4–8 bars, or vary a fill.`,
      atSong(a, a.bars[bestAt].start, -1)
    );
  }

  // Contrast between 8-bar blocks.
  if (nb >= 24 && max >= 3) {
    /** const blocks: Int[] */
    const blocks = [];
    for (let b = 0; b < nb; b = b + 8) {
      /** const set: String[] */
      const set = [];
      for (let q = b; q < Math.min(nb, b + 8); q++) for (const g of els[q]) if (!set.includes(g)) set.push(g);
      if (set.length > 0) blocks.push(set.length);
    }
    if (blocks.length >= 3 && maxInt(blocks) - minInt(blocks) < 2) {
      note(
        a,
        "no-contrast",
        "info",
        "The density never changes",
        `Every 8-bar section has ${minInt(blocks)}–${maxInt(blocks)} parts playing. Drop parts out for a breakdown so the full sections hit harder.`,
        atSong(a, 0, -1)
      );
    }
  }

  // Too many parts at once.
  const crowded = counts.map((c, i) => i).filter((i) => counts[i] > 6);
  if (crowded.length > 0) {
    const b = crowded[0];
    note(
      a,
      "too-many-elements",
      "info",
      `Up to ${max} parts play at once`,
      `${plural(crowded.length, "bar has", "bars have")} more than six parts (drums counted as one), first bar ${b + 1}: ${els[b].map((g) => (g === "drums" ? "drums" : nameOf(a, g))).join(", ")}. Keep the focus on rhythm, a melody and one wildcard.`,
      atSong(a, a.bars[b].start, -1)
    );
  }

  // Entrances and endings.
  if (nb >= 16 && max >= 4) {
    const first = counts.findIndex((c) => c > 0);
    if (first >= 0 && counts[first] >= Math.ceil(max * 0.8)) {
      note(
        a,
        "full-intro",
        "info",
        "Everything enters at once",
        `Bar ${first + 1} already has ${counts[first]} of the song's ${max} parts; an intro that builds gives the song somewhere to go.`,
        atSong(a, a.bars[first].start, -1)
      );
    }
    let last = nb - 1;
    while (last > 0 && counts[last] === 0) last = last - 1;
    const tail = a.bars[Math.max(0, last - 3)].start;
    const automatedEnd = p.automation.some((l) => l.points.some((pt) => pt.beat >= tail));
    if (counts[last] >= Math.ceil(max * 0.8) && !automatedEnd) {
      note(
        a,
        "abrupt-ending",
        "info",
        "The song stops at full density",
        `The last bar still has ${counts[last]} parts playing. Thin it out, add an outro or fade it with volume automation.`,
        atSong(a, a.bars[last].start, -1)
      );
    }
  }
  if (nb >= 32 && p.automation.length === 0) {
    note(
      a,
      "no-movement",
      "info",
      "No automation",
      "Filter sweeps and volume rides into new sections carry the energy of a song; nothing moves here.",
      atSong(a, 0, -1)
    );
  }
}

// ------------------------------------------------------------------ low end

/** Channels routed to an insert. */
/** function feeding(a: Ana, i: Int) => ChInfo[] */
function feeding(a, i) {
  return a.chans.filter((c) => c.insert === i);
}

/** function isKick(c: ChInfo) => Boolean */
function isKick(c) {
  return c.role === "drums" && c.kind === "kick";
}

/** function lowRole(c: ChInfo) => Boolean */
function lowRole(c) {
  return c.role === "bass" || isKick(c);
}

/** function checkLowEnd(a: Ana) => Undefined */
function checkLowEnd(a) {
  const p = a.p;
  for (const part of a.parts) {
    const ch = part.ch;
    if (!ch.pitched) continue;
    const low = part.idx.filter((i) => Math.round(part.pat.notes[i].pitch) + ch.shift < 28);
    if (low.length === 0) continue;
    const pid = part.pat.id;
    const lowest = minInt(low.map((i) => Math.round(part.pat.notes[i].pitch) + ch.shift));
    add(
      a,
      "sub-too-low",
      lowest < 24 ? "warn" : "info",
      `${plural(low.length, "note is", "notes are")} below E1`,
      `${ch.name} goes down to ${noteName(lowest)} (${Math.round(hz(lowest))} Hz), lower than most speakers reproduce.`,
      atPart(part, low),
      "Move them up an octave",
      (q) =>
        editNotes(q, pid, low, (n, k) => {
          n.pitch = n.pitch + 12;
        })
    );
  }

  // Kick against sustained bass.
  const kicks = a.chans.filter(isKick);
  const basses = a.chans.filter((c) => c.role === "bass");
  for (const notes of scopes(a)) {
    const hits = notes.filter((n) => kicks.some((k) => k.id === n.channel));
    if (hits.length < 8 || basses.length === 0) continue;
    const bassNotes = notes.filter((n) => basses.some((b) => b.id === n.channel));
    const ringing = hits.filter((h) => bassNotes.some((b) => b.start < h.start - 0.05 && b.end > h.start + 0.1)).length;
    const ducked = p.automation.some((l) => basses.some((b) => l.target === `channel/${b.id}/volume` || l.target === `insert/${b.insert}/volume`));
    if (ringing >= 0.5 * hits.length && !ducked) {
      note(
        a,
        "kick-bass",
        "info",
        "The bass rings through the kick",
        `On ${ringing} of ${hits.length} kicks a bass note is already sounding. Shorten the bass around the kick, or duck it under each hit.`,
        where0(a, notes, hits[0].start)
      );
    }
  }

  for (const c of a.chans) {
    if (!lowRole(c) || c.count === 0) continue;
    const id = c.id;
    if (Math.abs(c.pan) > 0.1) {
      add(
        a,
        "lowend-panned",
        "warn",
        `${c.name} is panned ${c.pan < 0 ? "left" : "right"}`,
        `${c.role === "bass" ? "A bass" : "A kick"} belongs in the center.`,
        atChannel(c),
        "Center it",
        (q) => {
          const x = findChannel(q, id);
          if (x) x.pan = 0;
        }
      );
    }
    if (c.role !== "bass") continue;
    const ch = findChannel(p, id);
    if (!ch) continue;
    const inst = ch.instrument;
    if (inst.type === "wavetable" && param(inst, "unison", 3) > 1 && param(inst, "width", 0.8) > 0.3) {
      add(
        a,
        "lowend-wide",
        "warn",
        `${c.name} is a wide unison bass`,
        `Its unison voices spread ${Math.round(param(inst, "width", 0.8) * 100)}% across the stereo field; the low end smears and thins out in mono.`,
        atChannel(c),
        "Narrow the unison to 15%",
        (q) => {
          const x = findChannel(q, id);
          if (x) setParam(x.instrument, "width", 0.15);
        }
      );
    } else if (inst.type === "analog" && param(inst, "unison", 1) > 1) {
      note(
        a,
        "lowend-wide",
        "info",
        `${c.name} uses stereo unison`,
        "Detuned unison copies spread across the stereo field and phase in mono; keep the sub mono (a mono sub layer under the unison works).",
        atChannel(c)
      );
    }
    const ins = p.mixer.inserts[c.insert];
    if (c.insert > 0 && ins && ins.effects.some((e) => e.enabled && (e.type === "chorus" || e.type === "phaser"))) {
      note(a, "lowend-wide", "info", `Modulation on ${c.name}'s insert`, "A chorus or phaser on a bass widens and smears the low end.", atInsert(p, c.insert));
    }
    if (inst.type === "fm") {
      const odd = [1, 2, 3, 4, 5, 6].filter((k) => {
        const key = `op${k}Ratio`;
        if (!hasParam(inst, key) || param(inst, `op${k}Level`, 1) <= 0.01) return false;
        const r = param(inst, key, 1);
        return Math.abs(r - Math.round(r)) > 0.02 && Math.abs(r * 2 - Math.round(r * 2)) > 0.04;
      });
      if (odd.length > 0) {
        note(
          a,
          "fm-bass",
          "info",
          `${c.name} has inharmonic FM ratios`,
          `Operator ${odd.join(", ")} ${odd.length === 1 ? "has a" : "have"} non-integer ratio${odd.length === 1 ? "" : "s"}: the bass's pitch turns vague. Keep one clean carrier at 1:1 for the sub.`,
          atChannel(c)
        );
      }
    }
  }

  // Inserts that carry only kick and bass: no reverb.
  for (let i = 1; i < p.mixer.inserts.length; i++) {
    const src = feeding(a, i).filter((c) => c.count > 0);
    if (src.length === 0 || !src.every(lowRole)) continue;
    const ins = p.mixer.inserts[i];
    const k = ins.effects.findIndex((e) => e.enabled && e.type === "reverb" && param(e, "mix", 0.25) > 0.1);
    if (k < 0) continue;
    add(
      a,
      "reverb-on-bass",
      "warn",
      `Reverb on ${ins.name}`,
      `${src.map((c) => c.name).join(" and ")} go through a reverb at ${Math.round(param(ins.effects[k], "mix", 0.25) * 100)}% wet; the tail muddies the low end.`,
      atInsert(p, i),
      "Bring the reverb down to 5%",
      (q) => {
        const e = q.mixer.inserts[i] ? q.mixer.inserts[i].effects[k] : undefined;
        if (e) setParam(e, "mix", 0.05);
      }
    );
  }

  // Inserts of parts that live above the bass: a low cut.
  for (let i = 1; i < p.mixer.inserts.length; i++) {
    const src = feeding(a, i).filter((c) => c.count > 0);
    if (src.length === 0 || !src.every((c) => c.pitched && c.role !== "bass" && c.low >= 55)) continue;
    const ins = p.mixer.inserts[i];
    const cut = ins.effects.some((e) => (e.type === "filter" && optionValue(e, "mode") === "highpass") || (e.type === "eq" && param(e, "low", 0) <= -6));
    if (cut) continue;
    const lowest = minInt(src.map((c) => c.low));
    const f = Math.round(clamp(hz(lowest) / 2, 60, 250));
    add(
      a,
      "no-highpass",
      "info",
      `No low cut on ${ins.name}`,
      `Its parts play nothing under ${noteName(lowest)} (${Math.round(hz(lowest))} Hz), yet whatever they carry below that adds to the low end.`,
      atInsert(p, i),
      `Add a high-pass at ${f} Hz`,
      (q) => {
        const x = q.mixer.inserts[i];
        if (!x) return undefined;
        const d = newDevice("filter");
        setOption(d, "mode", "highpass");
        setParam(d, "cutoff", f);
        setParam(d, "resonance", 0);
        setParam(d, "mix", 1);
        x.effects.unshift(d);
      }
    );
  }
}

// ------------------------------------------------------------------ mix

/** function checkMix(a: Ana) => Undefined */
function checkMix(a) {
  const p = a.p;
  const live = a.chans.filter((c) => c.count > 0 || c.layerOf !== "");

  const hotCh = p.channels.filter((c) => c.volume > 1.001);
  const hotIns = p.mixer.inserts.filter((x, i) => i > 0 && x.volume > 1.001);
  if (hotCh.length + hotIns.length > 0) {
    const names = hotCh.map((c) => c.name).concat(hotIns.map((x) => x.name));
    add(
      a,
      "hot-faders",
      "info",
      `${plural(names.length, "fader is", "faders are")} above unity`,
      `${names.slice(0, 4).join(", ")}${names.length > 4 ? ", …" : ""} ${names.length === 1 ? "is" : "are"} pushed above 0 dB. Turning everything down together keeps the balance and wins headroom.`,
      hotCh.length > 0 ? atChannel(analyzeChannel(p, hotCh[0])) : atInsert(p, p.mixer.inserts.indexOf(hotIns[0])),
      "Pull the faders down together to unity",
      (q) => {
        const mc = maxOf(q.channels.map((c) => c.volume));
        if (mc > 1) for (const c of q.channels) c.volume = r3(c.volume / mc);
        const mi = maxOf(q.mixer.inserts.filter((x, i) => i > 0).map((x) => x.volume));
        if (mi > 1) for (let i = 1; i < q.mixer.inserts.length; i++) q.mixer.inserts[i].volume = r3(q.mixer.inserts[i].volume / mi);
      }
    );
  }

  if (live.length >= 4 && live.every((c) => Math.abs(c.volume - live[0].volume) < 1e-6)) {
    note(
      a,
      "unmixed",
      "info",
      `All ${live.length} channels at ${db(live[0].volume)}`,
      "Set a balance: start with the most important part and bring the others in around it.",
      atProject("Channel rack")
    );
  }

  const direct = a.chans.filter((c) => c.insert === 0 && c.count > 0 && c.layerOf === "");
  if (direct.length >= 3) {
    const ids = direct.map((c) => c.id);
    add(
      a,
      "not-routed",
      "info",
      `${plural(direct.length, "channel plays", "channels play")} straight into the master`,
      `${direct
        .slice(0, 5)
        .map((c) => c.name)
        .join(", ")}${direct.length > 5 ? ", …" : ""} have no insert of their own to EQ, compress or level.`,
      atProject("Mixer"),
      "Give each its own insert (drums share one)",
      (q) => {
        let drums = -1;
        for (const c of q.channels) {
          if (!ids.includes(c.id) || insertIndex(c.mixer) !== 0) continue;
          if (drumKitOf(c)) {
            if (drums < 0) {
              drums = q.mixer.inserts.length;
              q.mixer.inserts.push({ name: "Drums", volume: 1, pan: 0, mute: false, solo: false, effects: [] });
            }
            c.mixer = insertIx(drums);
          } else {
            c.mixer = insertIx(q.mixer.inserts.length);
            q.mixer.inserts.push({ name: c.name, volume: 1, pan: 0, mute: false, solo: false, effects: [] });
          }
        }
      }
    );
  }

  for (let i = 1; i < p.mixer.inserts.length; i++) {
    const ins = p.mixer.inserts[i];
    if (ins.effects.length === 0) continue;
    if (a.chans.some((c) => c.insert === i) || p.playlist.clips.some((c) => insertIndex(c.mixer) === i)) continue;
    note(
      a,
      "unused-insert",
      "info",
      `Nothing plays into ${ins.name}`,
      `It has ${plural(ins.effects.length, "effect", "effects")} but no channel or clip is routed to it.`,
      atInsert(p, i)
    );
  }

  if (p.mixer.inserts.some((x) => x.solo)) {
    const i = p.mixer.inserts.findIndex((x) => x.solo);
    add(
      a,
      "solo",
      "warn",
      `${p.mixer.inserts[i].name} is soloed`,
      "Every other insert is silent while a solo is on.",
      atInsert(p, i),
      "Turn the solos off",
      (q) => {
        for (const x of q.mixer.inserts) x.solo = false;
      }
    );
  }

  /** const muted: String[] */
  const muted = [];
  for (const c of a.chans) if (c.mute && c.count > 0) muted.push(c.name);
  for (let t = 0; t < p.playlist.tracks.length; t++) {
    const tr = p.playlist.tracks[t];
    if (tr.mute && p.playlist.clips.some((c) => trackIndex(c.track) === t)) muted.push(tr.name);
  }
  for (let i = 0; i < p.mixer.inserts.length; i++) if (p.mixer.inserts[i].mute) muted.push(p.mixer.inserts[i].name);
  if (muted.length > 0) {
    note(
      a,
      "muted",
      "info",
      `${plural(muted.length, "muted part", "muted parts")}`,
      `${muted.slice(0, 6).join(", ")}${muted.length > 6 ? ", …" : ""} won't be heard in the song or the render. Delete what you no longer need.`,
      atProject("Mute")
    );
  }

  for (let i = 0; i < p.automation.length; i++) {
    const l = p.automation[i];
    if (l.points.length < 2 || !l.points.every((pt) => Math.abs(pt.value - l.points[0].value) < 1e-6)) continue;
    const id = l.id;
    add(
      a,
      "flat-automation",
      "info",
      `"${l.name !== "" ? l.name : l.target}" never moves`,
      `All its points are at ${r3(l.points[0].value)}.`,
      atLane(l, i),
      "Remove the lane",
      (q) => {
        q.automation = q.automation.filter((x) => x.id !== id);
      }
    );
  }
}

// ------------------------------------------------------------------ stereo

/** function checkStereo(a: Ana) => Undefined */
function checkStereo(a) {
  const p = a.p;
  const center = ["kick", "snare", "clap"];
  // The lead (the busiest single line) stays in the middle with the kick, snare and bass.
  const leads = a.chans.filter((c) => c.role === "lead").sort((x, y) => y.count - x.count);
  const lead = leads.length > 0 ? leads[0].id : "";
  const spreadable = a.chans.filter((c) => c.count > 0 && c.layerOf === "" && !lowRole(c) && c.id !== lead && !center.includes(c.kind) && c.role !== "fx");
  const all = a.chans.filter((c) => c.count > 0);
  const centered = all.every((c) => Math.abs(c.pan) < 0.02) && p.mixer.inserts.every((x) => Math.abs(x.pan) < 0.02);
  if (spreadable.length >= 3 && centered) {
    const ids = spreadable.map((c) => c.id);
    add(
      a,
      "all-center",
      "info",
      "Everything is panned to the center",
      `${spreadable
        .slice(0, 5)
        .map((c) => c.name)
        .join(", ")}${spreadable.length > 5 ? ", …" : ""} could move out to the sides; kick, snare, bass and lead stay in the middle.`,
      atProject("Panning"),
      "Spread the supporting parts",
      (q) => {
        const amounts = [0.3, 0.45, 0.2, 0.55];
        for (let k = 0; k < ids.length; k++) {
          const c = findChannel(q, ids[k]);
          if (c) c.pan = (k % 2 === 0 ? -1 : 1) * amounts[Math.floor(k / 2) % amounts.length];
        }
      }
    );
  }
  const side = a.chans.filter((c) => c.count > 0 && !lowRole(c));
  if (side.length >= 3) {
    let w = 0;
    let s = 0;
    for (const c of side) {
      w = w + c.volume;
      s = s + c.volume * c.pan;
    }
    const tilt = w > 0 ? s / w : 0;
    if (Math.abs(tilt) > 0.25) {
      note(
        a,
        "lopsided",
        "info",
        `The mix leans ${tilt < 0 ? "left" : "right"}`,
        `Weighted by volume, the parts sit ${Math.round(Math.abs(tilt) * 100)}% to the ${tilt < 0 ? "left" : "right"}; balance them across both sides.`,
        atProject("Panning")
      );
    }
  }
}

// ------------------------------------------------------------------ effects

// Delay times that are note values (beats): 64ths to whole notes, dotted and triplet.
const NOTE_VALUES = [1 / 16, 1 / 12, 1 / 8, 1 / 6, 3 / 16, 1 / 4, 1 / 3, 3 / 8, 1 / 2, 2 / 3, 3 / 4, 1, 4 / 3, 3 / 2, 2, 8 / 3, 3, 4];

/** function nearestValue(t: Number) => Number */
function nearestValue(t) {
  let best = NOTE_VALUES[0];
  for (const v of NOTE_VALUES) if (Math.abs(Math.log(t / v)) < Math.abs(Math.log(t / best))) best = v;
  return best;
}

/** function checkEffects(a: Ana) => Undefined */
function checkEffects(a) {
  const p = a.p;
  for (let i = 0; i < p.mixer.inserts.length; i++) {
    const ins = p.mixer.inserts[i];
    const fx = ins.effects;
    const where = atInsert(p, i);
    const src = feeding(a, i).filter((c) => c.count > 0);

    // Time-based effects before dynamics.
    const firstTime = fx.findIndex((e) => e.enabled && (e.type === "reverb" || e.type === "delay"));
    const lastDyn = fx.reduce((m, e, k) => (e.enabled && ["compressor", "eq", "drive"].includes(e.type) ? k : m), -1);
    if (firstTime >= 0 && lastDyn > firstTime) {
      add(
        a,
        "fx-order",
        "info",
        `${ins.name}: ${fx[firstTime].type} before ${fx[lastDyn].type}`,
        "Time effects usually come after EQ, compression and saturation, so their tails are not squashed or colored.",
        where,
        "Move the delays and reverbs after the dynamics",
        (q) => {
          const x = q.mixer.inserts[i];
          if (!x) return undefined;
          const time = (e) => e.type === "reverb" || e.type === "delay";
          x.effects = x.effects
            .filter((e) => !time(e) && e.type !== "limiter")
            .concat(x.effects.filter(time))
            .concat(x.effects.filter((e) => !time(e) && e.type === "limiter"));
        }
      );
    }

    const reverbs = fx.filter((e) => e.enabled && e.type === "reverb");
    if (reverbs.length >= 2)
      note(a, "double-reverb", "info", `${ins.name} has ${reverbs.length} reverbs`, "One reverb with the right size usually does the job of two.", where);

    for (let k = 0; k < fx.length; k++) {
      const e = fx[k];
      if (!e.enabled) continue;
      const setter = (key, v) => (q) => {
        const x = q.mixer.inserts[i];
        const d = x ? x.effects[k] : undefined;
        if (d) setParam(d, key, v);
      };
      if (e.type === "reverb") {
        const mix = param(e, "mix", 0.25);
        if (i === 0 && mix > 0.15) {
          add(
            a,
            "reverb-wet",
            "warn",
            `Reverb on the master at ${Math.round(mix * 100)}%`,
            "A reverb on the whole mix blurs every part, the low end included.",
            where,
            "Bring it down to 10%",
            setter("mix", 0.1)
          );
        } else if (i > 0 && mix >= 0.5 && src.length > 0) {
          add(
            a,
            "reverb-wet",
            "info",
            `${ins.name}'s reverb is ${Math.round(mix * 100)}% wet`,
            "More reverb than dry signal pushes the part far back.",
            where,
            "Bring it down to 30%",
            setter("mix", 0.3)
          );
        }
      } else if (e.type === "delay") {
        const t = param(e, "time", 0.75);
        const v = nearestValue(t);
        if (Math.abs(t - v) / v > 0.03) {
          add(
            a,
            "delay-sync",
            "info",
            `${ins.name}'s delay is ${r3(t)} beats`,
            `That is not a note value; the nearest is ${r3(v)} beats.`,
            where,
            `Set it to ${r3(v)} beats`,
            setter("time", v)
          );
        }
        const fb = param(e, "feedback", 0.35);
        if (fb >= 0.85)
          add(
            a,
            "delay-feedback",
            "info",
            `${ins.name}'s delay feedback is ${Math.round(fb * 100)}%`,
            "The repeats barely fade and pile up.",
            where,
            "Lower it to 60%",
            setter("feedback", 0.6)
          );
      } else if (e.type === "compressor") {
        const ratio = param(e, "ratio", 4);
        const thr = param(e, "threshold", -18);
        if (ratio >= 8 && thr <= -30)
          note(
            a,
            "crushing-compressor",
            "info",
            `${ins.name}'s compressor crushes`,
            `A ${r3(ratio)}:1 ratio from ${r3(thr)} dB flattens the part's punch; parallel compression keeps it.`,
            where
          );
        const att = param(e, "attack", 10);
        if (att < 3 && src.length > 0 && src.every((c) => c.role === "drums")) {
          add(
            a,
            "drum-attack",
            "info",
            `${ins.name}'s compressor attack is ${r3(att)} ms`,
            "So fast an attack clamps the drums' transients; a slower one lets the crack through.",
            where,
            "Set the attack to 10 ms",
            setter("attack", 10)
          );
        }
      } else if (e.type === "eq") {
        const boosts = ["low", "mid", "high"].filter((key) => param(e, key, 0) >= 9);
        if (boosts.length > 0) {
          add(
            a,
            "eq-boost",
            "info",
            `${ins.name}: EQ boost of ${r3(maxOf(boosts.map((key) => param(e, key, 0))))} dB`,
            `The ${boosts.join(" and ")} band${boosts.length > 1 ? "s are" : " is"} boosted 9 dB or more. Cutting what masks the part usually sounds cleaner.`,
            where,
            "Halve the big boosts",
            (q) => {
              const x = q.mixer.inserts[i];
              const d = x ? x.effects[k] : undefined;
              if (d) for (const key of boosts) setParam(d, key, r3(param(d, key, 0) / 2));
            }
          );
        }
      }
    }
    const off = fx.filter((e) => !e.enabled).length;
    if (off > 0) note(a, "disabled", "info", `${ins.name} has ${plural(off, "disabled effect", "disabled effects")}`, "Remove what you no longer use.", where);
  }

  for (const c of a.chans) {
    const ch = findChannel(p, c.id);
    if (!ch || (ch.instrument.type !== "analog" && ch.instrument.type !== "wavetable")) continue;
    const r = param(ch.instrument, "resonance", ch.instrument.type === "analog" ? 0.3 : 0.1);
    if (r >= 0.9 && c.count > 0)
      note(
        a,
        "resonance",
        "info",
        `${c.name}'s filter resonance is ${Math.round(r * 100)}%`,
        "Near the top it whistles and self-oscillates; fine for acid, harsh elsewhere.",
        atChannel(c)
      );
  }
}

// ------------------------------------------------------------------ master

/** function checkMaster(a: Ana) => Undefined */
function checkMaster(a) {
  const p = a.p;
  const m = p.mixer.inserts[0];
  if (!m) return undefined;
  const where = atInsert(p, 0);
  if (m.volume > 1.001) {
    add(a, "master-hot", "warn", `The master fader is at ${db(m.volume)}`, "Lower the parts rather than raising the master.", where, "Set it to 0 dB", (q) => {
      q.mixer.inserts[0].volume = 1;
    });
  }
  const on = m.effects.filter((e) => e.enabled);
  const lims = m.effects.map((e, k) => k).filter((k) => m.effects[k].enabled && m.effects[k].type === "limiter");
  if (a.parts.length > 0 && lims.length === 0) {
    add(
      a,
      "no-limiter",
      "info",
      "No limiter on the master",
      "A limiter with a −1 dB ceiling keeps the render from clipping (and does little else when the mix has headroom).",
      where,
      "Add a limiter (−1 dB ceiling)",
      (q) => {
        const d = newDevice("limiter");
        setParam(d, "gain", 0);
        setParam(d, "ceiling", -1);
        q.mixer.inserts[0].effects.push(d);
      }
    );
  }
  if (lims.length > 0) {
    const lastOn = m.effects.reduce((x, e, k) => (e.enabled ? k : x), -1);
    if (lims[lims.length - 1] !== lastOn) {
      add(
        a,
        "limiter-last",
        "warn",
        "The master limiter is not last",
        `${m.effects[lastOn].type} comes after it and can push the signal past the ceiling.`,
        where,
        "Move the limiter to the end",
        (q) => {
          const fx = q.mixer.inserts[0].effects;
          q.mixer.inserts[0].effects = fx.filter((e) => e.type !== "limiter").concat(fx.filter((e) => e.type === "limiter"));
        }
      );
    }
    const k = lims[lims.length - 1];
    const lim = m.effects[k];
    const ceil = param(lim, "ceiling", -0.3);
    if (ceil > -1) {
      add(
        a,
        "limiter-ceiling",
        "info",
        `Limiter ceiling at ${r3(ceil)} dB`,
        "Streaming services ask for −1 dB true peak: lossy encoding and inter-sample peaks overshoot a higher ceiling.",
        where,
        "Set the ceiling to −1 dB",
        (q) => {
          const d = q.mixer.inserts[0].effects[k];
          if (d) setParam(d, "ceiling", -1);
        }
      );
    }
    const gain = param(lim, "gain", 0);
    if (gain > 6) {
      add(
        a,
        "limiter-drive",
        "warn",
        `The limiter is driven ${r3(gain)} dB`,
        "Streaming turns loud masters down to about −14 LUFS: the extra loudness is lost, the crushed transients are not.",
        where,
        "Drive it 3 dB",
        (q) => {
          const d = q.mixer.inserts[0].effects[k];
          if (d) setParam(d, "gain", 3);
        }
      );
    }
  }
  if (on.length > 4 || lims.length > 1) {
    note(
      a,
      "master-chain",
      "info",
      `${plural(on.length, "device", "devices")} on the master`,
      lims.length > 1 ? "Several limiters in a row fight each other." : "A long master chain often fixes in mastering what belongs in the mix.",
      where
    );
  }
}

// ------------------------------------------------------------------ project

/** function checkProject(a: Ana) => Undefined */
function checkProject(a) {
  const p = a.p;
  const t = p.transport;
  if (Math.abs(t.bpm - Math.round(t.bpm)) > 0.01) {
    const bpm = Math.round(t.bpm);
    add(
      a,
      "tempo",
      "info",
      `The tempo is ${t.bpm} BPM`,
      "A fractional tempo is usually an accident.",
      atProject("Transport"),
      `Round it to ${bpm} BPM`,
      (q) => {
        q.transport.bpm = bpm;
      }
    );
  } else if (t.bpm < 60 || t.bpm > 200) {
    note(
      a,
      "tempo",
      "info",
      `${t.bpm} BPM`,
      t.bpm < 60 ? "Very slow: is this half time of a faster groove?" : "Very fast: is this double time of a slower groove?",
      atProject("Transport")
    );
  }
  const bar = Math.max(1, t.beatsPerBar);
  for (const pat of p.patterns) {
    if (pat.notes.length === 0 || pat.length <= 0) continue;
    const bars = pat.length / bar;
    if (Math.abs(bars - Math.round(bars)) > 1e-6) {
      const id = pat.id;
      const len = Math.ceil(bars - 1e-6) * bar;
      add(
        a,
        "pattern-bars",
        "info",
        `"${pat.name}" is ${pat.length} beats`,
        `That is ${r3(bars)} bars; the next loop starts mid-bar.`,
        atPattern(pat),
        `Make it ${plural(Math.round(len / bar), "bar", "bars")}`,
        (q) => {
          const x = findPattern(q, id);
          if (x) x.length = len;
        }
      );
    } else if ([3, 5, 7].includes(Math.round(bars))) {
      note(
        a,
        "pattern-bars",
        "info",
        `"${pat.name}" is ${Math.round(bars)} bars`,
        "Odd phrase lengths can be deliberate; usually phrases come in 2, 4 or 8 bars.",
        atPattern(pat)
      );
    }
  }
  const layered = p.channels.map((c) => c.layerOf);
  for (const c of a.chans) {
    if (c.count === 0 && c.layerOf === "" && !layered.includes(c.id) && c.type !== "sampler" && c.type !== "plugin") {
      note(a, "unused-channel", "info", `${c.name} has no notes`, "It plays nothing in any pattern.", atChannel(c));
    }
  }
  const dflt = /^(pattern|channel|insert|track|untitled)\s*#?\d*$/i;
  /** const names: String[] */
  const names = [];
  for (const pat of p.patterns) if (dflt.test(pat.name.trim())) names.push(pat.name);
  for (const c of p.channels) if (dflt.test(c.name.trim())) names.push(c.name);
  if (p.meta.title.trim() === "" || p.meta.title === "Untitled") names.unshift("the song");
  if (names.length > 0)
    note(
      a,
      "names",
      "info",
      `${plural(names.length, "default name", "default names")}`,
      `${names.slice(0, 5).join(", ")}${names.length > 5 ? ", …" : ""}: names that describe the part help when you come back.`,
      atProject("Names")
    );
}

// ------------------------------------------------------------------ entry

/** Run every check on a project. `off` names rules not to report. */
/** function critique(p: Project, off: String[]) => Finding[] */
export function critique(p, off) {
  const a = analyze(p);
  findKey(a);
  checkHarmony(a);
  checkKey(a);
  checkClashes(a);
  checkMelody(a);
  checkRhythm(a);
  checkArrangement(a);
  checkLowEnd(a);
  checkMix(a);
  checkStereo(a);
  checkEffects(a);
  checkMaster(a);
  checkProject(a);
  const out = a.out.filter((f) => !off.includes(f.rule));
  const rank = (f) => CATEGORIES.indexOf(f.category) * 4 + (f.level === "warn" ? 0 : 2) + (f.fix !== "" ? 0 : 1);
  return out
    .map((f, i) => i)
    .sort((x, y) => rank(out[x]) - rank(out[y]) || x - y)
    .map((i) => out[i]);
}

/** The key the Critic hears the song in ("" when there are no pitched notes). */
/** function songKey(p: Project) => String */
export function songKey(p) {
  const a = analyze(p);
  findKey(a);
  return a.keyLabel;
}
