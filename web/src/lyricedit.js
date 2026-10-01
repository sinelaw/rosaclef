// Editing lyrics through the notes that sing them.
//
// A lyric line's verse is text in the notation (lyrics.js), and its tokens
// go to the notes of its channel in time order (expand.js). Edits here go
// through tokens: parse the verse, change one token, write it back
// (writeLyrics), so a syllable typed under a note lands in the line and
// verse that note sings from. `keepWords` does the same for edits that move
// notes between patterns (Make unique, Make reference): each note keeps the
// words it sang.

import { parseLyrics, verseText, writeLyrics } from "./lyrics.js";
import { expandPattern, verseCount, useNotes } from "./expand.js";
import { round6 } from "./model.js";

/** Where a note's syllable is written: `patterns[pattern].lyrics[line]` (-1: a line to
 * create for `channel`), the verse `key`, the token's `index` in it. `error`: why the verse
 * cannot be edited token by token (it does not parse), else "". */
/** type WordsAt = { pattern: Int, line: Int, channel: String, key: String, index: Int, error: String } */

/** function hold() => LyricToken */
function hold() {
  return { kind: "hold", text: "", pos: "single", phonemes: [], brk: "" };
}

/** The first lyric line of a pattern for a channel, or -1. */
/** function lineIndex(pat: Pattern, channel: String) => Int */
export function lineIndex(pat, channel) {
  for (let i = 0; i < pat.lyrics.length; i++) if (pat.lyrics[i].channel === channel) return i;
  return -1;
}

/** A pattern's lyric line for a channel, made (singing, with no verses) when it has none. */
/** function lineFor(pat: Pattern, channel: String) => Lyrics */
export function lineFor(pat, channel) {
  const i = lineIndex(pat, channel);
  if (i >= 0) return pat.lyrics[i];
  const line = { channel: channel, lang: "", mode: "sing", verses: [], timing: [] };
  pat.lyrics.push(line);
  return line;
}

/** The text of a verse as written (not its fallback), "" when the line lacks it. */
/** function verseOf(line: Lyrics, key: String) => String */
export function verseOf(line, key) {
  const e = line.verses.find((x) => x.key === key);
  return e ? e.value : "";
}

/** Set a verse's text, adding the verse when the line lacks it. */
/** function setVerse(line: Lyrics, key: String, text: String) => Undefined */
export function setVerse(line, key, text) {
  const e = line.verses.find((x) => x.key === key);
  if (e) e.value = text;
  else line.verses.push({ key: key, value: text });
}

/** The notes a pattern's own line for `channel` gives words to (those its uses left without), in time order. */
/** function lineNotes(p: Project, index: Int, channel: String, verse: Int) => Sounding[] */
export function lineNotes(p, index, channel, verse) {
  return ownNotes(expandPattern(p, index, verse), index, lineIndex(p.patterns[index], channel), channel);
}

/** Of a pattern's sounding notes, those on `channel` its line `line` sings or that have no words. */
/** function ownNotes(notes: Sounding[], index: Int, line: Int, channel: String) => Sounding[] */
function ownNotes(notes, index, line, channel) {
  return notes.filter((n) => {
    const s = n.sung;
    return n.channel === channel && (s === undefined || (s.pattern === index && s.line === line));
  });
}

/** Where the syllable of `note` (one of `notes`, patterns[index] singing `verse`) is written:
 * the line and verse it sings from, or — without words — the next free place in this
 * pattern's line for its channel (the notes before it hold). */
/** function wordsAt(p: Project, index: Int, verse: Int, notes: Sounding[], note: Sounding) => WordsAt */
export function wordsAt(p, index, verse, notes, note) {
  const s = note.sung;
  if (s !== undefined) {
    const line = p.patterns[s.pattern].lyrics[s.line];
    const key = String(s.verse);
    return { pattern: s.pattern, line: s.line, channel: line.channel, key: key, index: s.index, error: parseError(verseOf(line, key)) };
  }
  const pat = p.patterns[index];
  const li = lineIndex(pat, note.channel);
  const own = ownNotes(notes, index, li, note.channel);
  if (li < 0) return { pattern: index, line: -1, channel: note.channel, key: String(verse), index: own.indexOf(note), error: "" };
  const v = verseText(pat.lyrics[li], verse);
  const key = String(v.verse > 0 ? v.verse : verse);
  return { pattern: index, line: li, channel: note.channel, key: key, index: own.indexOf(note), error: parseError(v.text) };
}

/** Why a verse's text does not parse ("" when it does). */
/** function parseError(text: String) => String */
function parseError(text) {
  const r = parseLyrics(text);
  return r.error < 0 ? "" : r.message;
}

/** The token typed for one note: "_" holds, "(br)" breathes, a trailing "-" means the word goes on; undefined when empty. */
/** function typedToken(typed: String) => LyricToken | Undefined */
export function typedToken(typed) {
  const s = typed.trim();
  if (s === "") return undefined;
  if (s === "_") return hold();
  if (s === "(br)") return { kind: "breath", text: "", pos: "single", phonemes: [], brk: "" };
  const on = s.length > 1 && s.endsWith("-");
  return { kind: "syllable", text: on ? s.slice(0, s.length - 1) : s, pos: on ? "begin" : "single", phonemes: [], brk: "" };
}

/** Write a typed syllable where `at` says (see wordsAt; it must have no error). Past the
 * verse's end the notes in between hold; an empty syllable holds, or ends the verse when last. */
