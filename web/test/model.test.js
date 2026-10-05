// Tests for the project model's wire format (model.js), run with:
// node web/test/model.test.js (also type-checked by inty via web/check.sh).
import { emptyProject, decodeProject, projectJson, drumName, moveRole } from "../src/model.js";
import { trackIx, trackIndex, insertIx } from "#brands";

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
/** What the song plays, as the engine plays it: each clip loops its pattern from its offset, plays
 * the notes that start inside it and ends them at its end ("track channel pitch start-end" each). */
/** function audible(p: Project) => String[] */
function audible(p) {
  /** const out: String[] */
  const out = [];
  for (const c of p.playlist.clips) {
    const pat = p.patterns.find((x) => x.id === c.pattern);
    if (!pat) continue;
    const end = c.start + c.length;
    for (let k = 0; c.start - c.offset + k * pat.length < end; k++) {
      for (const n of pat.notes) {
        const t = c.start - c.offset + k * pat.length + n.start;
        if (n.start >= pat.length || t < c.start - 1e-9 || t >= end - 1e-9) continue;
        const r = (x) => Math.round(x * 1000) / 1000;
        out.push(`${trackIndex(c.track)} ${n.channel} ${n.pitch} ${r(t)}-${r(Math.min(t + n.length, end))}`);
      }
    }
  }
  return out.sort();
}
/** What the song should play after the notes of channel `part` starting (as written, on `snap`) in [t0, t1)
 * of the scope (the song, or one track) moves to `to`. */
/** function expected(before: String[], scope: Scope, part: String, to: String, t0: Number, t1: Number, snap: Number) => String[] */
function expected(before, scope, part, to, t0, t1, snap) {
  return before
    .map((e) => {
      const f = e.split(" ");
      const t = Number(f[3].split("-")[0]);
      const q = snap > 0 ? Math.round(t / snap) * snap : t;
      const here = scope.kind !== "track" || Number(f[0]) === scope.track;
      return here && f[1] === part && q >= t0 && q < t1 ? `${f[0]} ${to} ${f[2]} ${f[3]}` : e;
    })
    .sort();
}
const song = { kind: "song", track: 0, pattern: "" };
/** Move, and check that the song plays just what it did with the part's notes in the window on `to`. */
/** function moveChecked(name: String, p: Project, scope: Scope, part: String, to: String, t0: Number, t1: Number, snap: Number) => Moved */
function moveChecked(name, p, scope, part, to, t0, t1, snap) {
  const before = audible(p);
  const res = moveRole(p, scope, { from: [part], to: to, t0: t0, t1: t1, snap: snap });
  const after = audible(p);
  const want = expected(before, scope, part, to, t0, t1, snap);
  check(`${name}: the song plays the same, the part on its new channel`, after.join("|") === want.join("|"));
  if (after.join("|") !== want.join("|")) console.log("   want", want.join(" | "), "\n   got ", after.join(" | "));
  return res;
}
const r1 = roleSong();
const m1 = moveRole(r1, { kind: "pattern", track: 0, pattern: "a" }, { from: ["piano"], to: "flute", t0: 1, t1: 3, snap: 0 });
check("a passage of a pattern moves to another channel", m1.notes === 2 && chOf(r1.patterns[0]) === "pffps");
const r2 = roleSong();
const m2 = moveChecked("the whole song", r2, song, "piano", "flute", 0, Infinity, 0);
check("the whole song moves the part in place", m2.notes === 4 && m2.copies === 0 && m2.unrolled === 0 && chOf(r2.patterns[0]) === "ffffs");
const r3 = roleSong();
const m3 = moveChecked("a passage of whole clips", r3, song, "piano", "flute", 4, 8, 0);
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
const m4 = moveChecked("a passage inside a looping clip", r4, song, "piano", "flute", 10, 14, 0);
const cl4 = r4.playlist.clips;
check(
  "no clip is cut: a clip across the edges gets a pattern of its own",
  m4.unrolled === 1 && cl4.length === 3 && cl4[2].pattern !== "a" && cl4[2].offset === 0
);
check(
  "the strings held across the edges still sound whole",
  audible(r4)
    .filter((e) => e.includes("strings"))
    .join() === "0 strings 48 0-4,0 strings 48 12-16,0 strings 48 4-8,0 strings 48 8-12"
);
const r5 = roleSong();
r5.playlist.clips.push({ pattern: "a", sample: "", track: trackIx(1), start: 0, length: 4, offset: 0, gain: 1, mixer: insertIx(0) });
moveChecked("one track", r5, { kind: "track", track: 1, pattern: "" }, "piano", "flute", 0, Infinity, 0);
check("one track's part leaves the other tracks alone", chOf(r5.patterns[0]) === "pppps" && r5.playlist.clips[3].pattern !== "a");
const r6 = roleSong();
const m6 = moveRole(r6, song, { from: ["strings"], to: "flute", t0: 5, t1: 8, snap: 0 });
check("nothing to move, nothing changes", m6.notes === 0 && m6.unrolled === 0 && r6.playlist.clips.length === 3 && r6.patterns.length === 1);
const r7 = roleSong();
const m7 = moveRole(r7, song, { from: ["piano"], to: "nobody", t0: 0, t1: Infinity, snap: 0 });
check("no such channel, nothing changes", m7.notes === 0 && chOf(r7.patterns[0]) === "pppps");
// A clip that starts partway into its pattern, across an edge of the window.
const r8 = roleSong();
r8.playlist.clips = [{ pattern: "a", sample: "", track: trackIx(0), start: 0, length: 10, offset: 2, gain: 1, mixer: insertIx(0) }];
moveChecked("a clip with an offset across an edge", r8, song, "piano", "flute", 4, 8, 0);
check("its own pattern starts where the clip did", r8.playlist.clips[0].offset === 0 && r8.patterns[1].length === 10 && r8.patterns[1].notes[0].start === 0);
// Notes played a little early count in the bar they are written in (rounded to the grid).
const r9 = roleSong();
r9.patterns[0].notes[0].start = 3.98 - 3; // 0.98: written on beat 1
r9.patterns[0].notes[2].start = 1.97; // written on beat 2
r9.patterns[0].notes.splice(1, 1);
const m9 = moveRole(r9, { kind: "pattern", track: 0, pattern: "a" }, { from: ["piano"], to: "flute", t0: 1, t1: 2, snap: 0.25 });
check(
  "a note played early counts in the beat it is written on",
  m9.notes === 1 && r9.patterns[0].notes[0].channel === "flute" && r9.patterns[0].notes[1].channel === "piano"
);
const r10 = roleSong();
r10.patterns[0].notes[3].start = 11.98 - 8; // 3.98 in the pattern: past its end, never plays
r10.patterns[0].notes[2].start = 1.98; // song 9.98 in the long clip: written on 10
moveChecked("early notes at the window's edge", r10, song, "piano", "flute", 10, 12, 0.25);
// A drummer's pattern: its copy is kept as edited by hand, so writing the drums again leaves it.
const r11 = roleSong();
r11.patterns[0].drums.on = true;
r11.patterns[0].drums.groove = "rock-8ths";
moveChecked("a drummer's pattern", r11, song, "piano", "flute", 4, 8, 0);
check("a drummer's pattern copied is kept as edited", r11.patterns[1].drums.on && r11.patterns[1].drums.edited && !r11.patterns[0].drums.edited);

if (failures > 0) {
  console.log(`${failures} model test(s) failed`);
  throw new Error("model tests failed");
}
console.log("all model tests passed");
