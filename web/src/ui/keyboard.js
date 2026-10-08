// The on-screen piano: a strip of keys along the bottom of the studio that
// plays the selected channel, or an instrument tried out from the browser
// (instruments.js; its header always names which) — with the mouse, or with several fingers on a
// touch screen (slide along the keys for a glissando; lower on a key is
// louder). The computer-keyboard piano (keys.js) lights the same keys, and
// its letters are printed on them; the letter shortcuts take Shift, so every
// letter row is free to play.
//
// The record button counts in a bar of metronome clicks, then plays the
// selected pattern and writes what is played into it in real time: each key where it went down and for as long as it was held
// (keys together make a chord, the gaps between them rests), both ends
// snapped to the piano roll's grid. The pattern does not loop: it grows a bar
// at a time for as long as it records, and ends after the last note played.

import { drag, loadPref, savePref, now } from "#platform";
import { state, AUDITION, currentChannel, currentPattern, invalidate, hint, commit, changed } from "../store.js";
import { noteOn, noteOff, preview, startAudio, livePosition, seek, setMode, playCountIn, stop, setOpenEnded } from "../audio.js";
import { isBlackKey, noteName, snapTo, optionValue } from "../model.js";
import { glyph } from "./widgets.js";
import { openDock } from "./panes.js";
import { revealNote } from "./pianoroll.js";
import { toast } from "./toast.js";
import { keysTarget, keepTried, stopTrying, onTried } from "./instruments.js";
import { t, tf } from "../i18n.js";

/** A sounding key: who holds it (a pointer or a computer key), on which channel;
 * whether it is being recorded (`take`), into which pattern, from which beat
 * (`start`, on the grid; `beat`, where it really went down), since when (ms). */
/** type Held = { source: String, channel: String, pitch: Number, velocity: Number, take: Boolean, pattern: String, start: Number, beat: Number, at: Number } */

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
/** Keys struck within this long of each other (ms) are one chord when recording. */
const CHORD_MS = 60;
/** The highest semitone of the typing piano. */
const TYPED_SPAN = 29;
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
  /** Recording what is played on the keys into the piano roll. */
  armed: false,
};

/** Whether the piano helps in a dock tab: not in the mixer, nor while
 * singing into the microphone (a phone hides it there to free the room). */
/** const held: Held[] */
const held = [];

/** The take being recorded: into which pattern, how long it was before, and
 * whether it has grown yet (the first bar added is the take's undo step);
 * `run` tells a stale growth timer from the current one. */
const take = { pattern: "", before: 0, grew: false, run: 0 };

// The geometry of the last render, for hit tests.
const geo = { whiteW: 0, blackW: 0, octaves: 1 };
/** const whites: Number[] */
const whites = [];
/** const blacks: BlackKey[] */
const blacks = [];

export function loadKeyboard() {
  if (loadPref(PREF + "shown") === "no") keyboard.shown = false;
  const low = Number(loadPref(PREF + "low"));
  if (low >= LOWEST && low <= HIGHEST - 7 && low % 12 === 0) keyboard.low = low;
  const typed = Number(loadPref(PREF + "typed"));
  if (typed >= LOWEST && typed <= TYPED_TOP && typed % 12 === 0) keyboard.typed = typed;
}

function saveKeyboard() {
  savePref(PREF + "shown", keyboard.shown ? "yes" : "no");
  savePref(PREF + "low", String(keyboard.low));
  savePref(PREF + "typed", String(keyboard.typed));
}

/** The pitch that shows an instrument off in one short note. */
/** function showPitch(d: Device) => Number */
function showPitch(d) {
  if (d.type === "soundfont") {
    const coll = state.catalog.collections.find((c) => c.instrument === "soundfont");
    const pr = coll ? coll.presets.find((x) => x.name === optionValue(d, "program")) : undefined;
    // A drum kit: the snare (GM drum map).
    if (pr && pr.bank === 128) return 38;
  }
  return keyboard.typed;
}

