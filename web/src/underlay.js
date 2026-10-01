// Sheet music, the words: the syllables notation.js puts under a staff
// (`Staff.lyrics`), engraved — centered under their notes, flush left with
// the note when held over others (a melisma), on one baseline per verse
// line below the staff's ink; hyphens centered between the syllables of a
// word; an extender line under the notes a word's last syllable is held
// over; "1." "2." at the start of each line when verses are stacked. The
// spacing makes room for the syllables, as it does for accidentals.

import { G } from "./smufl.js";
import { rect, extend } from "./paint.js";
// For its types (Staff, Syl).
import "./notation.js";

/** Where a syllable's text goes, from a note at `cx`: its ink from x0 to x1, written at `tx` anchored at `anchor`. */
/** type SylBox = { x0: Number, x1: Number, tx: Number, anchor: String } */
/** How far a syllable reaches left and right of its note (for spacing). */
/** type SylExtent = { ev: Int, left: Number, right: Number } */

/** Lyric text size, in staff spaces. */
export const LYRIC_SIZE = 1.6;
/** From one verse line to the next. */
const LINE = 2.2;
/** A hyphen: its length and thickness, how high above the baseline, the room it needs. */
const HYPHEN_W = 0.7;
const HYPHEN_T = 0.12;
const HYPHEN_Y = 0.48;
const HYPHEN_ROOM = 1.5;
const EXTENDER_T = 0.1;

/** Rough width of a syllable at the lyric size (a book face). */
/** function sylWidth(text: String) => Number */
export function sylWidth(text) {
  return Array.from(text).length * 0.48 * LYRIC_SIZE;
}

/** Whether a syllable is held over notes after its own with nothing following in its word (it gets an extender). */
/** function melisma(s: Syl) => Boolean */
function melisma(s) {
  return s.until > s.ev && !s.hyphen;
}

/** Where a syllable's text goes under a note whose column is at `cx`. */
/** function sylBox(s: Syl, cx: Number) => SylBox */
export function sylBox(s, cx) {
  const w = sylWidth(s.text);
  if (melisma(s)) return { x0: cx - 0.1, x1: cx - 0.1 + w, tx: cx - 0.1, anchor: "start" };
  const mid = cx + G.noteheadBlack.x1 / 2;
  return { x0: mid - w / 2, x1: mid + w / 2, tx: mid, anchor: "middle" };
}

/** How far each syllable reaches around its note, room for a hyphen after it included. */
/** function lyricExtents(st: Staff) => SylExtent[] */
export function lyricExtents(st) {
  return st.lyrics.map((s) => {
    const box = sylBox(s, 0);
    return { ev: s.ev, left: Math.max(0, -box.x0), right: box.x1 + (s.hyphen ? HYPHEN_ROOM : 0.5) };
  });
}

/** The lines of a staff's lyrics: one per stacked verse. */
/** function lineCount(st: Staff) => Int */
function lineCount(st) {
  let n = 0;
  for (const s of st.lyrics) n = Math.max(n, s.row + 1);
  return n;
}

/** Draw the lyrics of a staff in a system (measures [a, b), music from x0 to x1): text as
 * labels (staff coordinates), hyphens and extenders as ink, below everything drawn so far. */
/** function drawLyrics(p: Inker, labels: Label[], st: Staff, a: Int, b: Int, colAt: (Int) => Number, x0: Number, x1: Number) => Undefined */
export function drawLyrics(p, labels, st, a, b, colAt, x0, x1) {
  const lines = lineCount(st);
  if (lines === 0) return undefined;
  /** function inside(i: Int) => Boolean */
  const inside = (i) => st.events[i].measure >= a && st.events[i].measure < b;
  const base = Math.max(6.4, p.bottom + 2.1);
  /** const shown: Int[] */
  const shown = [];
  for (let k = 0; k < st.lyrics.length; k++) {
    const s = st.lyrics[k];
    const next = nextOnLine(st.lyrics, k);
    const y = base + s.row * LINE;
    if (inside(s.ev)) {
      const box = sylBox(s, colAt(st.events[s.ev].start));
      labels.push({ x: box.tx, y: y, text: s.text, cls: "lyric", anchor: box.anchor, color: "" });
      if (!shown.includes(s.row)) shown.push(s.row);
      connect(p, st, s, next, box.x1, y, inside, colAt, x1);
    } else if (st.events[s.ev].measure < a && reaches(st, s, next, a)) {
      // Carried over from the system before: the hyphen or extender goes on from the start.
      connect(p, st, s, next, x0 - 0.6, y, inside, colAt, x1);
    }
  }
  extend(p, base - LYRIC_SIZE, base + (lines - 1) * LINE + 0.6);
  if (lines > 1) verseNumbers(labels, st, shown, inside, base, x0);
}

/** The next syllable on the same line after the `k`-th, or undefined. */
/** function nextOnLine(syls: Syl[], k: Int) => Syl | Undefined */
function nextOnLine(syls, k) {
  for (let j = k + 1; j < syls.length; j++) if (syls[j].row === syls[k].row) return syls[j];
  return undefined;
}

/** Whether a syllable from before measure `a` still reaches into it (by a hyphen or an extender). */
/** function reaches(st: Staff, s: Syl, next: Syl | Undefined, a: Int) => Boolean */
function reaches(st, s, next, a) {
  if (s.hyphen) return next !== undefined && st.events[next.ev].measure >= a;
  return melisma(s) && st.events[s.until].measure >= a;
}

/** What follows a syllable whose text ends at `x`: a hyphen to the next syllable of its word,
 * or an extender to the end of the last note it is held over (each cut at the system's end). */
/** function connect(p: Inker, st: Staff, s: Syl, next: Syl | Undefined, x: Number, y: Number, inside: (Int) => Boolean, colAt: (Int) => Number, x1: Number) => Undefined */
function connect(p, st, s, next, x, y, inside, colAt, x1) {
  if (s.hyphen) {
    if (next === undefined) return undefined;
    const to = inside(next.ev) ? sylBox(next, colAt(st.events[next.ev].start)).x0 : x1;
    hyphens(p, x, to, y);
  } else if (melisma(s)) {
    const to = inside(s.until) ? colAt(st.events[s.until].start) + G.noteheadBlack.x1 : x1;
    if (to - x > 0.5) rect(p, x + 0.15, y + 0.05, to - x - 0.15, EXTENDER_T, "");
  }
}

/** Hyphens centered between two syllables: one, or more spread over a wide gap. */
/** function hyphens(p: Inker, x0: Number, x1: Number, y: Number) => Undefined */
function hyphens(p, x0, x1, y) {
  const gap = x1 - x0;
  if (gap < HYPHEN_W * 0.6) return undefined;
  const w = Math.min(HYPHEN_W, gap * 0.6);
  const n = Math.max(1, Math.floor(gap / 7));
  for (let i = 0; i < n; i++) {
    const mid = x0 + (gap * (i + 0.5)) / n;
    rect(p, mid - w / 2, y - HYPHEN_Y, w, HYPHEN_T, "");
  }
}

/** "1." "2." before each line of stacked verses shown in the system (the verse of its first syllable here). */
/** function verseNumbers(labels: Label[], st: Staff, shown: Int[], inside: (Int) => Boolean, base: Number, x0: Number) => Undefined */
function verseNumbers(labels, st, shown, inside, base, x0) {
  for (const row of shown) {
    const first = st.lyrics.find((s) => s.row === row && inside(s.ev));
    if (first !== undefined) labels.push({ x: x0 - 0.7, y: base + row * LINE, text: `${first.verse}.`, cls: "lyric num", anchor: "end", color: "" });
  }
}
