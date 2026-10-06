// The Voice dock (F8): sing, hum, whistle or beatbox into the microphone and
// get notes. A melody take becomes a piano-roll pattern (quantized, snapped
// to a scale); a beatbox take becomes a drum loop on kick, tom, snare, hat
// and open-hat channels. It is a flow of three steps:
//  1. Take — record, open or pick a recording; the server analyzes it
//     (GET /api/transcribe);
//  2. Shape — crop the take, and the settings re-shape the result instantly
//     (../voice.js), as many rounds as it takes, heard with Play before
//     anything changes;
//  3. Add to song — a pattern and a playlist clip (one undo step).

import { getJson, drag, fmt, now, recStart, recStop, previewAudio, stopPreview, pickFiles, uploadFile, audioPost } from "#platform";
import { state, commit, invalidate, hint, selectPattern, currentChannel, currentPattern, engineJson } from "../store.js";
import { uniqueId, paletteColor, setOption, optionValue, snapDown, newDevice, cloneProject, projectJson, noArp, noPatternDrums } from "../model.js";
import { startAudio, play, stop, setMode } from "../audio.js";
import {
  SCALES,
  KEY_NAMES,
  DRUMS,
  scaleLabel,
  scaleSteps,
  resolveKey,
  melodyNotes,
  drumHits,
  loopBeats,
  strengthNeeded,
  takeStart,
  DETAILS,
  nextDrum,
  drumLabel,
  decodeTake,
  emptyTake,
  cropTake,
} from "../voice.js";
import { button, iconButton, select, glyph, clamp01 } from "./widgets.js";
import { pushChannel } from "./instruments.js";
import { toast } from "./toast.js";
import { insertIx, trackIx, trackIndex } from "#brands";
import { t, tf, tk } from "../i18n.js";

/** Longest take (s): the analysis stays quick. */
const MAX_TAKE = 90;

/** type VoiceResult = { notes: Placed[], length: Number } */

export const voice = {
  /** "melody" (sing, hum, whistle) or "drums" (beatbox). */
  mode: "melody",
  /** "idle", "recording" or "analyzing". */
  status: "idle",
  recStarted: 0,
  /** "off" (sing alone), "pattern" or "song" (sing along). */
  playAlong: "off",
  /** Sung alone: the first note starts the pattern. Sung along: the take keeps its place. */
  aligned: true,
  /** Where the take began within its bar (beats), when sung along. */
  origin: 0,
  /** The song beat the result goes to (sung along the song), or -1 for the playhead. */
  at: -1,
  /** The analyzed take (a project path), "" before the first one. */
  path: "",
  take /*: Take */: emptyTake(),
  /** The part of the take kept (seconds): -1 = from its first note or hit
   * (sung alone) or its start (sung along); -1 = to its end. Once the
   * start is set, the result's time starts there. */
  cropStart /*: Number */: -1,
  cropEnd /*: Number */: -1,
  /** Per hit: a drum chosen by clicking it, "" = as heard. */
  kinds /*: String[] */: [],
  /** "auto" (the selected channel if melodic, else a new one), "new" or a channel id. */
  melodyChannel: "auto",
  /** Per drum: a channel id, "" = the first matching drum machine channel (or a new one). */
  drumChannels /*: KS[] */: [
    { key: "kick", value: "" },
    { key: "tom", value: "" },
    { key: "snare", value: "" },
    { key: "hat", value: "" },
    { key: "openhat", value: "" },
  ],
  repeat: 1,
  /** The take is playing (Listen). */
  playing: false,
  /** The shaped result is playing (Play), and what was sent for it. */
  previewing: false,
  previewKey: "",
  previewAt: 0,
  settings /*: VoiceSettings */: {
    detail: 2,
    grid: 0.25,
    strength: 1,
    lengths: true,
    key: -1,
    scale: "major",
    octave: 0,
    legato: false,
    dynamics: true,
    sensitivity: 0.5,
    separation: 0.03,
    bars: 0,
  },
};

// ------------------------------------------------------------------ result

/** type Shaped = { take: Take, origin: Number, aligned: Boolean } */

/** The kept part of the take (seconds). */
/** function cropSpan() => Number[] */
function cropSpan() {
  const d = voice.take.duration;
  const a = voice.cropStart < 0 ? 0 : Math.min(voice.cropStart, d);
  const z = voice.cropEnd < 0 ? d : Math.max(a, Math.min(voice.cropEnd, d));
  return [a, z];
}

/** The cropped take and where it goes: once the crop's start is set, time
 * starts there (sung along, it keeps its place against the song). */
/** function shaped() => Shaped */
function shaped() {
  const span = cropSpan();
  const take = cropTake(voice.take, span[0], span[1]);
  const started = voice.cropStart >= 0;
  const bps = state.project.transport.bpm / 60;
  return {
    take: take,
    origin: voice.aligned ? voice.origin : voice.origin + span[0] * bps,
    aligned: voice.aligned && !started,
  };
}

/** The notes the settings make of the take, and the pattern length. */
/** function voiceResult() => VoiceResult */
export function voiceResult() {
  const sh = shaped();
  const t = sh.take;
  const p = state.project;
  const bpm = p.transport.bpm;
  const s = voice.settings;
  /** const empty: Placed[] */
  const empty = [];
  const all =
    t.mode === "drums" ? drumHits(t, s, bpm, sh.origin, sh.aligned, voice.kinds) : t.mode === "melody" ? melodyNotes(t, s, bpm, sh.origin, sh.aligned) : empty;
  const length = loopBeats(all, p.transport.beatsPerBar, s.bars);
  /** const notes: Placed[] */
  const notes = [];
  for (const n of all) {
    if (n.start >= length - 1e-6) continue;
    if (n.start + n.length > length) n.length = Math.round((length - n.start) * 10000) / 10000;
    notes.push(n);
  }
  return { notes: notes, length: length };
}

// ------------------------------------------------------------------ takes

/** function analyze(path: String) => Undefined */
function analyze(path) {
  stopResult();
  const mode = voice.mode;
  // A new recording is kept whole (the same one read again keeps its crop).
  if (voice.path !== path) {
    voice.cropStart = -1;
    voice.cropEnd = -1;
  }
  voice.path = path;
  voice.status = "analyzing";
  invalidate();
  getJson(`/api/transcribe?path=${encodeURIComponent(path)}&mode=${mode}`)
    .then((r) => {
      // A newer take or mode won.
      if (voice.path !== path || voice.mode !== mode) return false;
      voice.take = decodeTake(r);
      voice.kinds = [];
      voice.status = "idle";
      const found = voice.take.mode === "drums" ? voice.take.hits.length : voice.take.notes.length;
      if (found === 0) toast(mode === "drums" ? t("voice.analyze.noHits.toast.title") : t("voice.analyze.noNotes.toast.title"), t("voice.analyze.nothingFound.toast.body"), "info");
      invalidate();
      return true;
    })
    .catch((e) => {
      if (voice.path === path) voice.status = "idle";
      toast(t("voice.analyze.failed.toast.title"), String(e), "error");
      invalidate();
      return false;
    });
}

