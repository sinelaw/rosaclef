// Voice to notes: from an analyzed take (GET /api/transcribe, see
// crates/studio/src/transcribe.rs) to notes on the beat grid.
//
// The server finds the notes (seconds, fractional pitches) or the drum hits
// (seconds, which drum) once; everything here — cropping, quantizing,
// snapping to a scale, keeping hits by strength — is cheap and runs on every
// redraw, so the settings change the result instantly.

// ------------------------------------------------------------------ scales

/** type Scale = { id: String, label: String, steps: Int[] } */

/** const SCALES: Scale[] */
export const SCALES = [
  { id: "chromatic", label: "Chromatic", steps: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] },
  { id: "major", label: "Major", steps: [0, 2, 4, 5, 7, 9, 11] },
  { id: "minor", label: "Minor", steps: [0, 2, 3, 5, 7, 8, 10] },
  { id: "harmonic", label: "Harmonic minor", steps: [0, 2, 3, 5, 7, 8, 11] },
  { id: "dorian", label: "Dorian", steps: [0, 2, 3, 5, 7, 9, 10] },
  { id: "mixolydian", label: "Mixolydian", steps: [0, 2, 4, 5, 7, 9, 10] },
  { id: "penta", label: "Major pentatonic", steps: [0, 2, 4, 7, 9] },
  { id: "minpenta", label: "Minor pentatonic", steps: [0, 3, 5, 7, 10] },
  { id: "blues", label: "Blues", steps: [0, 3, 5, 6, 7, 10] },
];

export const KEY_NAMES = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"];

/** function scaleSteps(id: String) => Int[] */
export function scaleSteps(id) {
  for (const s of SCALES) {
    if (s.id === id) return s.steps;
  }
  return SCALES[0].steps;
}

/** function scaleLabel(id: String) => String */
export function scaleLabel(id) {
  for (const s of SCALES) {
    if (s.id === id) return s.label;
  }
  return SCALES[0].label;
}

/** function pitchClass(p: Number) => Int */
function pitchClass(p) {
  return ((Math.round(p) % 12) + 12) % 12;
}

/** The scale note nearest to a fractional pitch (auto-tune). With a
 * chromatic scale this is the nearest semitone. */
/** function snapPitch(p: Number, key: Int, steps: Int[]) => Int */
export function snapPitch(p, key, steps) {
  let best = Math.round(p);
  let dist = 99;
  for (let m = Math.floor(p) - 2; m <= Math.ceil(p) + 2; m++) {
    if (!steps.includes((((m - key) % 12) + 12) % 12)) continue;
    const d = Math.abs(m - p);
    if (d < dist - 1e-9) {
      dist = d;
      best = m;
    }
  }
  return best;
}

// Krumhansl–Kessler key profiles.
const MAJOR_PROFILE = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR_PROFILE = [6.33, 2.68, 3.52, 5.38, 2.6, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];

/** type KeyGuess = { key: Int, scale: String } */

/** function correlate(hist: Number[], profile: Number[], key: Int) => Number */
function correlate(hist, profile, key) {
  let mh = 0;
  let mp = 0;
  for (let i = 0; i < 12; i++) {
    mh = mh + hist[i] / 12;
    mp = mp + profile[i] / 12;
  }
  let num = 0;
  let dh = 0;
  let dp = 0;
  for (let i = 0; i < 12; i++) {
    const h = hist[(i + key) % 12] - mh;
    const q = profile[i] - mp;
    num = num + h * q;
    dh = dh + h * h;
    dp = dp + q * q;
  }
  return dh > 0 && dp > 0 ? num / Math.sqrt(dh * dp) : 0;
}

