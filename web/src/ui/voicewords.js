// Words for a sung take, in the Voice tab's melody mode: typed in the
// lyric notation, they become the new pattern's lyric line for its channel
// (verse 1) when the take is added to the song, so a sung take turns into
// notes and words in one step.

import { lyricsVerdict } from "../lyricedit.js";

/** The lyric lines of a pattern made from a take: the words on its channel as verse 1 (none without words). */
/** function takeLyrics(channel: String, text: String) => Lyrics[] */
export function takeLyrics(channel, text) {
  const words = text.trim();
  if (words === "") return [];
  return [{ channel: channel, lang: "", mode: "sing", verses: [{ key: "1", value: words }], timing: [] }];
}

/** Why the words cannot go with the take ("" when they can). */
/** function wordsError(text: String) => String */
export function wordsError(text) {
  const v = lyricsVerdict(text.trim(), 0);
  return v.error ? v.text : "";
}

/** The Lyrics field: the words, and how they fit the `notes` notes of the take. */
/** function wordsField(b: Builder, text: String, notes: Int, onInput: (String) => Undefined) => Undefined */
export function wordsField(b, text, notes, onInput) {
  const v = lyricsVerdict(text.trim(), notes);
  b.open("div", "words", "voice-words");
  b.leaf("label", "l", "voice-words-label", "Lyrics");
  b.leaf("textarea", "t", "voice-words-text", "");
  b.attr("spellcheck", "false");
  b.attr("rows", "2");
  b.attr("placeholder", "Words sung on these notes: Hel-lo dark-ness my old friend _");
  b.attr("title", "space ends a word · Hel-lo splits syllables · _ holds a syllable over a note · / ends a line");
  b.prop("value", text);
  b.on("input", (e) => onInput(e.value));
  if (text.trim() !== "") b.leaf("div", "v", v.error ? "voice-words-fit error" : v.fits ? "voice-words-fit ok" : "voice-words-fit", v.text);
  b.close();
}