/** Keep the timer moving while recording; stop at the limit. */
function recTick() {
  if (voice.status !== "recording") return undefined;
  if ((now() - voice.recStarted) / 1000 >= MAX_TAKE) {
    stopTake();
    return undefined;
  }
  invalidate();
  setTimeout(recTick, 250);
}

/** Record a take from the microphone (optionally over the song or pattern). */
export async function startTake() {
  if (voice.status === "recording") {
    stopTake();
    return false;
  }
  if (voice.status === "analyzing") return false;
  if (state.recording) {
    toast(t("voice.record.alreadyRecording.toast.title"), t("voice.record.alreadyRecording.toast.body"), "error");
    return false;
  }
  stopPreview();
  voice.playing = false;
  stopResult();
  await startAudio();
  const ok = await recStart().catch((e) => false);
  if (!ok) {
    toast(t("audio.micUnavailable.title"), t("audio.micUnavailable.body"), "error");
    return false;
  }
  const bpb = state.project.transport.beatsPerBar;
  voice.aligned = voice.playAlong === "off";
  voice.origin = 0;
  voice.at = -1;
  if (voice.playAlong !== "off") {
    if (state.playing) stop();
    setMode(voice.playAlong);
    const pos = state.position;
    voice.origin = pos - snapDown(pos, bpb);
    if (voice.playAlong === "song") voice.at = snapDown(pos, bpb);
    play();
  }
  voice.status = "recording";
  voice.recStarted = now();
  recTick();
  invalidate();
  return true;
}

export function stopTake() {
  if (voice.status !== "recording") return undefined;
  voice.status = "analyzing";
  if (voice.playAlong !== "off") stop();
  invalidate();
  const name = voice.mode === "drums" ? "beatbox" : "voice";
  recStop(`${name}-${Date.now()}.wav`)
    .then((path) => {
      if (path === "") {
        voice.status = "idle";
        toast(t("voice.record.nothingRecorded.toast.title"), t("voice.record.nothingRecorded.toast.body"), "error");
        invalidate();
        return false;
      }
      analyze(path);
      return true;
    })
    .catch((e) => {
      voice.status = "idle";
      toast(t("voice.record.saveFailed.toast.title"), String(e), "error");
      invalidate();
      return false;
    });
}

/** function setVoiceMode(mode: String) => Undefined */
export function setVoiceMode(mode) {
  if (voice.mode === mode) return undefined;
  voice.mode = mode;
  // The same take, read the other way.
  if (voice.path !== "" && voice.status === "idle") analyze(voice.path);
  invalidate();
}

/** Pick an earlier take (any sample of the project). */
/** function pickTake(path: String) => Undefined */
function pickTake(path) {
  if (path === "" || voice.status !== "idle") return undefined;
  voice.aligned = true;
  voice.origin = 0;
  voice.at = -1;
  analyze(path);
}

/** Listen to the take (again: stop). */
function listenTake() {
  if (voice.playing) {
    stopPreview();
    voice.playing = false;
    return undefined;
  }
  if (voice.path === "") return undefined;
  stopResult();
  voice.playing = true;
  previewAudio(
    `/files/${voice.path
      .split("/")
      .map((x) => encodeURIComponent(x))
      .join("/")}`,
    () => {
      voice.playing = false;
      invalidate();
    }
  );
}

/** Analyze the current take again (after a change of mind, or an update). */
function analyzeAgain() {
  if (voice.path === "" || voice.status !== "idle") return undefined;
  analyze(voice.path);
}

/** Add an audio file from the device to the project's samples and analyze it. */
function openRecording() {
  if (voice.status !== "idle") return undefined;
  pickFiles("audio/*", (files) => {
    if (files.length === 0) return undefined;
    const f = files[0];
    voice.status = "analyzing";
    invalidate();
    uploadFile(`/api/samples?name=${encodeURIComponent(f.name)}`, f)
      .then((r) => {
        voice.status = "idle";
        voice.aligned = true;
        voice.origin = 0;
        voice.at = -1;
        analyze(String(r.path));
        return true;
      })
      .catch((e) => {
        voice.status = "idle";
        toast(t("voice.open.failed.toast.title"), String(e), "error");
        invalidate();
        return false;
      });
  });
}

// ------------------------------------------------------------------ channels

/** function channelExists(id: String) => Boolean */
function channelExists(id) {
  return state.project.channels.some((c) => c.id === id);
}

/** A drum machine channel playing `kind` (a snare also takes a clap, a hat a shaker). */
/** function drumChannel(kind: String) => String */
function drumChannel(kind) {
  const also = kind === "snare" ? "clap" : kind === "hat" ? "shaker" : kind;
  for (const want of [kind, also]) {
    for (const c of state.project.channels) {
      if (c.instrument.type !== "drum") continue;
      const k = optionValue(c.instrument, "kind");
      if (k === want || (k === "" && want === "kick")) return c.id;
    }
  }
  return "";
}

/** function channelName(id: String) => String */
function channelName(id) {
  for (const c of state.project.channels) {
    if (c.id === id) return c.name;
  }
  return id;
}

/** function chosenDrum(kind: String) => String */
function chosenDrum(kind) {
  for (const e of voice.drumChannels) {
    if (e.key === kind) return e.value;
  }
  return "";
}

/** The melody's channel: "" means a new one. */
/** function melodyTarget() => String */
function melodyTarget() {
  const m = voice.melodyChannel;
  if (m === "new") return "";
  if (m !== "auto") return channelExists(m) ? m : "";
  return melodicSelection();
}

/** The selected channel when it is not a drum, else "". */
/** function melodicSelection() => String */
function melodicSelection() {
  const cur = currentChannel();
  if (!cur) return "";
  return cur.instrument.type === "drum" ? "" : cur.id;
}

/** function laneValue(lanes: KS[], lane: String) => String */
function laneValue(lanes, lane) {
  for (const l of lanes) {
    if (l.key === lane) return l.value;
  }
  return "";
}

/** The existing channel a lane goes to, "" when it needs a new one. */
/** function existingLaneChannel(lane: String) => String */
function existingLaneChannel(lane) {
  if (lane === "melody") return melodyTarget();
  const chosen = chosenDrum(lane);
  if (chosen !== "" && channelExists(chosen)) return chosen;
  return drumChannel(lane);
}

/** A drum's texts, in English (t() translates them where shown): its name,
 * the tip of its channel choice, and its hit count for one and for several. */
/** type DrumText = { kind: String, name: String, tip: String, one: String, many: String } */

/** const DRUM_TEXTS: DrumText[] */
const DRUM_TEXTS = [
  { kind: "kick", name: tk("drum.kick"), tip: tk("voice.target.kick.title"), one: tk("voice.summary.kick.one"), many: tk("voice.summary.kick.other") },
  { kind: "tom", name: tk("voice.drum.tom.label"), tip: tk("voice.target.tom.title"), one: tk("voice.summary.tom.one"), many: tk("voice.summary.tom.other") },
  { kind: "snare", name: tk("drum.snare"), tip: tk("voice.target.snare.title"), one: tk("voice.summary.snare.one"), many: tk("voice.summary.snare.other") },
  { kind: "hat", name: tk("voice.drum.hat.label"), tip: tk("voice.target.hat.title"), one: tk("voice.summary.hat.one"), many: tk("voice.summary.hat.other") },
  { kind: "openhat", name: tk("drum.openHat"), tip: tk("voice.target.openHat.title"), one: tk("voice.summary.openHat.one"), many: tk("voice.summary.openHat.other") },
];