/** The key the notes fit best (duration-weighted Krumhansl–Schmuckler). */
/** function detectKey(notes: VoiceNote[]) => KeyGuess */
export function detectKey(notes) {
  /** const hist: Number[] */
  const hist = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
  for (const n of notes) {
    const pc = pitchClass(n.pitch);
    hist[pc] = hist[pc] + Math.max(0.05, n.end - n.start);
  }
  let best = { key: 0, scale: "major" };
  let score = -2;
  for (let k = 0; k < 12; k++) {
    const maj = correlate(hist, MAJOR_PROFILE, k);
    if (maj > score) {
      score = maj;
      best = { key: k, scale: "major" };
    }
    const min = correlate(hist, MINOR_PROFILE, k);
    if (min > score) {
      score = min;
      best = { key: k, scale: "minor" };
    }
  }
  return best;
}

// ------------------------------------------------------------------ time

/** Move `t` towards the grid by `strength` (0 = as played, 1 = on the grid). */
/** function quantize(t: Number, grid: Number, strength: Number) => Number */
export function quantize(t, grid, strength) {
  if (grid <= 0) return t;
  const q = Math.round(t / grid) * grid;
  return t + (q - t) * Math.max(0, Math.min(1, strength));
}

/** function round4(x: Number) => Number */
function round4(x) {
  return Math.round(x * 10000) / 10000;
}

// ------------------------------------------------------------------ drums

/** Onset strength a hit needs at a sensitivity (0..1). */
/** function strengthNeeded(sensitivity: Number) => Number */
export function strengthNeeded(sensitivity) {
  const s = 1 - Math.max(0, Math.min(1, sensitivity));
  return 0.02 + 0.4 * s * s;
}

/** The drums a hit can be (crates/studio/src/transcribe/drums.rs). */
export const DRUMS = ["kick", "tom", "snare", "hat", "openhat"];

/** function drumLabel(kind: String) => String */
export function drumLabel(kind) {
  if (kind === "openhat") return "open hat";
  return kind;
}

/** The next drum when a hit is clicked. */
/** function nextDrum(kind: String) => String */
export function nextDrum(kind) {
  const i = DRUMS.indexOf(kind);
  return DRUMS[(i + 1) % DRUMS.length];
}

// ------------------------------------------------------------------ result

/** Settings shared by both modes; see the Voice panel (ui/voice.js). */
/** `detail` picks one of the take's detail levels (0 smooth … 4 every note). */
/** type VoiceSettings = { detail: Int, grid: Number, strength: Number, lengths: Boolean, key: Int, scale: String, octave: Int, legato: Boolean, dynamics: Boolean, sensitivity: Number, separation: Number, bars: Int } */

/** The detail levels of the Detail control, smoothest first (the server's
 * DETAIL_CHANGES in crates/studio/src/transcribe.rs). */
export const DETAILS = ["Smooth", "Clean", "Balanced", "Detailed", "Every note"];
export const DEFAULT_DETAIL = 2;

/** The notes of a melody take at a detail level (the default level's when
 * the take has no such level). */
/** function sungNotes(take: Take, detail: Int) => VoiceNote[] */
export function sungNotes(take, detail) {
  if (detail >= 0 && detail < take.details.length) return take.details[detail];
  return take.notes;
}

/** A resolved key: `key` is -1 for "detect". */
/** function resolveKey(take: Take, s: VoiceSettings) => KeyGuess */
export function resolveKey(take, s) {
  if (s.key >= 0) return { key: s.key, scale: s.scale };
  const g = detectKey(sungNotes(take, s.detail));
  // A detected key keeps the chosen scale unless it is major/minor.
  if (s.scale === "major" || s.scale === "minor") return g;
  return { key: g.key, scale: s.scale };
}

/** When the take starts on the grid (s): the first note or hit when the
 * take was sung without the song, else the start of the take. */
/** function takeStart(take: Take, detail: Int, aligned: Boolean) => Number */
export function takeStart(take, detail, aligned) {
  if (!aligned) return 0;
  if (take.mode === "drums") {
    for (const h of take.hits) {
      if (h.strength >= 0) return h.time;
    }
    return 0;
  }
  const notes = sungNotes(take, detail);
  return notes.length > 0 ? notes[0].start : 0;
}

/** Melody: notes on the grid, snapped to the scale. `origin` is the beat
 * the take's start lands on. */
