// Tests for the sheet-music logic (notation.js, engrave.js), run with:
// node web/test/score.test.js (also type-checked by inty via web/check.sh).
import { emptyProject } from "../src/model.js";
import { spell, spelledName, keyAlter, stepPitch, pieces, buildScore, gather, autoClef, gmDrum, TPQ, NO_ACC, passesText } from "../src/notation.js";
import { engrave, timeX, xTick } from "../src/engrave.js";
import { scorePdf, pathOps, pdfString, pdfLayout, pageSvg } from "../src/pdf.js";
import { trackIx, insertIx } from "#brands";

let failures = 0;
/** function check(name: String, ok: Boolean) => Undefined */
function check(name, ok) {
  if (!ok) failures = failures + 1;
  console.log(`${ok ? "ok  " : "FAIL"} ${name}`);
}

/** A 4/4 project with one channel and one pattern holding `notes` ([pitch, start, length] in beats). */
/** function song(notes: Number[][], length: Number) => Project */
function song(notes, length) {
  const p = emptyProject();
  p.channels.push({
    id: "lead",
    name: "Lead",
    color: "#d4af37",
    instrument: { type: "prisme", enabled: true, params: [], options: [] },
    volume: 0.8,
    pan: 0,
    mute: false,
    mixer: insertIx(0),
  });
  p.patterns.push({
    id: "a",
    name: "A",
    color: "#d4af37",
    length: length,
    notes: notes.map((n) => ({ channel: "lead", pitch: n[0], start: n[1], length: n[2], velocity: 0.8 })),
  });
  p.playlist.tracks.push({ name: "Track 1", mute: false });
  return p;
}

const PAT = { kind: "pattern", track: 0, pattern: "a" };

// ------------------------------------------------------------------ spelling

check(
  "C major spells C♯, E♭, F♯, G♯ and B♭",
  spelledName(61, 0) === "C♯4" && spelledName(63, 0) === "E♭4" && spelledName(66, 0) === "F♯4" && spelledName(68, 0) === "G♯4" && spelledName(70, 0) === "B♭4"
);
check("E♭ major spells A♭, not G♯", spelledName(68, -3) === "A♭4");
check("C minor spells its leading tone B♮, not C♭", spelledName(71, -3) === "B4");
check("E major spells D♯, not E♭", spelledName(63, 4) === "D♯4");
check("middle C is step 35, B3 step 34", spell(60, 0).step === 35 && spell(59, 0).step === 34);
check("B♯3 in C♯ major is a B", spell(60, 7).step === 34 && spell(60, 7).alter === 1);
check(
  "key signatures alter the right letters",
  keyAlter(3, 1) === 1 && keyAlter(0, 1) === 0 && keyAlter(6, -1) === -1 && keyAlter(2, -1) === 0 && keyAlter(5, -3) === -1
);
check("steps turn back into pitches", stepPitch(35, 0) === 60 && stepPitch(37, -1) === 63 && stepPitch(38, 1) === 66);

// ------------------------------------------------------------------ rhythm

const m44 = { start: 0, length: 192, num: 4, den: 4, beat: 48, meter: true, number: 1, count: 1 };
/** function vals(ps: Piece[]) => String */
function vals(ps) {
  return ps.map((p) => `${p.d}`).join(",");
}
check("a whole bar is a whole note", vals(pieces(m44, 0, 192, false)) === "192");
check("a half note on beat 2 is tied across the middle of the bar", vals(pieces(m44, 48, 144, false)) === "48,48");
check("a dotted quarter on the beat stays dotted", vals(pieces(m44, 0, 72, false)) === "72");
check("an offbeat quarter is tied across the beat", vals(pieces(m44, 24, 72, false)) === "24,24");
check("sixteenth–eighth–sixteenth keeps the eighth", vals(pieces(m44, 0, 12, false)) === "12" && vals(pieces(m44, 12, 36, false)) === "24");
check("a rest of three beats from the downbeat is a half and a quarter", vals(pieces(m44, 0, 144, true)) === "96,48");
const m68 = { start: 0, length: 144, num: 6, den: 8, beat: 72, meter: true, number: 1, count: 1 };
check("6/8: a dotted quarter beat, and a dotted half bar", vals(pieces(m68, 0, 72, false)) === "72" && vals(pieces(m68, 0, 144, false)) === "144");