/** function drumText(kind: String) => DrumText */
function drumText(kind) {
  for (const d of DRUM_TEXTS) {
    if (d.kind === kind) return d;
  }
  const l = drumLabel(kind);
  return { kind: kind, name: l, tip: l, one: l, many: l };
}

/** A drum's name as shown (translated). */
/** function drumTitle(kind: String) => String */
function drumTitle(kind) {
  return t(drumText(kind).name);
}

/** A drum's name for a new channel (project data: not translated). */
/** function drumName(kind: String) => String */
function drumName(kind) {
  const l = drumLabel(kind);
  return l.slice(0, 1).toUpperCase() + l.slice(1);
}

/** The channel for a lane, created if needed (call inside `commit`). */
/** function laneChannel(lane: String) => String */
function laneChannel(lane) {
  const id = existingLaneChannel(lane);
  if (id !== "") return id;
  if (lane === "melody") return pushChannel("analog", "Voice", (d) => undefined);
  return pushChannel("drum", drumName(lane), (d) => setOption(d, "kind", lane));
}

// ------------------------------------------------------------------ listening

/** The pattern the result plays as while shaping it. */
const PREVIEW = "voice-preview";

/** What the preview plays: the notes and where they go (to notice changes). */
/** function previewKey(r: VoiceResult) => String */
function previewKey(r) {
  const lanes = DRUMS.concat(["melody"])
    .map((l) => existingLaneChannel(l))
    .join(",");
  const notes = r.notes.map((n) => `${n.lane}:${n.pitch}:${n.start}:${n.length}:${n.velocity}`).join(" ");
  return `${r.length}|${lanes}|${notes}`;
}

/** The song as it is plus the result as a pattern (and any channel it still
 * needs), for the engine to play; the project itself is left alone. */
/** function previewJson(r: VoiceResult) => String */
function previewJson(r) {
  const p = cloneProject(state.project);
  const drums = voice.take.mode === "drums";
  /** const lanes: KS[] */
  const lanes = [];
  for (const lane of drums ? DRUMS : ["melody"]) {
    let id = existingLaneChannel(lane);
    if (id === "") {
      id = `${PREVIEW}-${lane}`;
      const dev = newDevice(drums ? "drum" : "analog");
      if (drums) setOption(dev, "kind", lane);
      p.channels.push({ id: id, name: id, color: "#d4af37", instrument: dev, volume: 0.8, pan: 0, mute: false, mixer: insertIx(0), arp: noArp(), layerOf: "" });
    }
    lanes.push({ key: lane, value: id });
  }
  p.patterns.push({
    id: PREVIEW,
    name: "Voice preview",
    color: "#d4af37",
    length: r.length,
    notes: r.notes.map((n) => {
      return { channel: laneValue(lanes, n.lane), pitch: n.pitch, start: n.start, length: n.length, velocity: n.velocity };
    }),
    drums: noPatternDrums(),
  });
  return projectJson(p);
}

/** Send the result to the engine (again, when the settings changed it). */
/** function sendPreview(r: VoiceResult) => Undefined */
function sendPreview(r) {
  const key = previewKey(r);
  if (key === voice.previewKey) return undefined;
  voice.previewKey = key;
  audioPost({ t: "project", json: previewJson(r) });
}

/** Play the shaped result, looping, in the browser engine. */
export async function playResult() {
  const r = voiceResult();
  if (r.notes.length === 0) return false;
  stopPreview();
  voice.playing = false;
  await startAudio();
  voice.previewKey = "";
  sendPreview(r);
  audioPost({ t: "mode", pattern: PREVIEW });
  audioPost({ t: "seek", beat: 0 });
  audioPost({ t: "play" });
  voice.previewing = true;
  voice.previewAt = now();
  previewTick();
  invalidate();
  return true;
}

/** Stop the result and give the engine the song back. */
export function stopResult() {
  if (!voice.previewing) return undefined;
  voice.previewing = false;
  voice.previewKey = "";
  audioPost({ t: "stop" });
  audioPost({ t: "project", json: engineJson() });
  const pat = currentPattern();
  audioPost({ t: "mode", pattern: state.mode === "pattern" && pat ? pat.id : "" });
  invalidate();
}

/** While the result plays: follow the settings, and notice the transport
 * stopping it. */
function previewTick() {
  if (!voice.previewing) return undefined;
  if (state.output === "browser" && !state.playing && now() - voice.previewAt > 800) {
    stopResult();
    return undefined;
  }
  sendPreview(voiceResult());
  invalidate();
  setTimeout(previewTick, 120);
}

// ------------------------------------------------------------------ insert

/** A playlist track with room between two beats: the selected one, else the
 * first free one, else a new track (call inside `commit`). */
/** function freeTrack(lo: Number, hi: Number, name: String) => TrackIx */
function freeTrack(lo, hi, name) {
  const p = state.project;
  const busy = (i) => p.playlist.clips.some((c) => trackIndex(c.track) === i && c.start < hi - 1e-6 && c.start + c.length > lo + 1e-6);
  const sel = trackIndex(state.track);
  if (sel < p.playlist.tracks.length && !busy(sel)) return state.track;
  for (let i = 0; i < p.playlist.tracks.length; i++) {
    if (!busy(i)) return trackIx(i);
  }
  p.playlist.tracks.push({ name: name, mute: false });
  return trackIx(p.playlist.tracks.length - 1);
}

/** Add the result as a new pattern with a clip on the playlist. */
export function addToSong() {
  stopResult();
  const r = voiceResult();
  if (r.notes.length === 0) {
    toast(t("voice.addToSong.nothing.toast.title"), voice.path === "" ? t("voice.addToSong.noTake.toast.body") : t("voice.addToSong.noNotes.toast.body"), "error");
    return undefined;
  }
  const p = state.project;
  const drums = voice.take.mode === "drums";
  const bpb = p.transport.beatsPerBar;
  let id = "";
  let name = "";
  let start = 0;
  commit(() => {
    /** const lanes: KS[] */
    const lanes = [];
    for (const lane of drums ? DRUMS : ["melody"]) {
      if (r.notes.some((n) => n.lane === lane)) lanes.push({ key: lane, value: laneChannel(lane) });
    }
    const base = drums ? "Beatbox" : "Voice";
    id = uniqueId(
      base.toLowerCase(),
      p.patterns.map((x) => x.id)
    );
    let k = 1;
    while (p.patterns.some((x) => x.name === `${base} ${k}`)) k = k + 1;
    name = `${base} ${k}`;
    p.patterns.push({
      id: id,
      name: name,
      color: paletteColor(p.patterns.length + 2),
      length: r.length,
      notes: r.notes.map((n) => {
        return { channel: laneValue(lanes, n.lane), pitch: n.pitch, start: n.start, length: n.length, velocity: Math.round(n.velocity * 1000) / 1000 };
      }),
      drums: noPatternDrums(),
    });
    start = voice.at >= 0 ? voice.at : snapDown(state.position, bpb);
    const length = r.length * voice.repeat;
    const track = freeTrack(start, start + length, base);
    p.playlist.clips.push({ pattern: id, sample: "", track: track, start: start, length: length, offset: 0, gain: 1, mixer: insertIx(0) });
  });
  selectPattern(id);
  const bar = Math.floor(start / bpb) + 1;
  const count = String(r.notes.length);
  const at = String(bar);
  const times = String(voice.repeat);
  let body = "";
  if (drums) {
    body =
      voice.repeat > 1
        ? tf("voice.addToSong.done.toast.body.hitsLooped", [count, at, times])
        : tf("voice.addToSong.done.toast.body.hits", [count, at]);
  } else {
    body =
      voice.repeat > 1
        ? tf("voice.addToSong.done.toast.body.notesLooped", [count, at, times])
        : tf("voice.addToSong.done.toast.body.notes", [count, at]);
  }
  toast(tf("voice.addToSong.done.toast.title", [name]), body, "info");
}

