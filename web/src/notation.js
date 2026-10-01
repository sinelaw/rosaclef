// Sheet music, part 1: from beats to notation.
//
// `buildScore` reads the project (the whole song, one playlist track or one
// pattern) and writes it down the way an engraver would: one staff per
// channel (a piano's grand staff, a drum staff), measures from the meters,
// a key signature, pitches spelled in that key, onsets quantized to a grid,
// durations split into notatable values that show the beat (tied where they
// cross it), rests, and beam groups. It is pure; `engrave.js` places it on
// the page.
//
// Time is in ticks: 48 to the quarter note (a 32nd is 6, a triplet eighth 16).

import { trackIndex } from "#brands";
import { meterMap, optionValue } from "./model.js";
import { detectKey } from "./voice.js";

export const TPQ = 48;

/** A note as it sounds in the score's time: `origin` is where its pattern's beat 0 falls. */
/** type SrcNote = { pitch: Int, start: Number, end: Number, velocity: Number, channel: String, pattern: String, index: Int, origin: Number } */

/** What to write down: the "song", one playlist "track" or one "pattern". */
/** type Scope = { kind: String, track: Int, pattern: String } */

/** A notehead. `acc`: the accidental to draw (-2..2), or NO_ACC. `head`: "" (normal), "x" or "o" (drums). */
/** type Head = { pitch: Int, step: Int, alter: Int, acc: Int, head: String, src: Int, tieIn: Boolean, tieOut: Boolean } */

/** A note, chord or rest. `start` (ticks, score time) and `dur` (ticks); `base` is the
 * undotted written value; `whole` is a whole-measure rest; `beam` groups beamed notes (-1: none). */
/** `tuplet`: the start tick of the triplet beat it is in (-1: none); its `dur` is real time, `base` the written value. */
/** type NEv = { start: Int, dur: Int, base: Int, dots: Int, rest: Boolean, whole: Boolean, heads: Head[], beam: Int, measure: Int, tuplet: Int } */

/** A part before it is split into staves: one channel, or the synthesized drums together. */
/** type Part = { channels: Channel[], idx: Int[], kit: String, clef: String, name: String, color: String } */

/** One staff (`channels`: whose notes it holds; `channel` is the first). `clef`: treble, treble8vb, bass, alto or percussion. A grand staff is two
 * staves of one channel (`part` 0 and 1) that share a `group`. */
/** type Staff = { channel: String, channels: String[], name: String, color: String, clef: String, drum: Boolean, part: Int, group: Int, events: NEv[] } */

/** A measure: start and length in ticks, its meter, the beat (ticks) and whether the time signature
 * shows; `number` is its bar number, and `count` > 1 makes it a multi-measure rest of that many bars. */
/** type Measure = { start: Int, length: Int, num: Int, den: Int, beat: Int, meter: Boolean, number: Int, count: Int } */

/** A colored passage resolved to score time (beats); `channels` empty = every staff. */
/** type MarkSpan = { start: Number, end: Number, color: String, label: String, channels: String[], mark: Int } */

/** type Score = { staves: Staff[], measures: Measure[], fifths: Int, minor: Boolean, keyName: String, notes: SrcNote[], marks: MarkSpan[], bpm: Number, end: Int, empty: Boolean } */

/** No accidental to draw. */
export const NO_ACC = 9;

/** Later than any tick. */
const FAR = 1 << 30;

// ------------------------------------------------------------------ keys

/** type KeyDef = { name: String, fifths: Int, minor: Boolean } */

/** const KEYS: KeyDef[] */
export const KEYS = [
  { name: "C", fifths: 0, minor: false },
  { name: "G", fifths: 1, minor: false },
  { name: "D", fifths: 2, minor: false },
  { name: "A", fifths: 3, minor: false },
  { name: "E", fifths: 4, minor: false },
  { name: "B", fifths: 5, minor: false },
  { name: "F#", fifths: 6, minor: false },
  { name: "C#", fifths: 7, minor: false },
  { name: "F", fifths: -1, minor: false },
  { name: "Bb", fifths: -2, minor: false },
  { name: "Eb", fifths: -3, minor: false },
  { name: "Ab", fifths: -4, minor: false },
  { name: "Db", fifths: -5, minor: false },
  { name: "Gb", fifths: -6, minor: false },
  { name: "Cb", fifths: -7, minor: false },
  { name: "Am", fifths: 0, minor: true },
  { name: "Em", fifths: 1, minor: true },
  { name: "Bm", fifths: 2, minor: true },
  { name: "F#m", fifths: 3, minor: true },
  { name: "C#m", fifths: 4, minor: true },
  { name: "G#m", fifths: 5, minor: true },
  { name: "D#m", fifths: 6, minor: true },
  { name: "A#m", fifths: 7, minor: true },
  { name: "Dm", fifths: -1, minor: true },
  { name: "Gm", fifths: -2, minor: true },
  { name: "Cm", fifths: -3, minor: true },
  { name: "Fm", fifths: -4, minor: true },
  { name: "Bbm", fifths: -5, minor: true },
  { name: "Ebm", fifths: -6, minor: true },
  { name: "Abm", fifths: -7, minor: true },
];

