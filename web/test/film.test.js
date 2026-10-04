// Tests for the film of the song (film.js), run with:
// node web/test/film.test.js (also type-checked by inty via web/check.sh).
import { emptyProject, noArp, decodeAnimation, decodeShot, encodeShot, encodeProject, decodeProject } from "../src/model.js";
import { buildScore, TPQ } from "../src/notation.js";
import {
  findRoles,
  stavesFor,
  autoScenes,
  plan,
  makeFilm,
  cameraAt,
  sceneAt,
  region,
  sparks,
  bake,
  layDesk,
  onDesk,
  ease,
  fxOf,
  shotScene,
} from "../src/film.js";
import { insertIx, trackIx } from "#brands";

let failures = 0;
/** function check(name: String, ok: Boolean) => Undefined */
function check(name, ok) {
  if (!ok) failures = failures + 1;
  console.log(`${ok ? "ok  " : "FAIL"} ${name}`);
}

/** A channel of a project. */
/** function channel(id: String, name: String, type: String) => Channel */
function channel(id, name, type) {
  return {
    id: id,
    name: name,
    color: "#d4af37",
    instrument: { type: type, enabled: true, params: [], options: type === "drum" ? [{ key: "kind", value: "kick" }] : [] },
    volume: 0.8,
    pan: 0,
    mute: false,
    mixer: insertIx(0),
    arp: noArp(),
    layerOf: "",
  };
}

/**
 * Sixteen bars of 4/4: the drums alone for four bars, the bass comes in at
 * bar 5, the pad (chords) and the lead (a busy high line) at bar 9.
 */
/** function band() => Project */
function band() {
  const p = emptyProject();
  p.channels.push(channel("drums", "Drums", "drum"));
  p.channels.push(channel("bass", "Bass", "prisme"));
  p.channels.push(channel("pad", "Pad", "prisme"));
  p.channels.push(channel("lead", "Lead", "prisme"));
  /** const notes: Note[] */
  const notes = [];
  for (let b = 0; b < 64; b++) notes.push({ channel: "drums", pitch: 36, start: b, length: 0.25, velocity: 0.9 });
  for (let b = 16; b < 64; b = b + 0.5) notes.push({ channel: "bass", pitch: 36 + ((b * 2) % 5), start: b, length: 0.5, velocity: 0.8 });
  for (let b = 32; b < 64; b = b + 4) for (const k of [60, 64, 67]) notes.push({ channel: "pad", pitch: k, start: b, length: 4, velocity: 0.6 });
  for (let b = 32; b < 64; b = b + 0.5) notes.push({ channel: "lead", pitch: 72 + ((b * 4) % 7), start: b, length: 0.5, velocity: 0.8 });
  p.patterns.push({ id: "song", name: "Song", color: "#d4af37", length: 64, notes: notes });
  p.playlist.tracks.push({ name: "Track 1", mute: false });
  p.playlist.clips.push({ pattern: "song", sample: "", track: trackIx(0), start: 0, length: 64, offset: 0, gain: 1, mixer: insertIx(0) });
  return p;
}

const SONG = { kind: "song", track: 0, pattern: "" };

/** A film as written in project.json. */
/** function filmOf(json: String) => Animation */
function filmOf(json) {
  return decodeAnimation(JSON.parse(json));
}
const p = band();
const sc = buildScore(p, SONG, 12);

// ------------------------------------------------------------------ roles

const roles = findRoles(sc);
/** function staffOf(id: String) => Int */
function staffOf(id) {
  return sc.staves.findIndex((s) => s.channels.includes(id));
}
check("the drums keep the rhythm", roles.roles[staffOf("drums")] === "rhythm");
check("so does the low single line (the bass)", roles.roles[staffOf("bass")] === "rhythm");
check("the chords are the background", roles.roles[staffOf("pad")] === "background");
check("the busy high line is the lead", roles.roles[staffOf("lead")] === "lead" && roles.lead === staffOf("lead"));
check("a role names its staves", stavesFor(sc, roles, [], "rhythm").length === 2 && stavesFor(sc, roles, [], "lead")[0] === staffOf("lead"));
check("channels name theirs", stavesFor(sc, roles, ["pad"], "")[0] === staffOf("pad") && stavesFor(sc, roles, [], "").length === 0);

// ------------------------------------------------------------------ the director

