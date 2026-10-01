// Widgets: components are plain functions that write descriptions into a
// Builder. State comes in as arguments; changes go out through callbacks.

import { drag, fmt } from "#platform";
import { begin, changed, hint, commit } from "../store.js";
import { isAutomated, shownValue, openMenu } from "../automation.js";

/** function clamp01(v: Number) => Number */
export function clamp01(v) {
  return Math.max(0, Math.min(1, v));
}

// ------------------------------------------------------------------ params

/** function toUnit(spec: ParamSpec, v: Number) => Number */
export function toUnit(spec, v) {
  if (spec.curve === "exp" && spec.min > 0) return clamp01(Math.log(v / spec.min) / Math.log(spec.max / spec.min));
  return clamp01((v - spec.min) / (spec.max - spec.min));
}

/** function fromUnit(spec: ParamSpec, t: Number) => Number */
export function fromUnit(spec, t) {
  const u = clamp01(t);
  let v = spec.min + (spec.max - spec.min) * u;
  if (spec.curve === "exp" && spec.min > 0) v = spec.min * Math.pow(spec.max / spec.min, u);
  if (spec.integer) v = Math.round(v);
  return Math.max(spec.min, Math.min(spec.max, v));
}

/** function num(v: Number) => String */
function num(v) {
  const a = Math.abs(v);
  if (a >= 100) return fmt(v, 0);
  if (a >= 10) return fmt(v, 1);
  return fmt(v, 2);
}

/** function paramText(spec: ParamSpec, v: Number) => String */
export function paramText(spec, v) {
  const u = spec.unit;
  if (spec.integer) return `${Math.round(v)}${u === "" ? "" : " " + u}`;
  if (u === "Hz") return v >= 1000 ? `${fmt(v / 1000, 2)} kHz` : `${fmt(v, 0)} Hz`;
  if (u === "s") return v < 1 ? `${fmt(v * 1000, 0)} ms` : `${fmt(v, 2)} s`;
  if (u === "ms") return `${num(v)} ms`;
  if (u === "dB") return `${v > 0 ? "+" : ""}${fmt(v, 1)} dB`;
  if (u === "beats") return `${num(v)} beats`;
  if (u === "" && spec.min >= -1 && spec.max <= 1) return `${Math.round(v * 100)}%`;
  if (u === "") return spec.key.toLowerCase().includes("ratio") ? `${num(v)}×` : num(v);
  return `${num(v)} ${u}`;
}

// ------------------------------------------------------------------ knob

/** Rotary knob. `v` is 0..1; `onSet` receives 0..1 while dragging. */
/** function knob(b: Builder, key: String, cls: String, v: Number, label: String, tip: String, dflt: Number, onSet: (Number) => Undefined) => Undefined */
export function knob(b, key, cls, v, label, tip, dflt, onSet) {
  return knobAt(b, key, cls, v, label, tip, dflt, "", onSet);
}

/** Right-click menu of a control bound to an automation target ("" = none).
 * Call right after opening the control's node. */
/** function automatable(b: Builder, target: String) => Undefined */
function automatable(b, target) {
  if (target === "") return undefined;
  b.on("contextmenu", (e) => {
    e.preventDefault();
    e.stopPropagation();
    openMenu(target, e.clientX, e.clientY);
  });
}

/** The gold "automated" dot (inside the control's node). */
/** function autoDot(b: Builder, target: String) => Undefined */
function autoDot(b, target) {
  if (isAutomated(target)) {
    b.leaf("i", "auto", "auto-dot", "");
    b.attr("title", "Automated — right-click for the automation lane");
  }
}

/** Knob bound to an automation target (e.g. "channel/pad/pan"). */
/** function knobAt(b: Builder, key: String, cls: String, v: Number, label: String, tip: String, dflt: Number, target: String, onSet: (Number) => Undefined) => Undefined */
export function knobAt(b, key, cls, v, label, tip, dflt, target, onSet) {
  b.open("div", key, isAutomated(target) ? `knob automated ${cls}` : `knob ${cls}`);
  automatable(b, target);
  b.style("--v", fmt(clamp01(v), 4));
  b.attr("title", tip);
  b.on("pointerenter", (e) => hint(tip));
  b.on("pointerdown", (e) => {
    e.preventDefault();
    if (e.button === 2) return undefined;
    begin();
    const y0 = e.clientY;
    const v0 = v;
    drag(
      e,
      (m) => {
        const scale = m.shiftKey ? 900 : 180;
        onSet(clamp01(v0 + (y0 - m.clientY) / scale));
        changed(true);
      },
      (u) => undefined
    );
  });
  b.on("dblclick", (e) => {
    commit(() => onSet(dflt));
  });
  b.on("wheel", (e) => {
    e.preventDefault();
    begin();
    onSet(clamp01(v - e.deltaY / 2000));
    changed(true);
  });
  b.leaf("div", "ring", "knob-ring", "");
  b.open("div", "cap", "knob-cap");
  b.leaf("div", "dot", "knob-dot", "");
  b.close();
  if (label !== "") b.leaf("span", "label", "knob-label", label);
  autoDot(b, target);
  b.close();
}

