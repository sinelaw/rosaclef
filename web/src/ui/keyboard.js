// The on-screen piano: a strip of keys along the bottom of the studio that
// plays the selected channel — with the mouse, or with several fingers on a
// touch screen (slide along the keys for a glissando; lower on a key is
// louder). The computer-keyboard piano (keys.js) lights the same keys, and
// its letters are printed on them.
//
// The record button writes what is played into the selected pattern: in time
// while the pattern plays, or one step at a time (chords together) while it is
// stopped.

import { drag, loadPref, savePref, now } from "#platform";
import { state, currentChannel, currentPattern, invalidate, hint, commit } from "../store.js";
import { noteOn, noteOff, startAudio, livePosition, seek, setMode } from "../audio.js";
import { isBlackKey, noteName } from "../model.js";
import { glyph } from "./widgets.js";
import { openDock } from "./panes.js";
import { revealNote } from "./pianoroll.js";
import { toast } from "./toast.js";

/** A sounding key: who holds it (a pointer or a computer key), on which channel;
 * `take` is how it is being recorded ("" not, "live" in time, "step" step by
 * step), into which pattern, from which beat, since when (ms). */
/** type Held = { source: String, channel: String, pitch: Number, velocity: Number, take: String, pattern: String, start: Number, at: Number } */

/** A computer key of the typing piano: its `code`, the letter printed on the
 * on-screen key, semitones above the base C, and whether it is on the lower
 * (Z) row. */
/** type TypedKey = { code: String, label: String, semi: Number, low: Boolean } */

/** A black key's pitch and left edge in the strip. */
/** type BlackKey = { pitch: Number, x: Number } */

const PREF = "rosaclef.keyboard.";
/** Lowest and highest playable keys. */
const LOWEST = 12;
const HIGHEST = 127;

/** Two rows of the computer keyboard as two piano octaves, the way trackers
 * and FL Studio lay them out: Z–/ plays C–E (black keys on S D G H J L ;),
 * Q–[ plays C–F an octave up (black keys on 2 3 5 6 7 9 0). By position, so
 * it works the same on AZERTY or QWERTZ. */
/** const TYPED: TypedKey[] */
const TYPED = [
  { code: "KeyZ", label: "Z", semi: 0, low: true },
  { code: "KeyS", label: "S", semi: 1, low: true },
  { code: "KeyX", label: "X", semi: 2, low: true },
  { code: "KeyD", label: "D", semi: 3, low: true },
  { code: "KeyC", label: "C", semi: 4, low: true },
  { code: "KeyV", label: "V", semi: 5, low: true },
  { code: "KeyG", label: "G", semi: 6, low: true },
  { code: "KeyB", label: "B", semi: 7, low: true },
  { code: "KeyH", label: "H", semi: 8, low: true },
  { code: "KeyN", label: "N", semi: 9, low: true },
  { code: "KeyJ", label: "J", semi: 10, low: true },
  { code: "KeyM", label: "M", semi: 11, low: true },
  { code: "Comma", label: ",", semi: 12, low: true },
  { code: "KeyL", label: "L", semi: 13, low: true },
  { code: "Period", label: ".", semi: 14, low: true },
  { code: "Semicolon", label: ";", semi: 15, low: true },
  { code: "Slash", label: "/", semi: 16, low: true },
  { code: "KeyQ", label: "Q", semi: 12, low: false },
  { code: "Digit2", label: "2", semi: 13, low: false },
  { code: "KeyW", label: "W", semi: 14, low: false },
  { code: "Digit3", label: "3", semi: 15, low: false },
  { code: "KeyE", label: "E", semi: 16, low: false },
  { code: "KeyR", label: "R", semi: 17, low: false },
  { code: "Digit5", label: "5", semi: 18, low: false },
  { code: "KeyT", label: "T", semi: 19, low: false },
  { code: "Digit6", label: "6", semi: 20, low: false },
  { code: "KeyY", label: "Y", semi: 21, low: false },
  { code: "Digit7", label: "7", semi: 22, low: false },
  { code: "KeyU", label: "U", semi: 23, low: false },
  { code: "KeyI", label: "I", semi: 24, low: false },
  { code: "Digit9", label: "9", semi: 25, low: false },
  { code: "KeyO", label: "O", semi: 26, low: false },
  { code: "Digit0", label: "0", semi: 27, low: false },
  { code: "KeyP", label: "P", semi: 28, low: false },
  { code: "BracketLeft", label: "[", semi: 29, low: false },
];
/** The highest semitone of the typing piano (and of the lower row alone). */
const TYPED_SPAN = 29;
const LOW_SPAN = 12;
/** The highest base C that keeps the typing piano in range. */
const TYPED_TOP = 96;

