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
    automation: (raw.automation ?? []).map((l) => ({
      id: String(l.id),
      name: String(l.name ?? ""),
      target: String(l.target),
      color: String(l.color ?? "#8a6bb0"),
      mute: l.mute === true,
      points: (l.points ?? []).map((pt) => ({ beat: Number(pt.beat), value: Number(pt.value), curve: Number(pt.curve ?? 0) })),
    })),
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

/** The wire (project.json) form of a clip. */
/** function encodeClipWire<R>(c: Clip) => R */
export function encodeClipWire(c) {
  return encodeClip(c);
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
  if (p.automation.length > 0) o.automation = p.automation.map(encodeLane);
  return o;
}

/** function encodePoint<R>(pt: AutomationPoint) => R */
function encodePoint(pt) {
  const o = JSON.parse("{}");
  o.beat = round6(pt.beat);
  o.value = round6(pt.value);
  if (pt.curve !== 0) o.curve = round6(pt.curve);
  return o;
}

/** function encodeLane<R>(l: AutomationLane) => R */
function encodeLane(l) {
  const o = JSON.parse("{}");
  o.id = l.id;
  o.name = l.name;
  o.target = l.target;
  o.color = l.color;
  if (l.mute) o.mute = true;
  o.points = l.points.map(encodePoint);
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
  const slug =
    base
      .toLowerCase()
      .replaceAll(" ", "-")
      .replace(/[^a-z0-9_.-]/g, "") || "item";
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
    automation: [],
  };
}

// ------------------------------------------------------------------ edit log

/** function fmtNum(x: Number) => String */
function fmtNum(x) {
  return String(Math.round(x * 1000) / 1000);
}

/** function deviceDiff(label: String, a: Device, b: Device, out: String[]) => Undefined */
function deviceDiff(label, a, b, out) {
  if (a.type !== b.type) {
    out.push(`${label}: ${a.type} → ${b.type}`);
    return undefined;
  }
  /** const parts: String[] */
  const parts = [];
  for (const e of b.params) {
    const old = a.params.find((x) => x.key === e.key);
    if (!old || Math.abs(old.value - e.value) > 1e-9) parts.push(`${e.key} ${old ? fmtNum(old.value) : "default"} → ${fmtNum(e.value)}`);
  }
  for (const e of b.options) {
    const old = a.options.find((x) => x.key === e.key);
    if (!old || old.value !== e.value) parts.push(`${e.key} → ${e.value}`);
  }
  if (a.enabled !== b.enabled) parts.push(b.enabled ? "enabled" : "bypassed");
  if (parts.length > 0) out.push(`${label}: ${parts.slice(0, 4).join(", ")}${parts.length > 4 ? ", …" : ""}`);
}

/** function noteText(n: Note) => String */
function noteText(n) {
  return `${noteName(n.pitch)} at beat ${fmtNum(n.start)}`;
}