const auto = filmOf(`{ "mode": "auto", "energy": 0.5 }`);
const scenes = autoScenes(sc, roles, auto, 1, 4);
check(
  "the director's scenes cover the song, in order and without gaps",
  scenes.length > 3 &&
    scenes[0].start === 0 &&
    Math.abs(scenes[scenes.length - 1].end - 64) < 1e-9 &&
    scenes.every((k, i) => i === 0 || Math.abs(scenes[i - 1].end - k.start) < 1e-9)
);
check("it opens wide and swoops down", scenes[0].frame === "page" && scenes[0].why === "the opening" && scenes[1].transition === "swoop");
check("it closes on the page", scenes[scenes.length - 1].frame === "page" && scenes[scenes.length - 1].why === "the close");
/** function takeOf(beat: Number) => Scene */
function takeOf(beat) {
  return scenes[sceneAt(scenes, beat)];
}
check("the drums playing alone are framed alone", takeOf(8).staves.length === 1 && takeOf(8).staves[0] === staffOf("drums") && takeOf(8).why === "Drums alone");
check(
  "then the bass, as it comes in",
  takeOf(17).staves.includes(staffOf("bass")) && !takeOf(17).staves.includes(staffOf("drums")) && takeOf(17).why === "Bass comes in"
);
check(
  "then the lead, as it comes in with the pad (up close, alone in the frame), for two bars",
  takeOf(33).why === "Lead comes in" && takeOf(33).staves.join() === String(staffOf("lead")) && takeOf(33).frame === "close" && takeOf(33).end === 40
);
check(
  "between them, and after, the full score: every staff",
  takeOf(24).why === "the full score" && takeOf(24).staves.length === 0 && takeOf(24).frame === "system"
);
const later = scenes.filter((k) => k.start >= 36 && k.why !== "the close");
check("with the whole band playing it shows the full score", later.length > 0 && later.every((k) => k.why === "the full score"));
/** Beats of a film showing the full score (or the desk or a page). */
/** function wideBeats(list: Scene[]) => Number */
function wideBeats(list) {
  let n = 0;
  for (const k of list) if (k.staves.length === 0) n = n + (k.end - k.start);
  return n;
}
check(
  "close shots lean and turn (diagonals), and push in as they go",
  takeOf(17).tilt > 30 && Math.abs(takeOf(17).turn) > 1 && takeOf(17).zoom1 > takeOf(17).zoom
);
check("the director leaves the paper evenly lit (no spotlight unless the film sets one)", takeOf(8).fx.spotlight === 0);
const calm = autoScenes(sc, roles, filmOf(`{ "energy": 0 }`), 1, 4);
const busy = autoScenes(sc, roles, filmOf(`{ "energy": 1 }`), 1, 4);
/** The longest the full score is shown at a stretch. */
/** function longest(list: Scene[]) => Number */
function longest(list) {
  let n = 0;
  for (const k of list) if (k.why === "the full score") n = Math.max(n, k.end - k.start);
  return n;
}
check("energy: a calm film holds the full score longer, a busy one cuts it up more", longest(calm) >= longest(scenes) && longest(scenes) > longest(busy));
check(
  "at any energy, the full score is most of the film once the band plays",
  [calm, scenes, busy].every((list) => wideBeats(list.filter((k) => k.start >= 16)) >= 0.5 * 48)
);