export const keyboard = {
  shown: true,
  /** The first (leftmost) key: always a C. */
  low: 48,
  /** Measured width of the keys, in pixels. */
  width: 0,
  /** The C the Z key plays (Q plays the octave above). */
  typed: 60,
  /** Both rows of the computer keyboard play notes (the letter shortcuts
   * they cover are off). Off, only Z–M and "," play. */
  typing: false,
  /** Recording what is played on the keys into the piano roll. */
  armed: false,
};

/** Whether the piano helps in a dock tab: not in the mixer, nor while
 * singing into the microphone (a phone hides it there to free the room). */
/** function keysHelp(dock: String) => Boolean */
export function keysHelp(dock) {
  return dock !== "mixer" && dock !== "voice";
}

/** const held: Held[] */
const held = [];

// The geometry of the last render, for hit tests.
const geo = { whiteW: 0, blackW: 0, octaves: 1 };
/** const whites: Number[] */
const whites = [];
/** const blacks: BlackKey[] */
const blacks = [];

export function loadKeyboard() {
  if (loadPref(PREF + "shown") === "no") keyboard.shown = false;
  if (loadPref(PREF + "typing") === "yes") keyboard.typing = true;
  const low = Number(loadPref(PREF + "low"));
  if (low >= LOWEST && low <= HIGHEST - 7 && low % 12 === 0) keyboard.low = low;
  const typed = Number(loadPref(PREF + "typed"));
  if (typed >= LOWEST && typed <= TYPED_TOP && typed % 12 === 0) keyboard.typed = typed;
}

function saveKeyboard() {
  savePref(PREF + "shown", keyboard.shown ? "yes" : "no");
  savePref(PREF + "typing", keyboard.typing ? "yes" : "no");
  savePref(PREF + "low", String(keyboard.low));
  savePref(PREF + "typed", String(keyboard.typed));
}

export function toggleKeyboard() {
  keyboard.shown = !keyboard.shown;
  saveKeyboard();
  invalidate();
}

/** Scroll the strip so its first key is `low` (clamped so the strip stays full). */
/** function setLow(low: Number) => Undefined */
function setLow(low) {
  // The top octave that still fills the strip.
  const top = Math.max(LOWEST, HIGHEST + 1 - 12 * geo.octaves - ((HIGHEST + 1) % 12));
  keyboard.low = Math.max(LOWEST, Math.min(top, low));
}

/** function shiftOctave(by: Number) => Undefined */
function shiftOctave(by) {
  setLow(keyboard.low + by * 12);
  saveKeyboard();
  invalidate();
}

/** Whether both rows of the computer keyboard play (always while recording). */
export function typingOn() {
  return keyboard.typing || keyboard.armed;
}

/** The pitch a computer key plays (by `KeyboardEvent.code`), -1 for none. */
/** function typedPitch(code: String) => Number */
export function typedPitch(code) {
  const all = typingOn();
  for (const k of TYPED) {
    if (k.code === code && (all || (k.low && k.semi <= LOW_SPAN))) return keyboard.typed + k.semi;
  }
  return -1;
}

/** The computer keys that play `pitch`, as printed on the on-screen key. */
/** function typedLabel(pitch: Number) => String */
function typedLabel(pitch) {
  const all = typingOn();
  /** const out: String[] */
  const out = [];
  for (const k of TYPED) {
    if (keyboard.typed + k.semi === pitch && (all || (k.low && k.semi <= LOW_SPAN))) out.push(k.label);
  }
  return out.join(" ");
}

/** What the computer keys play, for hints. */
function typedHint() {
  const base = keyboard.typed;
  if (typingOn())
    return `Z–/ play ${noteName(base)}–${noteName(base + 16)}, Q–[ play ${noteName(base + 12)}–${noteName(base + TYPED_SPAN)} · - and = change octave`;
  return `Z–M play ${noteName(base)}–${noteName(base + LOW_SPAN)} · - and = change octave · the keyboard button adds the Q row`;
}