/** A short human description of what changed between two versions. */
/** function describeChange(a: Project, b: Project) => String[] */
export function describeChange(a, b) {
  /** const out: String[] */
  const out = [];
  if (a.meta.title !== b.meta.title) out.push(`title → "${b.meta.title}"`);
  if (a.transport.bpm !== b.transport.bpm) out.push(`tempo ${fmtNum(a.transport.bpm)} → ${fmtNum(b.transport.bpm)} BPM`);
  if (a.transport.swing !== b.transport.swing) out.push(`swing → ${Math.round(b.transport.swing * 100)}%`);

  for (const c of b.channels) {
    const old = a.channels.find((x) => x.id === c.id);
    if (!old) {
      out.push(`added channel "${c.id}" (${c.instrument.type})`);
      continue;
    }
    if (old.name !== c.name) out.push(`channel "${c.id}" renamed to "${c.name}"`);
    if (old.volume !== c.volume) out.push(`channel "${c.id}" volume → ${fmtNum(c.volume)}`);
    if (old.pan !== c.pan) out.push(`channel "${c.id}" pan → ${fmtNum(c.pan)}`);
    if (old.mute !== c.mute) out.push(`channel "${c.id}" ${c.mute ? "muted" : "unmuted"}`);
    if (old.mixer !== c.mixer) out.push(`channel "${c.id}" rerouted to another insert`);
    deviceDiff(`channel "${c.id}"`, old.instrument, c.instrument, out);
  }
  for (const c of a.channels) if (!b.channels.some((x) => x.id === c.id)) out.push(`removed channel "${c.id}"`);

  for (const p of b.patterns) {
    const old = a.patterns.find((x) => x.id === p.id);
    if (!old) {
      out.push(`added pattern "${p.id}"`);
      continue;
    }
    if (old.length !== p.length) out.push(`pattern "${p.id}" length → ${fmtNum(p.length)} beats`);
    if (old.name !== p.name) out.push(`pattern "${p.id}" renamed to "${p.name}"`);
    /** const key: (Note) => String */
    const key = (n) => `${n.channel}|${n.pitch}|${n.start}|${n.length}|${n.velocity}`;
    const oldKeys = old.notes.map(key);
    const newKeys = p.notes.map(key);
    const added = p.notes.filter((n) => !oldKeys.includes(key(n)));
    const removed = old.notes.filter((n) => !newKeys.includes(key(n)));
    if (added.length > 0 && removed.length === added.length) {
      out.push(
        `pattern "${p.id}": edited ${added.length} note${added.length > 1 ? "s" : ""} (${noteText(removed[0])} → ${noteText(added[0])}${added.length > 1 ? ", …" : ""})`,
      );
    } else {
      if (added.length > 0)
        out.push(
          `pattern "${p.id}": +${added.length} note${added.length > 1 ? "s" : ""} on "${added[0].channel}" (${noteText(added[0])}${added.length > 1 ? ", …" : ""})`,
        );
      if (removed.length > 0)
        out.push(`pattern "${p.id}": −${removed.length} note${removed.length > 1 ? "s" : ""} (${noteText(removed[0])}${removed.length > 1 ? ", …" : ""})`);
    }
  }
  for (const p of a.patterns) if (!b.patterns.some((x) => x.id === p.id)) out.push(`removed pattern "${p.id}"`);

  const ca = a.playlist.clips.length;
  const cb = b.playlist.clips.length;
  if (cb > ca) out.push(`playlist: +${cb - ca} clip${cb - ca > 1 ? "s" : ""}`);
  else if (cb < ca) out.push(`playlist: −${ca - cb} clip${ca - cb > 1 ? "s" : ""}`);
  else if (JSON.stringify(a.playlist.clips.map(encodeClipKey)) !== JSON.stringify(b.playlist.clips.map(encodeClipKey)))
    out.push("playlist: clips moved or resized");
  for (let t = 0; t < b.playlist.tracks.length && t < a.playlist.tracks.length; t++) {
    const x = a.playlist.tracks[t];
    const y = b.playlist.tracks[t];
    if (x.name !== y.name) out.push(`track ${t} renamed to "${y.name}"`);
    if (x.mute !== y.mute) out.push(`track ${t} ${y.mute ? "muted" : "unmuted"}`);
  }
  if (b.playlist.tracks.length > a.playlist.tracks.length) out.push("playlist: added a track");

  for (let i = 0; i < b.mixer.inserts.length; i++) {
    const y = b.mixer.inserts[i];
    if (i >= a.mixer.inserts.length) {
      out.push(`mixer: added insert ${i}`);
      continue;
    }
    const x = a.mixer.inserts[i];
    const label = `insert ${i} "${y.name}"`;
    if (x.volume !== y.volume) out.push(`${label} volume → ${fmtNum(y.volume)}`);
    if (x.pan !== y.pan) out.push(`${label} pan → ${fmtNum(y.pan)}`);
    if (x.mute !== y.mute) out.push(`${label} ${y.mute ? "muted" : "unmuted"}`);
    if (x.solo !== y.solo) out.push(`${label} ${y.solo ? "soloed" : "unsoloed"}`);
    if (x.effects.length !== y.effects.length)
      out.push(`${label}: effects ${x.effects.map((e) => e.type).join(", ") || "none"} → ${y.effects.map((e) => e.type).join(", ") || "none"}`);
    else for (let k = 0; k < y.effects.length; k++) deviceDiff(`${label} effect ${k} (${y.effects[k].type})`, x.effects[k], y.effects[k], out);
  }

  for (const l of b.automation) {
    const old = a.automation.find((x) => x.id === l.id);
    if (!old) {
      out.push(`added automation lane "${l.id}" (${l.target}, ${plural(l.points.length, "point")})`);
      continue;
    }
    laneDiff(old, l, out);
  }
  for (const l of a.automation) if (!b.automation.some((x) => x.id === l.id)) out.push(`removed automation lane "${l.id}"`);
  return out.slice(0, 8);
}

/** function plural(n: Int, word: String) => String */
function plural(n, word) {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

/** function laneDiff(a: AutomationLane, b: AutomationLane, out: String[]) => Undefined */
function laneDiff(a, b, out) {
  const label = `automation "${b.id}"`;
  if (a.target !== b.target) out.push(`${label}: target → ${b.target}`);
  if (a.name !== b.name) out.push(`${label} renamed to "${b.name}"`);
  if (a.mute !== b.mute) out.push(`${label} ${b.mute ? "muted" : "unmuted"}`);
  const d = b.points.length - a.points.length;
  if (d > 0) out.push(`${label}: +${plural(d, "point")}`);
  else if (d < 0) out.push(`${label}: −${plural(-d, "point")}`);
  else {
    for (let k = 0; k < b.points.length; k++) {
      const x = a.points[k];
      const y = b.points[k];
      /** const parts: String[] */
      const parts = [];
      if (x.beat !== y.beat) parts.push(`beat ${fmtNum(x.beat)} → ${fmtNum(y.beat)}`);
      if (x.value !== y.value) parts.push(`value ${fmtNum(x.value)} → ${fmtNum(y.value)}`);
      if (x.curve !== y.curve) parts.push(`curve → ${fmtNum(y.curve)}`);
      if (parts.length > 0) {
        out.push(`${label}: point ${k} ${parts.join(", ")}`);
        break;
      }
    }
  }
}

/** function encodeClipKey(c: Clip) => String */
function encodeClipKey(c) {
  return `${c.pattern}|${c.sample}|${trackIndex(c.track)}|${c.start}|${c.length}|${c.offset}`;
}