/** function writeSyllable(p: Project, at: WordsAt, typed: String) => Undefined */
export function writeSyllable(p, at, typed) {
  const line = at.line >= 0 ? p.patterns[at.pattern].lyrics[at.line] : lineFor(p.patterns[at.pattern], at.channel);
  const text = verseOf(line, at.key);
  const tokens = parseLyrics(text).tokens;
  const t = typedToken(typed);
  if (at.index < tokens.length) {
    const old = tokens[at.index];
    if (t === undefined && at.index === tokens.length - 1) tokens.pop();
    else tokens[at.index] = replaced(old, t === undefined ? hold() : t);
  } else if (t !== undefined) {
    // A verse that ends mid-word ("Hel-") goes on into what is added.
    const last = tokens.length > 0 ? tokens[tokens.length - 1] : undefined;
    if (last !== undefined && last.kind === "syllable" && endsMidWord(text)) last.pos = last.pos === "single" ? "begin" : "middle";
    while (tokens.length < at.index) tokens.push(hold());
    tokens.push(t);
  }
  setVerse(line, at.key, writeLyrics(tokens));
}

/** Whether a verse's text ends with a hyphen that splits a word (not an escaped one). */
/** function endsMidWord(text: String) => Boolean */
function endsMidWord(text) {
  const chars = Array.from(text.trimEnd());
  if (chars.length === 0 || chars[chars.length - 1] !== "-") return false;
  let escapes = 0;
  while (escapes + 2 <= chars.length && chars[chars.length - 2 - escapes] === "\\") escapes = escapes + 1;
  return escapes % 2 === 0;
}

/** A new token in an old one's place: it keeps the line break, and the pronunciation while the syllable reads the same. */
/** function replaced(old: LyricToken, t: LyricToken) => LyricToken */
function replaced(old, t) {
  const same = old.kind === t.kind && old.text === t.text;
  return { kind: t.kind, text: t.text, pos: t.pos, phonemes: same ? old.phonemes : [], brk: old.brk };
}

// ------------------------------------------------------------------ keeping words

/** A word a note sang before an edit: where the note was (channel, beat, pitch) and its token. */
/** type Said = { channel: String, key: String, token: LyricToken } */

/** function noteKey(n: Sounding) => String */
function noteKey(n) {
  return `${n.channel}|${round6(n.start)}|${n.pitch}`;
}

/** The words a pattern's notes sing in a verse. */
/** function wordsOf(p: Project, index: Int, verse: Int) => Said[] */
function wordsOf(p, index, verse) {
  /** const out: Said[] */
  const out = [];
  for (const n of expandPattern(p, index, verse)) {
    const s = n.sung;
    if (s !== undefined) out.push({ channel: n.channel, key: noteKey(n), token: s.token });
  }
  return out;
}

/** Take (once) the word a note sang. */
/** function take(said: Said[], key: String) => LyricToken | Undefined */
function take(said, key) {
  const i = said.findIndex((w) => w.key === key);
  if (i < 0) return undefined;
  const t = said[i].token;
  said.splice(i, 1);
  return t;
}

/** Run `edit`, which moves notes within the tree of patterns[index], so that every note keeps
 * the words it sang: afterwards the lyric lines of the patterns `created` (indexes into its
 * uses, for patterns the edit made) and then its own are written again from those words. */
/** function keepWords(p: Project, index: Int, created: Int[], edit: () => Undefined) => Undefined */
export function keepWords(p, index, created, edit) {
  /** const before: Said[][] */
  const before = [];
  for (let v = 1; v <= verseCount(p, index); v++) before.push(wordsOf(p, index, v));
  /** const channels: String[] */
  const channels = [];
  for (const said of before) for (const w of said) if (!channels.includes(w.channel)) channels.push(w.channel);
  const model = p.patterns[index].lyrics;
  edit();
  const uses = p.patterns[index].uses;
  for (const k of created) {
    const q = p.patterns.findIndex((x) => x.id === uses[k].pattern);
    if (q >= 0) for (const c of channels) rewriteLine(p, q, c, model, before, (v) => useNotes(p, index, k, v));
  }
  for (const c of channels) rewriteLine(p, index, c, model, before, (v) => expandPattern(p, index, v));
}

/** Write patterns[q]'s line for `channel` again: each verse gives its notes (`notesOf`, in time
 * order) the words they sang before. A line whose verses do not parse is left alone. */
/** function rewriteLine(p: Project, q: Int, channel: String, model: Lyrics[], before: Said[][], notesOf: (Int) => Sounding[]) => Undefined */
function rewriteLine(p, q, channel, model, before, notesOf) {
  const pat = p.patterns[q];
  const li = lineIndex(pat, channel);
  if (li >= 0 && pat.lyrics[li].verses.some((e) => parseError(e.value) !== "")) return undefined;
  /** const texts: String[] */
  const texts = [];
  for (let v = 1; v <= before.length; v++) {
    const words = ownNotes(notesOf(v), q, li, channel).map((n) => take(before[v - 1], noteKey(n)));
    while (words.length > 0 && words[words.length - 1] === undefined) words.pop();
    texts.push(writeLyrics(words.map((t) => (t === undefined ? hold() : t))));
  }
  if (li < 0 && texts.every((t) => t === "")) return undefined;
  const line = li >= 0 ? pat.lyrics[li] : lineFor(pat, channel);
  const like = model.find((l) => l.channel === channel);
  if (li < 0 && like) {
    line.lang = like.lang;
    line.mode = like.mode;
  }
  for (let v = 1; v <= texts.length; v++) {
    const key = String(v);
    const old = verseText(line, v);
    // Unchanged words keep their spelling; a verse the same as the first is left to sing it.
    if (writeLyrics(parseLyrics(old.text).tokens) === texts[v - 1] && (old.verse === v || v > 1)) continue;
    if (v > 1 && texts[v - 1] === texts[0]) line.verses = line.verses.filter((e) => e.key !== key);
    else setVerse(line, key, texts[v - 1]);
  }
}