// ------------------------------------------------------------------ building

{
  // C D E F as quarters, then a long G over the bar line.
  const p = song(
    [
      [60, 0, 1],
      [62, 1, 1],
      [64, 2, 1],
      [65, 3, 1],
      [67, 3.5, 2],
    ],
    8
  );
  const sc = buildScore(p, PAT, 12);
  const evs = sc.staves[0].events;
  check("a pattern of 8 beats is two measures", sc.measures.length === 2);
  check("the melody is written on a treble staff", sc.staves[0].clef === "treble");
  const tied = evs.filter((e) => e.heads.some((h) => h.tieOut));
  check("a note over the bar line is tied", tied.length >= 1 && evs.some((e) => e.measure === 1 && e.heads.some((h) => h.tieIn)));
  check("the second measure ends in rests", evs[evs.length - 1].rest);
  check("the F under the G is cut to an eighth (one voice)", evs[3].dur === 24);
  check("eighths in a beat are beamed", evs[3].beam >= 0 && evs[3].beam === evs[4].beam);
}
{
  // A short note just before the end rounds onto the final bar line (an
  // arpeggio written out by the LMMS importer): the score must still hold it,
  // and building it must not hang.
  const p = song(
    [
      [60, 0, 1],
      [72, 3.95, 0.04],
    ],
    4
  );
  const sc = buildScore(p, PAT, 12);
  const heads = sc.staves[0].events.filter((e) => !e.rest);
  check("a note quantized onto the last bar line gets a bar of its own", sc.measures.length === 2 && heads.length === 2);
  check("the note sits on the downbeat of that bar", heads[1].start === 4 * TPQ && heads[1].measure === 1);
  const last = song([[60, 3.75, 0.25]], 4);
  check("a last sixteenth ending on the bar line adds no bar", buildScore(last, PAT, 12).measures.length === 1);
}

{
  const p = song(
    [
      [61, 0, 1],
      [61, 1, 1],
      [60, 2, 1],
    ],
    4
  );
  p.score.key = "C";
  const sc = buildScore(p, PAT, 12);
  const evs = sc.staves[0].events;
  check("an accidental shows once per measure", evs[0].heads[0].acc === 1 && evs[1].heads[0].acc === NO_ACC);
  check("a natural cancels it later in the measure", evs[2].heads[0].acc === 0);
}

{
  const p = song(
    [
      [36, 0, 1],
      [43, 1, 1],
      [72, 0, 1],
      [79, 2, 2],
    ],
    4
  );
  check("a wide range gets a grand staff", autoClef([36, 43, 48, 72, 76, 79, 84]) === "grand");
  check("a low part gets a bass clef", autoClef([36, 40, 43, 45, 48]) === "bass");
  p.score.clefs.push({ key: "lead", value: "grand" });
  const sc = buildScore(p, PAT, 12);
  check(
    "a grand staff is two staves of one channel",
    sc.staves.length === 2 && sc.staves[0].clef === "treble" && sc.staves[1].clef === "bass" && sc.staves[1].group === sc.staves[0].group
  );
  p.score.hidden.push("lead");
  check("a hidden channel has no staff", buildScore(p, PAT, 12).empty);
}

{
  const p = song([[60, 0, 4]], 20);
  const sc = buildScore(p, PAT, 12);
  check("bars where everything rests join into one multi-measure rest", sc.measures.length === 2 && sc.measures[1].count === 4 && sc.measures[1].number === 2);
  check("the multi-measure rest is one rest", sc.staves[0].events.length === 2 && sc.staves[0].events[1].whole && sc.staves[0].events[1].dur === 4 * 192);
}

check("General MIDI drums sit where drummers read them", gmDrum(36).step === 38 && gmDrum(38).step === 42 && gmDrum(42).head === "x");