/** Knob bound to a catalog parameter. */
/** function paramKnob(b: Builder, spec: ParamSpec, value: Number, onSet: (Number) => Undefined) => Undefined */
export function paramKnob(b, spec, value, onSet) {
  return paramKnobAt(b, spec, value, "", onSet);
}

/** Parameter knob bound to an automation target; while the song plays it
 * shows the automated value. */
/** function paramKnobAt(b: Builder, spec: ParamSpec, value: Number, target: String, onSet: (Number) => Undefined) => Undefined */
export function paramKnobAt(b, spec, value, target, onSet) {
  const shown = shownValue(target, value);
  const tip = `${spec.label}: ${paramText(spec, shown)} — ${spec.doc}`;
  b.open("div", spec.key, "param");
  knobAt(b, "k", "", toUnit(spec, shown), "", tip, toUnit(spec, spec.default), target, (t) => onSet(fromUnit(spec, t)));
  b.leaf("div", "name", "param-name", spec.label);
  b.leaf("div", "val", "param-value", paramText(spec, shown));
  b.close();
}

// ------------------------------------------------------------------ fader

/** Vertical fader, `v` 0..1. */
/** function fader(b: Builder, key: String, v: Number, tip: String, dflt: Number, onSet: (Number) => Undefined) => Undefined */
export function fader(b, key, v, tip, dflt, onSet) {
  return faderAt(b, key, v, tip, dflt, "", onSet);
}

/** Fader bound to an automation target. */
/** function faderAt(b: Builder, key: String, v: Number, tip: String, dflt: Number, target: String, onSet: (Number) => Undefined) => Undefined */
export function faderAt(b, key, v, tip, dflt, target, onSet) {
  b.open("div", key, isAutomated(target) ? "fader automated" : "fader");
  automatable(b, target);
  b.style("--v", fmt(clamp01(v), 4));
  b.attr("title", tip);
  b.on("pointerenter", (e) => hint(tip));
  b.on("pointerdown", (e) => {
    e.preventDefault();
    if (e.button === 2) return undefined;
    begin();
    const h = Math.max(20, e.targetHeight - 24);
    const pos = clamp01(1 - (e.clientY - e.targetTop - 12) / h);
    const grabbed = Math.abs(pos - v) < 0.06;
    const y0 = e.clientY;
    const v0 = grabbed ? v : pos;
    if (!grabbed) onSet(pos);
    changed(true);
    drag(
      e,
      (m) => {
        const scale = m.shiftKey ? h * 5 : h;
        onSet(clamp01(v0 + (y0 - m.clientY) / scale));
        changed(true);
      },
      (u) => undefined
    );
  });
  b.on("dblclick", (e) => {
    commit(() => onSet(dflt));
  });
  b.leaf("div", "track", "fader-track", "");
  b.leaf("div", "cap", "fader-cap", "");
  autoDot(b, target);
  b.close();
}

// ------------------------------------------------------------------ meter

/** Stereo peak meter; levels are linear peaks. */
/** function meter(b: Builder, key: String, l: Number, r: Number) => Undefined */
export function meter(b, key, l, r) {
  b.open("div", key, "meter");
  for (const ch of [
    { side: "l", v: l },
    { side: "r", v: r },
  ]) {
    const side = ch.side;
    const v = ch.v;
    // Map -60..+6 dB onto 0..1.
    const db = 20 * Math.log10(Math.max(v, 0.000001));
    const t = clamp01((db + 60) / 66);
    b.open("div", side, "meter-bar");
    b.leaf("div", "fill", v > 0.99 ? "meter-fill clip" : "meter-fill", "");
    b.style("transform", `scaleY(${fmt(t, 3)})`);
    b.close();
  }
  b.close();
}

