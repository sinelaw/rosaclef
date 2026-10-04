// Tests for the Critic's checks (critic.js), run with: node web/test/critic.test.js
// (also type-checked by inty via web/check.sh).
import { emptyProject, noArp, cloneProject, newDevice, setParam, setOption } from "../src/model.js";
import { critique, RULES, CATEGORIES, ruleOf } from "../src/critic.js";
import { trackIx, insertIx } from "#brands";

let failures = 0;
/** function check(name: String, ok: Boolean) => Undefined */
function check(name, ok) {
  if (!ok) failures = failures + 1;
  console.log(`${ok ? "ok  " : "FAIL"} ${name}`);
}

// ------------------------------------------------------------------ builders

/** function channel(p: Project, id: String, type: String, program: String) => Channel */
function channel(p, id, type, program) {
  const inst = newDevice(type);
  if (type === "soundfont") setOption(inst, "program", program);
  if (type === "drum") setOption(inst, "kind", program);
  const c = { id: id, name: id, color: "#d4af37", instrument: inst, volume: 0.8, pan: 0, mute: false, mixer: insertIx(0), arp: noArp(), layerOf: "" };
  p.channels.push(c);
  return c;
}

/** A pattern of [channel, pitch, start, length, velocity] notes. */
/** function pattern(p: Project, id: String, length: Number, notes: { c: String, n: Number[] }[]) => Pattern */
function pattern(p, id, length, notes) {
  const pat = {
    id: id,
    name: id,
    color: "#d4af37",
    length: length,
    notes: notes.map((x) => ({ channel: x.c, pitch: x.n[0], start: x.n[1], length: x.n[2], velocity: x.n.length > 3 ? x.n[3] : 0.8 })),
  };
  p.patterns.push(pat);
  return pat;
}

/** function place(p: Project, pat: String, track: Int, start: Number, length: Number) => Undefined */
function place(p, pat, track, start, length) {
  while (p.playlist.tracks.length <= track) p.playlist.tracks.push({ name: `T${p.playlist.tracks.length + 1}`, mute: false });
  p.playlist.clips.push({ pattern: pat, sample: "", track: trackIx(track), start: start, length: length, offset: 0, gain: 1, mixer: insertIx(0) });
}

/** function notesOf(c: String, list: Number[][]) => { c: String, n: Number[] }[] */
function notesOf(c, list) {
  return list.map((n) => ({ c: c, n: n }));
}

/** function rules(p: Project) => String[] */
function rules(p) {
  return critique(p, []).map((f) => f.rule);
}

/** Every fix makes its own finding go away (and changes nothing when there is none). */
/** function fixesResolve(name: String, p: Project, only: String) => Undefined */
function fixesResolve(name, p, only) {
  const found = critique(p, []).filter((f) => f.fix !== "" && (only === "" || f.rule === only));
  if (only !== "") check(`${name}: ${only} is offered as a fix`, found.length > 0);
  for (const f of found) {
    const q = cloneProject(p);
    // The finding as found on the copy (indexes and ids match the original's).
    const g = critique(q, []).find((x) => x.key === f.key);
    if (!g) {
      check(`${name}: ${f.key} reproduces on a copy`, false);
      continue;
    }
    g.apply(q);
    const after = critique(q, []);
    check(`${name}: "${f.fix}" resolves ${f.rule}`, !after.some((x) => x.key === f.key));
  }
}

// ------------------------------------------------------------------ catalog

check(
  "every rule has a known category and a reason",
  RULES.every((r) => CATEGORIES.includes(r.category) && r.why.length > 10)
);
check(
  "rule ids are unique",
  RULES.every((r, i) => RULES.findIndex((x) => x.id === r.id) === i)
);
check("an unknown rule still has a category", ruleOf("nope").category === "Project");
check("an empty project has nothing to say", critique(emptyProject(), []).filter((f) => f.rule !== "names").length === 0);

