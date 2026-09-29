// Project model helpers: decoding the wire JSON into the typed in-memory
// model, encoding it back, and small musical utilities.

import { insertIx, insertIndex, trackIx, trackIndex } from "#brands";

// ------------------------------------------------------------------ decode

/** function decodeNums<T>(o: T) => KV[] */
function decodeNums(o) {
  /** const out: KV[] */
  const out = [];
  if (!o) return out;
  for (const k of Object.keys(o)) out.push({ key: k, value: Number(o[k]) });
  return out;
}

/** function decodeStrs<T>(o: T) => KS[] */
function decodeStrs(o) {
  /** const out: KS[] */
  const out = [];
  if (!o) return out;
  for (const k of Object.keys(o)) out.push({ key: k, value: String(o[k]) });
  return out;
}

/** function decodeDevice<T>(d: T) => Device */
export function decodeDevice(d) {
  return {
    type: String(d.type),
    enabled: d.enabled !== false,
    params: decodeNums(d.params),
    options: decodeStrs(d.options),
  };
}

/** function decodeCatalog<T>(raw: T) => Catalog */
export function decodeCatalog(raw) {
  return {
    devices: raw.devices,
    plugins: raw.plugins,
    presets: (raw.presets ?? []).map((p) => ({
      name: String(p.name),
      type: String(p.type),
      tags: String(p.tags),
      doc: String(p.doc),
      params: decodeNums(p.params),
      options: decodeStrs(p.options),
    })),
  };
}

/** A fresh device configured from a preset. */
/** function presetDevice(p: PresetInfo) => Device */
export function presetDevice(p) {
  return {
    type: p.type,
    enabled: true,
    params: p.params.map((e) => ({ key: e.key, value: e.value })),
    options: p.options.map((e) => ({ key: e.key, value: e.value })),
  };
}

/** function decodeProject<T>(raw: T) => Project */
export function decodeProject(raw) {
  const t = raw.transport;
  const pl = raw.playlist ?? { tracks: [], clips: [] };
  return {
    format: String(raw.format),
    meta: { title: String(raw.meta.title), author: String(raw.meta.author ?? ""), description: String(raw.meta.description ?? "") },
    transport: { bpm: Number(t.bpm), beatsPerBar: Number(t.beatsPerBar ?? 4), swing: Number(t.swing ?? 0) },
    channels: (raw.channels ?? []).map((c) => ({
      id: String(c.id),
      name: String(c.name),
      color: String(c.color ?? "#c9a45c"),
      instrument: decodeDevice(c.instrument),
      volume: Number(c.volume ?? 0.8),
      pan: Number(c.pan ?? 0),
      mute: c.mute === true,
      mixer: insertIx(Math.round(Number(c.mixer ?? 0))),
    })),
    patterns: (raw.patterns ?? []).map((p) => ({
      id: String(p.id),
      name: String(p.name),
      color: String(p.color ?? "#c9a45c"),
      length: Number(p.length),
      notes: (p.notes ?? []).map((n) => ({
        channel: String(n.channel),
        pitch: Number(n.pitch),
        start: Number(n.start),
        length: Number(n.length),
        velocity: Number(n.velocity ?? 0.8),
      })),
    })),
    playlist: {
      tracks: (pl.tracks ?? []).map((tr) => ({ name: String(tr.name), mute: tr.mute === true })),
      clips: (pl.clips ?? []).map((c) => ({
        pattern: String(c.pattern ?? ""),
        sample: String(c.sample ?? ""),
        track: trackIx(Math.round(Number(c.track))),
        start: Number(c.start),
        length: Number(c.length),
        offset: Number(c.offset ?? 0),
        gain: Number(c.gain ?? 1),
        mixer: insertIx(Math.round(Number(c.mixer ?? 0))),
      })),
    },
    mixer: {
      inserts: raw.mixer.inserts.map((i) => ({
        name: String(i.name),
        volume: Number(i.volume ?? 1),
        pan: Number(i.pan ?? 0),
        mute: i.mute === true,
        solo: i.solo === true,
        effects: (i.effects ?? []).map(decodeDevice),
      })),
    },
  };
}

