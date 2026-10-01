// Patterns used by reference (Pattern.uses), as the piano roll edits them:
// the box around what a use brings in, turning a use into plain notes
// (Make unique), notes into a new pattern played where they were (Make
// reference), and taking a use out. Edits that move notes between patterns
// keep each note's words (keepWords). Call them inside `commit`.

import { patternIndex, useNotes } from "./expand.js";
import { keepWords } from "./lyricedit.js";
import { semitonesText, uniqueId, paletteColor, round6 } from "./model.js";

/** What one use brings into a pattern: its notes (in the pattern's beats) and the box around them. */
/** type UseBox = { use: Int, notes: Sounding[], start: Number, end: Number, low: Number, high: Number, label: String } */

/** How a use reads on its box: the used pattern, then how it is changed ("Hook +5 v2 → bass"). */
/** function useLabel(p: Project, u: Use) => String */
export function useLabel(p, u) {
  const j = patternIndex(p, u.pattern);
  let s = j >= 0 ? p.patterns[j].name : `${u.pattern}?`;
  if (u.transpose !== 0) s = `${s} ${semitonesText(u.transpose)}`;
  if (u.verse > 0) s = `${s} v${u.verse}`;
  if (u.channel !== "") {
    const c = p.channels.find((x) => x.id === u.channel);
    s = `${s} → ${c ? c.name : u.channel}`;
  }
  return s;
}

/** The beats a use spans in the using pattern. */
/** function useSpan(p: Project, u: Use) => Number */
export function useSpan(p, u) {
  const j = patternIndex(p, u.pattern);
  const to = u.to >= 0 ? u.to : j >= 0 ? p.patterns[j].length : u.from;
  return Math.max(0, to - u.from);
}

/** The boxes of every use of patterns[index] (singing `verse`). */
/** function useBoxes(p: Project, index: Int, verse: Int) => UseBox[] */
export function useBoxes(p, index, verse) {
  /** const out: UseBox[] */
  const out = [];
  const uses = p.patterns[index].uses;
  for (let k = 0; k < uses.length; k++) {
    const u = uses[k];
    const notes = useNotes(p, index, k, verse);
    let low = notes.length > 0 ? 127 : 60;
    let high = notes.length > 0 ? 0 : 64;
    for (const n of notes) {
      low = Math.min(low, n.pitch);
      high = Math.max(high, n.pitch);
    }
    out.push({ use: k, notes: notes, start: u.start, end: u.start + Math.max(0.25, useSpan(p, u)), low: low, high: high, label: useLabel(p, u) });
  }
  return out;
}

/** Make use `k` of patterns[index] plain notes of the pattern (the notes it sounds, words kept).
 * Returns the indexes of the new notes. */
/** function makeUnique(p: Project, index: Int, k: Int) => Int[] */
export function makeUnique(p, index, k) {
  const pat = p.patterns[index];
  /** const fresh: Int[] */
  const fresh = [];
  keepWords(p, index, [], () => {
    const notes = useNotes(p, index, k, 1);
    pat.uses.splice(k, 1);
    for (const n of notes) {
      pat.notes.push({ channel: n.channel, pitch: n.pitch, start: round6(n.start), length: round6(n.length), velocity: round6(n.velocity) });
      fresh.push(pat.notes.length - 1);
    }
  });
  return fresh;
}

/** Take use `k` out of patterns[index] (the words of the notes that stay are kept). */
/** function removeUse(p: Project, index: Int, k: Int) => Undefined */
export function removeUse(p, index, k) {
  keepWords(p, index, [], () => {
    p.patterns[index].uses.splice(k, 1);
  });
}

/** A name for a pattern made from part of another ("Verse part 2"). */
/** function partName(p: Project, base: String) => String */
function partName(p, base) {
  let k = 1;
  while (p.patterns.some((x) => x.name === `${base} part ${k}`)) k = k + 1;
  return `${base} part ${k}`;
}

/** Move notes of patterns[index] (`picked`: indexes into its notes) into a new pattern that it
 * uses where they were: times from the beat the first one is in, its length whole beats.
 * Returns the new pattern's id ("" when nothing is picked). */
/** function makeReference(p: Project, index: Int, picked: Int[]) => String */
export function makeReference(p, index, picked) {
  const pat = p.patterns[index];
  const notes = picked.filter((i) => i >= 0 && i < pat.notes.length).map((i) => pat.notes[i]);
  if (notes.length === 0) return "";
  let first = Infinity;
  let last = 0;
  for (const n of notes) {
    first = Math.min(first, n.start);
    last = Math.max(last, n.start + n.length);
  }
  const origin = Math.floor(first + 1e-9);
  const name = partName(p, pat.name);
  const id = uniqueId(
    name,
    p.patterns.map((x) => x.id)
  );
  keepWords(p, index, [pat.uses.length], () => {
    p.patterns.push({
      id: id,
      name: name,
      color: paletteColor(p.patterns.length + 2),
      length: Math.max(1, Math.ceil(last - origin - 1e-9)),
      notes: notes.map((n) => ({ channel: n.channel, pitch: n.pitch, start: round6(n.start - origin), length: n.length, velocity: n.velocity })),
      uses: [],
      lyrics: [],
    });
    pat.notes = pat.notes.filter((n) => !notes.includes(n));
    pat.uses.push({ pattern: id, start: origin, from: 0, to: -1, transpose: 0, channel: "", velocity: 1, verse: 0 });
  });
  return id;
}