// ------------------------------------------------------------------ the song

{
  const p = song([[60, 0, 1]], 4);
  p.playlist.tracks.push({ name: "Track 2", mute: false });
  p.playlist.clips.push({ pattern: "a", sample: "", track: trackIx(0), start: 4, length: 8, offset: 0, gain: 1, mixer: insertIx(0) });
  p.playlist.clips.push({ pattern: "a", sample: "", track: trackIx(1), start: 16, length: 2, offset: 0, gain: 1, mixer: insertIx(0) });
  const g = gather(p, { kind: "song", track: 0, pattern: "" });
  check("a clip loops its pattern", g.notes.length === 3 && g.notes[0].start === 4 && g.notes[1].start === 8 && g.notes[1].origin === 8);
  check(
    "each sounding note knows its pattern note",
    g.notes.every((n) => n.pattern === "a" && n.index === 0)
  );
  check("a track's score holds only its clips", gather(p, { kind: "track", track: 1, pattern: "" }).notes.length === 1);
  p.score.hiddenTracks.push(trackIx(1));
  check("a hidden track is left out of the song", gather(p, { kind: "song", track: 0, pattern: "" }).notes.length === 2);
  p.score.marks.push({ start: 0, end: 1, color: "#c97b84", label: "Motif", pattern: "a", channels: [] });
  const sc = buildScore(p, { kind: "song", track: 0, pattern: "" }, 12);
  check("a pattern's color follows it into the song", sc.marks.length === 2 && sc.marks[0].start === 4 && sc.marks[1].start === 8);
}

// ------------------------------------------------------------------ engraving

{
  /** const notes: Number[][] */
  const notes = [];
  for (let i = 0; i < 64; i++) notes.push([60 + (i % 12), i * 0.5, 0.5]);
  const p = song(notes, 32);
  const sc = buildScore(p, PAT, 12);
  const page = engrave(sc, { width: 120, hideEmpty: false });
  check("eight bars of eighths break into several systems", page.systems.length >= 2);
  let full = true;
  for (let i = 0; i + 1 < page.systems.length; i++) {
    const s = page.systems[i];
    if (Math.abs(s.x1 - 120) > 0.05) full = false;
  }
  check("every system but the last is justified to the width", full);
  const last = page.systems[page.systems.length - 1];
  check("systems follow each other down the page", page.systems[1].top > page.systems[0].top && page.height >= last.top + last.height);
  let heads = 0;
  for (const s of page.systems) heads = heads + s.heads.length;
  check("every note gets a notehead to click", heads === 64);
  const s0 = page.systems[0];
  check("time maps to x and back", Math.abs(xTick(s0.times, timeX(s0.times, 2 * TPQ)) - 2 * TPQ) < 1e-6 && timeX(s0.times, TPQ) < timeX(s0.times, 2 * TPQ));
}

// ------------------------------------------------------------------ repeats

check("endings name their passes", passesText([1]) === "1." && passesText([2, 1]) === "1.–2." && passesText([1, 3]) === "1., 3.");
{
  /** const notes: Number[][] */
  const notes = [];
  for (let i = 0; i < 4; i++) notes.push([60 + i, i, 1]);
  const p = song(notes, 24);
  p.playlist.clips.push({ pattern: "a", sample: "", track: trackIx(0), start: 0, length: 24, offset: 0, gain: 1, mixer: insertIx(0) });
  p.repeats.push({
    start: 4,
    end: 12,
    times: 3,
    endings: [
      { start: 8, end: 12, passes: [1, 2] },
      { start: 12, end: 16, passes: [3] },
    ],
  });
  const SONG = { kind: "song", track: 0, pattern: "" };
  const sc = buildScore(p, SONG, 12);
  check("the song's repeats come into its score", sc.repeats.length === 1);
  check("resting bars join up only between repeat signs and endings", sc.measures.map((m) => m.count).join(",") === "1,1,1,1,2");
  check("a pattern's score has no repeats", buildScore(p, PAT, 12).repeats.length === 0);
  const page = engrave(sc, { width: 200, hideEmpty: false });
  /** const texts: String[] */
  const texts = [];
  for (const s of page.systems) for (const l of s.labels) texts.push(`${l.cls}:${l.text}`);
  check("a repeat played three times says so", texts.includes("reptimes:×3"));
  check("endings are bracketed with their passes", texts.includes("volta:1.–2.") && texts.includes("volta:3."));
  p.repeats[0].times = 2;
  p.repeats[0].endings = [];
  const twice = engrave(buildScore(p, SONG, 12), { width: 200, hideEmpty: false });
  check(
    "a plain repeat (twice) has no count and no brackets",
    twice.systems.every((s) => s.labels.every((l) => l.cls !== "reptimes" && l.cls !== "volta"))
  );
}