/** Mono level bar (channel rack). */
/** function led(b: Builder, key: String, v: Number) => Undefined */
export function led(b, key, v) {
  const db = 20 * Math.log10(Math.max(v, 0.000001));
  b.open("div", key, "led-meter");
  b.leaf("div", "fill", "led-fill", "");
  b.style("transform", `scaleX(${fmt(clamp01((db + 48) / 48), 3)})`);
  b.close();
}

// ------------------------------------------------------------------ controls

/** function button(b: Builder, key: String, cls: String, label: String, tip: String, onClick: () => Undefined) => Undefined */
export function button(b, key, cls, label, tip, onClick) {
  b.leaf("button", key, `btn ${cls}`, label);
  b.attr("title", tip);
  b.on("pointerenter", (e) => hint(tip));
  b.on("click", (e) => {
    onClick();
  });
}

/** function iconButton(b: Builder, key: String, cls: String, icon: String, tip: String, onClick: () => Undefined) => Undefined */
export function iconButton(b, key, cls, icon, tip, onClick) {
  b.open("button", key, `btn icon ${cls}`);
  b.attr("title", tip);
  b.attr("aria-label", tip);
  b.on("pointerenter", (e) => hint(tip));
  b.on("click", (e) => {
    onClick();
  });
  glyph(b, icon);
  b.close();
}

/** function select(b: Builder, key: String, cls: String, value: String, choices: String[], labels: String[], tip: String, onSet: (String) => Undefined) => Undefined */
export function select(b, key, cls, value, choices, labels, tip, onSet) {
  b.open("select", key, `select ${cls}`);
  // Applied after the options exist (the reconciler sets props last).
  b.prop("value", value);
  b.attr("title", tip);
  b.on("pointerenter", (e) => hint(tip));
  b.on("change", (e) => {
    onSet(e.value);
  });
  for (let i = 0; i < choices.length; i++) {
    b.leaf("option", choices[i], "", i < labels.length ? labels[i] : choices[i]);
    b.attr("value", choices[i]);
  }
  b.close();
}

/** Text that turns into an input on double click is overkill; a plain input it is. */
/** function textInput(b: Builder, key: String, cls: String, value: String, placeholder: String, onSet: (String) => Undefined) => Undefined */
export function textInput(b, key, cls, value, placeholder, onSet) {
  b.leaf("input", key, `text-input ${cls}`, "");
  b.attr("placeholder", placeholder);
  b.attr("spellcheck", "false");
  b.prop("value", value);
  b.on("change", (e) => {
    onSet(e.value);
  });
  b.on("keydown", (e) => {
    if (e.key === "Enter") onSet(e.value);
  });
}

// ------------------------------------------------------------------ icons