// ------------------------------------------------------------------ controls

/** A knob for a panel setting (not a project value: no undo step). */
/** function dial(b: Builder, key: String, label: String, text: String, v: Number, tip: String, onSet: (Number) => Undefined) => Undefined */
function dial(b, key, label, text, v, tip, onSet) {
  b.open("div", key, "param");
  b.open("div", "k", "knob");
  b.style("--v", fmt(clamp01(v), 4));
  b.attr("title", tip);
  b.on("pointerenter", (e) => hint(tip));
  b.on("pointerdown", (e) => {
    e.preventDefault();
    const y0 = e.clientY;
    const v0 = v;
    drag(
      e,
      (m) => {
        onSet(clamp01(v0 + (y0 - m.clientY) / (m.shiftKey ? 900 : 180)));
        invalidate();
      },
      (u) => undefined
    );
  });
  b.on("wheel", (e) => {
    e.preventDefault();
    onSet(clamp01(v - e.deltaY / 2000));
    invalidate();
  });
  b.leaf("div", "ring", "knob-ring", "");
  b.open("div", "cap", "knob-cap");
  b.leaf("div", "dot", "knob-dot", "");
  b.close();
  b.close();
  b.leaf("div", "name", "param-name", label);
  b.leaf("div", "val", "param-value", text);
  b.close();
}

/** A labelled dropdown. */
/** function choice(b: Builder, key: String, label: String, value: String, choices: String[], labels: String[], tip: String, onSet: (String) => Undefined) => Undefined */
function choice(b, key, label, value, choices, labels, tip, onSet) {
  b.open("label", key, "voice-field");
  b.leaf("span", "l", "voice-label", label);
  select(b, "s", "", value, choices, labels, tip, (v) => {
    onSet(v);
    invalidate();
  });
  b.close();
}

/** An on/off switch. */
/** function toggle(b: Builder, key: String, label: String, on: Boolean, tip: String, onSet: (Boolean) => Undefined) => Undefined */
function toggle(b, key, label, on, tip, onSet) {
  button(b, key, on ? "small on" : "small", label, tip, () => {
    onSet(!on);
    invalidate();
  });
}

/** The Separation dial's range (seconds). */
const MAX_SEPARATION = 0.25;

const GRIDS = [0, 0.125, 0.25, 0.5, 1, 1 / 6, 1 / 3];
const GRID_LABELS = [tk("common.off"), "1/32", "1/16", "1/8", tk("term.beat"), "1/16 T", "1/8 T"];

/** The names of the Detail levels (DETAILS in ../voice.js, in order). */
const DETAIL_NAMES = [tk("voice.settings.detail.option.smooth"), tk("voice.settings.detail.option.clean"), tk("voice.settings.detail.option.balanced"), tk("voice.settings.detail.option.detailed"), tk("voice.settings.detail.option.everyNote")];

/** A scale's name as shown (SCALES in ../voice.js). */
/** function scaleName(id: String) => String */
function scaleName(id) {
  if (id === "major") return t("voice.settings.scale.option.major");
  if (id === "minor") return t("voice.settings.scale.option.minor");
  if (id === "harmonic") return t("voice.settings.scale.option.harmonic");
  if (id === "dorian") return t("voice.settings.scale.option.dorian");
  if (id === "mixolydian") return t("voice.settings.scale.option.mixolydian");
  if (id === "penta") return t("voice.settings.scale.option.penta");
  if (id === "minpenta") return t("voice.settings.scale.option.minpenta");
  if (id === "blues") return t("voice.settings.scale.option.blues");
  if (id === "chromatic") return t("voice.settings.scale.option.chromatic");
  return scaleLabel(id);
}

/** A key and scale in the summary ("C♯ major"). */
/** function keyText(key: String, scale: String) => String */
function keyText(key, scale) {
  if (scale === "chromatic") return t("term.chromatic");
  if (scale === "major") return tf("voice.summary.key.major", [key]);
  if (scale === "minor") return tf("voice.summary.key.minor", [key]);
  if (scale === "harmonic") return tf("voice.summary.key.harmonic", [key]);
  if (scale === "dorian") return tf("voice.summary.key.dorian", [key]);
  if (scale === "mixolydian") return tf("voice.summary.key.mixolydian", [key]);
  if (scale === "penta") return tf("voice.summary.key.penta", [key]);
  if (scale === "minpenta") return tf("voice.summary.key.minpenta", [key]);
  if (scale === "blues") return tf("voice.summary.key.blues", [key]);
  return `${key} ${scaleLabel(scale).toLowerCase()}`;
}

/** The settings of step 2, in three groups: what is detected, when the
 * notes fall, and (melody) which notes they are. */
