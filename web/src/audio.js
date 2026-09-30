// Transport and audio routing.
//
// Two engines can play the project — the same Rust code in both cases:
//  - "browser": the engine compiled to WebAssembly, running in an
//    AudioWorklet (works anywhere, no plugins);
//  - "native": the server's engine on the system audio device (plugins,
//    lowest latency).
// The UI sends the same transport commands to whichever one is selected.

import { audioStart, audioPost, audioPostSample, audioLoadPreset, audioResume, decodeAudioUrl, recStart, recStop, now } from "#platform";
import { state, hooks, invalidate, commit, currentPattern, reportContext } from "./store.js";
import { send } from "./net.js";
import { toast } from "./ui/toast.js";
import { projectJson } from "./model.js";
import { insertIx, trackIndex } from "#brands";

/** const loaded: String[] */
const loaded = [];
/** const loading: String[] */
const loading = [];

/** function onEngineMessage(m: AudioMsg) => Undefined */
function onEngineMessage(m) {
  if (m.t === "status") {
    if (state.output !== "browser") return undefined;
    state.position = m.position;
    state.positionAt = now();
    state.playing = m.playing;
    state.loopLength = m.loopLength;
    const nIns = Math.round(m.meters.length > 0 ? m.meters[0] : 0);
    const nCh = Math.round(m.meters.length > 1 ? m.meters[1] : 0);
    state.meters = m.meters.slice(2, 2 + nIns * 2);
    state.chMeters = m.meters.slice(2 + nIns * 2, 2 + nIns * 2 + nCh);
    invalidate();
  } else if (m.t === "loaded") {
    for (const path of m.missing) loadSample(path);
    for (const p of m.presets) loadPreset(p);
  } else if (m.t === "loadError") {
    toast("The audio engine rejected the project", m.message, "error");
  }
}

/** function loadSample(path: String) => Undefined */
function loadSample(path) {
  if (loaded.includes(path) || loading.includes(path)) return undefined;
  loading.push(path);
  decodeAudioUrl(`/files/${path}`)
    .then((d) => {
      audioPostSample(path, d);
      loaded.push(path);
      return true;
    })
    .catch((e) => {
      toast("Could not load sample", path, "error");
      return false;
    });
}

/** Soundfont presets in the browser engine, or on their way ("font/bank/program"). */
/** const presets: String[] */
const presets = [];

/** Load a soundfont preset (decoded in a worker, see platform.js); the
 * instrument is silent until it arrives. */
/** function loadPreset(p: PresetRef) => Undefined */
function loadPreset(p) {
  const key = `${p.font}/${p.bank}/${p.program}`;
  if (presets.includes(key)) return undefined;
  presets.push(key);
  audioLoadPreset(p.font, p.bank, p.program).catch((e) => {
    presets.splice(presets.indexOf(key), 1);
    toast("Could not load an instrument", String(e), "error");
    return false;
  });
}

/** const startup: Promise<Boolean>[] */
const startup = [];

/** Start the browser engine (must follow a user gesture). Every caller waits
 * for the same startup, so a play pressed as the very first gesture is not
 * lost. */
export function startAudio() {
  if (state.audioReady) return audioResume();
  if (startup.length > 0) return startup[0];
  const p = boot();
  startup.push(p);
  return p;
}

async function boot() {
  try {
    await audioStart("engine/worklet.js", "engine/rosaclef.wasm", onEngineMessage);
    state.audioReady = true;
    audioPost({ t: "project", json: projectJson(state.project) });
    invalidate();
  } catch (e) {
    toast("Could not start browser audio", String(e), "error");
    startup.length = 0;
    return false;
  }
  return await audioResume();
}

/** The project folder whose samples the browser engine holds. */
const sampleCache = { folder: "" };

/** Another project was opened: it may hold different audio at the same
 * paths, so decode again every cached sample it uses and forget the rest. */
function forgetSamples() {
  /** const used: String[] */
  const used = [];
  for (const c of state.project.channels) {
    for (const o of c.instrument.options) {
      if (o.key === "sample" && o.value !== "") used.push(o.value);
    }
  }
  for (const c of state.project.playlist.clips) {
    if (c.sample !== "") used.push(c.sample);
  }
  const old = loaded.slice();
  loaded.length = 0;
  loading.length = 0;
  for (const path of old) {
    if (used.includes(path)) loadSample(path);
  }
}

export function installEngine() {
  hooks.engine = (json) => {
    if (state.folder !== sampleCache.folder) {
      if (sampleCache.folder !== "") forgetSamples();
      sampleCache.folder = state.folder;
    }
    if (state.audioReady) audioPost({ t: "project", json: json });
  };
}

/** Pattern id for the engine: "" plays the song. */
function modeTarget() {
  const p = currentPattern();
  if (state.mode !== "pattern" || !p) return "";
  return p.id;
}