// ------------------------------------------------------------------ rhythm

{
  const p = emptyProject();
  p.meta.title = "Rhythm";
  channel(p, "hat", "drum", "hat");
  const hats = [];
  for (let i = 0; i < 16; i++) hats.push([60, i * 0.25, 0.25, 0.8]);
  pattern(p, "beat", 4, notesOf("hat", hats));
  check("flat velocities are reported", rules(p).includes("flat-velocity"));
  fixesResolve("hats", p, "flat-velocity");
  check("rules can be turned off", !critique(p, ["flat-velocity"]).some((f) => f.rule === "flat-velocity"));
}
{
  const p = emptyProject();
  channel(p, "keys", "fm", "");
  const ns = [];
  for (let i = 0; i < 10; i++) ns.push([60 + (i % 5), i * 0.5, 0.5, i === 3 ? 0.9 : 1]);
  pattern(p, "loud", 8, notesOf("keys", ns));
  check("full velocities are reported", rules(p).includes("max-velocity"));
  fixesResolve("loud", p, "max-velocity");
}
{
  const p = emptyProject();
  channel(p, "lead", "analog", "");
  pattern(
    p,
    "messy",
    4,
    notesOf("lead", [
      [64, 0, 1, 0.8],
      [64, 0, 1, 0.7],
      [67, 1, 2, 0.8],
      [67, 2, 1, 0.6],
      [69, 3, 0.01, 0.8],
      [71, 3.5, 0.5, 0],
      [72, 5, 1, 0.8],
    ])
  );
  const r = rules(p);
  check("duplicates are reported", r.includes("duplicate-notes"));
  check("same-pitch overlaps are reported", r.includes("same-pitch-overlap"));
  check("very short notes are reported", r.includes("tiny-notes"));
  check("silent notes are reported", r.includes("silent-notes"));
  check("notes after the pattern's end are reported", r.includes("past-end"));
  fixesResolve("messy", p, "");
}
{
  const p = emptyProject();
  channel(p, "piano", "soundfont", "Acoustic Grand Piano");
  const ns = [];
  for (let i = 0; i < 16; i++) ns.push([60 + (i % 3) * 2, i * 0.5, 0.5, 0.6 + (i % 4) * 0.1]);
  pattern(p, "grid", 8, notesOf("piano", ns));
  check("a sampled piano exactly on the grid is reported", rules(p).includes("rigid-timing"));
  fixesResolve("grid", p, "rigid-timing");
  const q = cloneProject(p);
  q.patterns[0].notes[5].start = 2.52;
  check("a note a hair off an otherwise quantized part is reported", rules(q).includes("sloppy-timing"));
  fixesResolve("slip", q, "sloppy-timing");
}

// ------------------------------------------------------------------ harmony