/** A key's name as people write it (B♭ major, F♯ minor). */
/** function keyLabel(name: String) => String */
export function keyLabel(name) {
  const minor = name.endsWith("m");
  const root = (minor ? name.slice(0, name.length - 1) : name).replace("#", "♯").replace("b", "♭");
  return `${root} ${minor ? "minor" : "major"}`;
}

/** function findKey(name: String) => KeyDef? */
function findKey(name) {
  return KEYS.find((k) => k.name === name);
}

// Major keys by tonic pitch class, as fifths (F♯ over G♭, D♭ over C♯).
const MAJOR_FIFTHS = [0, -5, 2, -3, 4, -1, 6, 1, -4, 3, -2, 5];

/** The key name for a tonic pitch class and mode. */
/** function keyNameOf(tonic: Int, minor: Boolean) => String */
function keyNameOf(tonic, minor) {
  const fifths = MAJOR_FIFTHS[(tonic + (minor ? 3 : 0)) % 12];
  const k = KEYS.find((x) => x.fifths === fifths && x.minor === minor);
  return k ? k.name : "C";
}

// ------------------------------------------------------------------ spelling

// Letters in line-of-fifths order (F C G D A E B) as indexes C=0 … B=6.
const LOF_LETTER = [3, 0, 4, 1, 5, 2, 6];
const LETTER_PC = [0, 2, 4, 5, 7, 9, 11];

/** type Spelled = { step: Int, alter: Int } */

/** Spell a MIDI pitch in a key: its staff step (C4 = 35, one per letter) and its
 * alteration. Chromatic notes take the spelling nearest the key on the line of fifths. */
/** function spell(pitch: Int, fifths: Int) => Spelled */
export function spell(pitch, fifths) {
  const pc = ((pitch % 12) + 12) % 12;
  const center = fifths + 2;
  let best = 0;
  let bestD = 99;
  for (let q = -15; q <= 19; q++) {
    if ((((q * 7) % 12) + 12) % 12 !== pc) continue;
    const d = Math.abs(q - center);
    // Ties: the plainer spelling (B♮ over C♭ in C minor), then sharps in sharp keys and C.
    const plain = Math.abs(Math.floor((q + 1) / 7)) - Math.abs(Math.floor((best + 1) / 7));
    if (d < bestD || (d === bestD && (plain < 0 || (plain === 0 && (fifths >= 0 ? q > best : q < best))))) {
      best = q;
      bestD = d;
    }
  }
  const letter = LOF_LETTER[(((best + 1) % 7) + 7) % 7];
  const alter = Math.floor((best + 1) / 7);
  const natural = pitch - alter;
  return { step: Math.floor(natural / 12) * 7 + letter, alter: alter };
}

const LETTERS = ["C", "D", "E", "F", "G", "A", "B"];

/** A pitch's name as the key spells it ("E♭4", "F♯3"). */
/** function spelledName(pitch: Int, fifths: Int) => String */
export function spelledName(pitch, fifths) {
  const sp = spell(pitch, fifths);
  const acc = sp.alter === 1 ? "♯" : sp.alter === -1 ? "♭" : sp.alter === 2 ? "𝄪" : sp.alter === -2 ? "𝄫" : "";
  return `${LETTERS[((sp.step % 7) + 7) % 7]}${acc}${Math.floor(sp.step / 7) - 1}`;
}

/** The alteration a key signature gives a letter (C=0 … B=6). */
/** function keyAlter(letter: Int, fifths: Int) => Int */
export function keyAlter(letter, fifths) {
  // Position of the letter on the line of fifths (F=-1 … B=5).
  const q = LOF_LETTER.indexOf(letter) - 1;
  // Sharps come in the order F C G D A E B, flats in the reverse.
  if (fifths > 0 && q <= fifths - 2) return 1;
  if (fifths < 0 && q >= 6 + fifths) return -1;
  return 0;
}

/** The MIDI pitch of a staff step with an alteration. */
/** function stepPitch(step: Int, alter: Int) => Int */
export function stepPitch(step, alter) {
  const oct = Math.floor(step / 7);
  const letter = step - oct * 7;
  return oct * 12 + LETTER_PC[letter] + alter;
}

// ------------------------------------------------------------------ clefs

/** The step on the bottom line of a clef's staff (E4 for treble). */
/** function bottomStep(clef: String) => Int */
export function bottomStep(clef) {
  if (clef === "bass") return 25;
  if (clef === "alto") return 31;
  if (clef === "treble8vb") return 30;
  return 37;
}

// ------------------------------------------------------------------ drums

/** type DrumPos = { step: Int, head: String } */

