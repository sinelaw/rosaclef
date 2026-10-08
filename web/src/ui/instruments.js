// Instruments as the producer picks them: adding a channel, trying one out on
// the piano before adding it (an "audition"), and swapping a channel's
// instrument in place.
//
// What the piano plays — the keyboard target — is whatever was chosen last:
// a channel (selected in the rack, the piano roll, the browser's "In this
// song") or an instrument tried out from the browser.

import { state, commit, selectChannel, invalidate, hint, hooks, deviceSpec, AUDITION } from "../store.js";
import { newDevice, uniqueId, paletteColor, noArp, optionValue } from "../model.js";
import { retargetLanes } from "../automation.js";
import { insertIx } from "#brands";
import { toast } from "./toast.js";
import { t, tf } from "../i18n.js";

/** What the piano does with the instrument just picked: shows its strip and
 * plays a short note (keyboard.js says so with `onTried`: it imports this module). */
/** const pickHooks: { tried: (Device) => Undefined } */
const pickHooks = { tried: (d) => undefined };

/** function onTried(fn: (Device) => Undefined) => Undefined */
export function onTried(fn) {
  pickHooks.tried = fn;
}

/** The instrument being dragged from the browser (dropped on the rack: onto
 * a channel it replaces the channel's instrument, elsewhere it adds one).
 * `over`: the channel id it is over ("+" = the rack's empty space, "" = none).
 * Read it anywhere; change it with the functions below. */
export const dragPick = { on: false, over: "", pick: { key: "", name: "", device: newDevice("") } };

/** An instrument starts being dragged from the browser. */
/** function startPickDrag(pick: Pick) => Undefined */
export function startPickDrag(pick) {
  dragPick.on = true;
  dragPick.pick = pick;
}

/** The dragged instrument is over this drop target (a channel id, or "+"):
 * true when that is news (the view then shows it). */
/** function pickDragOver(target: String) => Boolean */
export function pickDragOver(target) {
  if (dragPick.over === target) return false;
  dragPick.over = target;
  return true;
}

/** The drag ended (dropped, or let go elsewhere). */
export function endPickDrag() {
  dragPick.on = false;
  dragPick.over = "";
}

/** function copyDevice(d: Device) => Device */
export function copyDevice(d) {
  return {
    type: d.type,
    enabled: true,
    params: d.params.map((x) => ({ key: x.key, value: x.value })),
    options: d.options.map((x) => ({ key: x.key, value: x.value })),
  };
}

/** Add a channel with a new instrument as part of the current edit (inside
 * `commit`); returns its id. */
/** function pushChannel(type: String, name: String, setup: (Device) => Undefined) => String */
export function pushChannel(type, name, setup) {
  const p = state.project;
  const id = uniqueId(name, p.channels.map((c) => c.id).concat([AUDITION]));
  const dev = newDevice(type);
  setup(dev);
  // Route to the first insert no channel uses yet (FL-style one insert per channel).
  let mixer = 1;
  while (mixer < p.mixer.inserts.length && p.channels.some((c) => c.mixer === insertIx(mixer))) mixer = mixer + 1;
  if (mixer >= p.mixer.inserts.length) mixer = 0;
  p.channels.push({
    id: id,
    name: name,
    color: paletteColor(p.channels.length),
    instrument: dev,
    volume: 0.8,
    pan: 0,
    mute: false,
    mixer: insertIx(mixer),
    arp: noArp(),
    layerOf: "",
  });
  return id;
}

/** Add a channel with a new instrument; returns its id. */
/** function addChannel(type: String, name: String, setup: (Device) => Undefined) => String */
export function addChannel(type, name, setup) {
  let id = "";
  commit(() => {
    id = pushChannel(type, name, setup);
  });
  selectChannel(id);
  return id;
}

/** Add a channel playing `pick`; returns its id. */
/** function addPick(pick: Pick) => String */
export function addPick(pick) {
  const dev = copyDevice(pick.device);
  const id = addChannel(dev.type, pick.name, (d) => {
    d.params = dev.params;
    d.options = dev.options;
  });
  toast(t("instruments.add.toast.title"), tf("instruments.add.toast.body", [pick.name]), "info");
  return id;
}

