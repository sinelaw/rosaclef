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
  const a = raw.arp;
  return {
    devices: raw.devices,
    plugins: raw.plugins,
    arp: a
      ? {
          chords: a.chords.map((x) => String(x)),
          directions: a.directions.map((x) => String(x)),
          modes: a.modes.map((x) => String(x)),
          rateMin: Number(a.rateMin),
          rateMax: Number(a.rateMax),
          gateMin: Number(a.gateMin),
          gateMax: Number(a.gateMax),
          octavesMax: Math.round(Number(a.octavesMax)),
        }
      : defaultArpCatalog(),
    collections: (raw.collections ?? []).map((c) => ({
      id: String(c.id),
      name: String(c.name),
      version: String(c.version),
      license: String(c.license),
      authors: String(c.authors),
      summary: String(c.summary),
      source: String(c.source),
      licenseFile: String(c.licenseFile),
      readmeFile: String(c.readmeFile),
      sourcesFile: String(c.sourcesFile),
      instrument: String(c.instrument),
      presets: (c.presets ?? []).map((p) => ({ name: String(p[0]), bank: Math.round(Number(p[1])), program: Math.round(Number(p[2])) })),
    })),
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

/** The arpeggiator's choices before the catalog arrives (or from an older server). */
/** function defaultArpCatalog() => ArpCatalog */
export function defaultArpCatalog() {
  return { chords: ["octave"], directions: ["up"], modes: ["free"], rateMin: 1 / 64, rateMax: 4, gateMin: 0.05, gateMax: 2, octavesMax: 8 };
}

/** No arpeggiator (its settings are what turning it on starts from). */
/** function noArp() => Arp */
export function noArp() {
  return { on: false, chord: "octave", octaves: 1, rate: 0.25, direction: "up", gate: 1, mode: "free" };
}

/** function copyArp(a: Arp) => Arp */
export function copyArp(a) {
  return { on: a.on, chord: a.chord, octaves: a.octaves, rate: a.rate, direction: a.direction, gate: a.gate, mode: a.mode };
}

/** A channel's `arp` from the project JSON (absent = off). */
/** function decodeArp<T>(a: T) => Arp */
function decodeArp(a) {
  if (!a) return noArp();
  return {
    on: true,
    chord: String(a.chord ?? "octave"),
    octaves: Math.round(Number(a.octaves ?? 1)),
    rate: Number(a.rate),
    direction: String(a.direction ?? "up"),
    gate: Number(a.gate ?? 1),
    mode: String(a.mode ?? "free"),
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
    transport: {
      bpm: Number(t.bpm),
      beatsPerBar: Number(t.beatsPerBar ?? 4),
      swing: Number(t.swing ?? 0),
      transpose: Math.round(Number(t.transpose ?? 0)),
      meters: (t.meters ?? []).map((m) => ({ bar: Number(m.bar), numerator: Number(m.numerator), denominator: Number(m.denominator) })),
    },
    channels: (raw.channels ?? []).map((c) => ({
      id: String(c.id),
      name: String(c.name),
      color: String(c.color ?? "#c9a45c"),
      instrument: decodeDevice(c.instrument),
      volume: Number(c.volume ?? 0.8),
      pan: Number(c.pan ?? 0),
      mute: c.mute === true,
      mixer: insertIx(Math.round(Number(c.mixer ?? 0))),
      arp: decodeArp(c.arp),
      layerOf: String(c.layerOf ?? ""),
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
      drums: decodePatternDrums(p.drums),
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
    score: decodeScore(raw.score),
    drums: decodeDrums(raw.drums),
    critic: {
      off: ((raw.critic ?? {}).off ?? []).map((x) => String(x)),
      suppress: ((raw.critic ?? {}).suppress ?? []).map((x) => String(x)),
    },
    repeats: (raw.repeats ?? []).map((r) => ({
      start: Number(r.start),
      end: Number(r.end),
      times: Math.round(Number(r.times ?? 2)),
      endings: (r.endings ?? []).map((e) => ({ start: Number(e.start), end: Number(e.end), passes: (e.passes ?? []).map((k) => Math.round(Number(k))) })),
    })),
    animation: decodeAnimation(raw.animation),
  };
}

// ------------------------------------------------------------------ film

/** An optional number: NaN when absent. */
/** function optNum<T>(x: T) => Number */
function optNum(x) {
  return x === undefined || x === null ? NaN : Number(x);
}

/** function decodeEffects<T>(list: T) => FilmEffect[] */
function decodeEffects(list) {
  return (list ?? []).map((e) => ({ type: String(e.type), amount: Number(e.amount ?? 1) }));
}

/** function decodeOffset<T>(o: T) => Number[] */
function decodeOffset(o) {
  return (o ?? []).map((x) => Number(x));
}

/** A shot's end camera, as written (or unset). */
/** function decodeMove<T>(m: T) => CameraMove */
function decodeMove(m) {
  if (!m) return noMove();
  return { zoom: optNum(m.zoom), tilt: optNum(m.tilt), turn: optNum(m.turn), offset: decodeOffset(m.offset) };
}

/** function noMove() => CameraMove */
export function noMove() {
  return { zoom: NaN, tilt: NaN, turn: NaN, offset: [] };
}

/** A shot from the project JSON. */
/** function decodeShot<T>(s: T) => Shot */
export function decodeShot(s) {
  return {
    start: Number(s.start),
    end: Number(s.end),
    label: String(s.label ?? ""),
    focus: (s.focus ?? []).map((x) => String(x)),
    role: String(s.role ?? ""),
    frame: String(s.frame ?? ""),
    zoom: optNum(s.zoom),
    tilt: optNum(s.tilt),
    turn: optNum(s.turn),
    offset: decodeOffset(s.offset),
    at: optNum(s.at),
    to: decodeMove(s.to),
    transition: String(s.transition ?? ""),
    glide: optNum(s.glide),
    ease: String(s.ease ?? ""),
    effects: decodeEffects(s.effects),
  };
}

/** The film from the project JSON (absent: none, directed automatically). */
/** function decodeAnimation<T>(a: T) => Animation */
export function decodeAnimation(a) {
  if (!a) return noAnimation();
  return {
    on: true,
    mode: String(a.mode ?? "auto"),
    view: String(a.view ?? "score"),
    surface: String(a.surface ?? "walnut"),
    energy: Number(a.energy ?? 0.5),
    effects: decodeEffects(a.effects),
    shots: (a.shots ?? []).map(decodeShot),
  };
}

/** function noAnimation() => Animation */
export function noAnimation() {
  return { on: false, mode: "auto", view: "score", surface: "walnut", energy: 0.5, effects: [], shots: [] };
}

/** function encodeEffect<R>(e: FilmEffect) => R */
function encodeEffect(e) {
  const o = JSON.parse("{}");
  o.type = e.type;
  if (e.amount !== 1) o.amount = round6(e.amount);
  return o;
}

/** function encodeMove<R>(m: CameraMove) => R */
function encodeMove(m) {
  const o = JSON.parse("{}");
  if (Number.isFinite(m.zoom)) o.zoom = round6(m.zoom);
  if (Number.isFinite(m.tilt)) o.tilt = round6(m.tilt);
  if (Number.isFinite(m.turn)) o.turn = round6(m.turn);
  if (m.offset.length === 2) o.offset = m.offset.map(round6);
  return o;
}

/** Whether a shot drifts anywhere by its end. */
/** function hasMove(m: CameraMove) => Boolean */
export function hasMove(m) {
  return Number.isFinite(m.zoom) || Number.isFinite(m.tilt) || Number.isFinite(m.turn) || m.offset.length === 2;
}

/** The wire form of a shot; unset fields are left out. */
/** function encodeShot<R>(s: Shot) => R */
export function encodeShot(s) {
  const o = JSON.parse("{}");
  o.start = round6(s.start);
  o.end = round6(s.end);
  if (s.label !== "") o.label = s.label;
  if (s.focus.length > 0) o.focus = s.focus;
  if (s.role !== "") o.role = s.role;
  if (s.frame !== "") o.frame = s.frame;
  if (Number.isFinite(s.zoom)) o.zoom = round6(s.zoom);
  if (Number.isFinite(s.tilt)) o.tilt = round6(s.tilt);
  if (Number.isFinite(s.turn)) o.turn = round6(s.turn);
  if (s.offset.length === 2) o.offset = s.offset.map(round6);
  if (Number.isFinite(s.at)) o.at = round6(s.at);
  if (hasMove(s.to)) o.to = encodeMove(s.to);
  if (s.transition !== "") o.transition = s.transition;
  if (Number.isFinite(s.glide)) o.glide = round6(s.glide);
  if (s.ease !== "") o.ease = s.ease;
  if (s.effects.length > 0) o.effects = s.effects.map(encodeEffect);
  return o;
}

/** function encodeAnimation<R>(a: Animation) => R */
function encodeAnimation(a) {
  const o = JSON.parse("{}");
  o.mode = a.mode;
  if (a.view !== "score") o.view = a.view;
  o.surface = a.surface;
  o.energy = round6(a.energy);
  if (a.effects.length > 0) o.effects = a.effects.map(encodeEffect);
  if (a.shots.length > 0) o.shots = a.shots.map(encodeShot);
  return o;
}

/** function decodeDrums<T>(d: T) => DrumPart */
export function decodeDrums(d) {
  if (!d) return noDrums();
  return {
    on: true,
    groove: String(d.groove),
    kit: String(d.kit ?? ""),
    feel: String(d.feel ?? "natural"),
    swing: Number(d.swing ?? 0),
    start: Math.round(Number(d.start ?? 1)),
    ending: String(d.ending ?? "hit"),
    variations: d.variations !== false,
    seed: Math.round(Number(d.seed ?? 1)),
    sections: (d.sections ?? []).map((x) => ({
      name: String(x.name ?? ""),
      bars: Math.round(Number(x.bars)),
      play: String(x.play ?? "a"),
      fill: String(x.fill ?? "none"),
      crash: x.crash === true,
      groove: String(x.groove ?? ""),
    })),
    grooves: Object.keys(d.grooves ?? {}).map((k) => ({
      groove: k,
      a: (d.grooves[k].a ?? []).map((r) => [String(r[0]), String(r[1])]),
      b: (d.grooves[k].b ?? []).map((r) => [String(r[0]), String(r[1])]),
    })),
    kept: Object.keys(d.kept ?? {}).map((k) => ({
      slot: k,
      name: String(d.kept[k].name ?? k),
      notes: (d.kept[k].notes ?? []).map((n) => ({ role: String(n.role), start: Number(n.start), length: Number(n.length), velocity: Number(n.velocity) })),
    })),
    written: Object.keys(d.written ?? {}).map((k) => ({ id: k, slot: String(d.written[k].slot ?? ""), print: String(d.written[k].print ?? "") })),
  };
}

/** A project without a drum part. */
/** function decodePatternDrums<T>(d: T) => PatternDrums */
export function decodePatternDrums(d) {
  if (!d) return noPatternDrums();
  return {
    on: true,
    groove: String(d.groove),
    play: String(d.play ?? "a"),
    fill: String(d.fill ?? "none"),
    crash: d.crash === true,
    turnaround: d.turnaround === true,
    kit: String(d.kit ?? ""),
    feel: String(d.feel ?? "natural"),
    swing: Number(d.swing ?? 0),
    seed: Math.round(Number(d.seed ?? 1)),
    edited: d.edited === true,
  };
}

/** An ordinary pattern's (no recipe). */
/** function noPatternDrums() => PatternDrums */
export function noPatternDrums() {
  return { on: false, groove: "", play: "a", fill: "none", crash: false, turnaround: false, kit: "", feel: "natural", swing: 0, seed: 1, edited: false };
}

/** function encodePatternDrums<R>(d: PatternDrums) => R */
function encodePatternDrums(d) {
  const o = JSON.parse("{}");
  o.groove = d.groove;
  o.play = d.play;
  if (d.fill !== "none") o.fill = d.fill;
  if (d.crash) o.crash = true;
  if (d.turnaround) o.turnaround = true;
  if (d.kit !== "") o.kit = d.kit;
  o.feel = d.feel;
  if (d.swing !== 0) o.swing = round6(d.swing);
  o.seed = d.seed;
  if (d.edited) o.edited = true;
  return o;
}

/** function noDrums() => DrumPart */
export function noDrums() {
  return {
    on: false,
    groove: "",
    kit: "",
    feel: "natural",
    swing: 0,
    start: 1,
    ending: "hit",
    variations: true,
    seed: 1,
    sections: [],
    grooves: [],
    kept: [],
    written: [],
  };
}

/** function decodeScore<T>(s: T) => ScoreSettings */
export function decodeScore(s) {
  if (!s) return emptyScore();
  const key = String(s.key ?? "");
  return {
    key: key === "auto" ? "" : key,
    hidden: (s.hidden ?? []).map((x) => String(x)),
    hiddenTracks: (s.hiddenTracks ?? []).map((x) => trackIx(Math.round(Number(x)))),
    clefs: decodeStrs(s.clefs),
    marks: (s.marks ?? []).map((m) => ({
      start: Number(m.start),
      end: Number(m.end),
      color: String(m.color ?? "#c97b84"),
      label: String(m.label ?? ""),
      pattern: String(m.pattern ?? ""),
      channels: (m.channels ?? []).map((x) => String(x)),
    })),
  };
}

/** function emptyScore() => ScoreSettings */
export function emptyScore() {
  return { key: "", hidden: [], hiddenTracks: [], clefs: [], marks: [] };
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
  o.transport = { bpm: p.transport.bpm, beatsPerBar: p.transport.beatsPerBar, swing: p.transport.swing };
  if (p.transport.transpose !== 0) o.transport.transpose = p.transport.transpose;
  if (p.transport.meters.length > 0) o.transport.meters = p.transport.meters;
  o.channels = p.channels.map((c) => {
    const ch = JSON.parse("{}");
    ch.id = c.id;
    ch.name = c.name;
    ch.color = c.color;
    ch.instrument = encodeDevice(c.instrument);
    ch.volume = round6(c.volume);
    ch.pan = round6(c.pan);
    ch.mute = c.mute;
    ch.mixer = insertIndex(c.mixer);
    const a = c.arp;
    if (a.on) ch.arp = { chord: a.chord, octaves: a.octaves, rate: round6(a.rate), direction: a.direction, gate: round6(a.gate), mode: a.mode };
    if (c.layerOf !== "") ch.layerOf = c.layerOf;
    return ch;
  });
  o.patterns = p.patterns.map((pt) => {
    const po = JSON.parse("{}");
    po.id = pt.id;
    po.name = pt.name;
    po.color = pt.color;
    po.length = round6(pt.length);
    po.notes = pt.notes.map((n) => ({
      channel: n.channel,
      pitch: n.pitch,
      start: round6(n.start),
      length: round6(n.length),
      velocity: round6(n.velocity),
    }));
    if (pt.drums.on) po.drums = encodePatternDrums(pt.drums);
    return po;
  });
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
  const sc = encodeScore(p.score);
  if (Object.keys(sc).length > 0) o.score = sc;
  if (p.repeats.length > 0) o.repeats = p.repeats.map(encodeRepeat);
  if (p.animation.on) o.animation = encodeAnimation(p.animation);
  if (p.drums.on) o.drums = encodeDrums(p.drums);
  if (p.critic.off.length > 0 || p.critic.suppress.length > 0) o.critic = { off: p.critic.off.slice(), suppress: p.critic.suppress.slice() };
  return o;
}

/** The wire form of the drum part; defaults are left out. */
/** function encodeDrums<R>(d: DrumPart) => R */
function encodeDrums(d) {
  const o = JSON.parse("{}");
  o.groove = d.groove;
  if (d.kit !== "") o.kit = d.kit;
  o.feel = d.feel;
  if (d.swing !== 0) o.swing = round6(d.swing);
  o.start = d.start;
  o.ending = d.ending;
  o.variations = d.variations;
  o.seed = d.seed;
  o.sections = d.sections.map((x) => {
    const sec = JSON.parse("{}");
    if (x.name !== "") sec.name = x.name;
    sec.bars = x.bars;
    sec.play = x.play;
    if (x.fill !== "none") sec.fill = x.fill;
    if (x.crash) sec.crash = true;
    if (x.groove !== "") sec.groove = x.groove;
    return sec;
  });
  if (d.grooves.length > 0) {
    o.grooves = JSON.parse("{}");
    for (const e of d.grooves) o.grooves[e.groove] = { a: e.a, b: e.b };
  }
  if (d.kept.length > 0) {
    o.kept = JSON.parse("{}");
    for (const k of d.kept) {
      o.kept[k.slot] = {
        name: k.name,
        notes: k.notes.map((n) => ({ role: n.role, start: round6(n.start), length: round6(n.length), velocity: round6(n.velocity) })),
      };
    }
  }
  if (d.written.length > 0) {
    o.written = JSON.parse("{}");
    for (const w of d.written) o.written[w.id] = { slot: w.slot, print: w.print };
  }
  return o;
}

/** The wire form of the score settings; empty fields are left out. */
/** function encodeScore<R>(s: ScoreSettings) => R */
function encodeScore(s) {
  const o = JSON.parse("{}");
  if (s.key !== "") o.key = s.key;
  if (s.hidden.length > 0) o.hidden = s.hidden;
  if (s.hiddenTracks.length > 0) o.hiddenTracks = s.hiddenTracks.map(trackIndex);
  if (s.clefs.length > 0) o.clefs = encodeStrs(s.clefs);
  if (s.marks.length > 0) o.marks = s.marks.map(encodeMark);
  return o;
}

/** function encodeRepeat<R>(r: Repeat) => R */
function encodeRepeat(r) {
  const o = JSON.parse("{}");
  o.start = round6(r.start);
  o.end = round6(r.end);
  o.times = r.times;
  if (r.endings.length > 0) o.endings = r.endings.map((e) => ({ start: round6(e.start), end: round6(e.end), passes: e.passes }));
  return o;
}

/** function encodeMark<R>(m: ScoreMark) => R */
function encodeMark(m) {
  const o = JSON.parse("{}");
  o.start = round6(m.start);
  o.end = round6(m.end);
  o.color = m.color;
  if (m.label !== "") o.label = m.label;
  if (m.pattern !== "") o.pattern = m.pattern;
  if (m.channels.length > 0) o.channels = m.channels;
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

/** The project without its film: what the score's pages are made of (the film leaves them as they are). */
/** function musicJson(p: Project) => String */
export function musicJson(p) {
  const film = p.animation;
  p.animation = noAnimation();
  const text = projectJson(p);
  p.animation = film;
  return text;
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

/** General MIDI drum sounds, from key 35. */
const GM_DRUMS = [
  "Kick 2",
  "Kick",
  "Side Stick",
  "Snare",
  "Clap",
  "Snare 2",
  "Floor Tom 2",
  "Closed Hat",
  "Floor Tom",
  "Pedal Hat",
  "Low Tom",
  "Open Hat",
  "Low-Mid Tom",
  "Mid Tom",
  "Crash",
  "High Tom",
  "Ride",
  "China",
  "Ride Bell",
  "Tambourine",
  "Splash",
  "Cowbell",
  "Crash 2",
  "Vibraslap",
  "Ride 2",
  "High Bongo",
  "Low Bongo",
  "Mute Conga",
  "High Conga",
  "Low Conga",
  "High Timbale",
  "Low Timbale",
  "High Agogo",
  "Low Agogo",
  "Cabasa",
  "Maracas",
  "Whistle",
  "Long Whistle",
  "Guiro",
  "Long Guiro",
  "Claves",
  "High Block",
  "Low Block",
  "Mute Cuica",
  "Open Cuica",
  "Mute Triangle",
  "Open Triangle",
];

/** The drum a key plays on a General MIDI drum kit channel, or "" (not a kit, or no drum there). */
/** function drumName(ch: Channel, pitch: Number) => String */
export function drumName(ch, pitch) {
  if (ch.instrument.type !== "soundfont") return "";
  // The General MIDI kits ("Jazz Kit") and their variations ("Jazz Kit 2").
  if (!/ Kit( \d+)?$/.test(optionValue(ch.instrument, "program"))) return "";
  const i = Math.round(pitch) - 35;
  return i >= 0 && i < GM_DRUMS.length ? GM_DRUMS[i] : "";
}

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

// ------------------------------------------------------------------ meters
// Bars follow `transport.meters` (see Transport::meter_map in
// crates/core/src/model.rs): each change holds from its bar (counted from 1)
// until the next; bars before the first change have `beatsPerBar` beats.

/** type MeterSpan = { bar: Number, beat: Number, barBeats: Number, label: String } */
/** type BarPos = { bar: Number, start: Number, length: Number } */

/** The bar grid: one span per meter, the first at bar 0, beat 0 (bars counted from 0). */
/** function meterMap(t: Transport) => MeterSpan[] */
export function meterMap(t) {
  /** const out: MeterSpan[] */
  const out = [{ bar: 0, beat: 0, barBeats: Math.max(1, t.beatsPerBar), label: `${t.beatsPerBar}/4` }];
  for (const m of t.meters) {
    const bar = Math.max(1, m.bar) - 1;
    const len = (4 * m.numerator) / Math.max(1, m.denominator);
    if (!(len > 0 && Number.isFinite(len))) continue;
    const label = `${m.numerator}/${m.denominator}`;
    const last = out[out.length - 1];
    if (bar === 0 && out.length === 1) out[0] = { bar: 0, beat: 0, barBeats: len, label: label };
    else if (bar > last.bar) out.push({ bar: bar, beat: last.beat + (bar - last.bar) * last.barBeats, barBeats: len, label: label });
  }
  return out;
}

/** The bar (counted from 0) that contains `beat`. */
/** function barAt(t: Transport, beat: Number) => BarPos */
export function barAt(t, beat) {
  const map = meterMap(t);
  let s = map[0];
  for (const m of map) if (m.beat <= beat + 1e-9) s = m;
  const k = Math.max(0, Math.floor((beat - s.beat) / s.barBeats + 1e-9));
  return { bar: s.bar + k, start: s.beat + k * s.barBeats, length: s.barBeats };
}

/** The bars that overlap the song time `lo`..`hi` (beats). */
/** function barLines(t: Transport, lo: Number, hi: Number) => BarPos[] */
export function barLines(t, lo, hi) {
  const map = meterMap(t);
  /** const out: BarPos[] */
  const out = [];
  for (let i = 0; i < map.length; i++) {
    const s = map[i];
    const stop = i + 1 < map.length ? map[i + 1].bar : Infinity;
    for (let bar = s.bar + Math.max(0, Math.floor((lo - s.beat) / s.barBeats)); bar < stop; bar++) {
      const start = s.beat + (bar - s.bar) * s.barBeats;
      if (start > hi || out.length >= 4096) return out;
      out.push({ bar: bar, start: start, length: s.barBeats });
    }
  }
  return out;
}

/** The meter label ("7/8") of a bar where the meter changes, else "". */
/** function meterChangeAt(t: Transport, bar: Number) => String */
export function meterChangeAt(t, bar) {
  if (t.meters.length === 0) return "";
  for (const s of meterMap(t)) if (s.bar === bar) return s.label;
  return "";
}

/** function barBeat(beat: Number, t: Transport) => String */
export function barBeat(beat, t) {
  const b = Math.max(0, beat);
  const pos = barAt(t, b);
  const bar = pos.bar + 1;
  const inBar = b - pos.start;
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
    transport: { bpm: 120, beatsPerBar: 4, swing: 0, transpose: 0, meters: [] },
    channels: [],
    patterns: [],
    playlist: { tracks: [], clips: [] },
    mixer: { inserts: [{ name: "Master", volume: 1, pan: 0, mute: false, solo: false, effects: [] }] },
    automation: [],
    score: emptyScore(),
    repeats: [],
    animation: noAnimation(),
    drums: noDrums(),
    critic: { off: [], suppress: [] },
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
  if (a.transport.transpose !== b.transport.transpose) out.push(`transpose → ${semitonesText(b.transport.transpose)}`);

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
    if (old.arp.on !== c.arp.on) out.push(`channel "${c.id}" arpeggiator ${c.arp.on ? "on" : "off"}`);
    else if (c.arp.on && JSON.stringify(old.arp) !== JSON.stringify(c.arp)) out.push(`channel "${c.id}" arpeggiator changed`);
    if (old.layerOf !== c.layerOf) out.push(c.layerOf === "" ? `channel "${c.id}" is no longer a layer` : `channel "${c.id}" layers "${c.layerOf}"`);
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
        `pattern "${p.id}": edited ${added.length} note${added.length > 1 ? "s" : ""} (${noteText(removed[0])} → ${noteText(added[0])}${added.length > 1 ? ", …" : ""})`
      );
    } else {
      if (added.length > 0)
        out.push(
          `pattern "${p.id}": +${added.length} note${added.length > 1 ? "s" : ""} on "${added[0].channel}" (${noteText(added[0])}${added.length > 1 ? ", …" : ""})`
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
  scoreDiff(a.score, b.score, out);
  repeatsDiff(a.repeats, b.repeats, out);
  filmDiff(a.animation, b.animation, out);
  return out.slice(0, 8);
}

/** function repeatText(r: Repeat) => String */
function repeatText(r) {
  const ends = r.endings.length > 0 ? ` with ${plural(r.endings.length, "ending")}` : "";
  return `beats ${fmtNum(r.start)}–${fmtNum(r.end)} ×${r.times}${ends}`;
}

/** function repeatsDiff(a: Repeat[], b: Repeat[], out: String[]) => Undefined */
function repeatsDiff(a, b, out) {
  const ka = a.map((r) => JSON.stringify(r));
  const kb = b.map((r) => JSON.stringify(r));
  for (let i = 0; i < b.length; i++) {
    if (ka.includes(kb[i])) continue;
    const old = a.find((r) => Math.abs(r.start - b[i].start) < 1e-6);
    out.push(old ? `repeat at beat ${fmtNum(b[i].start)} → ${repeatText(b[i])}` : `added a repeat: ${repeatText(b[i])}`);
  }
  for (const r of a) {
    if (!b.some((x) => Math.abs(x.start - r.start) < 1e-6)) out.push(`removed the repeat at beat ${fmtNum(r.start)}`);
  }
}

/** function filmDiff(a: Animation, b: Animation, out: String[]) => Undefined */
function filmDiff(a, b, out) {
  if (a.mode !== b.mode) out.push(`film: ${b.mode} mode`);
  if (a.surface !== b.surface) out.push(`film: pages on ${b.surface}`);
  if (Math.abs(a.energy - b.energy) > 1e-9) out.push(`film: energy → ${fmtNum(b.energy)}`);
  const ka = a.shots.map((x) => JSON.stringify(encodeShot(x)));
  const kb = b.shots.map((x) => JSON.stringify(encodeShot(x)));
  const added = kb.filter((k) => !ka.includes(k)).length;
  const gone = ka.filter((k) => !kb.includes(k)).length;
  if (added > 0 && gone > 0 && a.shots.length === b.shots.length) out.push(`film: changed ${plural(added, "shot")}`);
  else {
    if (added > 0) out.push(`film: added ${plural(added, "shot")}`);
    if (gone > 0) out.push(`film: removed ${plural(gone, "shot")}`);
  }
  const ea = JSON.stringify(a.effects);
  if (ea !== JSON.stringify(b.effects)) out.push("film: effects changed");
}

/** function scoreDiff(a: ScoreSettings, b: ScoreSettings, out: String[]) => Undefined */
function scoreDiff(a, b, out) {
  if (a.key !== b.key) out.push(`score: key → ${b.key === "" ? "auto" : b.key}`);
  for (const id of b.hidden) if (!a.hidden.includes(id)) out.push(`score: hid "${id}"`);
  for (const id of a.hidden) if (!b.hidden.includes(id)) out.push(`score: showed "${id}"`);
  const ta = a.hiddenTracks.map(trackIndex);
  const tb = b.hiddenTracks.map(trackIndex);
  for (const t of tb) if (!ta.includes(t)) out.push(`score: left out track ${t}`);
  for (const t of ta) if (!tb.includes(t)) out.push(`score: brought back track ${t}`);
  for (const c of b.clefs) {
    const old = a.clefs.find((x) => x.key === c.key);
    if (!old || old.value !== c.value) out.push(`score: "${c.key}" clef → ${c.value}`);
  }
  /** const key: (ScoreMark) => String */
  const key = (m) => `${m.pattern}|${m.start}|${m.end}|${m.color}|${m.label}|${m.channels.join(",")}`;
  const ka = a.marks.map(key);
  const kb = b.marks.map(key);
  for (const m of b.marks) {
    if (ka.includes(key(m))) continue;
    const where = m.pattern !== "" ? `pattern "${m.pattern}" beats` : "beats";
    out.push(`score: colored ${where} ${fmtNum(m.start)}–${fmtNum(m.end)} ${m.color}${m.label !== "" ? ` "${m.label}"` : ""}`);
  }
  const gone = a.marks.filter((m) => !kb.includes(key(m))).length;
  if (gone > 0) out.push(`score: removed ${plural(gone, "colored passage")}`);
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

/** Semitones as shown: "+2", "−3", "0". */
/** function semitonesText(n: Int) => String */
export function semitonesText(n) {
  return n > 0 ? `+${n}` : n < 0 ? `−${-n}` : "0";
}