/** Where a General MIDI drum sits on a drum staff (after the Percussive Arts Society key). */
/** function gmDrum(pitch: Int) => DrumPos */
export function gmDrum(pitch) {
  if (pitch === 35 || pitch === 36) return { step: 38, head: "" };
  if (pitch === 37) return { step: 42, head: "x" };
  if (pitch === 38 || pitch === 40) return { step: 42, head: "" };
  if (pitch === 39) return { step: 42, head: "x" };
  if (pitch === 41 || pitch === 43) return { step: 40, head: "" };
  if (pitch === 42) return { step: 46, head: "x" };
  if (pitch === 44) return { step: 36, head: "x" };
  if (pitch === 45 || pitch === 47) return { step: 43, head: "" };
  if (pitch === 46) return { step: 46, head: "o" };
  if (pitch === 48 || pitch === 50) return { step: 44, head: "" };
  if (pitch === 49 || pitch === 57) return { step: 47, head: "x" };
  if (pitch === 51 || pitch === 59 || pitch === 53) return { step: 45, head: "x" };
  if (pitch === 52 || pitch === 55) return { step: 48, head: "x" };
  if (pitch === 54 || pitch === 56) return { step: 44, head: "x" };
  if (pitch < 35) return { step: 37, head: "" };
  return { step: 39 + (pitch % 5), head: "x" };
}

/** Where a synthesized drum (options.kind) sits. */
/** function kindDrum(kind: String) => DrumPos */
export function kindDrum(kind) {
  if (kind === "kick") return { step: 38, head: "" };
  if (kind === "snare") return { step: 42, head: "" };
  if (kind === "clap" || kind === "rim") return { step: 42, head: "x" };
  if (kind === "hat") return { step: 46, head: "x" };
  if (kind === "openhat") return { step: 46, head: "o" };
  if (kind === "tom") return { step: 40, head: "" };
  if (kind === "cowbell") return { step: 44, head: "x" };
  if (kind === "shaker") return { step: 47, head: "x" };
  return { step: 42, head: "" };
}

/** The GM drum written at a step (for writing notes on a drum staff). */
/** function drumAt(step: Int) => Int */
export function drumAt(step) {
  for (const p of [36, 38, 42, 41, 45, 48, 51, 49, 44, 52]) {
    if (gmDrum(p).step === step) return p;
  }
  return 38;
}

/** A synthesized drum channel's kind ("kick", "snare", …). */
/** function channelKind(p: Project, id: String) => String */
export function channelKind(p, id) {
  const c = p.channels.find((x) => x.id === id);
  return c ? optionValue(c.instrument, "kind") : "";
}

/** "gm" (a General MIDI kit), "synth" (synthesized drums) or "" (pitched). */
/** function drumKit(c: Channel) => String */
export function drumKit(c) {
  if (c.instrument.type === "drum") return "synth";
  if (c.instrument.type === "soundfont" && optionValue(c.instrument, "program").toLowerCase().includes("kit")) return "gm";
  return "";
}

// ------------------------------------------------------------------ gather

/** The notes of a scope in score time, sorted, and the scope's length (beats). */
/** type Gathered = { notes: SrcNote[], length: Number } */

/** function gather(p: Project, scope: Scope) => Gathered */
export function gather(p, scope) {
  /** const out: SrcNote[] */
  const out = [];
  let length = 0;
  if (scope.kind === "pattern") {
    const pat = p.patterns.find((x) => x.id === scope.pattern);
    if (pat) {
      length = pat.length;
      for (let i = 0; i < pat.notes.length; i++) {
        const n = pat.notes[i];
        // A pattern's notes end with it (as when it loops in the song).
        if (n.start >= pat.length - 1e-9) continue;
        out.push({
          pitch: Math.round(n.pitch),
          start: n.start,
          end: Math.min(n.start + n.length, pat.length),
          velocity: n.velocity,
          channel: n.channel,
          pattern: pat.id,
          index: i,
          origin: 0,
        });
      }
    }
  } else {
    const hidden = p.score.hiddenTracks.map(trackIndex);
    for (const c of p.playlist.clips) {
      if (c.pattern === "") continue;
      const tr = trackIndex(c.track);
      if (scope.kind === "track" ? tr !== scope.track : hidden.includes(tr)) continue;
      const pat = p.patterns.find((x) => x.id === c.pattern);
      if (!pat || pat.length <= 0) continue;
      length = Math.max(length, c.start + c.length);
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
          const at = c.start + t - w0;
          out.push({
            pitch: Math.round(n.pitch),
            start: at,
            end: Math.min(Math.min(at + n.length, clipEnd), c.start + base + L - w0),
            velocity: n.velocity,
            channel: n.channel,
            pattern: pat.id,
            index: i,
            origin: c.start - w0 + base,
          });
        }
      }
    }
  }
  out.sort((a, b) => a.start - b.start || a.pitch - b.pitch);
  return { notes: out, length: length };
}

/** Where a pattern plays in the scope: score-time windows and the origin of pattern beat 0. */
/** type Occurrence = { origin: Number, from: Number, to: Number } */

