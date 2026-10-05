// Tests for the project model's wire format (model.js), run with:
// node web/test/model.test.js (also type-checked by inty via web/check.sh).
import { emptyProject, decodeProject, projectJson, drumName, moveRole } from "../src/model.js";
import { trackIx, insertIx } from "#brands";

let failures = 0;
/** function check(name: String, ok: Boolean) => Undefined */
function check(name, ok) {
  if (!ok) failures = failures + 1;
  console.log(`${ok ? "ok  " : "FAIL"} ${name}`);
}

// A project without a drum part writes none.
const bare = emptyProject();
check("no drum part, no drums key", !projectJson(bare).includes('"drums"'));
check("no drum part decodes as off", decodeProject(JSON.parse(projectJson(bare))).drums.on === false);

// A drum part survives a round trip (the studio re-encodes the project on every edit).
const p = emptyProject();
p.drums = {
  on: true,
  groove: "rock-8ths",
  kit: "Ebony",
  feel: "loose",
  swing: 0.3,
  start: 5,
  ending: "none",
  variations: false,
  seed: 7,
  sections: [
    { name: "Verse", bars: 8, play: "a", fill: "beat", crash: false, groove: "" },
    { name: "Bridge", bars: 4, play: "b", fill: "none", crash: true, groove: "rock-halftime" },
  ],
  grooves: [{ groove: "rock-8ths", a: [["kick", "X... ...."]], b: [] }],
  kept: [{ slot: "rock-8ths/a+crash", name: "Straight 8ths A + crash", notes: [{ role: "snare", start: 1, length: 0.25, velocity: 0.5 }] }],
  written: [{ id: "drums-straight-8ths-a", slot: "rock-8ths/a", print: "0badf00d" }],
};
const text = projectJson(p);
const back = decodeProject(JSON.parse(text));
check("drum part round-trips", JSON.stringify(back.drums) === JSON.stringify(p.drums));
const wire = JSON.parse(text).drums;
check("defaults are left out of sections", wire.sections[0].crash === undefined && wire.sections[1].fill === undefined);
check(
  "written maps pattern ids to slot and fingerprint",
  wire.written["drums-straight-8ths-a"].print === "0badf00d" && wire.written["drums-straight-8ths-a"].slot === "rock-8ths/a"
);
check("kept patterns are keyed by slot", wire.kept["rock-8ths/a+crash"].notes[0].role === "snare");
check("groove edits are keyed by groove", wire.grooves["rock-8ths"].a[0][1] === "X... ....");

// The server's defaults fill a sparse part.
const sparse = decodeProject(JSON.parse(projectJson(emptyProject())));
const raw = JSON.parse(projectJson(sparse));
raw.drums = { groove: "house", sections: [{ bars: 4 }] };
const d = decodeProject(raw).drums;
check(
  "sparse part gets defaults",
  d.on &&
    d.feel === "natural" &&
    d.start === 1 &&
    d.ending === "hit" &&
    d.variations &&
    d.seed === 1 &&
    d.sections[0].play === "a" &&
    d.sections[0].fill === "none"
);

// The piano roll names the drums of every General MIDI kit, numbered variations too.
const withKits = JSON.parse(projectJson(emptyProject()));
withKits.channels = ["Jazz Kit", "Jazz Kit 2", "Tenor Sax"].map((program, i) => ({
  id: `c${i}`,
  name: program,
  instrument: { type: "soundfont", options: { program: program } },
}));
const chans = decodeProject(withKits).channels;
check("a kit names its drums", drumName(chans[0], 38) === "Snare");
check("so does a kit variation", drumName(chans[1], 38) === "Snare");
check("an instrument does not", drumName(chans[2], 38) === "");

// The Critic's settings travel with the song (and are left out when empty).
check("no critic settings, no critic key", !projectJson(bare).includes('"critic"'));
const linted = emptyProject();
linted.critic.off.push("loopitis");
linted.critic.suppress.push("flat-velocity|pattern:beat:hat:-1");
const relinted = decodeProject(JSON.parse(projectJson(linted)));
check("checks turned off survive a save", relinted.critic.off.join() === "loopitis");
check("suppressed findings survive a save", relinted.critic.suppress.join() === "flat-velocity|pattern:beat:hat:-1");
linted.critic.on.push("parallel-fifths");
check("checks turned on survive a save", decodeProject(JSON.parse(projectJson(linted))).critic.on.join() === "parallel-fifths");