// ------------------------------------------------------------------ encode

/** function round6(x: Number) => Number */
export function round6(x) {
  return Math.round(x * 1e6) / 1e6;
}

/** function encodeNums<R>(list: KV[]) => R */
function encodeNums(list) {
  const o = JSON.parse("{}");
  for (const e of list) o[e.key] = round6(e.value);
  return o;
}

/** function encodeStrs<R>(list: KS[]) => R */
function encodeStrs(list) {
  const o = JSON.parse("{}");
  for (const e of list) o[e.key] = e.value;
  return o;
}

/** function encodeDevice<R>(d: Device) => R */
export function encodeDevice(d) {
  const o = JSON.parse("{}");
  o.type = d.type;
  if (!d.enabled) o.enabled = false;
  o.params = encodeNums(d.params);
  o.options = encodeStrs(d.options);
  return o;
}

/** function encodeClip<R>(c: Clip) => R */
function encodeClip(c) {
  const o = JSON.parse("{}");
  if (c.pattern !== "") o.pattern = c.pattern;
  if (c.sample !== "") o.sample = c.sample;
  o.track = trackIndex(c.track);
  o.start = round6(c.start);
  o.length = round6(c.length);
  if (c.offset !== 0) o.offset = round6(c.offset);
  if (c.sample !== "") {
    o.gain = c.gain;
    o.mixer = insertIndex(c.mixer);
  }
  return o;
}

/** function encodeProject<R>(p: Project) => R */
export function encodeProject(p) {
  const o = JSON.parse("{}");
  o["$schema"] = "./project.schema.json";
  o.format = p.format;
  o.meta = p.meta;
  o.transport = p.transport;
  o.channels = p.channels.map((c) => ({
    id: c.id,
    name: c.name,
    color: c.color,
    instrument: encodeDevice(c.instrument),
    volume: round6(c.volume),
    pan: round6(c.pan),
    mute: c.mute,
    mixer: insertIndex(c.mixer),
  }));
  o.patterns = p.patterns.map((pt) => ({
    id: pt.id,
    name: pt.name,
    color: pt.color,
    length: round6(pt.length),
    notes: pt.notes.map((n) => ({
      channel: n.channel,
      pitch: n.pitch,
      start: round6(n.start),
      length: round6(n.length),
      velocity: round6(n.velocity),
    })),
  }));
  o.playlist = { tracks: p.playlist.tracks, clips: p.playlist.clips.map(encodeClip) };
  o.mixer = {
    inserts: p.mixer.inserts.map((i) => ({
      name: i.name,
      volume: round6(i.volume),
      pan: round6(i.pan),
      mute: i.mute,
      solo: i.solo,
      effects: i.effects.map(encodeDevice),
    })),
  };
  return o;
}

/** function projectJson(p: Project) => String */
export function projectJson(p) {
  return JSON.stringify(encodeProject(p));
}

/** function cloneProject(p: Project) => Project */
export function cloneProject(p) {
  return decodeProject(JSON.parse(projectJson(p)));
}

// ------------------------------------------------------------------ devices

/** function getParam(d: Device, spec: ParamSpec) => Number */
export function getParam(d, spec) {
  const e = d.params.find((p) => p.key === spec.key);
  return e ? e.value : spec.default;
}

/** function setParam(d: Device, key: String, v: Number) => Undefined */
export function setParam(d, key, v) {
  const e = d.params.find((p) => p.key === key);
  if (e) e.value = v;
  else d.params.push({ key: key, value: v });
  return undefined;
}

/** function getOption(d: Device, spec: OptionSpec) => String */
export function getOption(d, spec) {
  const e = d.options.find((p) => p.key === spec.key);
  return e ? e.value : spec.default;
}

/** function optionValue(d: Device, key: String) => String */
export function optionValue(d, key) {
  const e = d.options.find((p) => p.key === key);
  return e ? e.value : "";
}