/** const ICONS: { name: String, d: String }[] */
const ICONS = [
  { name: "play", d: "M7 4.5v15l12.5-7.5z" },
  { name: "pause", d: "M6.5 4.5h4v15h-4zM13.5 4.5h4v15h-4z" },
  { name: "stop", d: "M5.5 5.5h13v13h-13z" },
  { name: "record", d: "M12 5.2a6.8 6.8 0 1 0 0 13.6 6.8 6.8 0 1 0 0-13.6z" },
  { name: "undo", d: "M9 7H4.5V2.5M4.8 7A8 8 0 1 1 4 13" },
  { name: "redo", d: "M15 7h4.5V2.5M19.2 7A8 8 0 1 0 20 13" },
  { name: "plus", d: "M12 4.5v15M4.5 12h15" },
  { name: "close", d: "M6 6l12 12M18 6L6 18" },
  { name: "export", d: "M12 3.5v11M7.5 10l4.5 4.5 4.5-4.5M4.5 16.5v3h15v-3" },
  { name: "spark", d: "M12 2.5l2.1 6.2 6.4 1.3-5 4.2 1.5 6.5L12 17.2l-5 3.5 1.5-6.5-5-4.2 6.4-1.3z" },
  { name: "terminal", d: "M4 5.5h16v13H4zM7 9.5l3 2.5-3 2.5M12.5 15h4.5" },
  { name: "restart", d: "M19.5 12a7.5 7.5 0 1 1-2.2-5.3M19.5 4.5v4.5H15" },
  { name: "pattern", d: "M4.5 5h4v4h-4zM10 5h4v4h-4zM15.5 5h4v4h-4zM4.5 10.5h4v4h-4zM15.5 10.5h4v4h-4zM10 16h4v4h-4z" },
  { name: "song", d: "M3.5 6h9v3h-9zM8 11h12v3H8zM3.5 16h7v3h-7z" },
  { name: "mixer", d: "M6 3.5v17M12 3.5v17M18 3.5v17M4 14h4M10 8h4M16 12h4" },
  { name: "piano", d: "M3.5 4.5h17v15h-17zM8 4.5v9M12 4.5v9M16 4.5v9M8 13.5v6M12 13.5v6M16 13.5v6" },
  { name: "rack", d: "M4 5h16M4 10h16M4 15h16M4 20h16" },
  { name: "playlist", d: "M4 5.5h7v5H4zM13 5.5h7v5h-7zM4 13.5h10v5H4z" },
  { name: "mute", d: "M4 9.5h3.5l5-4v13l-5-4H4zM16 9l5 6M21 9l-5 6" },
  { name: "speaker", d: "M4 9.5h3.5l5-4v13l-5-4H4zM16 8.5a5 5 0 0 1 0 7M18.5 6a8.5 8.5 0 0 1 0 12" },
  { name: "mic", d: "M12 3.5a3 3 0 0 1 3 3v5a3 3 0 0 1-6 0v-5a3 3 0 0 1 3-3zM6 11a6 6 0 0 0 12 0M12 17v3.5" },
  { name: "loop", d: "M17 3.5l3 3-3 3M4 11V9.5a3 3 0 0 1 3-3h13M7 20.5l-3-3 3-3M20 13v1.5a3 3 0 0 1-3 3H4" },
  { name: "draw", d: "M4 20l4-1 11-11-3-3L5 16zM14 6l3 3" },
  { name: "select", d: "M4.5 4.5h4M4.5 4.5v4M19.5 4.5h-4M19.5 4.5v4M4.5 19.5h4M4.5 19.5v-4M19.5 19.5h-4M19.5 19.5v-4" },
  { name: "trash", d: "M5 7h14M9.5 7V4.5h5V7M7 7l1 12.5h8L17 7" },
  { name: "folder", d: "M3.5 6.5h6l2 2h9v10h-17z" },
  { name: "wave", d: "M3 12h2l2-6 3 12 3-9 2 6 2-3h4" },
  { name: "plug", d: "M9 3.5v5M15 3.5v5M6.5 8.5h11v3a5.5 5.5 0 0 1-11 0zM12 17v3.5" },
  { name: "follow", d: "M3.5 12h12M11.5 7.5l4.5 4.5-4.5 4.5M20 4.5v15" },
  { name: "copy", d: "M8.5 8.5h11v11h-11zM5.5 15.5h-1v-11h11v1" },
  { name: "minimize", d: "M6 16.5h12" },
  { name: "maximize", d: "M5.5 5.5h13v13h-13zM5.5 9h13" },
  { name: "restore", d: "M5 10h9v9H5zM5 13h9M9 10V6h10v9h-5" },
  { name: "left", d: "M14.5 6l-6 6 6 6" },
  { name: "right", d: "M9.5 6l6 6-6 6" },
  { name: "minus", d: "M5 12h14" },
  { name: "moon", d: "M19.5 14.5A8 8 0 1 1 9.5 4.5a6.5 6.5 0 0 0 10 10z" },
  { name: "sidebar", d: "M3.5 5h17v14h-17zM9 5v14M5.5 8.5h1.5M5.5 11.5h1.5" },
  { name: "score", d: "M3 6.5h18M3 9.5h18M3 12.5h18M3 15.5h18M3 18.5h18M14 19a2.2 1.6-20 1 1-.6-3.6M16.2 17.6V4.5l3.3 1.8" },
  { name: "keys", d: "M3.5 6.5h17v11h-17zM8.5 6.5v6.5M12 6.5v6.5M15.5 6.5v6.5M6 13h5M13 13h5M8.5 13v4.5M15.5 13v4.5" },
];

/** function glyph(b: Builder, name: String) => Undefined */
export function glyph(b, name) {
  b.open("svg", "g", `glyph glyph-${name}`);
  b.attr("viewBox", "0 0 24 24");
  b.attr("aria-hidden", "true");
  b.leaf("path", "p", "", "");
  let d = "";
  for (const i of ICONS) {
    if (i.name === name) d = i.d;
  }
  b.attr("d", d);
  b.close();
}
