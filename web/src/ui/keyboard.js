// The on-screen piano: a strip of keys along the bottom of the studio that
// plays the selected channel — with the mouse, or with several fingers on a
// touch screen (slide along the keys for a glissando; lower on a key is
// louder). The computer-keyboard piano (keys.js) lights the same keys.

import { drag, loadPref, savePref } from "#platform";
import { state, currentChannel, invalidate, hint } from "../store.js";
import { noteOn, noteOff, startAudio } from "../audio.js";
import { isBlackKey, noteName } from "../model.js";
import { glyph } from "./widgets.js";

/** A sounding key: who holds it (a pointer or a computer key), on which channel. */
/** type Held = { source: String, channel: String, pitch: Number } */

/** A black key's pitch and left edge in the strip. */
/** type BlackKey = { pitch: Number, x: Number } */

const PREF = "rosaclef.keyboard.";
/** Lowest and highest playable keys. */
const LOWEST = 12;
const HIGHEST = 127;

export const keyboard = {
  shown: true,
  /** The first (leftmost) key: always a C. */
  low: 48,
  /** Measured width of the keys, in pixels. */
  width: 0,
};

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
  const low = Number(loadPref(PREF + "low"));
  if (low >= LOWEST && low <= HIGHEST - 7 && low % 12 === 0) keyboard.low = low;
}

function saveKeyboard() {
  savePref(PREF + "shown", keyboard.shown ? "yes" : "no");
  savePref(PREF + "low", String(keyboard.low));
}

export function toggleKeyboard() {
  keyboard.shown = !keyboard.shown;
  saveKeyboard();
  invalidate();
}

/** function shiftOctave(by: Number) => Undefined */
function shiftOctave(by) {
  // The top octave that still fills the strip.
  const top = Math.max(LOWEST, HIGHEST + 1 - 12 * geo.octaves - ((HIGHEST + 1) % 12));
  keyboard.low = Math.max(LOWEST, Math.min(top, keyboard.low + by * 12));
  saveKeyboard();
  invalidate();
}

/** Start a note on the selected channel for `source` (it stops the note that source held). */
/** function pressKey(source: String, pitch: Number, velocity: Number) => Undefined */
export function pressKey(source, pitch, velocity) {
  releaseKey(source);
  const ch = currentChannel();
  if (!ch || pitch < 0) return undefined;
  held.push({ source: source, channel: ch.id, pitch: pitch });
  noteOn(ch.id, pitch, velocity);
  invalidate();
}

/** Stop the note `source` holds, if any. */
/** function releaseKey(source: String) => Undefined */
export function releaseKey(source) {
  let at = -1;
  for (let i = 0; i < held.length; i++) if (held[i].source === source) at = i;
  if (at < 0) return undefined;
  const h = held[at];
  held.splice(at, 1);
  noteOff(h.channel, h.pitch);
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
  b.open("div", "oct", "kb-oct");
  octaveButton(b, "down", "left", -1);
  b.leaf("span", "o", "kb-octave", noteName(keyboard.low));
  octaveButton(b, "up", "right", 1);
  b.close();
  b.close();

  b.open("div", "keys", "kb-keys");
  b.on("resize", (e) => {
    if (Math.abs(e.targetWidth - keyboard.width) >= 1) {
      keyboard.width = e.targetWidth;
      invalidate();
    }
  });
  b.on("pointerenter", (e) =>
    hint(ch ? `Play ${ch.name} — slide for a glissando; lower on a key is louder · Z–M on the keyboard plays C4–C5` : "Select a channel to play it")
  );
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
  for (let i = 0; i < whites.length; i++) {
    const p = whites[i];
    b.open("div", `w${p}`, isDown(p) ? "kb-white down" : "kb-white");
    b.style("left", `${i * geo.whiteW}px`);
    b.style("width", `${geo.whiteW}px`);
    if (p % 12 === 0) b.leaf("span", "l", "", noteName(p));
    b.close();
  }
  for (const k of blacks) {
    b.leaf("div", `b${k.pitch}`, isDown(k.pitch) ? "kb-black down" : "kb-black", "");
    b.style("left", `${k.x}px`);
    b.style("width", `${geo.blackW}px`);
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