/** Move the computer keys an octave down (-1) or up (1); the strip follows them. */
/** function shiftTyped(by: Number) => Undefined */
export function shiftTyped(by) {
  keyboard.typed = Math.max(LOWEST, Math.min(TYPED_TOP, keyboard.typed + by * 12));
  revealTyped();
  saveKeyboard();
  hint(typedHint());
  invalidate();
}

/** Scroll the strip so it shows the keys the computer keyboard plays. */
function revealTyped() {
  const shown = 12 * geo.octaves;
  const need = Math.min(shown, typingOn() ? TYPED_SPAN + 1 : LOW_SPAN + 1);
  if (keyboard.typed < keyboard.low) setLow(keyboard.typed);
  else if (keyboard.typed + need > keyboard.low + shown) setLow(keyboard.typed + need - shown + 11 - ((keyboard.typed + need - shown + 11) % 12));
}

/** Turn the two-row typing keyboard on or off. */
export function toggleTyping() {
  keyboard.typing = !keyboard.typing;
  revealTyped();
  saveKeyboard();
  hint(typedHint());
  invalidate();
}

// ------------------------------------------------------------------ recording

/** Arm (or disarm) recording the keys into the selected pattern: it switches
 * to pattern mode and shows the piano roll. */
export function toggleRecordKeys() {
  if (keyboard.armed) {
    keyboard.armed = false;
    hint("");
    invalidate();
    return undefined;
  }
  const pat = currentPattern();
  if (!pat || !currentChannel()) {
    toast("Nothing to record into", "Select a pattern and a channel first.", "error");
    return undefined;
  }
  keyboard.armed = true;
  const switched = state.mode !== "pattern";
  if (switched) setMode("pattern");
  if (!state.playing && (switched || state.position >= pat.length)) seek(0);
  startAudio();
  openDock("piano");
  revealTyped();
  hint(`Recording notes into ${pat.name} — Space plays and records in time; stopped, each key or chord is one step (the snap) · Esc stops`);
  invalidate();
}

/** How a key pressed now is recorded: "" (not), "live" or "step". */
function takeNow() {
  if (!keyboard.armed || state.mode !== "pattern" || !currentPattern()) return "";
  return state.playing ? "live" : "step";
}

/** The length of one step: the snap, or a sixteenth without one. */
function stepLength() {
  return state.snap > 0 ? state.snap : 0.25;
}

/** function round3(x: Number) => Number */
function round3(x) {
  return Math.round(x * 1000) / 1000;
}

/** Add a recorded note; a step past the pattern's end lengthens it by whole bars. */
/** function writeNote(h: Held, length: Number) => Undefined */
function writeNote(h, length) {
  const pat = state.project.patterns.find((p) => p.id === h.pattern);
  if (!pat) return undefined;
  const bpb = state.project.transport.beatsPerBar;
  commit(() => {
    pat.notes.push({ channel: h.channel, pitch: h.pitch, start: round3(h.start), length: round3(length), velocity: h.velocity });
    if (h.start + length > pat.length) pat.length = Math.ceil((h.start + length) / bpb - 1e-6) * bpb;
  });
  if (pat.id === state.pattern) revealNote(h.start, h.pitch);
}

/** Where a key pressed now starts, for a take. */
/** function takeStart(take: String) => Number */
function takeStart(take) {
  const pat = currentPattern();
  if (take === "step") {
    // Keys pressed while another step key is down make a chord.
    const chord = held.find((x) => x.take === "step");
    return chord ? chord.start : state.position;
  }
  if (take !== "live" || !pat) return 0;
  const p = livePosition();
  // A hair before the loop comes round is meant for its first beat.
  return p > pat.length - 1 / 16 ? 0 : p;
}

