// Transport and audio routing.
//
// Two engines can play the project — the same Rust code in both cases:
//  - "browser": the engine compiled to WebAssembly, running in an
//    AudioWorklet (works anywhere, no plugins);
//  - "native": the server's engine on the system audio device (plugins,
//    lowest latency).
// The UI sends the same transport commands to whichever one is selected.

import { audioStart, audioPost, audioPostSample, audioLoadPreset, audioResume, decodeAudioUrl, recStart, recStop, now, loadPref, savePref } from "#platform";
import { state, hooks, invalidate, commit, currentPattern, reportContext, engineJson, refreshEngine, hint, AUDITION } from "./store.js";
import { send } from "./net.js";
import { toast } from "./ui/toast.js";
import { insertIx, trackIndex } from "#brands";
import { t } from "./i18n.js";

/** const loaded: String[] */
const loaded = [];
/** const loading: String[] */
const loading = [];

/** What the browser engine still lacks: `answered` once it has said which
 * samples and soundfont presets the last project sent needs, `inFlight` of
 * them on their way. */
const supply = { answered: false, inFlight: 0 };

/** A play waiting for the instruments (`on`), with its count-in; `gen` tells
 * it from a later one. */
const pending = { on: false, beats /*: Number */: 0, gen: 0 };

/** The longest a play waits for the instruments (ms). */
const SUPPLY_WAIT = 20000;

/** function supplied() => Boolean */
function supplied() {
  return supply.answered && supply.inFlight === 0;
}

/** A sample or preset arrived (or failed): a waiting play may start. */
function landed() {
  supply.inFlight = Math.max(0, supply.inFlight - 1);
  if (pending.on && supplied()) startPending();
}

/** function onEngineMessage(m: AudioMsg) => Undefined */
function onEngineMessage(m) {
  if (m.t === "status") {
    if (state.output !== "browser") return undefined;
    // A status sent before the engine applied the last play, pause, stop or
    // seek would undo it here (Space twice would play twice; a stop would
    // jump back to where it played).
    if (!m.stale) {
      state.position = m.position;
      state.positionAt = now();
      state.playing = m.playing;
    }
    state.loopLength = m.loopLength;
    const nIns = Math.round(m.meters.length > 0 ? m.meters[0] : 0);
    const nCh = Math.round(m.meters.length > 1 ? m.meters[1] : 0);
    state.meters = m.meters.slice(2, 2 + nIns * 2);
    state.chMeters = m.meters.slice(2 + nIns * 2, 2 + nIns * 2 + nCh);
    invalidate();
  } else if (m.t === "loaded") {
    for (const path of m.missing) loadSample(path);
    for (const p of m.presets) loadPreset(p);
    supply.answered = true;
    if (pending.on && supplied()) startPending();
  } else if (m.t === "loadError") {
    toast(t("audio.engine.rejected.toast.title"), m.message, "error");
    supply.answered = true;
    if (pending.on && supplied()) startPending();
  }
}