{
  const p = emptyProject();
  channel(p, "pad", "analog", "");
  pattern(
    p,
    "low",
    8,
    notesOf("pad", [
      [36, 0, 4],
      [40, 0, 4],
      [43, 0, 4],
      [36, 4, 4],
      [41, 4, 4],
      [45, 4, 4],
    ])
  );
  check("close intervals voiced low are reported", rules(p).includes("low-interval"));
  fixesResolve("low", p, "low-interval");
}
{
  const p = emptyProject();
  channel(p, "bass", "analog", "");
  channel(p, "keys", "analog", "");
  pattern(
    p,
    "verse",
    8,
    notesOf("bass", [
      [36, 0, 2],
      [36, 2, 2],
      [41, 4, 2],
      [43, 6, 2],
    ]).concat(
      notesOf("keys", [
        [43, 0, 4],
        [47, 0, 4],
        [50, 0, 4],
        [45, 4, 4],
        [47, 4, 4],
        [52, 4, 4],
      ])
    )
  );
  check("chords in the bass register are reported when there is a bass", rules(p).includes("chord-too-low"));
  fixesResolve("verse", p, "chord-too-low");
}
{
  const p = emptyProject();
  channel(p, "keys", "fm", "");
  const chords = [
    [60, 64, 67],
    [69, 72, 76],
    [53, 57, 60],
    [67, 71, 74],
    [60, 64, 67],
    [69, 72, 76],
  ];
  const ns = [];
  for (let i = 0; i < chords.length; i++) for (const x of chords[i]) ns.push([x, i * 2, 2]);
  pattern(p, "jumpy", 12, notesOf("keys", ns));
  check("chord changes that jump are reported", rules(p).includes("voice-leading"));
  fixesResolve("jumpy", p, "voice-leading");
}
{
  const p = emptyProject();
  channel(p, "strings", "soundfont", "String Ensemble 1");
  pattern(
    p,
    "fifths",
    8,
    notesOf("strings", [
      [48, 0, 2],
      [55, 0, 2],
      [64, 0, 2],
      [50, 2, 2],
      [57, 2, 2],
      [65, 2, 2],
    ])
  );
  check("parallel fifths in an acoustic part are reported", rules(p).includes("parallel-fifths"));
  const q = cloneProject(p);
  q.channels[0].instrument = newDevice("analog");
  check("... but not in a synth stack", !rules(q).includes("parallel-fifths"));
}
{
  const p = emptyProject();
  channel(p, "lead", "analog", "");
  const scale = [60, 62, 64, 65, 67, 69, 71, 72];
  const ns = [];
  for (let i = 0; i < 32; i++) ns.push([scale[i % 8], i * 0.5, 0.5]);
  ns.push([66, 16, 0.5]);
  for (const x of [48, 52, 55]) ns.push([x, 0, 8]);
  pattern(p, "tune", 17, notesOf("lead", ns));
  check("a stray note outside the key is reported", rules(p).includes("out-of-key"));
  fixesResolve("tune", p, "out-of-key");
  const q = cloneProject(p);
  q.score.key = "Eb";
  check("a score key the notes do not fit is reported", rules(q).includes("key-signature"));
  fixesResolve("key", q, "key-signature");
}

// ------------------------------------------------------------------ melody and ranges

{
  const p = emptyProject();
  channel(p, "flute", "soundfont", "Flute");
  pattern(
    p,
    "low",
    4,
    notesOf("flute", [
      [50, 0, 1],
      [62, 1, 1],
      [100, 2, 1],
    ])
  );
  check("notes beyond a real instrument's range are reported", rules(p).includes("instrument-range"));
  fixesResolve("flute", p, "instrument-range");
}
{
  const p = emptyProject();
  channel(p, "sub", "analog", "");
  pattern(
    p,
    "rumble",
    4,
    notesOf("sub", [
      [22, 0, 2],
      [26, 2, 2],
    ])
  );
  check("bass notes under E1 are reported", rules(p).includes("sub-too-low"));
  fixesResolve("rumble", p, "sub-too-low");
}
{
  const p = emptyProject();
  channel(p, "lead", "analog", "");
  const ns = [];
  for (let i = 0; i < 40; i++) ns.push([i % 2 === 0 ? 60 : 88, i, 1]);
  pattern(p, "wild", 40, notesOf("lead", ns));
  const r = rules(p);
  check("a wide melody is reported", r.includes("melody-range"));
  check("leaps over an octave are reported", r.includes("large-leap"));
  check("a melody without rests is reported", r.includes("no-rests"));
  check("a two-note melody is reported", r.includes("monotone"));
}

// ------------------------------------------------------------------ arrangement