/** function melodyNotes(take: Take, s: VoiceSettings, bpm: Number, origin: Number, aligned: Boolean) => Placed[] */
export function melodyNotes(take, s, bpm, origin, aligned) {
  const k = resolveKey(take, s);
  const steps = scaleSteps(k.scale);
  const t0 = takeStart(take, s.detail, aligned);
  const bps = bpm / 60;
  const minLen = s.grid > 0 ? s.grid : 0.0625;
  const sung = sungNotes(take, s.detail);
  /** const out: Placed[] */
  const out = [];
  for (let i = 0; i < sung.length; i++) {
    const n = sung[i];
    const raw = origin + (n.start - t0) * bps;
    const rawEnd = origin + (n.end - t0) * bps;
    const start = Math.max(0, quantize(raw, s.grid, s.strength));
    let end = s.lengths ? quantize(rawEnd, s.grid, s.strength) : start + (rawEnd - raw);
    if (end - start < minLen) end = start + minLen;
    const pitch = Math.max(0, Math.min(127, snapPitch(n.pitch + 12 * s.octave, k.key, steps)));
    const note = { lane: "melody", pitch: pitch, start: round4(start), length: round4(end - start), velocity: s.dynamics ? n.velocity : 0.8, raw: raw, src: i };
    // Two notes on one grid slot: the longer one wins.
    const at = out.length - 1;
    if (at >= 0 && Math.abs(out[at].start - note.start) < 1e-6) {
      if (note.length > out[at].length) out[at] = note;
      continue;
    }
    out.push(note);
  }
  // One voice: a note ends where the next begins; legato holds every note
  // until the next one.
  for (let i = 0; i + 1 < out.length; i++) {
    const a = out[i];
    const next = out[i + 1].start;
    if (a.start + a.length > next || s.legato) a.length = round4(Math.max(minLen / 2, next - a.start));
  }
  return out;
}

/** Each hit's time once the kept hits (strength at least `need`) closer
 * than `separation` seconds to the first of their group have joined it: a
 * flam, or one sound heard as two, becomes one moment. Weaker hits keep
 * their time and neither start nor join a group. */
/** function joinedTimes(take: Take, need: Number, separation: Number) => Number[] */
export function joinedTimes(take, need, separation) {
  /** const out: Number[] */
  const out = [];
  let open = false;
  let first = 0;
  for (const h of take.hits) {
    if (h.strength < need) {
      out.push(h.time);
    } else if (open && h.time - first < separation) {
      out.push(first);
    } else {
      open = true;
      first = h.time;
      out.push(h.time);
    }
  }
  return out;
}

/** Beatbox: the hits kept by the sensitivity, on the grid, each as the drum
 * the server heard (`kinds` overrides that per hit; "" = as heard). */
/** function drumHits(take: Take, s: VoiceSettings, bpm: Number, origin: Number, aligned: Boolean, kinds: String[]) => Placed[] */
export function drumHits(take, s, bpm, origin, aligned, kinds) {
  const need = strengthNeeded(s.sensitivity);
  const times = joinedTimes(take, need, s.separation);
  // The first kept hit starts the loop.
  let t0 = 0;
  if (aligned) {
    for (const h of take.hits) {
      if (h.strength >= need) {
        t0 = h.time;
        break;
      }
    }
  }
  const bps = bpm / 60;
  const len = s.grid > 0 ? s.grid : 0.25;
  /** const out: Placed[] */
  const out = [];
  for (let i = 0; i < take.hits.length; i++) {
    const h = take.hits[i];
    if (h.strength < need) continue;
    const kind = i < kinds.length && kinds[i] !== "" ? kinds[i] : h.kind;
    // Hits joined to the one before land with it; the same drum twice there
    // is one hit (below).
    const raw = origin + (times[i] - t0) * bps;
    const start = round4(Math.max(0, quantize(raw, s.grid, s.strength)));
    const velocity = s.dynamics ? h.velocity : 0.8;
    // The same drum twice on one slot: keep the louder.
    const twin = out.findIndex((o) => o.lane === kind && Math.abs(o.start - start) < 1e-6);
    if (twin >= 0) {
      if (velocity > out[twin].velocity) out[twin].velocity = velocity;
      continue;
    }
    out.push({ lane: kind, pitch: 60, start: start, length: len, velocity: velocity, raw: raw, src: i });
  }
  return out;
}

