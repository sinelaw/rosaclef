// Patterns as they sound: uses resolved, lyrics on their notes. Mirrors
// crates/core/src/expand.rs, so the piano roll and the score show what the
// engine plays.
//
// A pattern's notes are its own plus those of the patterns it uses (a range
// of another pattern, moved in time and pitch, maybe onto another channel).
// A pattern's lyric line gives its words, in time order, to the notes on its
// channel that its uses did not already give words. Notes a use moves to
// another channel leave their words behind.

import { parseLyrics, verseText } from "./lyrics.js";
import { drumKit } from "./model.js";

/** Nesting deeper than this is cut (validation reports cycles). */
const MAX_DEPTH = 16;

/** The syllable a note sings: the token, and where it is written (`patterns[pattern].lyrics[line]`, the verse sung, the token's index in it, the note's beat in that pattern). */
/** type Sung = { token: LyricToken, pattern: Int, line: Int, verse: Int, index: Int, at: Number } */
/** A note as it sounds in a pattern (its beats); `pattern`/`note` say where it is written. */
/** type Sounding = { channel: String, pitch: Number, start: Number, length: Number, velocity: Number, pattern: Int, note: Int, sung: Sung | Undefined } */

/** The index of the pattern with this id, or -1. */
/** function patternIndex(p: Project, id: String) => Int */
export function patternIndex(p, id) {
  for (let i = 0; i < p.patterns.length; i++) if (p.patterns[i].id === id) return i;
  return -1;
}

/** The sounding notes of `patterns[index]` singing `verse`, sorted by start. */
/** function expandPattern(p: Project, index: Int, verse: Int) => Sounding[] */
export function expandPattern(p, index, verse) {
  return expand(p, index, verse, []);
}

/** function expand(p: Project, index: Int, verse: Int, stack: Int[]) => Sounding[] */
function expand(p, index, verse, stack) {
  if (stack.includes(index) || stack.length >= MAX_DEPTH) return [];
  stack.push(index);
  const out = gather(p, index, verse, stack);
  const pat = p.patterns[index];
  for (let k = 0; k < pat.lyrics.length; k++) bind(out, index, k, pat.lyrics[k], verse);
  stack.pop();
  return out;
}

/** function gather(p: Project, index: Int, verse: Int, stack: Int[]) => Sounding[] */
function gather(p, index, verse, stack) {
  const pat = p.patterns[index];
  /** const out: Sounding[] */
  const out = pat.notes.map((n, k) => ({
    channel: n.channel,
    pitch: n.pitch,
    start: n.start,
    length: n.length,
    velocity: n.velocity,
    pattern: index,
    note: k,
    sung: undefined,
  }));
  for (const u of pat.uses) for (const n of brought(p, u, verse, stack)) out.push(n);
  out.sort((a, b) => a.start - b.start);
  return out;
}

/** The notes a use brings in, placed in the using pattern. */
/** function brought(p: Project, u: Use, verse: Int, stack: Int[]) => Sounding[] */
function brought(p, u, verse, stack) {
  const j = patternIndex(p, u.pattern);
  if (j < 0) return [];
  return place(p, u, p.patterns[j].length, expand(p, j, u.verse > 0 ? u.verse : verse, stack));
}

/** The sounding notes `patterns[index].uses[k]` brings in (singing `verse`), sorted by start. */
/** function useNotes(p: Project, index: Int, k: Int, verse: Int) => Sounding[] */
export function useNotes(p, index, k, verse) {
  const out = brought(p, p.patterns[index].uses[k], verse, [index]);
  out.sort((a, b) => a.start - b.start);
  return out;
}

/** Whether a channel's instrument plays its notes at their pitch (not drums). */
/** function pitched(p: Project, channel: String) => Boolean */
function pitched(p, channel) {
  for (const c of p.channels) if (c.id === channel) return drumKit(c) === "";
  return true;
}

/** The used pattern's notes in its range, moved into the using pattern. */
/** function place(p: Project, u: Use, usedLength: Number, notes: Sounding[]) => Sounding[] */
function place(p, u, usedLength, notes) {
  const to = u.to >= 0 ? u.to : usedLength;
  /** const out: Sounding[] */
  const out = [];
  for (const n of notes) {
    if (n.start < u.from - 1e-9 || n.start >= to - 1e-9) continue;
    const channel = u.channel !== "" ? u.channel : n.channel;
    // A doubling on another instrument: the words stay with the voice.
    const moved = channel !== n.channel;
    out.push({
      channel: channel,
      pitch: pitched(p, channel) ? Math.max(0, Math.min(127, n.pitch + u.transpose)) : n.pitch,
      start: u.start + (n.start - u.from),
      length: Math.min(n.length, to - n.start),
      velocity: Math.max(0, Math.min(1, n.velocity * u.velocity)),
      pattern: n.pattern,
      note: n.note,
      sung: moved ? undefined : n.sung,
    });
  }
  return out;
}

/** Give a lyric line's tokens, in order, to the notes on its channel without words. */
/** function bind(out: Sounding[], pattern: Int, line: Int, l: Lyrics, verse: Int) => Undefined */
function bind(out, pattern, line, l, verse) {
  const v = verseText(l, verse);
  if (v.verse === 0) return;
  const tokens = parseLyrics(v.text).tokens;
  let k = 0;
  for (const n of out) {
    if (k >= tokens.length) break;
    if (n.channel !== l.channel || n.sung !== undefined) continue;
    n.sung = { token: tokens[k], pattern: pattern, line: line, verse: v.verse, index: k, at: n.start };
    k = k + 1;
  }
}

/** The highest verse any lyric line in this pattern's tree has (at least 1). */
/** function verseCount(p: Project, index: Int) => Int */
export function verseCount(p, index) {
  return Math.max(1, versesIn(p, index, []));
}

/** Whether a lyric line in this pattern's tree has a verse (the pattern sings somewhere). */
/** function sings(p: Project, index: Int) => Boolean */
export function sings(p, index) {
  return versesIn(p, index, []) > 0;
}

/** function versesIn(p: Project, index: Int, stack: Int[]) => Int */
function versesIn(p, index, stack) {
  if (stack.includes(index) || stack.length >= MAX_DEPTH) return 0;
  stack.push(index);
  const pat = p.patterns[index];
  let most = 0;
  for (const l of pat.lyrics) for (const e of l.verses) most = Math.max(most, Math.round(Number(e.key)));
  for (const u of pat.uses) {
    const j = patternIndex(p, u.pattern);
    if (j >= 0) most = Math.max(most, versesIn(p, j, stack));
  }
  stack.pop();
  return most;
}