{
  const p = emptyProject();
  channel(p, "kick", "drum", "kick");
  const ns = [];
  for (let i = 0; i < 4; i++) ns.push([60, i, 0.5, 0.7 + i * 0.08]);
  pattern(p, "loop", 4, notesOf("kick", ns));
  check("an empty playlist is reported", rules(p).includes("empty-playlist"));
  fixesResolve("empty", p, "empty-playlist");
  place(p, "loop", 0, 0, 160);
  check("one bar for forty bars is loopitis", rules(p).includes("loopitis"));
  place(p, "loop", 0, 158.5, 4);
  check("overlapping clips are reported", rules(p).includes("clip-overlap"));
  fixesResolve("overlap", p, "clip-overlap");
  const q = emptyProject();
  channel(q, "kick", "drum", "kick");
  pattern(q, "loop", 4, notesOf("kick", ns));
  place(q, "loop", 0, 0.25, 4);
  check("a clip just off the bar is reported", rules(q).includes("clip-off-bar"));
  fixesResolve("offbar", q, "clip-off-bar");
  pattern(q, "copy", 4, notesOf("kick", ns));
  place(q, "copy", 1, 4, 4);
  check("identical patterns are reported", rules(q).includes("identical-patterns"));
  fixesResolve("copy", q, "identical-patterns");
}

// ------------------------------------------------------------------ mix and master

{
  const p = emptyProject();
  p.meta.title = "Mix";
  const names = ["kick", "bass", "hat", "keys", "pad", "pluck"];
  for (const n of names) channel(p, n, n === "kick" || n === "hat" ? "drum" : "analog", n === "kick" || n === "hat" ? n : "");
  const ns = [];
  for (let i = 0; i < 8; i++) ns.push({ c: "kick", n: [60, i, 0.5, 0.9 - (i % 2) * 0.2] });
  for (let i = 0; i < 4; i++) ns.push({ c: "bass", n: [36, i * 2, 2, 0.8 - i * 0.05] });
  for (let i = 0; i < 8; i++) ns.push({ c: "hat", n: [60, i + 0.5, 0.25, 0.5 + (i % 3) * 0.1] });
  for (const c of ["keys", "pad", "pluck"]) for (let i = 0; i < 4; i++) ns.push({ c: c, n: [64 + i, i * 2, 2, 0.6 + i * 0.05] });
  pattern(p, "all", 8, ns);
  place(p, "all", 0, 0, 8);
  p.channels[1].pan = 0.5;
  p.channels[3].volume = 1.3;
  p.mixer.inserts[0].volume = 1.4;
  p.mixer.inserts[0].effects.push(newDevice("limiter"));
  p.mixer.inserts[0].effects.push(newDevice("eq"));
  const r = rules(p);
  check("a panned bass is reported", r.includes("lowend-panned"));
  check("hot faders are reported", r.includes("hot-faders"));
  check("a boosted master is reported", r.includes("master-hot"));
  check("a limiter that is not last is reported", r.includes("limiter-last"));
  check("a limiter ceiling over −1 dB is reported", r.includes("limiter-ceiling"));
  check("channels straight into the master are reported", r.includes("not-routed"));
  fixesResolve("mix", p, "");

  const q = cloneProject(p);
  q.channels[1].pan = 0;
  for (let i = 0; i < 3; i++) q.mixer.inserts.push({ name: `Bus ${i + 1}`, volume: 0.8, pan: 0, mute: false, solo: false, effects: [] });
  q.channels[1].mixer = insertIx(1);
  q.channels[4].mixer = insertIx(2);
  q.channels[2].mixer = insertIx(3);
  const rev = newDevice("reverb");
  setParam(rev, "mix", 0.4);
  q.mixer.inserts[1].effects.push(rev);
  const dly = newDevice("delay");
  setParam(dly, "time", 0.7);
  q.mixer.inserts[2].effects.push(dly);
  q.mixer.inserts[2].effects.push(newDevice("compressor"));
  const comp = newDevice("compressor");
  setParam(comp, "attack", 0.5);
  q.mixer.inserts[3].effects.push(comp);
  q.mixer.inserts[3].solo = true;
  const r2 = rules(q);
  check("reverb on the bass is reported", r2.includes("reverb-on-bass"));
  check("a delay off the beat is reported", r2.includes("delay-sync"));
  check("time effects before dynamics are reported", r2.includes("fx-order"));
  check("a fast attack on drums is reported", r2.includes("drum-attack"));
  check("a forgotten solo is reported", r2.includes("solo"));
  check("pads above the bass without a low cut are reported", r2.includes("no-highpass"));
  check("an all-center mix is reported", r2.includes("all-center"));
  fixesResolve("buses", q, "");
}

