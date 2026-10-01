// The lyric row under the piano roll's grid: the syllable each note of the
// edited channel sings, in the chosen verse, under the note. Click a cell
// to type its syllable: Tab and Shift+Tab go to the next and previous note,
// Enter keeps it, Escape drops it, and "_" alone holds the syllable before.
// A typed syllable goes where the note's words are written (lyricedit.js):
// the line and verse it sings from, or this pattern's line for the channel.

import { state, commit, invalidate, hint } from "../store.js";
import { expandPattern, verseCount, sings } from "../expand.js";
import { tokenText } from "../lyrics.js";
import { wordsAt, writeSyllable, lineIndex, lineFor, setVerse } from "../lyricedit.js";
import { openSheet } from "./lyricsheet.js";
import { toast } from "./toast.js";

/** The verse shown; the note being typed under (an index into the channel's notes, -1: none) and in which pattern/channel/verse. */
export const lyr = { verse: 1, editing: -1, where: "" };

/** What the row shares with the grid: pixels per beat, the scroll and the visible width (px). */
/** type Strip = { zoom: Number, left: Number, width: Number } */

/** The verse shown in patterns[index]: the one chosen, within the verses it has. */
/** function shownVerse(p: Project, index: Int) => Int */
export function shownVerse(p, index) {
  return Math.max(1, Math.min(lyr.verse, verseCount(p, index)));
}

/** Whether a channel sings in patterns[index]: its own line, or words its uses bring. */
/** function channelSings(p: Project, index: Int, channel: String) => Boolean */
function channelSings(p, index, channel) {
  if (lineIndex(p.patterns[index], channel) >= 0) return true;
  return sings(p, index) && expandPattern(p, index, 1).some((n) => n.channel === channel && n.sung !== undefined);
}

/** How a note's cell reads: its syllable ("Hel-", "_"), or "" without words. */
/** function cellText(n: Sounding) => String */
function cellText(n) {
  const s = n.sung;
  return s === undefined ? "" : tokenText(s.token);
}

/** The row's label: "Lyrics", which opens the whole verse as text. */
/** function lyricLabel(b: Builder, index: Int, ch: Channel) => Undefined */
export function lyricLabel(b, index, ch) {
  const verse = shownVerse(state.project, index);
  b.leaf("button", "lyl", "lyr-label", "Lyrics…");
  b.attr("title", `Edit verse ${verse} of ${ch.name}'s words as text`);
  b.on("click", (e) => openSheet(index, ch.id, verse));
}

/** The lyric row: a cell under every note of the channel, one of them typed in. `reveal` scrolls a beat into view. */
/** function lyricRow(b: Builder, s: Strip, index: Int, ch: Channel, reveal: (Number) => Undefined) => Undefined */
export function lyricRow(b, s, index, ch, reveal) {
  const p = state.project;
  const verse = shownVerse(p, index);
  const where = `${p.patterns[index].id}/${ch.id}/${verse}`;
  if (lyr.where !== where) {
    lyr.where = where;
    lyr.editing = -1;
  }
  const notes = expandPattern(p, index, verse).filter((n) => n.channel === ch.id);
  b.open("div", "lyr", "lyr-lane");
  b.open("div", "in", "lyr-in");
  b.style("transform", `translateX(${-s.left}px)`);
  for (let k = 0; k < notes.length; k++) {
    const n = notes[k];
    const x = n.start * s.zoom;
    if (k !== lyr.editing && (x > s.left + s.width + 40 || x + n.length * s.zoom < s.left - 40)) continue;
    if (k === lyr.editing) cellEditor(b, s, index, ch.id, verse, notes, k, reveal);
    else cell(b, s, index, n, k);
  }
  b.close();
  if (channelSings(p, index, ch.id)) verseChips(b, index, ch.id, verse);
  b.close();
}

/** function cellBox(b: Builder, s: Strip, n: Sounding) => Undefined */
function cellBox(b, s, n) {
  b.style("left", `${n.start * s.zoom}px`);
  b.style("width", `${Math.max(16, n.length * s.zoom - 2)}px`);
}