// A lead handing over: the flute leads for eight bars, then the violin takes
// the tune while the flute holds long notes (both play throughout).
/** function duet() => Project */
function duet() {
  const p = emptyProject();
  p.channels.push(channel("piano", "Piano", "prisme"));
  p.channels.push(channel("flute", "Flute", "prisme"));
  p.channels.push(channel("violin", "Violin", "prisme"));
  /** const notes: Note[] */
  const notes = [];
  for (let b = 0; b < 64; b = b + 4) for (const k of [48, 55, 60, 64]) notes.push({ channel: "piano", pitch: k, start: b, length: 4, velocity: 0.5 });
  for (let b = 0; b < 32; b = b + 0.5) notes.push({ channel: "flute", pitch: 76 + ((b * 4) % 5), start: b, length: 0.5, velocity: 0.8 });
  for (let b = 32; b < 64; b = b + 4) notes.push({ channel: "flute", pitch: 72, start: b, length: 4, velocity: 0.4 });
  for (let b = 0; b < 32; b = b + 4) notes.push({ channel: "violin", pitch: 67, start: b, length: 4, velocity: 0.4 });
  for (let b = 32; b < 64; b = b + 0.5) notes.push({ channel: "violin", pitch: 74 + ((b * 4) % 6), start: b, length: 0.5, velocity: 0.85 });
  p.patterns.push({ id: "song", name: "Song", color: "#d4af37", length: 64, notes: notes });
  p.playlist.tracks.push({ name: "Track 1", mute: false });
  p.playlist.clips.push({ pattern: "song", sample: "", track: trackIx(0), start: 0, length: 64, offset: 0, gain: 1, mixer: insertIx(0) });
  return p;
}
const dsc = buildScore(duet(), SONG, 12);
const droles = findRoles(dsc);
const dscenes = autoScenes(dsc, droles, auto, 1, 4);
const violin = dsc.staves.findIndex((st) => st.channels.includes("violin"));
const takeover = dscenes.find((k) => k.why === "Violin takes the lead");
check(
  "a new lead taking the tune is followed for a couple of bars, then back to the full score",
  takeover !== undefined &&
    takeover.start === 32 &&
    takeover.end === 40 &&
    takeover.staves.join() === String(violin) &&
    takeover.frame === "medium" &&
    dscenes[sceneAt(dscenes, 41)].why === "the full score"
);

// ------------------------------------------------------------------ manual shots

const manual = filmOf(`{
  "mode": "manual",
  "effects": [{ "type": "glow", "amount": 0.9 }],
  "shots": [
    { "start": 20, "end": 28, "focus": ["lead"], "frame": "detail", "tilt": 40, "turn": -25, "to": { "zoom": 1.5 }, "transition": "cut", "label": "Solo",
      "effects": [{ "type": "spotlight", "amount": 0.7 }] },
    { "start": 44, "end": 52, "role": "background", "frame": "medium", "at": 46, "glide": 2, "ease": "out" }
  ]
}`);
const mt = plan(manual, sc, roles, 1, 4);
const solo = mt[sceneAt(mt, 22)];
check(
  "a shot is filmed as written",
  solo.src === 0 && solo.why === "Solo" && solo.frame === "detail" && solo.tilt === 40 && solo.turn === -25 && solo.zoom1 === 1.5 && solo.glide === 0
);
check("its effects override the film's, which override the defaults", solo.fx.spotlight === 0.7 && solo.fx.glow === 0.9 && solo.fx.vignette === 0.5);
check("a role is resolved to its staves", mt[sceneAt(mt, 45)].staves.join() === stavesFor(sc, roles, [], "background").join());
check(
  "the director fills the time around the shots",
  mt[sceneAt(mt, 10)].src === -1 && mt[sceneAt(mt, 30)].src === -1 && mt.every((k, i) => i === 0 || Math.abs(mt[i - 1].end - k.start) < 1e-9)
);
check("coming back after a shot, the director glides in", mt[sceneAt(mt, 28)].start === 28 && mt[sceneAt(mt, 28)].transition === "glide");
check(
  "auto mode ignores the shots",
  plan(filmOf(`{ "mode": "auto", "shots": [{ "start": 20, "end": 28, "frame": "detail" }] }`), sc, roles, 1, 4).every((k) => k.src === -1)
);
const later2 = filmOf(
  `{ "mode": "manual", "shots": [{ "start": 0, "end": 32, "frame": "page" }, { "start": 8, "end": 16, "frame": "detail", "focus": ["drums"] }] }`
);
const lt = plan(later2, sc, roles, 1, 4);
check("where shots overlap, the later one is filmed", lt[sceneAt(lt, 10)].src === 1 && lt[sceneAt(lt, 4)].src === 0 && lt[sceneAt(lt, 20)].src === 0);

// The shots round-trip through the project file.
const raw = `{"start":4,"end":8,"focus":["lead"],"frame":"close","tilt":30,"turn":-10,"offset":[0.1,-0.2],"at":6,"to":{"zoom":1.3},"transition":"whip","glide":0.5,"ease":"snap","effects":[{"type":"focus","amount":0.2}]}`;
check("a shot survives decoding and encoding", JSON.stringify(encodeShot(decodeShot(JSON.parse(raw)))) === raw);
const withFilm = band();
withFilm.animation = manual;
const again = decodeProject(JSON.parse(JSON.stringify(encodeProject(withFilm))));
check(
  "so does the film",
  again.animation.on && again.animation.mode === "manual" && again.animation.shots.length === 2 && again.animation.shots[0].to.zoom === 1.5
);
check("a project without a film writes none", encodeProject(band()).animation === undefined);