/** An instrument was picked from the browser to try: show the strip
 * (without changing the saved choice) and let it sound one short note. */
/** function triedInstrument(d: Device) => Undefined */
function triedInstrument(d) {
  keyboard.shown = true;
  const key = state.audition.key;
  const pitch = showPitch(d);
  // The engine gets the instrument with the next project update (~30 ms).
  setTimeout(() => {
    if (state.audition.on && state.audition.key === key) preview(AUDITION, pitch, 0.8);
  }, 90);
}
onTried(triedInstrument);

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

/** The pitch a computer key plays (by `KeyboardEvent.code`), -1 for none. */
/** function typedPitch(code: String) => Number */
export function typedPitch(code) {
  for (const k of TYPED) {
    if (k.code === code) return keyboard.typed + k.semi;
  }
  return -1;
}

/** The computer keys that play `pitch`, as printed on the on-screen key. */
/** function typedLabel(pitch: Number) => String */
function typedLabel(pitch) {
  /** const out: String[] */
  const out = [];
  for (const k of TYPED) {
    if (keyboard.typed + k.semi === pitch) out.push(k.label);
  }
  return out.join(" ");
}

/** What the computer keys play, for hints. */
function typedHint() {
  const base = keyboard.typed;
  return tf("keyboard.typed.hint", [noteName(base), noteName(base + 16), noteName(base + 12), noteName(base + TYPED_SPAN)]);
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
  const need = Math.min(shown, TYPED_SPAN + 1);
  if (keyboard.typed < keyboard.low) setLow(keyboard.typed);
  else if (keyboard.typed + need > keyboard.low + shown) setLow(keyboard.typed + need - shown + 11 - ((keyboard.typed + need - shown + 11) % 12));
}

// ------------------------------------------------------------------ recording

/** Arm recording the keys into the selected pattern: it switches to pattern
 * mode, shows the piano roll and plays the pattern from its start after a bar
 * of count-in, on past its end. Disarming stops it. */
export function toggleRecordKeys() {
  if (keyboard.armed) {
    keyboard.armed = false;
    take.run += 1;
    setOpenEnded(false);
    stop();
    trimTake();
    hint("");
    invalidate();
    return undefined;
  }
  const pat = currentPattern();
  // An instrument being tried becomes a channel to record onto.
  if (pat && state.audition.on) keepTried();
  if (!pat || !currentChannel()) {
    toast(t("keyboard.record.nothing.title"), t("keyboard.record.nothing.body"), "error");
    return undefined;
  }
  keyboard.armed = true;
  take.pattern = pat.id;
  take.before = pat.length;
  take.grew = false;
  take.run += 1;
  if (state.mode !== "pattern") setMode("pattern");
  setOpenEnded(true);
  if (!state.playing) {
    seek(0);
    playCountIn(state.project.transport.beatsPerBar);
  }
  growTake(take.run);
  openDock("piano");
  revealTyped();
  hint(tf("keyboard.record.started.hint", [pat.name, gridName()]));
  invalidate();
}

/** The pattern being recorded into, while it is the one playing. */
/** function takePattern() => Pattern? */
function takePattern() {
  if (state.mode !== "pattern" || state.pattern !== take.pattern) return undefined;
  return currentPattern();
}

/** Keep the pattern a bar ahead of the playhead while recording (a timer for
 * as long as take `run` lasts). */
/** function growTake(run: Number) => Undefined */
function growTake(run) {
  if (run !== take.run || !keyboard.armed) return undefined;
  const pat = takePattern();
  if (pat && state.playing) {
    const bpb = state.project.transport.beatsPerBar;
    const want = Math.ceil((livePosition() + 1) / bpb) * bpb;
    if (want > pat.length) {
      if (take.grew) {
        pat.length = want;
        changed();
      } else {
        commit(() => {
          pat.length = want;
        });
        take.grew = true;
      }
    }
  }
  setTimeout(() => growTake(run), 100);
}