// Moving a part to another channel: in a pattern, the song, a track or a passage.
/** function roleSong() => Project */
function roleSong() {
  const raw = JSON.parse(projectJson(emptyProject()));
  raw.channels = ["piano", "strings", "flute"].map((id) => ({ id: id, name: id, instrument: { type: "synth" } }));
  raw.patterns = [
    {
      id: "a",
      name: "A",
      length: 4,
      notes: [0, 1, 2, 3]
        .map((t) => ({ channel: "piano", pitch: 60 + t, start: t, length: 1 }))
        .concat([{ channel: "strings", pitch: 48, start: 0, length: 4 }]),
    },
  ];
  raw.playlist = {
    tracks: [{ name: "Keys" }, { name: "More" }],
    clips: [
      { pattern: "a", track: 0, start: 0, length: 4 },
      { pattern: "a", track: 0, start: 4, length: 4 },
      { pattern: "a", track: 0, start: 8, length: 8 },
    ],
  };
  return decodeProject(raw);
}
/** function chOf(pat: Pattern) => String */
function chOf(pat) {
  return pat.notes.map((n) => n.channel[0]).join("");
}
const song = { kind: "song", track: 0, pattern: "" };
const r1 = roleSong();
const m1 = moveRole(r1, { kind: "pattern", track: 0, pattern: "a" }, { from: ["piano"], to: "flute", t0: 1, t1: 3 });
check("a passage of a pattern moves to another channel", m1.notes === 2 && chOf(r1.patterns[0]) === "pffps");
const r2 = roleSong();
const m2 = moveRole(r2, song, { from: ["piano"], to: "flute", t0: 0, t1: Infinity });
check("the whole song moves the part in place", m2.notes === 4 && m2.copies === 0 && m2.splits === 0 && chOf(r2.patterns[0]) === "ffffs");
const r3 = roleSong();
const m3 = moveRole(r3, song, { from: ["piano"], to: "flute", t0: 4, t1: 8 });
check(
  "a pattern played elsewhere too is copied for the passage",
  m3.copies === 1 &&
    r3.patterns.length === 2 &&
    chOf(r3.patterns[0]) === "pppps" &&
    chOf(r3.patterns[1]) === "ffffs" &&
    r3.playlist.clips[1].pattern === r3.patterns[1].id
);
check("the copy keeps the other clips on the original", r3.playlist.clips[0].pattern === "a" && r3.playlist.clips[2].pattern === "a");
const r4 = roleSong();
const m4 = moveRole(r4, song, { from: ["piano"], to: "flute", t0: 10, t1: 14 });
const cl4 = r4.playlist.clips;
check("a clip crossing the passage is split at its edges", m4.splits === 2 && cl4.length === 5);
check(
  "the parts keep their place in the pattern",
  cl4[2].start === 8 && cl4[2].length === 2 && cl4[3].start === 10 && cl4[3].offset === 2 && cl4[3].length === 4 && cl4[4].start === 14 && cl4[4].offset === 6
);
check("only the part inside plays the copy", cl4[3].pattern !== "a" && cl4[2].pattern === "a" && cl4[4].pattern === "a");
const r5 = roleSong();
r5.playlist.clips.push({ pattern: "a", sample: "", track: trackIx(1), start: 0, length: 4, offset: 0, gain: 1, mixer: insertIx(0) });
moveRole(r5, { kind: "track", track: 1, pattern: "" }, { from: ["piano"], to: "flute", t0: 0, t1: Infinity });
check("one track's part leaves the other tracks alone", chOf(r5.patterns[0]) === "pppps" && r5.playlist.clips[3].pattern !== "a");
const r6 = roleSong();
const m6 = moveRole(r6, song, { from: ["strings"], to: "flute", t0: 5, t1: 8 });
check("nothing to move, nothing changes", m6.notes === 0 && m6.splits === 0 && r6.playlist.clips.length === 3);
const r7 = roleSong();
const m7 = moveRole(r7, song, { from: ["piano"], to: "nobody", t0: 0, t1: Infinity });
check("no such channel, nothing changes", m7.notes === 0 && chOf(r7.patterns[0]) === "pppps");

if (failures > 0) {
  console.log(`${failures} model test(s) failed`);
  throw new Error("model tests failed");
}
console.log("all model tests passed");