// ------------------------------------------------------------------ the camera

const film = makeFilm(manual, sc, "a4", false, 16 / 9, 4);
check("the pages lie on the desk", film.desk.pages.length === film.lay.pages.length && film.desk.x1 > film.desk.x0);
const desk3 = layDesk(3, 595, 842);
const c = onDesk(desk3, 1, 595 / 2, 842 / 2);
check(
  "a page's middle is its place on the desk; three pages lie in a row",
  Math.abs(c[0] - desk3.pages[1].x) < 1e-9 && Math.abs(desk3.pages[2].y - desk3.pages[0].y) < 842 * 0.1
);
check(
  "ease curves run from 0 to 1",
  ["smooth", "linear", "in", "out", "snap"].every((k) => Math.abs(ease(k, 0)) < 1e-9 && Math.abs(ease(k, 1) - 1) < 1e-9)
);
check("unset effects keep their defaults", fxOf([]).vignette === 0.5 && fxOf([[{ type: "vignette", amount: 0 }]]).vignette === 0);

// The camera moves smoothly: no jumps between frames (cuts aside).
let jumps = 0;
let prev = cameraAt(film, 0);
for (let t = 0.02; t < 64; t = t + 0.02) {
  const cam = cameraAt(film, t);
  const k = film.scenes[sceneAt(film.scenes, t)];
  const cut = k.transition === "cut" && t - k.start < 0.021;
  const step = Math.hypot(cam.x - prev.x, cam.y - prev.y) / Math.min(cam.span, prev.span);
  if (!cut && (step > 0.08 || Math.abs(Math.log(cam.span / prev.span)) > 0.08)) jumps = jumps + 1;
  prev = cam;
}
check("the camera glides from frame to frame (moving on to the next system too)", jumps === 0);
const wideCam = cameraAt(film, 0.1);
const close = cameraAt(film, 23);
check("a detail shot sees far less than the opening", close.span < wideCam.span / 3);
check("it leans and turns as written", Math.abs(close.tilt - 40) < 1e-6 && Math.abs(close.turn + 25) < 1e-6);
// A shot looking at a fixed beat stays put.
const fixed1 = cameraAt(film, 48);
const fixed2 = cameraAt(film, 50);
check("a shot `at` a beat does not follow the playhead", Math.abs(fixed1.x - fixed2.x) < 1e-6);
// Notes glow as they play: the framed staves' brightly, the others dimmed.
const lead = [staffOf("lead")];
const lit = sparks(film, 40.01, lead);
const leadY = lit.filter((s) => s.a > 0.5);
check("notes playing glow", lit.length >= 3 && leadY.length > 0 && lit.some((s) => s.a < 0.5));
// They are where the camera looks when it follows the lead.
const k40 = shotScene(sc, roles, manual, decodeShot(JSON.parse(`{ "start": 40, "end": 44, "focus": ["lead"] }`)), 0, 4);
const r40 = region(film, k40, 40.01);
const at40 = leadY.map((s) => onDesk(film.desk, s.page, s.x, s.y));
check(
  "a following shot frames the music being played",
  at40.some((q) => Math.abs(q[0] - r40[0]) < r40[2] / 2 && Math.abs(q[1] - r40[1]) < r40[3] / 2)
);

// Baking the director's scenes gives shots that film the same.
const baked = bake(sc, scenes);
const bakedFilm = filmOf(`{ "mode": "manual" }`);
bakedFilm.shots = baked;
const bt = plan(bakedFilm, sc, roles, 1, 4);
check("the director's scenes bake into shots", baked.length === scenes.length && baked[0].label === "the opening");
check(
  "which film as the director did",
  bt.length === scenes.length &&
    bt.every(
      (k, i) =>
        Math.abs(k.start - scenes[i].start) < 0.01 &&
        k.frame === scenes[i].frame &&
        k.staves.join() === scenes[i].staves.join() &&
        Math.abs(k.tilt - scenes[i].tilt) < 0.01
    )
);

if (failures > 0) {
  console.log(`${failures} film test(s) failed`);
  throw new Error("film tests failed");
}
console.log("all film tests passed");