/** function loadSample(path: String) => Undefined */
function loadSample(path) {
  if (loaded.includes(path) || loading.includes(path)) return undefined;
  loading.push(path);
  supply.inFlight += 1;
  decodeAudioUrl(`/files/${path}`)
    .then((d) => {
      audioPostSample(path, d);
      loaded.push(path);
      landed();
      return true;
    })
    .catch((e) => {
      toast(t("audio.sample.loadFailed.toast.title"), path, "error");
      landed();
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
  supply.inFlight += 1;
  audioLoadPreset(p.font, p.bank, p.program)
    .then((ok) => {
      landed();
      return ok;
    })
    .catch((e) => {
      presets.splice(presets.indexOf(key), 1);
      toast(t("audio.preset.loadFailed.toast.title"), String(e), "error");
      landed();
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
    supply.answered = false;
    audioPost({ t: "project", json: engineJson() });
    invalidate();
  } catch (e) {
    toast(t("audio.start.failed.toast.title"), String(e), "error");
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

/** Tell the engine which instrument is being tried out (or that none is):
 * the browser engine gets it with the project; the native one by message. */
function syncAudition() {
  refreshEngine();
  if (!state.nativeEnabled) return undefined;
  const msg = JSON.parse('{"t":"native.audition","channel":null}');
  if (state.audition.on) msg.channel = JSON.parse(engineJson()).channels.find((c) => c.id === AUDITION);
  send(msg);
}

export function installEngine() {
  hooks.audition = syncAudition;
  hooks.engine = (json) => {
    if (state.folder !== sampleCache.folder) {
      if (sampleCache.folder !== "") forgetSamples();
      sampleCache.folder = state.folder;
    }
    if (state.audioReady) {
      supply.answered = false;
      audioPost({ t: "project", json: json });
    }
  };
}

/** Pattern id for the engine: "" plays the song. */
function modeTarget() {
  const p = currentPattern();
  if (state.mode !== "pattern" || !p) return "";
  return p.id;
}

export async function play() {
  return await playCountIn(0);
}

/** Play after `beats` beats of count-in clicks; the position reads negative
 * (the beats left) until the sequencer starts. */
/** async function playCountIn(beats: Number) => Boolean */
export async function playCountIn(beats) {
  if (state.output === "native") {
    send({ t: "native.metronome", on: state.metronome });
    send({ t: "native.play", pattern: modeTarget(), countIn: beats });
    return true;
  }
  await startAudio();
  // Right after loading, the engine is still being sent the song's samples
  // and soundfont presets: playing now would start silent and bring the
  // instruments in one by one as they arrive. Wait for them (not forever).
  pending.gen = pending.gen + 1;
  pending.on = true;
  pending.beats = beats;
  if (supplied()) startPending();
  else {
    hint(t("audio.loadingInstruments.hint"));
    const gen = pending.gen;
    setTimeout(() => {
      if (pending.on && pending.gen === gen) startPending();
    }, SUPPLY_WAIT);
  }
  return true;
}

/** Start the play that waited for the instruments. */
function startPending() {
  if (!pending.on) return undefined;
  pending.on = false;
  if (state.hint === t("audio.loadingInstruments.hint")) hint("");
  const beats = pending.beats;
  audioPost({ t: "metronome", on: state.metronome });
  audioPost({ t: "mode", pattern: modeTarget() });
  audioPost({ t: "play", countIn: beats });
  if (!state.playing && beats > 0) {
    state.position = -beats;
    state.positionAt = now();
  }
  state.playing = true;
  reportContext();
  invalidate();
}

/** Drop a play still waiting for the instruments. */
function cancelPending() {
  if (!pending.on) return undefined;
  pending.on = false;
  if (state.hint === t("audio.loadingInstruments.hint")) hint("");
}

/** In pattern mode, play on past the pattern's end instead of looping (while
 * recording into a pattern that grows as it goes). */
/** function setOpenEnded(on: Boolean) => Undefined */
export function setOpenEnded(on) {
  if (state.output === "native") send({ t: "native.openEnded", on: on });
  else audioPost({ t: "openEnded", on: on });
}

const METRONOME_PREF = "rosaclef.metronome";

export function loadMetronome() {
  state.metronome = loadPref(METRONOME_PREF) === "on";
}

/** Turn the metronome on or off (it clicks every beat while playing). */
export function toggleMetronome() {
  state.metronome = !state.metronome;
  savePref(METRONOME_PREF, state.metronome ? "on" : "off");
  if (state.output === "native") send({ t: "native.metronome", on: state.metronome });
  else if (state.audioReady) audioPost({ t: "metronome", on: state.metronome });
  invalidate();
}

export function stop() {
  if (state.recording) {
    stopRecording();
    return;
  }
  cancelPending();
  if (state.output === "native") send({ t: "native.stop" });
  else audioPost({ t: "stop" });
  state.playing = false;
  state.position = 0;
  reportContext();
  invalidate();
}

export function togglePlay() {
  if (pending.on) cancelPending();
  else if (state.playing) {
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
  else if (state.audioReady) {
    startAudio();
    audioPost({ t: "note", channel: channel, key: key, velocity: velocity, on: true });
  } else {
    // The first note starts the engine: it plays once the engine is there.
    startAudio().then((ok) => {
      audioPost({ t: "note", channel: channel, key: key, velocity: velocity, on: true });
      return ok;
    });
  }
}

/** function noteOff(channel: String, key: Number) => Undefined */
export function noteOff(channel, key) {
  if (state.output === "native") send({ t: "native.note", channel: channel, key: key, velocity: 0, on: false });
  else if (state.audioReady) audioPost({ t: "note", channel: channel, key: key, velocity: 0, on: false });
  // (After a note still waiting for the engine to start.)
  else
    startAudio().then((ok) => {
      audioPost({ t: "note", channel: channel, key: key, velocity: 0, on: false });
      return ok;
    });
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
    toast(t("audio.micUnavailable.title"), t("audio.micUnavailable.body"), "error");
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
    toast(t("audio.record.placed.toast.title"), path, "info");
    return true;
  });
}