/** What instrument a device is, in a few words ("Grand Orchestra · Cello"). */
/** function instrumentLabel(d: Device) => String */
export function instrumentLabel(d) {
  const spec = deviceSpec(d.type, "instrument");
  const label = spec ? spec.label : d.type;
  if (d.type === "soundfont") return `${label} · ${optionValue(d, "program") || "Acoustic Grand Piano"}`;
  if (d.type === "drum") return `${label} · ${optionValue(d, "kind") || "kick"}`;
  if (d.type === "sampler") {
    const s = optionValue(d, "sample");
    return s === "" ? label : `${label} · ${s.split("/").pop() ?? s}`;
  }
  if (d.type === "plugin") {
    const pl = state.catalog.plugins.find((x) => x.id === optionValue(d, "id"));
    return pl ? tf("format.clapPlugin", [pl.name]) : t("instruments.label.plugin");
  }
  return label;
}

/** Let the piano play `pick` without adding it to the song; a short note
 * says what it sounds like. */
/** function tryPick(pick: Pick) => Undefined */
export function tryPick(pick) {
  const a = state.audition;
  a.on = true;
  a.key = pick.key;
  a.name = pick.name;
  a.color = "#d4af37";
  a.device = copyDevice(pick.device);
  if (hooks.audition) hooks.audition();
  pickHooks.tried(a.device);
  hint(tf("instruments.try.hint", [pick.name]));
  invalidate();
}

/** Stop trying an instrument: the piano plays the selected channel again. */
export function stopTrying() {
  if (!state.audition.on) return undefined;
  state.audition.on = false;
  if (hooks.audition) hooks.audition();
  invalidate();
}

/** Add the instrument being tried as a channel (the piano then plays it). */
/** function keepTried() => String */
export function keepTried() {
  const a = state.audition;
  if (!a.on) return "";
  return addPick({ key: a.key, name: a.name, device: a.device });
}

/** What the piano plays now. */
/** function keysTarget() => KeysTarget? */
export function keysTarget() {
  const a = state.audition;
  if (a.on) return { id: AUDITION, name: a.name, color: a.color, detail: instrumentLabel(a.device), trying: true };
  const ch = state.project.channels.find((c) => c.id === state.channel);
  if (!ch) return undefined;
  return { id: ch.id, name: ch.name, color: ch.color, detail: instrumentLabel(ch.instrument), trying: false };
}

/** Names a channel gets by default from its instrument (so a swap renames
 * it only if the producer had not named it). */
/** function defaultNames(d: Device) => String[] */
function defaultNames(d) {
  const spec = deviceSpec(d.type, "instrument");
  /** const out: String[] */
  const out = [d.type];
  if (spec) out.push(spec.label);
  for (const o of d.options) if (o.value !== "") out.push(o.value);
  for (const pr of state.catalog.presets) if (pr.type === d.type) out.push(pr.name);
  return out.map((n) => n.toLowerCase());
}

/** Swap the instrument of `ch` for `pick`'s, keeping its notes, mixer route,
 * volume, pan, arpeggio and layers. Automation of parameters the new
 * instrument does not have is removed (Ctrl+Z brings everything back). */
/** function replaceInstrument(ch: Channel, pick: Pick) => Undefined */
export function replaceInstrument(ch, pick) {
  const old = ch.instrument;
  const before = ch.name;
  const rename = defaultNames(old).includes(ch.name.toLowerCase().replace(/ \d+$/, ""));
  const dev = copyDevice(pick.device);
  // A sample the new sampler does not set stays.
  if (dev.type === "sampler" && old.type === "sampler" && optionValue(dev, "sample") === "") {
    const s = old.options.find((o) => o.key === "sample");
    if (s) dev.options.push({ key: s.key, value: s.value });
  }
  const spec = deviceSpec(dev.type, "instrument");
  commit(() => {
    ch.instrument = dev;
    if (rename) ch.name = pick.name;
    if (old.type !== dev.type) {
      const prefix = `channel/${ch.id}/`;
      retargetLanes((tg) => {
        if (!tg.startsWith(prefix)) return tg;
        const key = tg.slice(prefix.length);
        if (key === "volume" || key === "pan") return tg;
        return spec && spec.params.some((ps) => ps.key === key) ? tg : "";
      });
    }
  });
  selectChannel(ch.id);
  toast(t("instruments.replace.toast.title"), tf("instruments.replace.toast.body", [before, instrumentLabel(dev)]), "info");
}
