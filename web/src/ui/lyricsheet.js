// One verse of a pattern's words for a channel, as text in the lyric
// notation: a text box, where the text stops making sense (from
// parseLyrics) and how many syllables there are for how many notes.
// Opened from the lyric row's "Lyrics…"; Apply is one undo step.

import { state, commit, invalidate } from "../store.js";
import { parseLyrics } from "../lyrics.js";
import { lineIndex, lineFor, lineNotes, setVerse, verseOf } from "../lyricedit.js";
import { button } from "./widgets.js";

/** Whether the sheet is open, for which pattern (id), channel and verse, and the text as typed. */
const sheet = { open: false, pattern: "", channel: "", verse: 1, draft: "" };

/** Open the sheet on a verse of patterns[index]'s line for a channel. */
/** function openSheet(index: Int, channel: String, verse: Int) => Undefined */
export function openSheet(index, channel, verse) {
  const pat = state.project.patterns[index];
  const li = lineIndex(pat, channel);
  sheet.open = true;
  sheet.pattern = pat.id;
  sheet.channel = channel;
  sheet.verse = verse;
  sheet.draft = li >= 0 ? verseOf(pat.lyrics[li], String(verse)) : "";
  invalidate();
}

function closeSheet() {
  sheet.open = false;
  invalidate();
}

/** Keep the text as the verse. */
/** function applySheet(index: Int) => Undefined */
function applySheet(index) {
  const pat = state.project.patterns[index];
  const li = lineIndex(pat, sheet.channel);
  const text = sheet.draft.trim();
  if (li >= 0 ? verseOf(pat.lyrics[li], String(sheet.verse)) !== text : text !== "") {
    commit(() => setVerse(lineFor(pat, sheet.channel), String(sheet.verse), text));
  }
  closeSheet();
}

/** How the text reads: where it stops parsing, or its syllables against the notes it is for. */
/** `fits`: one syllable per note; `error`: the text does not parse (`before`/`at`/`after`: around where). */
/** type Verdict = { fits: Boolean, error: Boolean, text: String, before: String, at: String, after: String } */

/** function verdict(index: Int) => Verdict */
function verdict(index) {
  const r = parseLyrics(sheet.draft);
  if (r.error >= 0) {
    const chars = Array.from(sheet.draft);
    return {
      fits: false,
      error: true,
      text: `At character ${r.error + 1}: ${r.message}`,
      before: chars.slice(Math.max(0, r.error - 16), r.error).join(""),
      at: chars[r.error] ?? "",
      after: chars.slice(r.error + 1, r.error + 16).join(""),
    };
  }
  const notes = lineNotes(state.project, index, sheet.channel, sheet.verse).length;
  const n = r.tokens.length;
  const fit = n === notes ? "one for every note" : n > notes ? `${n - notes} more than there are notes` : `${notes - n} notes without words`;
  return {
    fits: n === notes,
    error: false,
    text: `${n} syllable${n === 1 ? "" : "s"} for ${notes} note${notes === 1 ? "" : "s"} — ${fit}`,
    before: "",
    at: "",
    after: "",
  };
}

/** The sheet over the piano roll, when it is open on patterns[index]. */
/** function lyricSheet(b: Builder, index: Int) => Undefined */
export function lyricSheet(b, index) {
  const p = state.project;
  if (!sheet.open || p.patterns[index].id !== sheet.pattern) return undefined;
  const ch = p.channels.find((c) => c.id === sheet.channel);
  const v = verdict(index);
  b.open("div", "lyr-sheet", "lyr-sheet");
  b.leaf("div", "t", "lyr-sheet-title", `Verse ${sheet.verse} · ${ch ? ch.name : sheet.channel} · ${p.patterns[index].name}`);
  b.leaf(
    "div",
    "h",
    "lyr-sheet-help",
    "space ends a word · Hel-lo splits syllables · _ holds a syllable over a note · / ends a line, // a paragraph · word[w ɜ d] pronounces · (br) breathes"
  );
  b.leaf("textarea", `in-${sheet.pattern}-${sheet.channel}-${sheet.verse}`, "lyr-sheet-text", "");
  b.attr("spellcheck", "false");
  b.attr(
    "placeholder",
    sheet.verse > 1 ? `Verse ${sheet.verse} is not written yet: it sings the first verse's words.` : "Hel-lo dark-ness my old friend _ / …"
  );
  b.prop("value", sheet.draft);
  b.prop("focus", "true");
  b.on("input", (e) => {
    sheet.draft = e.value;
    invalidate();
  });
  b.on("keydown", (e) => {
    if (e.key === "Escape") closeSheet();
    else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      sheet.draft = e.value;
      applySheet(index);
    }
  });
  b.open("div", "v", v.error ? "lyr-sheet-verdict error" : v.fits ? "lyr-sheet-verdict ok" : "lyr-sheet-verdict");
  b.leaf("span", "m", "", v.text);
  if (v.error) {
    b.open("code", "x", "lyr-sheet-excerpt");
    b.leaf("span", "a", "", v.before);
    b.leaf("mark", "b", "", v.at);
    b.leaf("span", "c", "", v.after);
    b.close();
  }
  b.close();
  b.open("div", "acts", "lyr-sheet-acts");
  button(b, "cancel", "small ghost", "Cancel", "Close without changing the words (Esc)", () => closeSheet());
  button(b, "apply", "small gold", "Apply", "Keep these words for the verse (Ctrl+Enter)", () => applySheet(index));
  b.close();
  b.close();
}