// ------------------------------------------------------------------ PDF

check("SVG paths become PDF paths", pathOps("M1 2L3 4H5V6C1 1 2 2 3 3Z") === "1 2 m\n3 4 l\n5 4 l\n5 6 l\n1 1 2 2 3 3 c\nh");
check("PDF strings escape and spell out", pdfString("A (b) ♭ é Œ") === "(A \\(b\\) -flat \\351 \\214)");
{
  /** const notes: Number[][] */
  const notes = [];
  for (let i = 0; i < 400; i++) notes.push([60 + (i % 12), i * 0.5, 0.5]);
  const sc = buildScore(song(notes, 200), PAT, 12);
  /** function measure(face: String, text: String) => Number */
  function measure(face, text) {
    return text.length * 0.45;
  }
  const objs = scorePdf(sc, { title: "Étude", subtitle: "C major", author: "", bpm: 96 }, "a4", false, measure);
  const pages = objs.filter((o) => o.head.startsWith("<< /Type /Page /"));
  check("a long score fills several A4 pages", pages.length >= 2 && pages[0].head.includes("595.28 841.89"));
  check("the catalog comes first and the document info last", objs[0].head.includes("/Catalog") && objs[objs.length - 1].head.includes("/Title (\\311tude)"));
  check("the pages list counts its pages", objs[1].head.includes(`/Count ${pages.length}`));
  check("noteheads are drawn from glyph forms", objs.some((o) => o.stream.includes(" Do Q")) && objs.some((o) => o.head.includes("/Subtype /Form")));
  const letter = scorePdf(sc, { title: "x", subtitle: "", author: "", bpm: 0 }, "letter", false, measure);
  check(
    "US Letter pages",
    letter.some((o) => o.head.includes("612 792"))
  );
  // As on screen: each page as SVG, on paper and through the ink filter.
  const lay = pdfLayout(sc, "a4", false);
  check("the pages as on screen are the PDF's pages", lay.pages.length === pages.length);
  const wet = pageSvg(lay, 0, { title: "Étude & co", subtitle: "", author: "", bpm: 96 }, { wet: true, gloss: 1, shine: 0.75 }, 2);
  check(
    "a page drawn as on screen is an SVG of its size in pixels",
    wet.startsWith("<svg") && wet.includes('width="1191" height="1684"') && wet.endsWith("</svg>")
  );
  check(
    "on paper, through the wet ink, with its glints",
    wet.includes('<pattern id="tooth"') && wet.includes("feSpecularLighting") && wet.includes('filter="url(#gloss)"')
  );
  check("its glyphs are defined once and placed", wet.includes('<path id="G') && wet.includes('<use href="#G'));
  check("its text is escaped", wet.includes("Étude &amp; co"));
  const dry = pageSvg(lay, 1, { title: "x", subtitle: "", author: "", bpm: 0 }, { wet: false, gloss: 1, shine: 0.75 }, 1);
  check("dry ink has no glints", dry.includes("feDisplacementMap") && !dry.includes('filter="url(#gloss)"'));
}

if (failures > 0) {
  console.log(`${failures} score test(s) failed`);
  throw new Error("score tests failed");
}
console.log("all score tests passed");