/** function occurrences(p: Project, scope: Scope, patternId: String) => Occurrence[] */
export function occurrences(p, scope, patternId) {
  /** const out: Occurrence[] */
  const out = [];
  const pat = p.patterns.find((x) => x.id === patternId);
  if (!pat || pat.length <= 0) return out;
  if (scope.kind === "pattern") {
    if (scope.pattern === patternId) out.push({ origin: 0, from: 0, to: pat.length });
    return out;
  }
  const hidden = p.score.hiddenTracks.map(trackIndex);
  for (const c of p.playlist.clips) {
    if (c.pattern !== patternId) continue;
    const tr = trackIndex(c.track);
    if (scope.kind === "track" ? tr !== scope.track : hidden.includes(tr)) continue;
    const L = pat.length;
    for (let j = Math.floor(c.offset / L); j * L < c.offset + c.length; j++) {
      const origin = c.start - c.offset + j * L;
      out.push({ origin: origin, from: Math.max(c.start, origin), to: Math.min(c.start + c.length, origin + L) });
    }
  }
  return out;
}

// ------------------------------------------------------------------ measures

/** function measuresFor(t: Transport, endTick: Int) => Measure[] */
function measuresFor(t, endTick) {
  const map = meterMap(t);
  /** const out: Measure[] */
  const out = [];
  let label = "";
  for (let i = 0; i < map.length && out.length < 4096; i++) {
    const s = map[i];
    const parts = s.label.split("/");
    const num = Math.max(1, Math.round(Number(parts[0])));
    const den = Math.max(1, Math.round(Number(parts[1])));
    const len = Math.round(s.barBeats * TPQ);
    const stop = i + 1 < map.length ? Math.round(map[i + 1].beat * TPQ) : Infinity;
    let at = Math.round(s.beat * TPQ);
    while (at < stop && (at < endTick || out.length === 0) && out.length < 4096) {
      out.push({ start: at, length: len, num: num, den: den, beat: beatTicks(num, den), meter: label !== s.label, number: out.length + 1, count: 1 });
      label = s.label;
      at = at + len;
    }
    if (at >= endTick && out.length > 0) break;
  }
  return out;
}

/** The beat of a meter, in ticks: a quarter in 4/4, a dotted quarter in 6/8. */
/** function beatTicks(num: Int, den: Int) => Int */
export function beatTicks(num, den) {
  const unit = Math.round((4 * TPQ) / den);
  if (den >= 8 && num % 3 === 0 && num >= 6) return unit * 3;
  return unit;
}