/** Start a note on the selected channel for `source` (it stops the note that source held). */
/** function pressKey(source: String, pitch: Number, velocity: Number) => Undefined */
export function pressKey(source, pitch, velocity) {
  releaseKey(source);
  const ch = currentChannel();
  const pat = currentPattern();
  if (!ch || pitch < 0) return undefined;
  const take = takeNow();
  const h = {
    source: source,
    channel: ch.id,
    pitch: pitch,
    velocity: Math.round(velocity * 100) / 100,
    take: take,
    pattern: pat ? pat.id : "",
    start: takeStart(take),
    at: now(),
  };
  held.push(h);
  noteOn(ch.id, pitch, velocity);
  if (take === "step") writeNote(h, stepLength());
  invalidate();
}

/** Stop the note `source` holds, if any; a recorded one is written down (in
 * time) or moves the step on once the whole chord is up. */
/** function releaseKey(source: String) => Undefined */
export function releaseKey(source) {
  let at = -1;
  for (let i = 0; i < held.length; i++) if (held[i].source === source) at = i;
  if (at < 0) return undefined;
  const h = held[at];
  held.splice(at, 1);
  noteOff(h.channel, h.pitch);
  if (h.take === "live") {
    const pat = state.project.patterns.find((p) => p.id === h.pattern);
    const beats = ((now() - h.at) / 60000) * state.project.transport.bpm;
    const room = pat ? pat.length - h.start : beats;
    writeNote(h, Math.max(1 / 32, Math.min(room, beats)));
  } else if (h.take === "step" && !held.some((x) => x.take === "step") && !state.playing) {
    const next = h.start + stepLength();
    seek(next);
    revealNote(next, h.pitch);
  }
  invalidate();
}

/** function isDown(pitch: Number) => Boolean */
function isDown(pitch) {
  return held.some((h) => h.pitch === pitch);
}

/** The key under a point of the strip (-1 for none); black keys sit on top. */
/** function pitchAt(x: Number, y: Number, height: Number) => Number */
function pitchAt(x, y, height) {
  if (geo.whiteW <= 0 || y < 0 || y > height) return -1;
  if (y < height * 0.6) {
    const b = blacks.find((k) => x >= k.x && x < k.x + geo.blackW);
    if (b) return b.pitch;
  }
  const i = Math.floor(x / geo.whiteW);
  return i >= 0 && i < whites.length ? whites[i] : -1;
}

/** function velocityAt(y: Number, height: Number) => Number */
function velocityAt(y, height) {
  const t = height > 0 ? Math.max(0, Math.min(1, y / height)) : 0.7;
  return Math.round((0.45 + 0.55 * t) * 100) / 100;
}

/** function whiteKeysFrom(low: Number) => Number */
function whiteKeysFrom(low) {
  let n = 0;
  for (let p = low; p <= HIGHEST; p++) if (!isBlackKey(p)) n += 1;
  return n;
}

/** Lay the keys out for the measured width: as many white keys as fit from `low`, stretched to fill. */
/** function layoutKeys(keyW: Number) => Undefined */
function layoutKeys(keyW) {
  whites.length = 0;
  blacks.length = 0;
  const fit = Math.max(7, Math.floor(keyboard.width / keyW));
  geo.octaves = Math.max(1, Math.floor(fit / 7));
  const n = Math.min(fit, whiteKeysFrom(keyboard.low));
  const ww = keyboard.width > 0 ? keyboard.width / n : keyW;
  geo.whiteW = ww;
  geo.blackW = ww * 0.62;
  let p = keyboard.low;
  while (whites.length < n && p <= HIGHEST) {
    if (isBlackKey(p)) blacks.push({ pitch: p, x: whites.length * ww - geo.blackW / 2 });
    else whites.push(p);
    p += 1;
  }
}