/** function settingsView(b: Builder) => Undefined */
function settingsView(b) {
  const s = voice.settings;
  const drums = voice.mode === "drums";
  b.open("div", "set", "voice-settings");

  b.open("div", "detect", "voice-group");
  b.leaf("div", "t", "voice-group-title", t("voice.settings.detection.label"));
  b.open("div", "row", "voice-row");
  if (!drums) {
    const last = DETAILS.length - 1;
    dial(
      b,
      "detail",
      t("voice.settings.detail.label"),
      s.detail >= 0 && s.detail < DETAIL_NAMES.length ? t(DETAIL_NAMES[s.detail]) : "",
      s.detail / last,
      t("voice.settings.detail.title"),
      (v) => {
        s.detail = Math.round(v * last);
      }
    );
    toggle(b, "dyn", t("voice.settings.dynamics.label"), s.dynamics, t("voice.settings.dynamics.title"), (v) => {
      s.dynamics = v;
    });
  } else {
    dial(b, "sens", t("voice.settings.sensitivity.label"), `${Math.round(s.sensitivity * 100)}%`, s.sensitivity, t("voice.settings.sensitivity.title"), (v) => {
      s.sensitivity = Math.round(v * 50) / 50;
    });
    dial(
      b,
      "sep",
      t("voice.settings.separation.label"),
      s.separation > 0 ? `${Math.round(s.separation * 1000)} ms` : t("common.off"),
      s.separation / MAX_SEPARATION,
      t("voice.settings.separation.title"),
      (v) => {
        s.separation = Math.round(v * MAX_SEPARATION * 200) / 200;
      }
    );
    toggle(b, "dyn", t("voice.settings.accents.label"), s.dynamics, t("voice.settings.accents.title"), (v) => {
      s.dynamics = v;
    });
  }
  b.close();
  b.close();

  b.open("div", "q", "voice-group");
  b.leaf("div", "t", "voice-group-title", t("voice.settings.timing.label"));
  b.open("div", "row", "voice-row");
  choice(
    b,
    "grid",
    t("term.grid"),
    String(s.grid),
    GRIDS.map((g) => String(g)),
    GRID_LABELS.map((l) => t(l)),
    t("voice.settings.grid.title"),
    (v) => {
      s.grid = Number(v);
    }
  );
  dial(b, "str", t("voice.settings.strength.label"), `${Math.round(s.strength * 100)}%`, s.strength, t("voice.settings.strength.title"), (v) => {
    s.strength = Math.round(v * 20) / 20;
  });
  if (!drums) {
    toggle(b, "ends", t("voice.settings.ends.label"), s.lengths, t("voice.settings.ends.title"), (v) => {
      s.lengths = v;
    });
    toggle(b, "legato", t("voice.settings.legato.label"), s.legato, t("voice.settings.legato.title"), (v) => {
      s.legato = v;
    });
  }
  choice(
    b,
    "bars",
    t("term.length"),
    String(s.bars),
    ["0", "1", "2", "4", "8"],
    [t("common.auto"), t("format.barsOne"), t("voice.settings.length.option.2"), t("voice.settings.length.option.4"), t("voice.settings.length.option.8")],
    t("voice.settings.length.title"),
    (v) => {
      s.bars = Math.round(Number(v));
    }
  );
  b.close();
  b.close();

  if (!drums) {
    const cropped = shaped().take;
    const k = resolveKey(cropped, s);
    b.open("div", "tune", "voice-group");
    b.leaf("div", "t", "voice-group-title", t("voice.settings.pitch.label"));
    b.open("div", "row", "voice-row");
    /** const keys: String[] */
    const keys = ["-1"];
    /** const keyLabels: String[] */
    const keyLabels = [cropped.notes.length > 0 && s.key < 0 ? tf("voice.settings.key.option.detectFound", [KEY_NAMES[k.key]]) : t("voice.settings.key.option.detect")];
    for (let i = 0; i < 12; i++) {
      keys.push(String(i));
      keyLabels.push(KEY_NAMES[i]);
    }
    choice(b, "key", t("term.key"), String(s.key), keys, keyLabels, t("voice.settings.key.title"), (v) => {
      s.key = Math.round(Number(v));
    });
    choice(
      b,
      "scale",
      t("voice.settings.scale.label"),
      s.scale,
      SCALES.map((x) => x.id),
      SCALES.map((x) => scaleName(x.id)),
      t("voice.settings.scale.title"),
      (v) => {
        s.scale = v;
      }
    );
    choice(b, "oct", t("voice.settings.octave.label"), String(s.octave), ["-2", "-1", "0", "1", "2"], ["−2", "−1", "0", "+1", "+2"], t("voice.settings.octave.title"), (v) => {
      s.octave = Math.round(Number(v));
    });
    b.close();
    b.close();
  }
  b.close();
}

/** Where the result goes: channels (for a drum loop, of the drums in it)
 * and loop count. */
/** function targetView(b: Builder, r: VoiceResult) => Undefined */
function targetView(b, r) {
  const drums = voice.mode === "drums";
  const chs = state.project.channels;
  b.open("div", "target", "voice-target");
  if (!drums) {
    /** const ids: String[] */
    const ids = ["auto", "new"];
    const sel = melodicSelection();
    /** const names: String[] */
    const names = [sel !== "" ? tf("voice.target.channel.option.selected", [channelName(sel)]) : t("voice.target.channel.option.selectedNew"), t("voice.target.channel.option.new")];
    for (const c of chs) {
      if (c.instrument.type === "drum") continue;
      ids.push(c.id);
      names.push(c.name);
    }
    choice(b, "ch", t("term.channel"), voice.melodyChannel, ids, names, t("voice.target.channel.title"), (v) => {
      voice.melodyChannel = v;
    });
  } else {
    const used = voice.drumChannels.filter((e) => r.notes.some((n) => n.lane === e.key));
    for (const e of used.length > 0 ? used : voice.drumChannels) {
      const found = drumChannel(e.key);
      /** const ids: String[] */
      const ids = [""];
      /** const names: String[] */
      const names = [found !== "" ? tf("voice.target.drum.option.auto", [channelName(found)]) : t("voice.target.drum.option.autoNew")];
      for (const c of chs) {
        ids.push(c.id);
        names.push(c.name);
      }
      choice(b, e.key, drumTitle(e.key), e.value, ids, names, t(drumText(e.key).tip), (v) => {
        e.value = v;
      });
    }
  }
  choice(b, "rep", t("voice.target.loop.label"), String(voice.repeat), ["1", "2", "4", "8"], ["×1", "×2", "×4", "×8"], t("voice.target.loop.title"), (v) => {
    voice.repeat = Math.round(Math.max(1, Number(v)));
  });
  b.close();
}

// ------------------------------------------------------------------ preview

const DRUM_COLORS = ["#d08a93", "#c9965a", "#46a58b", "#e8d5b0", "#8fb8d8"];

/** function drumColor(kind: String) => String */
function drumColor(kind) {
  const i = DRUMS.indexOf(kind);
  return i >= 0 ? DRUM_COLORS[i] : "#a39780";
}

/** The middle of a drum's row in the beatbox view (hats on top, kicks at
 * the bottom). */
/** function laneY(kind: String, h: Number) => Number */
function laneY(kind, h) {
  const i = Math.max(0, DRUMS.indexOf(kind));
  const row = h / DRUMS.length;
  return h - (i + 0.5) * row;
}

/** Beat lines (bars brighter). */
/** function paintGrid(g: Ctx, w: Number, h: Number, length: Number) => Undefined */
function paintGrid(g, w, h, length) {
  const bpb = state.project.transport.beatsPerBar;
  const grid = voice.settings.grid;
  const step = grid > 0 && (w / length) * grid >= 6 ? grid : 1;
  for (let i = 0; i * step <= length + 1e-6; i++) {
    const beat = i * step;
    const x = Math.round((beat / length) * w) + 0.5;
    const bar = Math.abs(beat / bpb - Math.round(beat / bpb)) < 1e-6;
    const onBeat = Math.abs(beat - Math.round(beat)) < 1e-6;
    g.fillStyle = bar ? "rgba(212, 175, 55, 0.28)" : onBeat ? "rgba(212, 175, 55, 0.12)" : "rgba(212, 175, 55, 0.05)";
    g.fillRect(x, 0, 1, h);
  }
}

