// Tests for the voice-to-notes logic, run with: node web/test/voice.test.js
// (also type-checked by inty via web/check.sh).
import {
  snapPitch,
  scaleSteps,
  detectKey,
  quantize,
  melodyNotes,
  drumHits,
  loopBeats,
  strengthNeeded,
  decodeTake,
  nextDrum,
  sungNotes,
  cropTake,
  takeStart,
} from "../src/voice.js";

let failures = 0;
/** function check(name: String, ok: Boolean) => Undefined */
function check(name, ok) {
  if (!ok) failures = failures + 1;
  console.log(`${ok ? "ok  " : "FAIL"} ${name}`);
}

/** function near(a: Number, b: Number) => Boolean */
function near(a, b) {
  return Math.abs(a - b) < 1e-3;
}

// ------------------------------------------------------------------ pitch

const major = scaleSteps("major");
check(
  "chromatic snapping rounds to the nearest semitone",
  snapPitch(61.4, 0, scaleSteps("chromatic")) === 61 && snapPitch(61.6, 0, scaleSteps("chromatic")) === 62
);
check("C major pulls C♯ to the nearer of C and D", snapPitch(61.3, 0, major) === 62 && snapPitch(60.7, 0, major) === 60);
check("in-scale notes stay", snapPitch(64.2, 0, major) === 64 && snapPitch(59.8, 0, major) === 60);
check("the key transposes the scale (D major keeps F♯)", snapPitch(66, 2, major) === 66 && snapPitch(65.2, 2, major) === 66);
check("pentatonic skips the gaps", snapPitch(65, 0, scaleSteps("penta")) === 64);

const cMajorTune = [60, 62, 64, 65, 67, 69, 71, 72, 67, 64, 60].map((p, i) => ({ start: i * 0.5, end: i * 0.5 + 0.45, pitch: p, velocity: 0.8 }));
const guess = detectKey(cMajorTune);
check("detects C major", guess.key === 0 && guess.scale === "major");
const aMinorTune = [57, 60, 64, 57, 59, 60, 62, 64, 57, 56, 57].map((p, i) => ({
  start: i * 0.5,
  end: i * 0.5 + (p === 57 ? 1.2 : 0.4),
  pitch: p,
  velocity: 0.8,
}));
const g2 = detectKey(aMinorTune);
check("detects A minor", g2.key === 9 && g2.scale === "minor");

// ------------------------------------------------------------------ time

check("full quantize lands on the grid", near(quantize(1.13, 0.25, 1), 1.25));
check("half strength goes halfway", near(quantize(1.15, 0.25, 0.5), 1.2));
check("no grid leaves time alone", near(quantize(1.13, 0, 1), 1.13));

// ------------------------------------------------------------------ melody

const settings = {
  detail: 2,
  grid: 0.25,
  strength: 1,
  lengths: true,
  key: 0,
  scale: "major",
  octave: 0,
  legato: false,
  dynamics: true,
  sensitivity: 0.5,
  bars: 0,
};
const take = decodeTake(
  JSON.parse(
    JSON.stringify({
      mode: "melody",
      duration: 3,
      step: 0.01,
      level: [],
      contour: [],
      notes: [
        { start: 0.52, end: 0.98, pitch: 60.2, velocity: 1 },
        { start: 1.03, end: 1.46, pitch: 61.4, velocity: 0.6 },
        { start: 1.51, end: 2.4, pitch: 67.1, velocity: 0.9 },
      ],
      hits: [],
    })
  )
);
// 120 bpm: 2 beats a second.
const notes = melodyNotes(take, settings, 120, 0, true);
check("three notes", notes.length === 3);
check("the first note starts the pattern", near(notes[0].start, 0) && near(notes[0].length, 1));
check("notes land on the grid", near(notes[1].start, 1) && near(notes[2].start, 2));
check("auto-tune snaps to C major", notes[0].pitch === 60 && notes[1].pitch === 62 && notes[2].pitch === 67);
check("dynamics keep the velocities", near(notes[1].velocity, 0.6));
const flat = melodyNotes(
  take,
  {
    detail: 2,
    grid: 0.25,
    strength: 1,
    lengths: true,
    key: 0,
    scale: "chromatic",
    octave: 1,
    legato: false,
    dynamics: false,
    sensitivity: 0.5,
    bars: 0,
  },
  120,
  0,
  true
);
check("chromatic keeps C♯ and the octave shifts", flat[1].pitch === 73 && flat[0].pitch === 72);
check("flat velocity", near(flat[1].velocity, 0.8));
const played = melodyNotes(take, settings, 120, 0.5, false);
check("unaligned takes keep their offset", near(played[0].start, 1.5));
check("loop length rounds up to bars", loopBeats(notes, 4, 0) === 4 && loopBeats(notes, 3, 0) === 6 && loopBeats(notes, 4, 2) === 8);

const gappy = decodeTake(
  JSON.parse(
    JSON.stringify({
      mode: "melody",
      duration: 2,
      step: 0.01,
      level: [],
      contour: [],
      notes: [
        { start: 0, end: 0.3, pitch: 64, velocity: 1 },
        { start: 0.5, end: 0.8, pitch: 65, velocity: 1 },
      ],
      hits: [],
    })
  )
);
const staccato = melodyNotes(gappy, settings, 120, 0, true);
const legato = melodyNotes(
  gappy,
  {
    detail: 2,
    grid: 0.25,
    strength: 1,
    lengths: true,
    key: 0,
    scale: "major",
    octave: 0,
    legato: true,
    dynamics: true,
    sensitivity: 0.5,
    bars: 0,
  },
  120,
  0,
  true
);
check("legato holds a note until the next", near(staccato[0].length, 0.5) && near(legato[0].length, 1));

