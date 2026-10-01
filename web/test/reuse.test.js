// Tests for editing uses and lyrics through notes (reuse.js, lyricedit.js),
// run with: node web/test/reuse.test.js (also type-checked by inty).
import { project, pat, use, words, line } from "./fixtures.js";
import { expandPattern } from "../src/expand.js";
import { useBoxes, makeUnique, makeReference, removeUse } from "../src/reuse.js";
import { wordsAt, writeSyllable, lineNotes, verseOf, lyricsVerdict } from "../src/lyricedit.js";

let failures = 0;
/** function check(name: String, ok: Boolean) => Undefined */
function check(name, ok) {
  if (!ok) failures = failures + 1;
  console.log(`${ok ? "ok  " : "FAIL"} ${name}`);
}

/** What the lead sings in a verse of patterns[index]. */
/** function sung(p: Project, index: Int, verse: Int) => String */
function sung(p, index, verse) {
  return words(expandPattern(p, index, verse).filter((n) => n.channel === "lead"));
}

/** The lead's pitches@beats in patterns[index]. */
/** function melody(p: Project, index: Int) => String */
function melody(p, index) {
  return expandPattern(p, index, 1)
    .filter((n) => n.channel === "lead")
    .map((n) => `${n.pitch}@${n.start}`)
    .join(" ");
}

/** A verse pattern that sings "I said" and uses a hook (two verses of words) twice: +2 at beat 2, verse 2 at beat 4. */
/** function song() => Project */
function song() {
  const p = project();
  const hook = pat("hook", 2, [
    [0, 67, 0],
    [0, 69, 1],
  ]);
  hook.lyrics.push(line(["oh yeah", "oh no"]));
  const verse = pat("verse", 8, [
    [0, 60, 0],
    [0, 62, 1],
  ]);
  verse.uses.push({ ...use("hook", 2), transpose: 2 });
  verse.uses.push({ ...use("hook", 4), verse: 2 });
  verse.lyrics.push(line(["I said"]));
  p.patterns = [hook, verse];
  return p;
}

{
  const p = song();
  const boxes = useBoxes(p, 1, 1);
  check("a box per use, labeled with how it is changed", boxes.length === 2 && boxes[0].label === "hook +2" && boxes[1].label === "hook v2");
  check("a box spans its use and its notes' pitches", boxes[0].start === 2 && boxes[0].end === 4 && boxes[0].low === 69 && boxes[0].high === 71);
}

{
  const p = song();
  const v1 = sung(p, 1, 1);
  const v2 = sung(p, 1, 2);
  const tune = melody(p, 1);
  const fresh = makeUnique(p, 1, 0);
  check("Make unique turns a use into plain notes", p.patterns[1].uses.length === 1 && p.patterns[1].notes.length === 4 && fresh.join(",") === "2,3");
  check("…that sound as before", melody(p, 1) === tune && p.patterns[1].notes[2].pitch === 69);
  check("…and sing the same words in every verse", sung(p, 1, 1) === v1 && sung(p, 1, 2) === v2);
  check(
    "the words moved into the pattern's own line",
    verseOf(p.patterns[1].lyrics[0], "1") === "I said oh yeah" && verseOf(p.patterns[1].lyrics[0], "2") === "I said oh no"
  );
}

{
  const p = song();
  const before = sung(p, 1, 1);
  removeUse(p, 1, 0);
  check("removing a use takes its notes and their words", p.patterns[1].uses.length === 1 && sung(p, 1, 1) === "I said oh no" && before !== sung(p, 1, 1));
}

{
  const p = project();
  const v = pat("v", 8, [
    [0, 60, 0],
    [0, 62, 1.5],
    [0, 64, 2],
    [0, 65, 3],
    [1, 36, 1.5],
  ]);
  v.lyrics.push(line(["a b c d", "e f g h"]));
  p.patterns = [v];
  const tune = melody(p, 0);
  const id = makeReference(p, 0, [1, 2, 4]);
  const made = p.patterns[1];
  check("Make reference makes a pattern of the picked notes", id === "v-part-1" && made.name === "v part 1" && made.notes.length === 3);
  check("…timed from the beat the first is in, whole beats long", made.notes[0].start === 0.5 && made.length === 2);
  check("…and uses it where they were", v.notes.length === 2 && v.uses.length === 1 && v.uses[0].start === 1 && melody(p, 0) === tune);
  check("the words go with the notes", sung(p, 0, 1) === "a b c d" && sung(p, 0, 2) === "e f g h");
  check("…into the new pattern's line", verseOf(made.lyrics[0], "1") === "b c" && verseOf(v.lyrics[0], "1") === "a d");
}