/** A note's syllable; a click types in it. Words written in a used pattern read paler. */
/** function cell(b: Builder, s: Strip, index: Int, n: Sounding, k: Int) => Undefined */
function cell(b, s, index, n, k) {
  const text = cellText(n);
  const sung = n.sung;
  let cls = "lyr-cell";
  if (text === "") cls = `${cls} empty`;
  else if (text === "_") cls = `${cls} hold`;
  if (sung !== undefined && sung.pattern !== index) cls = `${cls} used`;
  b.leaf("div", `c${k}`, cls, text);
  cellBox(b, s, n);
  b.on("pointerenter", (e) =>
    hint("Click to type this note's syllable — Tab goes on to the next note, “_” holds the one before, a trailing “-” joins the next")
  );
  // On click, not press: a cell being typed in blurs first and keeps its text.
  b.on("click", (e) => {
    lyr.editing = k;
    invalidate();
  });
}

/** The cell being typed in: an input over the note. */
/** function cellEditor(b: Builder, s: Strip, index: Int, channel: String, verse: Int, notes: Sounding[], k: Int, reveal: (Number) => Undefined) => Undefined */
function cellEditor(b, s, index, channel, verse, notes, k, reveal) {
  const n = notes[k];
  const text = cellText(n);
  b.leaf("input", `e${k}`, "lyr-input", "");
  cellBox(b, s, n);
  b.attr("spellcheck", "false");
  b.attr("aria-label", "Syllable");
  b.prop("value", text);
  b.prop("selectionStart", "0");
  b.prop("selectionEnd", String(text.length));
  b.prop("focus", "true");
  b.on("keydown", (e) => {
    // An input the edit has moved on from (keys typed before the next frame) does nothing.
    if (lyr.editing !== k) return undefined;
    if (e.key === "Escape") {
      e.preventDefault();
      stopEditing(k);
    } else if (e.key === "Enter") {
      e.preventDefault();
      keepCell(index, channel, verse, k, e.value);
      stopEditing(k);
    } else if (e.key === "Tab") {
      e.preventDefault();
      keepCell(index, channel, verse, k, e.value);
      const next = k + (e.shiftKey ? -1 : 1);
      if (next < 0 || next >= notes.length) stopEditing(k);
      else {
        lyr.editing = next;
        reveal(notes[next].start);
        invalidate();
      }
    }
  });
  // Clicking away keeps what was typed (unless the edit already moved on).
  b.on("blur", (e) => {
    if (lyr.editing !== k) return undefined;
    keepCell(index, channel, verse, k, e.value);
    stopEditing(k);
  });
}

/** function stopEditing(k: Int) => Undefined */
function stopEditing(k) {
  if (lyr.editing !== k) return undefined;
  lyr.editing = -1;
  invalidate();
}

/** Write what was typed under the channel's `k`-th note, as one undo step (nothing when unchanged). */
/** function keepCell(index: Int, channel: String, verse: Int, k: Int, typed: String) => Undefined */
function keepCell(index, channel, verse, k, typed) {
  const p = state.project;
  if (index >= p.patterns.length) return undefined;
  const all = expandPattern(p, index, verse);
  const note = all.filter((n) => n.channel === channel)[k];
  if (note === undefined || typed.trim() === cellText(note)) return undefined;
  const at = wordsAt(p, index, verse, all, note);
  if (at.error !== "") {
    toast("These words do not parse", `${at.error}. Fix them with Lyrics… first.`, "error");
    return undefined;
  }
  commit(() => writeSyllable(p, at, typed));
}

/** The verses to show (1, 2, …) and "+" to write another. */
/** function verseChips(b: Builder, index: Int, channel: String, verse: Int) => Undefined */
function verseChips(b, index, channel, verse) {
  const count = verseCount(state.project, index);
  b.open("div", "verses", "lyr-verses");
  for (let v = 1; v <= count; v++) {
    b.leaf("button", `v${v}`, v === verse ? "lyr-verse on" : "lyr-verse", String(v));
    b.attr("title", `Show and type verse ${v}`);
    b.on("click", (e) => {
      lyr.verse = v;
      invalidate();
    });
  }
  b.leaf("button", "add", "lyr-verse add", "+");
  b.attr("title", `Write verse ${count + 1}`);
  b.on("click", (e) => addVerse(index, channel, count + 1));
  b.close();
}

/** Start a new, empty verse in this pattern's line for the channel, and show it. */
/** function addVerse(index: Int, channel: String, verse: Int) => Undefined */
function addVerse(index, channel, verse) {
  commit(() => setVerse(lineFor(state.project.patterns[index], channel), String(verse), ""));
  lyr.verse = verse;
}