/** The strip; `compact` is the phone layout (bigger keys, no channel name). */
/** function keyboardStrip(b: Builder, compact: Boolean) => Undefined */
export function keyboardStrip(b, compact) {
  const ch = currentChannel();
  layoutKeys(compact ? 36 : 28);

  b.open("div", "keyboard", compact ? "keyboard compact" : "keyboard");
  b.open("div", "head", "kb-head");
  if (!compact) {
    b.open("div", "ch", "kb-ch");
    b.leaf("span", "l", "kb-label", "Keys");
    b.leaf("b", "n", "", ch ? ch.name : "No channel");
    if (ch) b.style("--c", ch.color);
    b.close();
  }
  b.open("div", "row", "kb-row");
  b.open("div", "oct", "kb-oct");
  octaveButton(b, "down", "left", -1);
  b.leaf("span", "o", "kb-octave", noteName(keyboard.low));
  octaveButton(b, "up", "right", 1);
  b.close();
  b.open("div", "tools", "kb-tools");
  if (!compact) {
    const typeTip = keyboard.typing
      ? "Typing keyboard on: Z–/ and Q–[ play about 2½ octaves (Q, E, P, R and L play notes instead of their shortcuts) — click to turn off"
      : "Typing keyboard: play two rows of the computer keyboard, Z–/ and Q–[ (about 2½ octaves)";
    toolButton(b, "type", keyboard.typing ? "kb-shift kb-type on" : "kb-shift kb-type", "keys", typeTip, () => toggleTyping());
  }
  const recTip = keyboard.armed
    ? "Stop recording notes (Esc)"
    : "Record notes from the keys into the piano roll — in time while the pattern plays (Space), one step at a time while it is stopped";
  toolButton(b, "rec", keyboard.armed ? "kb-shift kb-rec armed" : "kb-shift kb-rec", "record", recTip, () => toggleRecordKeys());
  b.close();
  b.close();
  b.close();

  b.open("div", "keys", "kb-keys");
  b.on("resize", (e) => {
    if (Math.abs(e.targetWidth - keyboard.width) >= 1) {
      keyboard.width = e.targetWidth;
      invalidate();
    }
  });
  b.on("pointerenter", (e) => hint(ch ? `Play ${ch.name} — slide for a glissando; lower on a key is louder · ${typedHint()}` : "Select a channel to play it"));
  b.on("contextmenu", (e) => {
    e.preventDefault();
  });
  b.on("pointerdown", (e) => {
    e.preventDefault();
    if (e.button !== 0) return undefined;
    startAudio();
    const source = `p${e.pointerId}`;
    const left = e.targetLeft;
    const top = e.targetTop;
    const h = e.targetHeight;
    let pitch = pitchAt(e.clientX - left, e.clientY - top, h);
    pressKey(source, pitch, velocityAt(e.clientY - top, h));
    drag(
      e,
      (m) => {
        const p = pitchAt(m.clientX - left, m.clientY - top, h);
        if (p === pitch) return undefined;
        pitch = p;
        if (p < 0) releaseKey(source);
        else pressKey(source, p, velocityAt(m.clientY - top, h));
      },
      (u) => releaseKey(source)
    );
  });
  // The computer keys' letters, where there is room (not on a phone).
  const letters = !compact && geo.whiteW >= 14;
  for (let i = 0; i < whites.length; i++) {
    const p = whites[i];
    b.open("div", `w${p}`, isDown(p) ? "kb-white down" : "kb-white");
    b.style("left", `${i * geo.whiteW}px`);
    b.style("width", `${geo.whiteW}px`);
    const typed = letters ? typedLabel(p) : "";
    if (typed !== "") b.leaf("i", "t", "kb-typed", typed);
    if (p % 12 === 0) b.leaf("span", "l", "", noteName(p));
    b.close();
  }
  for (const k of blacks) {
    b.open("div", `b${k.pitch}`, isDown(k.pitch) ? "kb-black down" : "kb-black");
    b.style("left", `${k.x}px`);
    b.style("width", `${geo.blackW}px`);
    const typed = letters ? typedLabel(k.pitch) : "";
    if (typed !== "") b.leaf("i", "t", "kb-typed", typed);
    b.close();
  }
  b.close();
  b.close();
}

/** function octaveButton(b: Builder, key: String, icon: String, by: Number) => Undefined */
function octaveButton(b, key, icon, by) {
  const tip = by < 0 ? "Octave down" : "Octave up";
  b.open("button", key, "kb-shift");
  b.attr("title", tip);
  b.attr("aria-label", tip);
  b.on("click", (e) => shiftOctave(by));
  glyph(b, icon);
  b.close();
}

/** function toolButton(b: Builder, key: String, cls: String, icon: String, tip: String, act: () => Undefined) => Undefined */
function toolButton(b, key, cls, icon, tip, act) {
  b.open("button", key, cls);
  b.attr("title", tip);
  b.attr("aria-label", tip);
  b.on("click", (e) => act());
  glyph(b, icon);
  b.close();
}