/** function setOption(d: Device, key: String, v: String) => Undefined */
export function setOption(d, key, v) {
  const e = d.options.find((p) => p.key === key);
  if (e) e.value = v;
  else d.options.push({ key: key, value: v });
  return undefined;
}

/** function newDevice(type: String) => Device */
export function newDevice(type) {
  return { type: type, enabled: true, params: [], options: [] };
}

// ------------------------------------------------------------------ music

const NOTE_NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];

/** function noteName(pitch: Number) => String */
export function noteName(pitch) {
  const p = Math.round(pitch);
  const octave = Math.floor(p / 12) - 1;
  return `${NOTE_NAMES[((p % 12) + 12) % 12]}${octave}`;
}

/** function isBlackKey(pitch: Number) => Boolean */
export function isBlackKey(pitch) {
  const k = ((Math.round(pitch) % 12) + 12) % 12;
  return k === 1 || k === 3 || k === 6 || k === 8 || k === 10;
}

/** function snapTo(x: Number, grid: Number) => Number */
export function snapTo(x, grid) {
  if (grid <= 0) return x;
  return Math.round(x / grid) * grid;
}

/** function snapDown(x: Number, grid: Number) => Number */
export function snapDown(x, grid) {
  if (grid <= 0) return x;
  return Math.floor(x / grid + 1e-9) * grid;
}

/** function barBeat(beat: Number, beatsPerBar: Number) => String */
export function barBeat(beat, beatsPerBar) {
  const b = Math.max(0, beat);
  const bar = Math.floor(b / beatsPerBar) + 1;
  const inBar = b - (bar - 1) * beatsPerBar;
  const beatN = Math.floor(inBar) + 1;
  const ticks = Math.floor((inBar - Math.floor(inBar)) * 96);
  const pad = (n, w) => String(n).padStart(w, "0");
  return `${pad(bar, 3)}:${pad(beatN, 2)}:${pad(ticks, 2)}`;
}

/** function uniqueId(base: String, taken: String[]) => String */
export function uniqueId(base, taken) {
  const slug = base.toLowerCase().replaceAll(" ", "-").replace(/[^a-z0-9_.-]/g, "") || "item";
  if (!taken.includes(slug)) return slug;
  let n = 2;
  while (taken.includes(`${slug}-${n}`)) n = n + 1;
  return `${slug}-${n}`;
}

export const PALETTE = ["#d4af37", "#c97b84", "#e8d5b0", "#8e3b46", "#3f8f7a", "#4a6fa5", "#8a6bb0", "#b08d57", "#d98c5f", "#6fa3a0"];

/** function paletteColor(i: Number) => String */
export function paletteColor(i) {
  return PALETTE[Math.abs(Math.floor(i)) % PALETTE.length];
}

/** function songLength(p: Project) => Number */
export function songLength(p) {
  let end = 0;
  for (const c of p.playlist.clips) end = Math.max(end, c.start + c.length);
  return end;
}

/** function dbText(gain: Number) => String */
export function dbText(gain) {
  if (gain <= 0.00001) return "-∞ dB";
  const db = 20 * Math.log10(gain);
  const r = Math.round(db * 10) / 10;
  return `${r > 0 ? "+" : ""}${r} dB`;
}

/** function panText(pan: Number) => String */
export function panText(pan) {
  const v = Math.round(pan * 100);
  if (v === 0) return "C";
  return v < 0 ? `${-v}L` : `${v}R`;
}

/** function emptyProject() => Project */
export function emptyProject() {
  return {
    format: "rosaclef/1",
    meta: { title: "Untitled", author: "", description: "" },
    transport: { bpm: 120, beatsPerBar: 4, swing: 0 },
    channels: [],
    patterns: [],
    playlist: { tracks: [], clips: [] },
    mixer: { inserts: [{ name: "Master", volume: 1, pan: 0, mute: false, solo: false, effects: [] }] },
  };
}