// ------------------------------------------------------------------ project

{
  const p = emptyProject();
  p.transport.bpm = 127.83;
  channel(p, "c", "analog", "");
  pattern(p, "Pattern 2", 6, notesOf("c", [[60, 0, 1]]));
  const r = rules(p);
  check("a fractional tempo is reported", r.includes("tempo"));
  check("a pattern of a bar and a half is reported", r.includes("pattern-bars"));
  check("default names are reported", r.includes("names"));
  fixesResolve("project", p, "");
}

// ------------------------------------------------------------------ more checks

{
  const p = emptyProject();
  p.meta.title = "More";
  p.transport.swing = 0.3;
  channel(p, "kick", "drum", "kick");
  channel(p, "snare", "drum", "snare");
  channel(p, "bass", "wavetable", "");
  channel(p, "sub", "analog", "");
  channel(p, "keys", "analog", "");
  channel(p, "pad", "analog", "");
  channel(p, "idle", "analog", "");
  const ns = [];
  for (let i = 0; i < 16; i++) ns.push({ c: "kick", n: [60, i, 0.25, 0.8 + (i % 2) * 0.1] });
  for (let i = 0; i < 16; i++) ns.push({ c: "snare", n: [60, i * 0.5, 0.25, i < 8 ? 0.7 : 0.9] });
  for (let i = 0; i < 4; i++) ns.push({ c: "bass", n: [36, i * 4, 4, 0.8 - i * 0.05] });
  for (let i = 0; i < 4; i++) ns.push({ c: "sub", n: [31, i * 4, 4, 0.8 - i * 0.05] });
  for (let i = 0; i < 4; i++) ns.push({ c: "keys", n: [61, i * 4, 4, 0.7] });
  for (let i = 0; i < 4; i++) ns.push({ c: "pad", n: [72, i * 4, 4, 0.7] });
  pattern(p, "a", 16, ns);
  pattern(p, "empty", 4, []);
  pattern(p, "spare", 4, notesOf("keys", [[60, 0, 1]]));
  place(p, "a", 0, 0, 160);
  place(p, "empty", 1, 0, 4);
  p.channels[2].mixer = insertIx(1);
  p.channels[0].mixer = insertIx(2);
  p.channels[1].mixer = insertIx(2);
  p.channels[5].pan = -0.9;
  p.channels[4].pan = -0.6;
  p.channels[5].mute = true;
  setParam(p.channels[2].instrument, "width", 0.9);
  setParam(p.channels[3].instrument, "resonance", 0.95);
  const ins = (name) => ({ name: name, volume: 0.8, pan: 0, mute: false, solo: false, effects: [] });
  for (const n of ["Bass", "Drums", "Ghost"]) p.mixer.inserts.push(ins(n));
  const comp = newDevice("compressor");
  setParam(comp, "ratio", 12);
  setParam(comp, "threshold", -40);
  const eq = newDevice("eq");
  setParam(eq, "mid", 12);
  const off = newDevice("chorus");
  off.enabled = false;
  for (const d of [comp, eq, off, newDevice("reverb"), newDevice("reverb")]) p.mixer.inserts[2].effects.push(d);
  const fb = newDevice("delay");
  setParam(fb, "feedback", 0.9);
  setParam(fb, "time", 0.5);
  const wet = newDevice("reverb");
  setParam(wet, "mix", 0.8);
  for (const d of [fb, wet]) p.mixer.inserts[3].effects.push(d);
  const lim = newDevice("limiter");
  setParam(lim, "gain", 12);
  setParam(lim, "ceiling", -1);
  const masterVerb = newDevice("reverb");
  setParam(masterVerb, "mix", 0.3);
  for (const d of [newDevice("eq"), newDevice("compressor"), newDevice("drive"), masterVerb, lim]) p.mixer.inserts[0].effects.push(d);
  p.automation.push({
    id: "flat",
    name: "",
    target: "tempo",
    color: "#fff",
    mute: false,
    points: [
      { beat: 0, value: 120, curve: 0 },
      { beat: 8, value: 120, curve: 0 },
    ],
  });
  const r = rules(p);
  for (const id of [
    "swing-unused",
    "low-crowding",
    "kick-bass",
    "semitone-clash",
    "lowend-wide",
    "resonance",
    "unused-pattern",
    "empty-clips",
    "loopitis",
    "full-intro",
    "abrupt-ending",
    "unused-insert",
    "muted",
    "lopsided",
    "crushing-compressor",
    "eq-boost",
    "disabled",
    "double-reverb",
    "delay-feedback",
    "reverb-wet",
    "limiter-drive",
    "master-chain",
    "unused-channel",
    "flat-automation",
    "no-ghost-notes",
  ])
    check(`${id} is reported`, r.includes(id));
  fixesResolve("more", p, "");
  const q = cloneProject(p);
  q.mixer.inserts[0].effects = [];
  check("no limiter on the master is reported", rules(q).includes("no-limiter"));
  fixesResolve("limiter", q, "no-limiter");
}
{
  const p = emptyProject();
  channel(p, "hat", "drum", "hat");
  const hats = [];
  for (let i = 0; i < 16; i++) hats.push([60, i * 0.25, 0.25, i < 4 ? 0.5 + i * 0.1 : 0.8]);
  pattern(p, "run", 4, notesOf("hat", hats));
  check("a machine-gun run is reported", rules(p).includes("machine-gun"));
  fixesResolve("run", p, "machine-gun");
  const q = emptyProject();
  channel(q, "keys", "analog", "");
  pattern(
    q,
    "gaps",
    8,
    notesOf("keys", [
      [48, 0, 2],
      [52, 0, 2],
      [79, 0, 2],
      [50, 2, 2],
      [53, 2, 2],
      [81, 2, 2],
      [48, 4, 2],
      [52, 4, 2],
      [79, 4, 2],
    ])
  );
  check("gaps over an octave in the upper voices are reported", rules(q).includes("wide-spacing"));
  fixesResolve("gaps", q, "wide-spacing");
}

