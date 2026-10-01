// Tests for the sheet-music logic (notation.js, engrave.js), run with:
// node web/test/score.test.js (also type-checked by inty via web/check.sh).
import { emptyProject } from "../src/model.js";
import { spell, spelledName, keyAlter, stepPitch, pieces, buildScore, gather, autoClef, gmDrum, TPQ, NO_ACC } from "../src/notation.js";
import { engrave, timeX, xTick } from "../src/engrave.js";
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
check("E major spells D♯, not E♭", spelledName(63, 4) === "D♯4");
check("middle C is step 35, B3 step 34", spell(60, 0).step === 35 && spell(59, 0).step === 34);
check("B♯3 in C♯ major is a B", spell(60, 7).step === 34 && spell(60, 7).alter === 1);
check(
  "key signatures alter the right letters",
  keyAlter(3, 1) === 1 && keyAlter(0, 1) === 0 && keyAlter(6, -1) === -1 && keyAlter(2, -1) === 0 && keyAlter(5, -3) === -1
);
check("steps turn back into pitches", stepPitch(35, 0) === 60 && stepPitch(37, -1) === 63 && stepPitch(38, 1) === 66);

// ------------------------------------------------------------------ rhythm

const m44 = { start: 0, length: 192, num: 4, den: 4, beat: 48, meter: true };
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
const m68 = { start: 0, length: 144, num: 6, den: 8, beat: 72, meter: true };
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

if (failures > 0) {
  console.log(`${failures} score test(s) failed`);
  throw new Error("score tests failed");
}
console.log("all score tests passed");