export async function play() {
  if (state.output === "native") {
    send({ t: "native.play", pattern: modeTarget() });
    return true;
  }
  await startAudio();
  audioPost({ t: "mode", pattern: modeTarget() });
  audioPost({ t: "play" });
  state.playing = true;
  reportContext();
  invalidate();
  return true;
}

export function stop() {
  if (state.recording) {
    stopRecording();
    return;
  }
  if (state.output === "native") send({ t: "native.stop" });
  else audioPost({ t: "stop" });
  state.playing = false;
  state.position = 0;
  reportContext();
  invalidate();
}

export function togglePlay() {
  if (state.playing) {
    if (state.output === "native") send({ t: "native.pause" });
    else audioPost({ t: "pause" });
    state.playing = false;
    invalidate();
  } else {
    play();
  }
}

/** function setMode(mode: String) => Undefined */
export function setMode(mode) {
  state.mode = mode;
  reportContext();
  if (state.output === "browser") audioPost({ t: "mode", pattern: modeTarget() });
  else if (state.playing) send({ t: "native.play", pattern: modeTarget() });
  invalidate();
}

/** Called when the selected pattern changes while in pattern mode. */
export function followPattern() {
  if (state.mode === "pattern") {
    if (state.output === "browser") audioPost({ t: "mode", pattern: modeTarget() });
    else if (state.playing) send({ t: "native.play", pattern: modeTarget() });
  }
}

/** function seek(beat: Number) => Undefined */
export function seek(beat) {
  if (state.output === "native") send({ t: "native.seek", beat: beat });
  else audioPost({ t: "seek", beat: beat });
  state.position = beat;
  invalidate();
}

/** The playhead right now: the last reported position, moved on by the time
 * since (reports come ~40 times a second), wrapped at the loop end. */
/** function livePosition() => Number */
export function livePosition() {
  if (!state.playing) return state.position;
  const ms = Math.max(0, Math.min(250, now() - state.positionAt));
  const p = state.position + (ms / 60000) * state.project.transport.bpm;
  return state.loopLength > 0 && p >= state.loopLength ? p - state.loopLength : p;
}

/** function noteOn(channel: String, key: Number, velocity: Number) => Undefined */
export function noteOn(channel, key, velocity) {
  if (state.output === "native") send({ t: "native.note", channel: channel, key: key, velocity: velocity, on: true });
  else {
    startAudio();
    audioPost({ t: "note", channel: channel, key: key, velocity: velocity, on: true });
  }
}

/** function noteOff(channel: String, key: Number) => Undefined */
export function noteOff(channel, key) {
  if (state.output === "native") send({ t: "native.note", channel: channel, key: key, velocity: 0, on: false });
  else audioPost({ t: "note", channel: channel, key: key, velocity: 0, on: false });
}

/** Preview a short note (piano roll clicks, step toggles). */
/** function preview(channel: String, key: Number, velocity: Number) => Undefined */
export function preview(channel, key, velocity) {
  noteOn(channel, key, velocity);
  setTimeout(() => {
    noteOff(channel, key);
  }, 220);
}

/** function setOutput(out: String) => Undefined */
export function setOutput(out) {
  if (out === state.output) return undefined;
  stop();
  if (out === "native") {
    if (!state.nativeEnabled) send({ t: "native.enable" });
    state.output = "native";
  } else {
    state.output = "browser";
    startAudio();
  }
  invalidate();
}

// ------------------------------------------------------------------ recording

let recStartBeat = 0;

/** Record from the microphone onto the selected playlist track. */
export async function record() {
  if (state.recording) {
    stopRecording();
    return false;
  }
  if (state.output === "native") {
    state.mode = "song";
    send({ t: "native.record", track: trackIndex(state.track) });
    state.recording = true;
    invalidate();
    return true;
  }
  await startAudio();
  const ok = await recStart().catch((e) => false);
  if (!ok) {
    toast("Microphone unavailable", "Allow microphone access to record.", "error");
    return false;
  }
  state.mode = "song";
  recStartBeat = state.position;
  state.recording = true;
  audioPost({ t: "mode", pattern: "" });
  audioPost({ t: "play" });
  invalidate();
  return true;
}

export function stopRecording() {
  state.recording = false;
  if (state.output === "native") {
    send({ t: "native.stop" });
    invalidate();
    return;
  }
  audioPost({ t: "stop" });
  const startBeat = recStartBeat;
  const endBeat = state.position;
  state.playing = false;
  invalidate();
  recStop(`take-${Date.now()}.wav`).then((path) => {
    if (path === "") return false;
    commit(() => {
      state.project.playlist.clips.push({
        pattern: "",
        sample: path,
        track: state.track,
        start: startBeat,
        length: Math.max(0.25, endBeat - startBeat),
        offset: 0,
        gain: 1,
        mixer: insertIx(0),
      });
    });
    toast("Recording placed on the playlist", path, "info");
    return true;
  });
}