// ------------------------------------------------------------------ detail

const leveled = decodeTake(
  JSON.parse(
    JSON.stringify({
      mode: "melody",
      duration: 2,
      step: 0.01,
      level: [],
      contour: [],
      notes: [{ start: 0.5, end: 1.5, pitch: 60, velocity: 1 }],
      details: [
        [{ start: 0.5, end: 1.5, pitch: 60, velocity: 1 }],
        [{ start: 0.5, end: 1.5, pitch: 60, velocity: 1 }],
        [{ start: 0.5, end: 1.5, pitch: 60, velocity: 1 }],
        [
          { start: 0.5, end: 1.0, pitch: 60, velocity: 1 },
          { start: 1.0, end: 1.5, pitch: 62, velocity: 1 },
        ],
        [
          { start: 0.5, end: 1.0, pitch: 60, velocity: 1 },
          { start: 1.0, end: 1.1, pitch: 61, velocity: 1 },
          { start: 1.1, end: 1.5, pitch: 62, velocity: 1 },
        ],
      ],
      hits: [],
    })
  )
);
check("each detail level has its notes", sungNotes(leveled, 2).length === 1 && sungNotes(leveled, 3).length === 2 && sungNotes(leveled, 4).length === 3);
check("a missing level falls back to the default notes", sungNotes(leveled, 9).length === 1 && sungNotes(take, 3).length === 3);
const detailed = melodyNotes(
  leveled,
  {
    detail: 3,
    grid: 0.25,
    strength: 1,
    lengths: true,
    key: 0,
    scale: "major",
    octave: 0,
    legato: false,
    dynamics: true,
    sensitivity: 0.5,
    bars: 0,
  },
  120,
  0,
  true
);
check("the detail setting picks the notes", detailed.length === 2 && detailed[1].pitch === 62 && near(detailed[1].start, 1));

// ------------------------------------------------------------------ drums

check("more sensitivity needs less strength", strengthNeeded(1) < strengthNeeded(0.5) && strengthNeeded(0.5) < strengthNeeded(0));
check("clicking cycles the drums", nextDrum("kick") === "tom" && nextDrum("hat") === "openhat" && nextDrum("openhat") === "kick");
const beat = decodeTake(
  JSON.parse(
    JSON.stringify({
      mode: "drums",
      duration: 2,
      step: 0.01,
      level: [],
      contour: [],
      notes: [],
      hits: [
        { time: 0.2, strength: 1, velocity: 1, kind: "kick" },
        { time: 0.46, strength: 0.6, velocity: 0.5, kind: "hat" },
        { time: 0.71, strength: 0.8, velocity: 0.9, kind: "snare" },
        { time: 0.83, strength: 0.03, velocity: 0.3, kind: "hat" },
        { time: 1.2, strength: 0.9, velocity: 0.8, kind: "tom" },
      ],
    })
  )
);
const hits = drumHits(beat, settings, 120, 0, true, []);
check("weak hits are dropped", hits.length === 4);
check("hits keep the drum the server heard", hits.map((h) => h.lane).join(",") === "kick,hat,snare,tom");
check("hits on the grid from the first one", near(hits[0].start, 0) && near(hits[1].start, 0.5) && near(hits[2].start, 1) && near(hits[3].start, 2));
const fixed = drumHits(beat, settings, 120, 0, true, ["", "snare"]);
check("a clicked hit keeps its drum", fixed[1].lane === "snare" && fixed[0].lane === "kick");
const all = drumHits(
  beat,
  {
    detail: 2,
    grid: 0.25,
    strength: 1,
    lengths: true,
    key: 0,
    scale: "major",
    octave: 0,
    legato: false,
    dynamics: true,
    sensitivity: 1,
    bars: 0,
  },
  120,
  0,
  true,
  []
);
check("full sensitivity keeps the ghost note", all.length === 5);

// ------------------------------------------------------------------ crop

const cut = cropTake(beat, 0.4, 1.0);
check("a crop starts the time at its start", near(cut.duration, 0.6) && near(cut.hits[1].time, 0.06));
check(
  "hits outside the crop keep their place but no strength",
  cut.hits.length === 5 && cut.hits[0].strength < 0 && cut.hits[4].strength < 0 && cut.hits[2].strength > 0
);
check("a cropped take starts at its first hit inside", near(takeStart(cut, 2, true), 0.06));
const cutHits = drumHits(cut, settings, 120, 0, false, []);
check("cropped hits count from the crop's start", cutHits.length === 2 && near(cutHits[0].start, 0) && near(cutHits[1].start, 0.5));
const kept = drumHits(cut, settings, 120, 0, true, ["", "snare"]);
check("a clicked hit keeps its drum after a crop", kept[0].lane === "snare");
const sungCut = cropTake(take, 0.96, 2.0);
check(
  "a crop cuts notes at its edges and drops slivers",
  sungCut.notes.length === 2 && near(sungCut.notes[0].start, 0.07) && near(sungCut.notes[1].start, 0.55) && near(sungCut.notes[1].end, 1.04)
);
check("an uncropped take is the take", cropTake(take, 0, 3) === take);

if (failures > 0) throw new Error(`${failures} test(s) failed`);
console.log("all voice-to-notes tests passed");