/** function paintMelody(g: Ctx, w: Number, h: Number, r: VoiceResult) => Undefined */
function paintMelody(g, w, h, r) {
  const sh = shaped();
  const t = sh.take;
  const s = voice.settings;
  const shift = 12 * s.octave;
  const t0 = takeStart(t, s.detail, sh.aligned);
  const bps = state.project.transport.bpm / 60;
  let lo = 127;
  let hi = 0;
  for (const n of r.notes) {
    lo = Math.min(lo, n.pitch);
    hi = Math.max(hi, n.pitch);
  }
  for (const c of t.contour) {
    if (c <= 0) continue;
    lo = Math.min(lo, c + shift);
    hi = Math.max(hi, c + shift);
  }
  if (lo > hi) {
    lo = 60;
    hi = 72;
  }
  lo = Math.floor(lo) - 2;
  hi = Math.ceil(hi) + 2;
  if (hi - lo < 14) {
    const mid = (hi + lo) / 2;
    lo = Math.floor(mid - 7);
    hi = lo + 14;
  }
  const rowH = h / (hi - lo);
  const yOf = (pitch) => h - (pitch - lo) * rowH;
  // Scale rows: the notes auto-tune may land on.
  const k = resolveKey(t, s);
  const steps = scaleSteps(k.scale);
  for (let p = lo; p < hi; p++) {
    const pc = (((Math.round(p) - k.key) % 12) + 12) % 12;
    if (s.scale !== "chromatic" && steps.includes(pc)) {
      g.fillStyle = pc === 0 ? "rgba(212, 175, 55, 0.09)" : "rgba(212, 175, 55, 0.04)";
      g.fillRect(0, yOf(p + 1), w, rowH);
    }
  }
  paintGrid(g, w, h, r.length);
  const xOf = (beat) => (beat / r.length) * w;
  // The level under everything.
  g.fillStyle = "rgba(232, 213, 176, 0.06)";
  for (let i = 0; i < t.level.length; i++) {
    const x = xOf(sh.origin + (i * t.step - t0) * bps);
    if (x < 0 || x > w) continue;
    const lh = t.level[i] * h * 0.35;
    g.fillRect(x, h - lh, Math.max(1, xOf(t.step * bps)), lh);
  }
  // The notes.
  g.shadowColor = "rgba(212, 175, 55, 0.6)";
  g.shadowBlur = 8;
  for (const n of r.notes) {
    g.globalAlpha = 0.55 + 0.45 * n.velocity;
    g.fillStyle = "#d4af37";
    g.fillRect(xOf(n.start) + 1, yOf(n.pitch + 1) + 1, Math.max(3, xOf(n.length) - 2), Math.max(3, rowH - 2));
  }
  g.globalAlpha = 1;
  g.shadowBlur = 0;
  // What was sung, over the notes.
  g.strokeStyle = "rgba(246, 238, 221, 0.75)";
  g.lineWidth = 1.25;
  g.beginPath();
  let pen = false;
  for (let i = 0; i < t.contour.length; i++) {
    const c = t.contour[i];
    const x = xOf(sh.origin + (i * t.step - t0) * bps);
    if (c <= 0 || x < 0 || x > w) {
      pen = false;
      continue;
    }
    const y = yOf(c + shift + 0.5);
    if (pen) g.lineTo(x, y);
    else g.moveTo(x, y);
    pen = true;
  }
  g.stroke();
  // Octave labels on the left.
  g.fillStyle = "rgba(163, 151, 128, 0.8)";
  g.font = "10px JetBrains Mono, monospace";
  for (let p = lo; p < hi; p++) {
    if (((p % 12) + 12) % 12 === 0) g.fillText(`C${Math.floor(p / 12) - 1}`, 4, yOf(p) - 3);
  }
}

/** Positions of the kept hits in the beatbox view (for drawing and clicks). */
/** type HitDot = { x: Number, y: Number, rawX: Number, lane: String, src: Int, velocity: Number } */

/** function hitDots(r: VoiceResult, w: Number, h: Number) => HitDot[] */
function hitDots(r, w, h) {
  /** const out: HitDot[] */
  const out = [];
  for (const n of r.notes) {
    out.push({ x: (n.start / r.length) * w, y: laneY(n.lane, h), rawX: (n.raw / r.length) * w, lane: n.lane, src: n.src, velocity: n.velocity });
  }
  return out;
}

/** function paintDrums(g: Ctx, w: Number, h: Number, r: VoiceResult) => Undefined */
function paintDrums(g, w, h, r) {
  const s = voice.settings;
  // A row per drum.
  const row = h / DRUMS.length;
  for (let i = 0; i < DRUMS.length; i++) {
    if (i % 2 === 1) {
      g.fillStyle = "rgba(232, 213, 176, 0.03)";
      g.fillRect(0, h - (i + 1) * row, w, row);
    }
  }
  paintGrid(g, w, h, r.length);
  g.font = "600 9px Manrope, sans-serif";
  for (const kind of DRUMS) {
    g.fillStyle = drumColor(kind);
    g.globalAlpha = 0.8;
    g.fillText(drumTitle(kind).toUpperCase(), 6, laneY(kind, h) - row / 2 + 11);
  }
  g.globalAlpha = 1;
  // Hits the sensitivity drops, where they were played (as drumHits places them).
  const need = strengthNeeded(s.sensitivity);
  const sh = shaped();
  const t = sh.take;
  const bps = state.project.transport.bpm / 60;
  let t0 = 0;
  if (sh.aligned) {
    for (const hit of t.hits) {
      if (hit.strength >= need) {
        t0 = hit.time;
        break;
      }
    }
  }
  g.fillStyle = "rgba(163, 151, 128, 0.35)";
  for (const hit of t.hits) {
    if (hit.strength >= need || hit.strength < 0) continue;
    const beat = sh.origin + (hit.time - t0) * bps;
    const x = (beat / r.length) * w;
    if (x < 0 || x > w) continue;
    g.beginPath();
    g.arc(x, laneY(hit.kind, h), 2.5, 0, Math.PI * 2);
    g.fill();
  }
  // Kept hits: where they were played → where they land.
  for (const d of hitDots(r, w, h)) {
    const col = drumColor(d.lane);
    g.strokeStyle = col;
    g.globalAlpha = 0.35;
    g.beginPath();
    g.moveTo(d.rawX, d.y);
    g.lineTo(d.x, d.y);
    g.stroke();
    g.globalAlpha = 1;
    g.fillStyle = col;
    g.shadowColor = col;
    g.shadowBlur = 8;
    g.beginPath();
    g.arc(d.x, d.y, 3 + 3 * d.velocity, 0, Math.PI * 2);
    g.fill();
    g.shadowBlur = 0;
    if (voice.kinds.length > d.src && voice.kinds[d.src] !== "") {
      g.strokeStyle = "#f6eedd";
      g.beginPath();
      g.arc(d.x, d.y, 5 + 3 * d.velocity, 0, Math.PI * 2);
      g.stroke();
    }
  }
}

/** Clicking a hit gives it the next drum. */
/** function clickHit(e: Ev) => Undefined */
function clickHit(e) {
  if (voice.take.mode !== "drums") return undefined;
  const r = voiceResult();
  let best = -1;
  let dist = 12;
  let lane = "";
  for (const d of hitDots(r, e.targetWidth, e.targetHeight)) {
    const dx = d.x - e.offsetX;
    const dy = d.y - e.offsetY;
    const dd = Math.sqrt(dx * dx + dy * dy);
    if (dd < dist) {
      dist = dd;
      best = d.src;
      lane = d.lane;
    }
  }
  if (best < 0) return undefined;
  while (voice.kinds.length < voice.take.hits.length) voice.kinds.push("");
  voice.kinds[best] = nextDrum(lane);
  invalidate();
}