/** After a take: the pattern ends with the bar of its last note (never shorter than before). */
function trimTake() {
  const pat = state.project.patterns.find((p) => p.id === take.pattern);
  if (!pat || !take.grew) return undefined;
  const bpb = state.project.transport.beatsPerBar;
  let end = 0;
  for (const n of pat.notes) end = Math.max(end, n.start + n.length);
  const len = Math.max(take.before, Math.ceil(end / bpb - 1e-6) * bpb);
  if (len < pat.length) {
    pat.length = len;
    changed();
  }
}

/** Whether a key pressed at `beat` is recorded: while armed and the pattern
 * plays (in the count-in, only just before the first beat). */
/** function takeAt(beat: Number) => Boolean */
function takeAt(beat) {
  if (!keyboard.armed || !state.playing || state.mode !== "pattern" || !currentPattern()) return false;
  return beat > -Math.max(0.25, grid() / 2);
}

/** The recording grid: the piano roll's snap (0: off, notes land where played). */
function grid() {
  return state.snap;
}

/** function gridName() => String */
function gridName() {
  const g = grid();
  if (g <= 0) return t("keyboard.record.grid.off");
  if (g >= 4) return t("keyboard.record.grid.bar");
  if (g >= 1) return t("keyboard.record.grid.beat");
  return `1/${Math.round(4 / g)}`;
}

/** function round3(x: Number) => Number */
function round3(x) {
  return Math.round(x * 1000) / 1000;
}

/** Add a recorded note; one past the pattern's end lengthens it by whole bars. */
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

/** Where a key pressed at `beat` starts: on the grid (a key in the count-in, on the first beat). */
/** function takeStart(beat: Number) => Number */
function takeStart(beat) {
  const g = grid();
  return Math.max(0, g > 0 ? snapTo(beat, g) : beat);
}

/** Start a note on the selected channel for `source` (it stops the note that source held). */
/** function pressKey(source: String, pitch: Number, velocity: Number) => Undefined */
export function pressKey(source, pitch, velocity) {
  releaseKey(source);
  const ch = keysTarget();
  const pat = currentPattern();
  if (!ch || pitch < 0) return undefined;
  const beat = livePosition();
  // Notes of an instrument only being tried are never written down.
  const taken = !ch.trying && takeAt(beat);
  // A key struck with one still held starts with it, even if the grid line
  // between their two starts fell in that instant.
  const at = now();
  const chord = held.find((x) => x.take && pat !== undefined && x.pattern === pat.id && at - x.at < CHORD_MS);
  held.push({
    source: source,
    channel: ch.id,
    pitch: pitch,
    velocity: Math.round(velocity * 100) / 100,
    take: taken,
    pattern: pat ? pat.id : "",
    start: taken ? (chord ? chord.start : takeStart(beat)) : 0,
    beat: chord ? chord.beat : beat,
    at: chord ? chord.at : at,
  });
  noteOn(ch.id, pitch, velocity);
  invalidate();
}

/** Stop the note `source` holds, if any; a recorded one is written down, held
 * for as long as the key was, its end on the grid. */
/** function releaseKey(source: String) => Undefined */
export function releaseKey(source) {
  let at = -1;
  for (let i = 0; i < held.length; i++) if (held[i].source === source) at = i;
  if (at < 0) return undefined;
  const h = held[at];
  held.splice(at, 1);
  noteOff(h.channel, h.pitch);
  if (h.take) recordNote(h);
  invalidate();
}