/** Type a syllable under the lead's `k`-th note of patterns[index] in a verse. */
/** function type(p: Project, index: Int, verse: Int, k: Int, text: String) => String */
function type(p, index, verse, k, text) {
  const notes = expandPattern(p, index, verse);
  const lead = notes.filter((n) => n.channel === "lead");
  const at = wordsAt(p, index, verse, notes, lead[k]);
  if (at.error === "") writeSyllable(p, at, text);
  return at.error;
}

{
  const p = song();
  type(p, 1, 1, 1, "sang");
  check("a syllable typed under a note replaces its word", verseOf(p.patterns[1].lyrics[0], "1") === "I sang");
  type(p, 1, 1, 2, "ah-");
  check("…in the pattern it is written in", verseOf(p.patterns[0].lyrics[0], "1") === "ah-yeah" && sung(p, 1, 1).startsWith("I sang ah- yeah"));
  type(p, 1, 2, 5, "_");
  check("…and the verse it sings (a hold)", verseOf(p.patterns[0].lyrics[0], "2") === "oh _" && verseOf(p.patterns[0].lyrics[0], "1") === "ah-yeah");
}

{
  const p = project();
  const v = pat("v", 8, [
    [0, 60, 0],
    [0, 62, 1],
    [0, 64, 2],
    [0, 65, 3],
  ]);
  p.patterns = [v];
  type(p, 0, 1, 2, "la");
  check("typing under a note without words makes the line, the notes before holding", v.lyrics.length === 1 && verseOf(v.lyrics[0], "1") === "_ _ la");
  type(p, 0, 1, 0, "Hel-");
  type(p, 0, 1, 1, "lo");
  check("a trailing hyphen joins the next syllable", verseOf(v.lyrics[0], "1") === "Hel-lo la");
  type(p, 0, 1, 2, "");
  check("emptying the last syllable ends the verse there", verseOf(v.lyrics[0], "1") === "Hel-lo");
  type(p, 0, 1, 0, "");
  check("emptying another one holds", verseOf(v.lyrics[0], "1") === "_ lo");
  type(p, 0, 3, 3, "end");
  check("a verse the line lacks writes into the one it sings", verseOf(v.lyrics[0], "1") === "_ lo _ end" && v.lyrics[0].verses.length === 1);
  check("the line's notes are those its uses left without words", lineNotes(p, 0, "lead", 1).length === 4);
  v.lyrics[0].verses[0].value = "a [b";
  check("a verse that does not parse is not edited by note", type(p, 0, 1, 0, "x") !== "" && verseOf(v.lyrics[0], "1") === "a [b");
  v.lyrics = [];
  type(p, 0, 1, 0, "won-");
  type(p, 0, 1, 1, "der");
  type(p, 0, 1, 2, "a\\-b-");
  type(p, 0, 1, 3, "c");
  check("a word typed on past the end of the verse stays one word", verseOf(v.lyrics[0], "1") === "won-der a\\\\\\-b-c");
}

{
  const bad = lyricsVerdict("Hel-lo [world", 3);
  check("words that stop parsing say where", bad.error && bad.text.startsWith("At character 8") && bad.at === "[" && bad.before === "Hel-lo ");
  check(
    "words count their syllables against the notes",
    lyricsVerdict("Hel-lo _", 3).fits && lyricsVerdict("a b", 3).text === "2 syllables for 3 notes — 1 note without words"
  );
}

if (failures > 0) {
  console.log(`${failures} reuse test(s) failed`);
  throw new Error("reuse tests failed");
}
console.log("all reuse tests passed");