/** function measureAt(ms: Measure[], tick: Int) => Int */
function measureAt(ms, tick) {
  let lo = 0;
  let hi = ms.length - 1;
  while (lo < hi) {
    const mid = Math.floor((lo + hi + 1) / 2);
    if (ms[mid].start <= tick) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

// ------------------------------------------------------------------ durations

// Written values, longest first: [ticks, undotted value, dots].
/** const VALUES: { d: Int, base: Int, dots: Int }[] */
const VALUES = [
  { d: 288, base: 192, dots: 1 },
  { d: 192, base: 192, dots: 0 },
  { d: 144, base: 96, dots: 1 },
  { d: 96, base: 96, dots: 0 },
  { d: 72, base: 48, dots: 1 },
  { d: 48, base: 48, dots: 0 },
  { d: 36, base: 24, dots: 1 },
  { d: 24, base: 24, dots: 0 },
  { d: 18, base: 12, dots: 1 },
  { d: 12, base: 12, dots: 0 },
  { d: 6, base: 6, dots: 0 },
];

/** type Piece = { s: Int, d: Int, base: Int, dots: Int } */

/** Split [s, e) (ticks from the measure start) into written values that show the
 * beat: within a beat anything goes; across beats a value must start on a beat,
 * and in 4/4 it may cross the middle of the bar only from the downbeat. */
/** function pieces(m: Measure, s0: Int, e: Int, rest: Boolean) => Piece[] */
export function pieces(m, s0, e, rest) {
  /** const out: Piece[] */
  const out = [];
  const beat = m.beat;
  const half = m.num === 4 && m.den === 4 ? 96 : m.num === 4 && m.den === 2 ? 192 : 0;
  const compound = beat === 3 * Math.round((4 * TPQ) / m.den);
  let s = s0;
  while (s < e) {
    let pick = -1;
    for (let i = 0; i < VALUES.length; i++) {
      const v = VALUES[i];
      if (v.d > e - s) continue;
      const end = s + v.d;
      const inside = Math.floor(s / beat) === Math.floor((end - 1) / beat);
      if (inside) {
        if (s % Math.max(1, Math.floor(v.base / 2)) !== 0) continue;
      } else {
        if (s % beat !== 0) continue;
        if (half > 0 && s !== 0 && s < half && end > half) continue;
        // Across beats, rests are dotted only as whole beats of a compound meter (6/8).
        if (rest && v.dots > 0 && !(compound && v.d % beat === 0)) continue;
        if (rest && v.base === 192 && s !== 0) continue;
      }
      pick = i;
      break;
    }
    if (pick < 0) {
      const d = Math.min(e - s, 6);
      out.push({ s: s, d: d, base: 6, dots: 0 });
      s = s + d;
      continue;
    }
    const v = VALUES[pick];
    out.push({ s: s, d: v.d, base: v.base, dots: v.dots });
    s = s + v.d;
  }
  return out;
}

// ------------------------------------------------------------------ staves

/** type Chord = { start: Int, end: Int, notes: Int[] } */

/** The pitch below which a grand staff writes notes on the bass staff. */
const SPLIT = 60;

/** Pick a clef from a channel's notes (auto): treble, bass or a grand staff. */
/** function autoClef(pitches: Int[]) => String */
export function autoClef(pitches) {
  if (pitches.length === 0) return "treble";
  const s = pitches.slice().sort((a, b) => a - b);
  const lo = s[Math.floor(s.length * 0.08)];
  const hi = s[Math.min(s.length - 1, Math.floor(s.length * 0.92))];
  const mid = s[Math.floor(s.length / 2)];
  if (lo < 55 && hi > 66 && hi - lo >= 19 && s.length >= 6) return "grand";
  return mid >= 57 ? "treble" : "bass";
}

/** Chords of one staff from its source notes (indexes into `notes`): onsets
 * quantized to `grid` ticks, each chord lasting to its longest note or the next onset. */
// ------------------------------------------------------------------ triplets

/** The quarter-note beat (of a simple meter) that holds a tick, or -1. */
/** function beatOf(ms: Measure[], t: Number) => Int */
function beatOf(ms, t) {
  const m = ms[measureAt(ms, Math.max(0, Math.floor(t)))];
  if (m.beat !== TPQ || t >= m.start + m.length) return -1;
  return m.start + Math.floor((t - m.start) / TPQ) * TPQ;
}

/** Beats written as triplets: every onset inside the beat sits on a third of
 * it (and nearer to one than to the grid), and at least one is clearly off the
 * grid. Note ends are not evidence (they are rarely played exactly), nor is
 * playing that only wavers around the grid. */
/** function tripletBeats(notes: SrcNote[], idx: Int[], ms: Measure[], grid: Int) => Int[] */
function tripletBeats(notes, idx, ms, grid) {
  /** const beats: Int[] */
  const beats = [];
  /** const fits: Boolean[] */
  const fits = [];
  /** const clear: Boolean[] */
  const clear = [];
  for (const i of idx) {
    const t = notes[i].start * TPQ;
    const b = beatOf(ms, t);
    if (b < 0) continue;
    const r = t - b;
    // On the beat itself both readings agree.
    if (r < 2 || r > TPQ - 2) continue;
    let k = beats.indexOf(b);
    if (k < 0) {
      beats.push(b);
      fits.push(true);
      clear.push(false);
      k = beats.length - 1;
    }
    const third = Math.abs(r - Math.round(r / 16) * 16);
    const straight = Math.abs(r - Math.round(r / grid) * grid);
    if (third > 2.5 || third >= straight) fits[k] = false;
    if (straight >= 3.5) clear[k] = true;
  }
  /** const out: Int[] */
  const out = [];
  for (let k = 0; k < beats.length; k++) if (fits[k] && clear[k]) out.push(beats[k]);
  return out;
}

/** Round a time (beats) to ticks: to thirds of the beat in a triplet beat, else to the grid. */
/** function quantizeAt(t: Number, ms: Measure[], trip: Int[], grid: Int) => Int */
function quantizeAt(t, ms, trip, grid) {
  const x = t * TPQ;
  const b = beatOf(ms, x);
  if (b >= 0 && trip.includes(b)) return b + Math.round((x - b) / 16) * 16;
  // Right after a triplet beat, the beat's end is the nearest grid point anyway.
  return Math.max(0, Math.round(x / grid) * grid);
}

/** function chordsOf(notes: SrcNote[], idx: Int[], grid: Int, drum: Boolean, ms: Measure[], trip: Int[]) => Chord[] */
function chordsOf(notes, idx, grid, drum, ms, trip) {
  /** const qs: { i: Int, s: Int, e: Int }[] */
  const qs = [];
  for (const i of idx) {
    const n = notes[i];
    const s = quantizeAt(n.start, ms, trip, grid);
    let e = quantizeAt(n.end, ms, trip, grid);
    if (e <= s) e = s + (trip.includes(beatOf(ms, s)) ? 16 : grid);
    qs.push({ i: i, s: s, e: e });
  }
  qs.sort((a, b) => a.s - b.s || notes[a.i].pitch - notes[b.i].pitch);
  /** const out: Chord[] */
  const out = [];
  for (const q of qs) {
    const last = out.length > 0 ? out[out.length - 1] : undefined;
    if (last !== undefined && last.start === q.s) {
      // One head per pitch.
      if (!last.notes.some((j) => notes[j].pitch === notes[q.i].pitch)) last.notes.push(q.i);
      last.end = Math.max(last.end, q.e);
    } else out.push({ start: q.s, end: q.e, notes: [q.i] });
  }
  for (let k = 0; k < out.length; k++) {
    const next = k + 1 < out.length ? out[k + 1].start : FAR;
    // Drum hits ring until the next one, at most a beat and never over a bar line.
    if (drum) {
      const m = ms[measureAt(ms, out[k].start)];
      out[k].end = Math.min(Math.min(next, out[k].start + Math.max(TPQ, out[k].end - out[k].start)), m.start + m.length);
    }
    out[k].end = Math.min(out[k].end, next);
  }
  return out;
}

/** Heads of a chord, top to bottom order not implied; tie flags set by the caller. */
/** function headsOf(p: Project, notes: SrcNote[], ch: Chord, fifths: Int, kit: String) => Head[] */
function headsOf(p, notes, ch, fifths, kit) {
  /** const out: Head[] */
  const out = [];
  for (const i of ch.notes) {
    const n = notes[i];
    if (kit !== "") {
      const pos = kit === "gm" ? gmDrum(n.pitch) : kindDrum(channelKind(p, n.channel));
      if (out.some((h) => h.step === pos.step)) continue;
      out.push({ pitch: n.pitch, step: pos.step, alter: 0, acc: NO_ACC, head: pos.head, src: i, tieIn: false, tieOut: false });
    } else {
      const sp = spell(n.pitch, fifths);
      out.push({ pitch: n.pitch, step: sp.step, alter: sp.alter, acc: NO_ACC, head: "", src: i, tieIn: false, tieOut: false });
    }
  }
  out.sort((a, b) => a.step - b.step);
  return out;
}

/** function copyHeads(hs: Head[], tieIn: Boolean, tieOut: Boolean) => Head[] */
function copyHeads(hs, tieIn, tieOut) {
  return hs.map((h) => ({ pitch: h.pitch, step: h.step, alter: h.alter, acc: NO_ACC, head: h.head, src: h.src, tieIn: tieIn, tieOut: tieOut }));
}

/** Lay a staff's chords into measures: rests between, split at bar lines and into written values. */
/** Written values for [a, b) inside a triplet beat starting at `bs`: thirds of the
 * beat are eighths, two thirds a quarter (under a 3); the whole beat is a plain quarter. */
/** function tripletPieces(bs: Int, a: Int, b: Int) => Piece[] */
function tripletPieces(bs, a, b) {
  /** const out: Piece[] */
  const out = [];
  if (a === bs && b === bs + TPQ) {
    out.push({ s: a, d: TPQ, base: TPQ, dots: 0 });
    return out;
  }
  let t = a;
  while (t < b) {
    const d = b - t >= 32 ? 32 : b - t >= 16 ? 16 : b - t;
    out.push({ s: t, d: d, base: d >= 32 ? 48 : d >= 16 ? 24 : 12, dots: 0 });
    t = t + d;
  }
  return out;
}

/** function eventsOf(chords: Chord[], heads: Head[][], ms: Measure[], trip: Int[]) => NEv[] */
function eventsOf(chords, heads, ms, trip) {
  /** const out: NEv[] */
  const out = [];
  /** function put(s: Int, e: Int, hs: Head[], rest: Boolean) => Undefined */
  function put(s, e, hs, rest) {
    let t = s;
    let first = true;
    while (t < e) {
      const mi = measureAt(ms, t);
      const m = ms[mi];
      let mEnd = Math.min(e, m.start + m.length);
      if (rest && t === m.start && mEnd === m.start + m.length) {
        out.push({ start: t, dur: m.length, base: 192, dots: 0, rest: true, whole: true, heads: [], beam: -1, measure: mi, tuplet: -1 });
        t = mEnd;
        continue;
      }
      // Triplet beats are written on their own; the stretch before one stops at it.
      const b = beatOf(ms, t);
      const inTrip = b >= 0 && trip.includes(b);
      if (inTrip) mEnd = Math.min(mEnd, b + TPQ);
      else for (const tb of trip) if (tb > t && tb < mEnd) mEnd = tb;
      const ps = inTrip ? tripletPieces(b, t, mEnd) : pieces(m, t - m.start, mEnd - m.start, rest);
      for (let k = 0; k < ps.length; k++) {
        const pc = ps[k];
        const at = inTrip ? pc.s : m.start + pc.s;
        const last = at + pc.d >= e;
        out.push({
          start: at,
          dur: pc.d,
          base: pc.base,
          dots: pc.dots,
          rest: rest,
          whole: false,
          heads: rest ? [] : copyHeads(hs, !first, !last),
          beam: -1,
          measure: mi,
          tuplet: inTrip && pc.d < TPQ ? b : -1,
        });
        first = false;
      }
      t = mEnd;
    }
  }
  let t = 0;
  for (let k = 0; k < chords.length; k++) {
    const c = chords[k];
    if (c.start > t) put(t, c.start, [], true);
    put(c.start, c.end, heads[k], false);
    t = c.end;
  }
  const end = ms[ms.length - 1].start + ms[ms.length - 1].length;
  if (t < end) put(t, end, [], true);
  return out;
}

/** Accidentals: shown when a note differs from the key signature or from an
 * earlier note on the same line or space in the measure. */
/** function markAccidentals(evs: NEv[], fifths: Int) => Undefined */
function markAccidentals(evs, fifths) {
  let measure = -1;
  /** const seen: { step: Int, alter: Int }[] */
  const seen = [];
  for (const ev of evs) {
    if (ev.measure !== measure) {
      measure = ev.measure;
      seen.length = 0;
    }
    for (const h of ev.heads) {
      if (h.head !== "") continue;
      const prior = seen.find((x) => x.step === h.step);
      const current = prior ? prior.alter : keyAlter(h.step % 7, fifths);
      if (h.tieIn) {
        if (!prior) seen.push({ step: h.step, alter: h.alter });
        continue;
      }
      if (h.alter !== current) {
        h.acc = h.alter;
        if (prior) prior.alter = h.alter;
        else seen.push({ step: h.step, alter: h.alter });
      }
    }
  }
}

/** Beam groups: eighths and shorter within a beat (a dotted quarter in compound
 * meters); in 4/4 two beats of plain eighths join into four. */
/** function markBeams(evs: NEv[], ms: Measure[], next: Int) => Int */
function markBeams(evs, ms, next) {
  let id = next;
  let k = 0;
  while (k < evs.length) {
    const ev = evs[k];
    if (ev.rest || ev.base > 24) {
      k = k + 1;
      continue;
    }
    const m = ms[ev.measure];
    const span = m.den >= 8 && m.beat < 72 ? Math.min(m.length, 72) : m.beat;
    const group = Math.floor((ev.start - m.start) / span);
    let j = k + 1;
    while (j < evs.length) {
      const x = evs[j];
      if (x.rest || x.base > 24 || x.measure !== ev.measure || Math.floor((x.start - m.start) / span) !== group) break;
      j = j + 1;
    }
    if (j - k >= 2) {
      for (let i = k; i < j; i++) evs[i].beam = id;
      id = id + 1;
    }
    k = j;
  }
  // 4/4: |♫ ♫| on beats 1–2 or 3–4 → one group of four eighths.
  for (let i = 0; i + 3 < evs.length; i++) {
    const a = evs[i];
    const m = ms[a.measure];
    if (m.num !== 4 || m.den !== 4 || a.beam < 0) continue;
    const rel = a.start - m.start;
    if (rel !== 0 && rel !== 96) continue;
    let plain = true;
    for (let q = 0; q < 4; q++) {
      const x = evs[i + q];
      if (x.base !== 24 || x.dots !== 0 || x.measure !== a.measure || x.start !== a.start + q * 24) plain = false;
    }
    if (plain && evs[i + 1].beam === a.beam && evs[i + 2].beam === evs[i + 3].beam && evs[i + 2].beam >= 0) {
      evs[i + 2].beam = a.beam;
      evs[i + 3].beam = a.beam;
    }
  }
  return id;
}

// ------------------------------------------------------------------ multi-measure rests

/** Bars where every staff rests (two or more in a row, in one meter) become one
 * multi-measure rest. Returns the new measures and renumbers the events. */
/** function multiRests(staves: Staff[], ms: Measure[]) => Measure[] */
function multiRests(staves, ms) {
  /** const empty: Boolean[] */
  const empty = ms.map((_) => true);
  for (const st of staves) for (const ev of st.events) if (!ev.whole) empty[ev.measure] = false;
  /** const out: Measure[] */
  const out = [];
  /** const map: Int[] */
  const map = [];
  let i = 0;
  while (i < ms.length) {
    let j = i + 1;
    if (empty[i]) while (j < ms.length && empty[j] && !ms[j].meter && ms[j].length === ms[i].length) j = j + 1;
    const m = ms[i];
    if (j - i >= 2)
      out.push({ start: m.start, length: m.length * (j - i), num: m.num, den: m.den, beat: m.beat, meter: m.meter, number: m.number, count: j - i });
    else {
      j = i + 1;
      out.push(m);
    }
    for (let k = i; k < j; k++) map.push(out.length - 1);
    i = j;
  }
  if (out.length === ms.length) return ms;
  for (const st of staves) {
    /** const evs: NEv[] */
    const evs = [];
    for (const ev of st.events) {
      const to = map[ev.measure];
      if (out[to].count > 1) {
        if (out[to].start !== ev.start) continue;
        evs.push({ start: ev.start, dur: out[to].length, base: 192, dots: 0, rest: true, whole: true, heads: [], beam: -1, measure: to, tuplet: -1 });
      } else {
        ev.measure = to;
        evs.push(ev);
      }
    }
    st.events = evs;
  }
  return out;
}

// ------------------------------------------------------------------ marks

/** function resolveMarks(p: Project, scope: Scope) => MarkSpan[] */
function resolveMarks(p, scope) {
  /** const out: MarkSpan[] */
  const out = [];
  const marks = p.score.marks;
  for (let i = 0; i < marks.length; i++) {
    const m = marks[i];
    if (m.pattern === "") {
      if (scope.kind !== "pattern") out.push({ start: m.start, end: m.end, color: m.color, label: m.label, channels: m.channels, mark: i });
      continue;
    }
    for (const o of occurrences(p, scope, m.pattern)) {
      const s = Math.max(o.from, o.origin + m.start);
      const e = Math.min(o.to, o.origin + m.end);
      if (e > s) out.push({ start: s, end: e, color: m.color, label: m.label, channels: m.channels, mark: i });
    }
  }
  return out;
}

// ------------------------------------------------------------------ build

/** Write a scope of the project as a score. `grid`: quantization in ticks (12 = sixteenths). */
/** function buildScore(p: Project, scope: Scope, grid: Int) => Score */
export function buildScore(p, scope, grid) {
  const g = gather(p, scope);
  const notes = g.notes;
  const settings = p.score;

  // Parts: channels in rack order, with notes here, not hidden. Synthesized
  // drums (one channel per drum) share one drum staff, like a drum kit.
  /** const parts: Part[] */
  const parts = [];
  for (const c of p.channels) {
    if (settings.hidden.includes(c.id)) continue;
    /** const idx: Int[] */
    const idx = [];
    for (let i = 0; i < notes.length; i++) if (notes[i].channel === c.id) idx.push(i);
    if (idx.length === 0) continue;
    const kit = drumKit(c);
    const set = settings.clefs.find((x) => x.key === c.id);
    const clef = set && set.value !== "auto" ? set.value : kit !== "" ? "percussion" : autoClef(idx.map((i) => notes[i].pitch));
    const kitPart = parts.find((x) => x.kit === "synth");
    if (kit === "synth" && clef === "percussion" && kitPart) {
      kitPart.channels.push(c);
      for (const i of idx) kitPart.idx.push(i);
      kitPart.name = "Drums";
      continue;
    }
    parts.push({ channels: [c], idx: idx, kit: kit, clef: clef, name: c.name, color: c.color });
  }

  // The key: named, or guessed from the pitched notes.
  let fifths = 0;
  let minor = false;
  let keyName = "C";
  const named = findKey(settings.key);
  if (named) {
    fifths = named.fifths;
    minor = named.minor;
    keyName = named.name;
  } else {
    // Guessed from the whole song (or, without one, from what is shown), so
    // every view of a part is written in the same key.
    const song = scope.kind === "song" ? notes : gather(p, { kind: "song", track: 0, pattern: "" }).notes;
    const source = song.length >= 4 ? song : notes;
    /** const drums: String[] */
    const drums = [];
    for (const c of p.channels) if (drumKit(c) !== "" || settings.clefs.some((x) => x.key === c.id && x.value === "percussion")) drums.push(c.id);
    /** const pitched: VoiceNote[] */
    const pitched = [];
    for (const n of source) if (!drums.includes(n.channel)) pitched.push({ start: n.start, end: n.end, pitch: n.pitch, velocity: n.velocity });
    if (pitched.length >= 4) {
      const guess = detectKey(pitched);
      minor = guess.scale === "minor";
      keyName = keyNameOf(guess.key, minor);
      const k = findKey(keyName);
      fifths = k ? k.fifths : 0;
    }
  }

  let endBeat = g.length;
  for (const n of notes) endBeat = Math.max(endBeat, n.end);
  const endTick = Math.max(1, Math.ceil(endBeat * TPQ - 1e-6));
  // A pattern's bars start at its beat 0 whatever the song's meters say there.
  const ms = measuresFor(p.transport, endTick);

  /** const staves: Staff[] */
  const staves = [];
  let beamId = 0;
  for (const pt of parts) {
    const drum = pt.clef === "percussion";
    const kit = drum ? (pt.kit === "" ? "gm" : pt.kit) : "";
    const clefs = pt.clef === "grand" ? ["treble", "bass"] : [pt.clef];
    const group = staves.length;
    const ids = pt.channels.map((c) => c.id);
    for (let part = 0; part < clefs.length; part++) {
      /** const idx: Int[] */
      const idx = [];
      for (const i of pt.idx) {
        if (clefs.length === 1 || (part === 0 ? notes[i].pitch >= SPLIT : notes[i].pitch < SPLIT)) idx.push(i);
      }
      idx.sort((a, b) => a - b);
      const trip = grid <= 12 ? tripletBeats(notes, idx, ms, grid) : [];
      const chords = chordsOf(notes, idx, grid, drum, ms, trip);
      const heads = chords.map((ch) => headsOf(p, notes, ch, fifths, kit));
      const evs = eventsOf(chords, heads, ms, trip);
      if (!drum) markAccidentals(evs, fifths);
      beamId = markBeams(evs, ms, beamId);
      staves.push({ channel: ids[0], channels: ids, name: pt.name, color: pt.color, clef: clefs[part], drum: drum, part: part, group: group, events: evs });
    }
  }

  return {
    staves: staves,
    measures: staves.length > 0 ? multiRests(staves, ms) : ms,
    fifths: fifths,
    minor: minor,
    keyName: keyName,
    notes: notes,
    marks: resolveMarks(p, scope),
    bpm: p.transport.bpm,
    end: endTick,
    empty: staves.length === 0,
  };
}