/** function previewView(b: Builder, r: VoiceResult) => Undefined */
function previewView(b, r) {
  b.open("div", "view", "voice-view");
  const hasTake = voice.take.mode !== "" && voice.status !== "recording";
  if (hasTake) {
    b.canvas("c", voice.take.mode === "drums" ? "voice-canvas drums" : "voice-canvas", (g, w, h) => {
      if (voice.take.mode === "drums") paintDrums(g, w, h, r);
      else paintMelody(g, w, h, r);
      // Where the result is while it plays.
      if (voice.previewing && state.playing && r.length > 0) {
        const x = Math.round(((state.position % r.length) / r.length) * w) + 0.5;
        g.fillStyle = "rgba(246, 238, 221, 0.9)";
        g.fillRect(x, 0, 1.5, h);
      }
    });
    b.on("pointerdown", (e) => clickHit(e));
    if (voice.take.mode === "drums") b.on("pointerenter", (e) => hint(t("voice.preview.hit.hint")));
  } else {
    b.open("div", "empty", "voice-empty");
    glyph(b, "mic");
    const drums = voice.mode === "drums";
    b.leaf(
      "div",
      "t",
      "voice-empty-title",
      voice.status === "recording" ? (drums ? t("voice.preview.empty.title.recordingDrums") : t("voice.preview.empty.title.recordingMelody")) : drums ? t("voice.preview.empty.title.drums") : t("voice.preview.empty.title.melody")
    );
    b.leaf(
      "div",
      "d",
      "voice-empty-doc",
      drums
        ? t(
            "voice.preview.empty.doc.drums"
          )
        : t("voice.preview.empty.doc.melody")
    );
    b.close();
  }
  b.close();
}

// ------------------------------------------------------------------ crop

/** function clock(t: Number) => String */
function clock(t) {
  const m = Math.floor(t / 60);
  const sec = t - m * 60;
  return `${m}:${sec < 10 ? "0" : ""}${fmt(sec, 1)}`;
}

/** The whole take, its level and what was found in it, with the kept part
 * lit between two handles. */
/** function paintCrop(g: Ctx, w: Number, h: Number) => Undefined */
function paintCrop(g, w, h) {
  const t = voice.take;
  const d = t.duration;
  if (d <= 0) return undefined;
  const span = cropSpan();
  const xOf = (sec) => (sec / d) * w;
  g.fillStyle = "rgba(232, 213, 176, 0.35)";
  const bw = Math.max(1, xOf(t.step));
  for (let i = 0; i < t.level.length; i++) {
    const lh = Math.max(1, t.level[i] * (h - 8));
    g.fillRect(xOf(i * t.step), (h - lh) / 2, bw, lh);
  }
  // What was found: hits as ticks, notes as bars.
  if (t.mode === "drums") {
    for (const hit of t.hits) {
      g.fillStyle = drumColor(hit.kind);
      g.fillRect(Math.round(xOf(hit.time)), h - 6, 1.5, 6);
    }
  } else {
    g.fillStyle = "#d4af37";
    for (const n of sungNotesOf(t)) g.fillRect(xOf(n.start), h - 4, Math.max(1, xOf(n.end - n.start)), 3);
  }
  // Outside the crop: dimmed.
  const xa = xOf(span[0]);
  const xz = xOf(span[1]);
  g.fillStyle = "rgba(9, 8, 11, 0.72)";
  g.fillRect(0, 0, xa, h);
  g.fillRect(xz, 0, w - xz, h);
  // The handles.
  g.fillStyle = "#d4af37";
  g.shadowColor = "rgba(212, 175, 55, 0.7)";
  g.shadowBlur = 6;
  for (const x of [xa, xz]) {
    g.fillRect(Math.round(x) - 1.5, 0, 3, h);
    g.fillRect(Math.round(x) - 5, h / 2 - 9, 10, 18);
  }
  g.shadowBlur = 0;
  g.strokeStyle = "rgba(212, 175, 55, 0.6)";
  g.lineWidth = 1;
  g.strokeRect(xa + 0.5, 0.5, Math.max(0, xz - xa - 1), h - 1);
}

/** The melody notes the crop strip marks (the chosen detail's). */
/** function sungNotesOf(t: Take) => VoiceNote[] */
function sungNotesOf(t) {
  const d = voice.settings.detail;
  return d >= 0 && d < t.details.length ? t.details[d] : t.notes;
}

/** Drag the nearer handle of the crop; a press away from it moves it there
 * first. */
/** function dragCrop(e: Ev) => Undefined */
function dragCrop(e) {
  const d = voice.take.duration;
  if (d <= 0 || e.targetWidth <= 0) return undefined;
  e.preventDefault();
  const span = cropSpan();
  const at = (e.offsetX / e.targetWidth) * d;
  const left = Math.abs(at - span[0]) <= Math.abs(at - span[1]);
  const gap = Math.min(0.1, d);
  const place = (v) => {
    const cur = cropSpan();
    const x = Math.round(Math.max(0, Math.min(d, v)) * 1000) / 1000;
    if (left) voice.cropStart = Math.min(x, cur[1] - gap);
    else voice.cropEnd = x >= d ? -1 : Math.max(x, cur[0] + gap);
    invalidate();
  };
  const near = Math.abs(((left ? span[0] : span[1]) / d) * e.targetWidth - e.offsetX) <= 10;
  const v0 = near ? (left ? span[0] : span[1]) : at;
  if (!near) place(v0);
  const x0 = e.clientX;
  drag(
    e,
    (m) => place(v0 + ((m.clientX - x0) / e.targetWidth) * d),
    (u) => undefined
  );
}

/** Crop: the take's two ends, dragged in; what is outside is left out and
 * the result's time starts at the left handle. */
/** function cropView(b: Builder) => Undefined */
function cropView(b) {
  const take = voice.take;
  const has = take.mode !== "" && voice.status !== "recording" && take.duration > 0;
  b.open("div", "crop", has ? "voice-crop" : "voice-crop off");
  b.open("div", "head", "voice-crop-head");
  b.leaf("span", "l", "voice-label", t("voice.crop.label"));
  const span = cropSpan();
  const cut = voice.cropStart >= 0 || voice.cropEnd >= 0;
  b.leaf(
    "span",
    "span",
    "voice-crop-span",
    has ? (cut ? tf("voice.crop.span.cut", [clock(span[0]), clock(span[1]), clock(take.duration)]) : tf("voice.crop.span.whole", [clock(take.duration)])) : ""
  );
  if (has && cut) {
    button(b, "reset", "small", t("voice.crop.reset.label"), t("voice.crop.reset.title"), () => {
      voice.cropStart = -1;
      voice.cropEnd = -1;
      invalidate();
    });
  }
  b.close();
  b.open("div", "strip", "voice-crop-strip");
  if (has) {
    b.canvas("c", "voice-canvas", (g, w, h) => paintCrop(g, w, h));
    b.on("pointerdown", (e) => dragCrop(e));
    b.on("pointerenter", (e) => hint(t("voice.crop.strip.hint")));
  }
  b.close();
  b.close();
}

// ------------------------------------------------------------------ view

