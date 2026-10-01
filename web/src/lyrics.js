// The lyric notation (mirrors crates/core/src/lyrics/notation.rs; both are
// tested against web/test/lyric-notation.json).
//
//   space ends a word · Hel-lo / Hel- lo splits syllables · _ holds the
//   previous syllable over a note · / ends a line, // a paragraph ·
//   word[w ɜ d] gives a pronunciation in IPA · (br) a breath · \x escapes x
//
// Every token takes one note.

/** One note's worth of lyrics. `kind`: "syllable", "hold" or "breath"; `pos` (syllables): "single", "begin", "middle" or "end"; `brk`: "", "line" or "paragraph". */
/** type LyricToken = { kind: String, text: String, pos: String, phonemes: String[], brk: String } */
/** `error`: the character a mistake is at (-1: none). */
/** type Parsed = { tokens: LyricToken[], error: Int, message: String } */

/** function token(kind: String, text: String, phonemes: String[]) => LyricToken */
function token(kind, text, phonemes) {
  return { kind: kind, text: text, pos: "single", phonemes: phonemes, brk: "" };
}

/** Read a verse into tokens. */
/** function parseLyrics(text: String) => Parsed */
export function parseLyrics(text) {
  const chars = Array.from(text);
  /** const tokens: LyricToken[] */
  const tokens = [];
  /** let word: Int[] */
  let word = [];
  /** let lastWord: Int[] */
  let lastWord = [];
  let syl = "";
  /** let phonemes: String[] */
  let phonemes = [];
  let cont = false;

  const endSyllable = () => {
    if (syl === "") return;
    word.push(tokens.length);
    tokens.push(token("syllable", syl, phonemes));
    syl = "";
    phonemes = [];
  };
  const endWord = () => {
    cont = false;
    const n = word.length;
    for (let k = 0; k < n; k++) {
      tokens[word[k]].pos = n === 1 ? "single" : k === 0 ? "begin" : k === n - 1 ? "end" : "middle";
    }
    if (n > 0) {
      lastWord = word;
      word = [];
    }
  };
  /** function fail(at: Int, message: String) => Parsed */
  const fail = (at, message) => ({ tokens: [], error: at, message: message });

  let i = 0;
  while (i < chars.length) {
    const c = chars[i];
    if (c.trim() === "") {
      endSyllable();
      if (!cont) endWord();
    } else if (c === "-") {
      if (syl === "" && word.length === 0 && !cont) {
        word = lastWord;
        lastWord = [];
      }
      endSyllable();
      cont = true;
    } else if (c === "_") {
      endSyllable();
      tokens.push(token("hold", "", []));
    } else if (c === "/") {
      let n = 0;
      while (i + n < chars.length && chars[i + n] === "/") n = n + 1;
      endSyllable();
      endWord();
      if (tokens.length > 0) tokens[tokens.length - 1].brk = n > 1 ? "paragraph" : "line";
      i = i + n;
      continue;
    } else if (c === "[") {
      if (syl === "") return fail(i, "a pronunciation [...] follows the syllable it is for");
      const close = chars.indexOf("]", i + 1);
      if (close < 0) return fail(i, "a [ without its ]");
      phonemes = chars
        .slice(i + 1, close)
        .join("")
        .split(/\s+/)
        .filter((s) => s !== "");
      if (phonemes.length === 0) return fail(i, "an empty pronunciation []");
      i = close + 1;
      continue;
    } else if (c === "]") {
      return fail(i, "a ] without its [");
    } else if (c === "(" && chars.slice(i, i + 4).join("") === "(br)") {
      endSyllable();
      endWord();
      tokens.push(token("breath", "", []));
      i = i + 4;
      continue;
    } else if (c === "\\") {
      syl = syl + (i + 1 < chars.length ? chars[i + 1] : "\\");
      cont = false;
      i = i + 2;
      continue;
    } else {
      syl = syl + c;
      cont = false;
    }
    i = i + 1;
  }
  endSyllable();
  endWord();
  return { tokens: tokens, error: -1, message: "" };
}

/** How a token reads under a note: a syllable with a hyphen when its word goes on, "_" for a hold. */
/** function tokenText(t: LyricToken) => String */
export function tokenText(t) {
  if (t.kind === "hold") return "_";
  if (t.kind === "breath") return "(br)";
  return t.pos === "begin" || t.pos === "middle" ? `${t.text}-` : t.text;
}

/** The text of a lyric line's verse: its own, or the lowest-numbered verse's (a chorus written once is sung every time). `verse` is 0 when the line has none. */
/** type VerseText = { verse: Int, text: String } */
/** function verseText(line: Lyrics, verse: Int) => VerseText */
export function verseText(line, verse) {
  /** let best: VerseText */
  let best = { verse: 0, text: "" };
  for (const e of line.verses) {
    const k = Math.round(Number(e.key));
    if (k === verse) return { verse: k, text: e.value };
    if (best.verse === 0 || k < best.verse) best = { verse: k, text: e.value };
  }
  return best;
}

/** Escape text typed for one syllable so the notation reads it as written. */
/** function escapeSyllable(s: String) => String */
export function escapeSyllable(s) {
  let out = "";
  for (const c of Array.from(s)) out = out + ("\\_/[]-".includes(c) || c.trim() === "" ? `\\${c}` : c);
  return out.replace(/\(br\)/g, "\\(br)");
}

/** Whether a syllable's word goes on after it (it is written with a hyphen). */
/** function continues(t: LyricToken) => Boolean */
export function continues(t) {
  return t.kind === "syllable" && (t.pos === "begin" || t.pos === "middle");
}

/** One token as notation: a syllable (escaped, with its pronunciation and, when its word goes on, a hyphen), `_` or `(br)`. */
/** function tokenNotation(t: LyricToken) => String */
function tokenNotation(t) {
  if (t.kind === "hold") return "_";
  if (t.kind === "breath") return "(br)";
  const ph = t.phonemes.length > 0 ? `[${t.phonemes.join(" ")}]` : "";
  return `${escapeSyllable(t.text)}${ph}${continues(t) ? "-" : ""}`;
}

/** Write tokens back as notation (the inverse of parseLyrics): the syllables of a
 * word joined by hyphens (Hel-lo), words, holds and breaths apart, then line and paragraph breaks. */
/** function writeLyrics(tokens: LyricToken[]) => String */
export function writeLyrics(tokens) {
  let out = "";
  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i];
    // A syllable that goes on joins the next one directly (Hel-lo); anything else is set apart.
    const joined = i > 0 && continues(tokens[i - 1]) && t.kind === "syllable" && tokens[i - 1].brk === "";
    out = out + (i === 0 || joined ? "" : " ") + tokenNotation(t);
    if (t.brk !== "") out = out + (t.brk === "paragraph" ? " //" : " /");
  }
  return out;
}
