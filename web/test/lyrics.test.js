// Tests for the lyric notation and pattern expansion (lyrics.js, expand.js),
// run with: node web/test/lyrics.test.js (also type-checked by inty).
import { LYRIC_CASES } from "./lyric-notation.js";
import { parseLyrics, tokenText, escapeSyllable, writeLyrics } from "../src/lyrics.js";
import { expandPattern, verseCount } from "../src/expand.js";
import { project, pat, use, words } from "./fixtures.js";

let failures = 0;
/** function check(name: String, ok: Boolean) => Undefined */
function check(name, ok) {
  if (!ok) failures = failures + 1;
  console.log(`${ok ? "ok  " : "FAIL"} ${name}`);
}

/** A token as the shared cases write it: hel< begin, lo> end, x~ middle, _ hold, (br), then / or //. */
/** function show(t: LyricToken) => String */
function show(t) {
  const ph = t.phonemes.length > 0 ? `[${t.phonemes.join(" ")}]` : "";
  const mark = t.pos === "begin" ? "<" : t.pos === "middle" ? "~" : t.pos === "end" ? ">" : "";
  const body = t.kind === "hold" ? "_" : t.kind === "breath" ? "(br)" : `${t.text}${ph}${mark}`;
  return body + (t.brk === "line" ? "/" : t.brk === "paragraph" ? "//" : "");
}

for (const c of LYRIC_CASES) {
  const r = parseLyrics(c.text);
  if (c.error >= 0) check(`error in ${JSON.stringify(c.text)} at ${c.error}`, r.error === c.error);
  else check(`parse ${JSON.stringify(c.text)}`, r.error < 0 && r.tokens.map(show).join(" ") === c.tokens.join(" "));
}
check("a begin syllable reads with its hyphen", tokenText(parseLyrics("Hel-lo").tokens[0]) === "Hel-");
check("escaped text reads back as typed", parseLyrics(escapeSyllable("a-b_c/(br) d\\e")).tokens[0].text === "a-b_c/(br) d\\e");

/** function shown(text: String) => String */
function shown(text) {
  return parseLyrics(text).tokens.map(show).join(" ");
}

for (const c of LYRIC_CASES) {
  if (c.error >= 0) continue;
  const written = writeLyrics(parseLyrics(c.text).tokens);
  check(`write ${JSON.stringify(c.text)} back as ${JSON.stringify(written)}`, shown(written) === shown(c.text));
}
check("words are written in their plain form", writeLyrics(parseLyrics("Hel- lo  dark -ness _ / (br) end //").tokens) === "Hel-lo dark-ness _ / (br) end //");
check("a hold inside a word keeps the word together", shown(writeLyrics(parseLyrics("a- _ gain").tokens)) === "a< _ gain>");

{
  const p = project();
  const hook = pat("hook", 2, [
    [0, 67, 0],
    [0, 69, 1],
    [1, 36, 0],
  ]);
  hook.lyrics.push({
    channel: "lead",
    lang: "",
    mode: "sing",
    verses: [
      { key: "1", value: "oh yeah" },
      { key: "2", value: "oh no" },
    ],
    timing: [],
  });
  const verse = pat("verse", 8, [
    [0, 60, 0],
    [0, 62, 1],
  ]);
  verse.uses.push({ ...use("hook", 2), transpose: 2 });
  verse.uses.push({ ...use("hook", 4), verse: 2 });
  verse.lyrics.push({ channel: "lead", lang: "", mode: "sing", verses: [{ key: "1", value: "I said" }], timing: [] });
  p.patterns = [hook, verse];
  const notes = expandPattern(p, 1, 1);
  const lead = notes.filter((n) => n.channel === "lead");
  check("uses are placed and transposed", lead.map((n) => `${n.pitch}@${n.start}`).join(" ") === "60@0 62@1 69@2 71@3 67@4 69@5");
  check(
    "drums are not transposed",
    notes
      .filter((n) => n.channel === "kit")
      .map((n) => n.pitch)
      .join(" ") === "36 36"
  );
  check("a used pattern brings its words, the line fills the rest", words(lead) === "I said oh yeah oh no");
  check("verse count looks inside uses", verseCount(p, 1) === 2);
  check("a verse a line does not have sings the first", words(expandPattern(p, 1, 3).filter((n) => n.channel === "lead")) === "I said oh yeah oh no");
}

{
  const p = project();
  const a = pat("a", 4, [[1, 36, 0]]);
  a.uses.push(use("b", 1));
  const b = pat("b", 4, [[1, 38, 0]]);
  b.uses.push(use("a", 2));
  p.patterns = [a, b];
  check("a cycle is cut", expandPattern(p, 0, 1).length === 2);
}

{
  const p = project();
  const h = pat("h", 4, [
    [0, 60, 0],
    [0, 62, 2],
    [0, 64, 3.5],
  ]);
  h.notes[2].length = 2;
  const v = pat("v", 8, []);
  v.uses.push({ ...use("h", 6), from: 2, to: 4 });
  p.patterns = [h, v];
  const notes = expandPattern(p, 1, 1);
  check("a range is cut at its end", notes.length === 2 && notes[0].start === 6 && notes[1].start === 7.5 && notes[1].length === 0.5);
}

if (failures > 0) {
  console.log(`${failures} lyrics test(s) failed`);
  throw new Error("lyrics tests failed");
}
console.log("all lyrics tests passed");