/** function summary(r: VoiceResult) => String */
function summary(r) {
  if (voice.status === "analyzing") return t("voice.summary.analyzing");
  if (voice.take.mode === "") return t("voice.summary.noTake");
  const bars = r.length / state.project.transport.beatsPerBar;
  const barText = bars === 1 ? t("format.barsOne") : tf("format.barsMany", [fmt(bars, Math.abs(bars - Math.round(bars)) < 1e-9 ? 0 : 2)]);
  if (voice.take.mode === "drums") {
    /** const parts: String[] */
    const parts = [];
    for (const lane of DRUMS) {
      const n = r.notes.filter((x) => x.lane === lane).length;
      const d = drumText(lane);
      if (n === 1) parts.push(t(d.one));
      else if (n > 0) parts.push(tf(d.many, [String(n)]));
    }
    if (parts.length === 0) parts.push(t("voice.summary.noHits"));
    parts.push(barText);
    return parts.join(" · ");
  }
  const k = resolveKey(shaped().take, voice.settings);
  const scale = voice.settings.scale === "chromatic" ? t("term.chromatic") : keyText(KEY_NAMES[k.key], k.scale);
  return tf("voice.summary.melody", [String(r.notes.length), scale, barText]);
}

/** A step's title bar: its number, name and purpose. Leaves the bar open
 * for buttons; the caller closes it. */
/** function stepHead(b: Builder, n: String, title: String, sub: String) => Undefined */
function stepHead(b, n, title, sub) {
  b.open("div", "head", "voice-step-head");
  b.leaf("span", "n", "voice-step-num", n);
  b.open("div", "t", "voice-step-titles");
  b.leaf("div", "title", "voice-step-title", title);
  b.leaf("div", "sub", "voice-step-sub", sub);
  b.close();
}

/** Step 1: what to turn into notes — a new take or a recording. */
/** function takeStep(b: Builder) => Undefined */
function takeStep(b) {
  const rec = voice.status === "recording";
  b.open("section", "take", "voice-step voice-take");
  stepHead(b, "1", t("term.take"), t("voice.step.take.subtitle"));
  b.close();
  b.open("div", "mode", "seg");
  button(b, "melody", voice.mode === "melody" ? "small on" : "small", t("term.melody"), t("voice.mode.melody.title"), () =>
    setVoiceMode("melody")
  );
  button(b, "drums", voice.mode === "drums" ? "small on" : "small", t("voice.mode.drums.label"), t("voice.mode.drums.title"), () =>
    setVoiceMode("drums")
  );
  b.close();
  b.open("div", "rec", "voice-rec");
  iconButton(
    b,
    "btn",
    rec ? "voice-recbtn armed" : "voice-recbtn",
    rec ? "stop" : "record",
    rec ? t("voice.record.stop.title") : t("voice.record.start.title"),
    () => {
      if (rec) stopTake();
      else startTake();
    }
  );
  const secs = rec ? (now() - voice.recStarted) / 1000 : 0;
  b.leaf(
    "div",
    "time",
    "voice-time",
    rec ? `${Math.floor(secs / 60)}:${String(Math.floor(secs % 60)).padStart(2, "0")}` : voice.status === "analyzing" ? t("voice.record.analyzing.label") : t("voice.record.start.label")
  );
  b.close();
  choice(
    b,
    "along",
    t("voice.playAlong.label"),
    voice.playAlong,
    ["off", "pattern", "song"],
    [t("voice.playAlong.option.off"), t("term.pattern"), t("term.song")],
    t("voice.playAlong.title"),
    (v) => {
      voice.playAlong = v;
    }
  );
  /** const takes: String[] */
  const takes = [""];
  /** const takeLabels: String[] */
  const takeLabels = [t("voice.take.option.earlier")];
  // The new take may not be in the samples list yet.
  const paths = state.samples.includes(voice.path) || voice.path === "" ? state.samples : state.samples.concat([voice.path]);
  for (const path of paths) {
    takes.push(path);
    takeLabels.push(path.split("/").pop() ?? path);
  }
  choice(b, "take", t("term.take"), voice.path, takes, takeLabels, t("voice.take.title"), (v) => pickTake(v));
  b.open("div", "acts", "voice-actions");
  button(b, "open", "small", t("voice.open.label"), t("voice.open.title"), () => openRecording());
  if (voice.path !== "") {
    button(b, "listen", voice.playing ? "small on" : "small", voice.playing ? t("common.stop") : t("voice.listen.label"), t("voice.listen.title"), () => listenTake());
  }
  b.close();
  b.close();
}

/** Step 2: shape the result — the preview and the settings, as many rounds
 * as it takes (the settings apply instantly; Analyze again re-reads the take). */
/** function shapeStep(b: Builder, r: VoiceResult, ready: Boolean) => Undefined */
function shapeStep(b, r, ready) {
  b.open("section", "shape", ready ? "voice-step voice-shape" : "voice-step voice-shape waiting");
  stepHead(b, "2", t("voice.step.shape.label"), ready ? t("voice.step.shape.subtitle.ready") : t("voice.step.shape.subtitle.waiting"));
  b.open("div", "acts", "voice-actions");
  if (ready) {
    iconButton(
      b,
      "play",
      voice.previewing ? "small on" : "small",
      voice.previewing ? "stop" : "play",
      voice.previewing ? t("voice.shape.result.stop.title") : t("voice.shape.result.play.title"),
      () => {
        if (voice.previewing) stopResult();
        else playResult();
      }
    );
    button(b, "take", voice.playing ? "small on" : "small", voice.playing ? t("common.stop") : t("voice.shape.take.label"), t("voice.shape.take.title"), () =>
      listenTake()
    );
  }
  if (voice.path !== "") button(b, "again", "small", t("voice.shape.again.label"), t("voice.shape.again.title"), () => analyzeAgain());
  b.close();
  b.close();
  previewView(b, r);
  cropView(b);
  settingsView(b);
  b.close();
}

/** Step 3: put the result in the song. */
/** function addStep(b: Builder, r: VoiceResult, ready: Boolean) => Undefined */
function addStep(b, r, ready) {
  b.open("section", "add", ready ? "voice-step voice-add" : "voice-step voice-add waiting");
  stepHead(b, "3", t("voice.step.add.label"), t("voice.step.add.subtitle"));
  b.close();
  b.leaf("div", "sum", "voice-summary", summary(r));
  targetView(b, r);
  button(
    b,
    "add",
    r.notes.length > 0 ? "gold voice-addbtn" : "voice-addbtn",
    t("voice.addToSong.label"),
    t("voice.addToSong.title"),
    () => addToSong()
  );
  b.close();
}

/** The Voice panel: take → shape → add, left to right when there is room,
 * top to bottom when there is not (voice.css). */
/** function voicePanel(b: Builder) => Undefined */
export function voicePanel(b) {
  const r = voiceResult();
  const ready = voice.take.mode !== "" && voice.status === "idle";
  b.open("div", "voice", `voice ${voice.mode}`);
  b.open("div", "flow", "voice-flow");
  takeStep(b);
  shapeStep(b, r, ready);
  addStep(b, r, ready && r.notes.length > 0);
  b.close();
  b.close();
}

/** Dock tab tools: nothing to add beyond the panel's own. */
/** function voiceTools(b: Builder) => Undefined */
export function voiceTools(b) {
  b.leaf("span", "l", "label", voice.mode === "drums" ? t("voice.tools.drums.label") : t("voice.tools.melody.label"));
}