/** Pattern length in beats: whole bars around the notes, or `bars` bars
 * (notes past the end are left out). */
/** function loopBeats(notes: Placed[], beatsPerBar: Number, bars: Int) => Number */
export function loopBeats(notes, beatsPerBar, bars) {
  if (bars > 0) return bars * beatsPerBar;
  let end = 0;
  for (const n of notes) end = Math.max(end, n.start + (n.lane === "melody" ? n.length : 0.001));
  return Math.max(1, Math.ceil(end / beatsPerBar - 1e-6)) * beatsPerBar;
}

/** function decodeNote<T>(n: T) => VoiceNote */
function decodeNote(n) {
  return { start: Number(n.start), end: Number(n.end), pitch: Number(n.pitch), velocity: Number(n.velocity) };
}

/** function decodeTake<T>(r: T) => Take */
export function decodeTake(r) {
  return {
    mode: String(r.mode),
    duration: Number(r.duration),
    step: Number(r.step),
    level: (r.level ?? []).map((v) => Number(v)),
    contour: (r.contour ?? []).map((v) => Number(v)),
    notes: (r.notes ?? []).map((n) => decodeNote(n)),
    details: (r.details ?? []).map((d) => d.map((n) => decodeNote(n))),
    hits: (r.hits ?? []).map((h) => ({
      time: Number(h.time),
      strength: Number(h.strength),
      velocity: Number(h.velocity),
      kind: String(h.kind),
    })),
  };
}

/** function emptyTake() => Take */
export function emptyTake() {
  return { mode: "", duration: 0, step: 0.01, level: [], contour: [], notes: [], details: [], hits: [] };
}

// ------------------------------------------------------------------ crop

/** Shortest note a crop leaves (seconds). */
const CROP_MIN_NOTE = 0.03;

/** function clipNotes(notes: VoiceNote[], a: Number, b: Number) => VoiceNote[] */
function clipNotes(notes, a, b) {
  /** const out: VoiceNote[] */
  const out = [];
  for (const n of notes) {
    const start = Math.max(n.start, a);
    const end = Math.min(n.end, b);
    if (end - start < CROP_MIN_NOTE) continue;
    out.push({ start: round4(start - a), end: round4(end - a), pitch: n.pitch, velocity: n.velocity });
  }
  return out;
}

/** The part of a take between `a` and `b` seconds, with time 0 at `a`.
 * Notes are cut at the edges (and dropped when little is left); hits
 * outside keep their place in the list (the per-hit drum overrides index
 * it) with strength -1, so no sensitivity keeps them. */
/** function cropTake(take: Take, a: Number, b: Number) => Take */
export function cropTake(take, a, b) {
  const lo = Math.max(0, Math.min(a, take.duration));
  const hi = Math.max(lo, Math.min(b, take.duration));
  if (lo <= 0 && hi >= take.duration) return take;
  const step = take.step > 0 ? take.step : 0.01;
  const i0 = Math.round(lo / step);
  const i1 = Math.round(hi / step);
  return {
    mode: take.mode,
    duration: round4(hi - lo),
    step: take.step,
    level: take.level.slice(i0, i1),
    contour: take.contour.slice(i0, i1),
    notes: clipNotes(take.notes, lo, hi),
    details: take.details.map((d) => clipNotes(d, lo, hi)),
    hits: take.hits.map((h) => {
      const inside = h.time >= lo && h.time < hi;
      return { time: round4(h.time - lo), strength: inside ? h.strength : -1, velocity: h.velocity, kind: h.kind };
    }),
  };
}