// ------------------------------------------------------------------ findings

{
  const p = emptyProject();
  channel(p, "hat", "drum", "hat");
  const hats = [];
  for (let i = 0; i < 16; i++) hats.push([60, i * 0.25, 0.25, 0.8]);
  pattern(p, "beat", 4, notesOf("hat", hats));
  const fs = critique(p, []);
  check(
    "finding keys are unique",
    fs.every((f, i) => fs.findIndex((g) => g.key === f.key) === i)
  );
  check(
    "findings come in category order",
    fs.every((f, i) => i === 0 || CATEGORIES.indexOf(fs[i - 1].category) <= CATEGORIES.indexOf(f.category))
  );
  const flat = fs.find((f) => f.rule === "flat-velocity");
  check("a finding points at its notes", flat !== undefined && flat.where.kind === "pattern" && flat.where.notes.length === 16 && flat.where.channel === "hat");
  check(
    "a finding's key survives an unrelated edit",
    (() => {
      const q = cloneProject(p);
      q.transport.bpm = 100;
      return critique(q, []).some((f) => flat !== undefined && f.key === flat.key);
    })()
  );
}

if (failures > 0) {
  console.log(`${failures} critic test(s) failed`);
  throw new Error("critic tests failed");
}
console.log("all critic tests passed");