/** Write down a recorded key that has just come up. */
/** function recordNote(h: Held) => Undefined */
function recordNote(h) {
  const pat = state.project.patterns.find((p) => p.id === h.pattern);
  if (!pat) return undefined;
  const bpm = state.project.transport.bpm;
  const beats = ((now() - h.at) / 60000) * bpm;
  const g = grid();
  // The end snaps too (a whole step at least), so held lengths and the rests
  // between keys land on the grid.
  const length = g > 0 ? Math.max(g, snapTo(h.beat + beats, g) - h.start) : Math.max(1 / 32, beats);
  // A short key let go just before the grid line it snapped forward to: wait
  // for the playhead to pass that line, or the pattern would play it again.
  const ahead = h.start - livePosition();
  if (state.playing && ahead > 0) {
    setTimeout(() => writeNote(h, length), (ahead / bpm) * 60000 + 60);
  } else writeNote(h, length);
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
  const ch = keysTarget();
  layoutKeys(compact ? 36 : 28);

  b.open("div", "keyboard", compact ? "keyboard compact" : "keyboard");
  b.open("div", "head", "kb-head");
  if (!compact) targetView(b, ch);
  b.open("div", "row", "kb-row");
  b.open("div", "oct", "kb-oct");
  octaveButton(b, "down", "left", -1);
  b.leaf("span", "o", "kb-octave", noteName(keyboard.low));
  octaveButton(b, "up", "right", 1);
  b.close();
  const recTip = keyboard.armed ? t("keyboard.record.stop.title") : t("keyboard.record.start.title");
  toolButton(b, "rec", keyboard.armed ? "kb-shift kb-rec armed" : "kb-shift kb-rec", "record", recTip, () => toggleRecordKeys());
  b.close();
  b.close();

  b.open("div", "keys", "kb-keys");
  b.on("resize", (e) => {
    if (Math.abs(e.targetWidth - keyboard.width) >= 1) {
      keyboard.width = e.targetWidth;
      invalidate();
    }
  });
  b.on("pointerenter", (e) => hint(ch ? tf("keyboard.keys.hint", [ch.name, ch.detail, typedHint()]) : t("keyboard.keys.noChannel.hint")));
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
  // On a phone, the name of what the keys play sits on them.
  if (compact && ch) {
    b.leaf("div", `tag-${ch.id}`, ch.trying ? "kb-tag trying" : "kb-tag", ch.trying ? tf("keyboard.keys.tryingTag.label", [ch.name]) : ch.name);
    b.style("--c", ch.color);
  }
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

/** The header's name of what the keys play: a channel, or an instrument
 * being tried from the browser (with buttons to add it or stop trying).
 * Keyed by the target, so a new one flashes in. */
/** function targetView(b: Builder, ch: KeysTarget?) => Undefined */
function targetView(b, ch) {
  if (!ch) {
    b.open("div", "ch-none", "kb-ch");
    b.leaf("span", "l", "kb-label", t("keyboard.target.keys.label"));
    b.leaf("b", "n", "", t("keyboard.target.noChannel.label"));
    b.close();
    return undefined;
  }
  b.open("div", `ch-${ch.trying ? state.audition.key : ch.id}`, ch.trying ? "kb-target trying" : "kb-target");
  b.style("--c", ch.color);
  b.attr("title", ch.trying ? tf("keyboard.target.trying.title", [ch.detail]) : tf("keyboard.target.channel.title", [ch.name, ch.detail]));
  b.open("div", "ch", "kb-ch");
  b.leaf("span", "l", "kb-label", ch.trying ? t("keyboard.target.trying.label") : t("keyboard.target.keys.label"));
  b.leaf("b", "n", "", ch.name);
  if (ch.trying) {
    toolButton(b, "keep", "kb-mini", "plus", t("keyboard.target.keep.title"), () => {
      keepTried();
    });
    toolButton(b, "drop", "kb-mini", "close", t("keyboard.target.drop.title"), () => stopTrying());
  }
  b.close();
  b.leaf("div", "d", "kb-detail", ch.detail);
  b.close();
}

/** function octaveButton(b: Builder, key: String, icon: String, by: Number) => Undefined */
function octaveButton(b, key, icon, by) {
  const tip = by < 0 ? t("keyboard.octave.down.title") : t("keyboard.octave.up.title");
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
