// The Voice dock (F8): sing, hum, whistle or beatbox into the microphone and
// get notes. A melody take becomes a piano-roll pattern (quantized, snapped
// to a scale); a beatbox take becomes a drum loop on kick, snare and hat
// channels. The take is analyzed once by the server (GET /api/transcribe);
// every setting here re-shapes the result instantly (../voice.js), and "Add
// to song" makes it a pattern with a clip on the playlist (one undo step).

import { getJson, drag, fmt, now, recStart, recStop, previewAudio, stopPreview, pickFiles, uploadFile } from "#platform";
import { state, commit, invalidate, hint, selectPattern, currentChannel } from "../store.js";
import { uniqueId, paletteColor, setOption, optionValue, snapDown } from "../model.js";
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
  decodeTake,
  emptyTake,
} from "../voice.js";
import { button, iconButton, select, glyph, clamp01 } from "./widgets.js";
import { pushChannel } from "./browser.js";
import { toast } from "./toast.js";
import { insertIx, trackIx, trackIndex } from "#brands";

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
  /** Per hit: a drum chosen by clicking it, "" = sorted by tone. */
  kinds /*: String[] */: [],
  /** "auto" (the selected channel if melodic, else a new one), "new" or a channel id. */
  melodyChannel: "auto",
  /** Per drum: a channel id, "" = the first matching Atelier channel (or a new one). */
  drumChannels /*: KS[] */: [
    { key: "kick", value: "" },
    { key: "snare", value: "" },
    { key: "hat", value: "" },
  ],
  repeat: 1,
  playing: false,
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
    kickBelow: 900,
    hatAbove: 4200,
    bars: 0,
  },
};

// ------------------------------------------------------------------ result

/** The notes the settings make of the take, and the pattern length. */
/** function voiceResult() => VoiceResult */
export function voiceResult() {
  const t = voice.take;
  const p = state.project;
  const bpm = p.transport.bpm;
  const s = voice.settings;
  /** const empty: Placed[] */
  const empty = [];
  const all =
    t.mode === "drums"
      ? drumHits(t, s, bpm, voice.origin, voice.aligned, voice.kinds)
      : t.mode === "melody"
        ? melodyNotes(t, s, bpm, voice.origin, voice.aligned)
        : empty;
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
  const mode = voice.mode;
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
      if (found === 0) toast(mode === "drums" ? "No hits found" : "No notes found", "Try a louder take, closer to the microphone.", "info");
      invalidate();
      return true;
    })
    .catch((e) => {
      if (voice.path === path) voice.status = "idle";
      toast("Could not analyze the take", String(e), "error");
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
    toast("Already recording", "Stop the playlist recording first.", "error");
    return false;
  }
  stopPreview();
  voice.playing = false;
  await startAudio();
  const ok = await recStart().catch((e) => false);
  if (!ok) {
    toast("Microphone unavailable", "Allow microphone access to record.", "error");
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
        toast("Nothing was recorded", "The microphone sent no audio.", "error");
        invalidate();
        return false;
      }
      analyze(path);
      return true;
    })
    .catch((e) => {
      voice.status = "idle";
      toast("Could not save the take", String(e), "error");
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
        toast("Could not add the recording", String(e), "error");
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

/** An Atelier channel playing `kind` (a snare also takes a clap, a hat an open hat). */
/** function drumChannel(kind: String) => String */
function drumChannel(kind) {
  const also = kind === "snare" ? "clap" : kind === "hat" ? "openhat" : kind;
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

/** The channel for a lane, created if needed (call inside `commit`). */
/** function laneChannel(lane: String) => String */
function laneChannel(lane) {
  if (lane === "melody") {
    const id = melodyTarget();
    return id !== "" ? id : pushChannel("synth", "Voice", (d) => undefined);
  }
  const chosen = chosenDrum(lane);
  if (chosen !== "" && channelExists(chosen)) return chosen;
  const found = drumChannel(lane);
  if (found !== "") return found;
  const name = lane === "kick" ? "Kick" : lane === "snare" ? "Snare" : "Hat";
  return pushChannel("drum", name, (d) => setOption(d, "kind", lane));
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
  const r = voiceResult();
  if (r.notes.length === 0) {
    toast("Nothing to add", voice.path === "" ? "Record a take first." : "No notes pass the current settings.", "error");
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
    });
    start = voice.at >= 0 ? voice.at : snapDown(state.position, bpb);
    const length = r.length * voice.repeat;
    const track = freeTrack(start, start + length, base);
    p.playlist.clips.push({ pattern: id, sample: "", track: track, start: start, length: length, offset: 0, gain: 1, mixer: insertIx(0) });
  });
  selectPattern(id);
  const bar = Math.floor(start / bpb) + 1;
  toast(
    `Added ${name}`,
    `${r.notes.length} ${drums ? "hits" : "notes"} on the playlist at bar ${bar}${voice.repeat > 1 ? `, looped ×${voice.repeat}` : ""}. Ctrl+Z undoes it.`,
    "info"
  );
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

/** Hz on a log dial between lo and hi. */
/** function hzUnit(v: Number, lo: Number, hi: Number) => Number */
function hzUnit(v, lo, hi) {
  return Math.log(v / lo) / Math.log(hi / lo);
}

/** function unitHz(t: Number, lo: Number, hi: Number) => Number */
function unitHz(t, lo, hi) {
  return Math.round((lo * Math.pow(hi / lo, t)) / 10) * 10;
}

/** function hzText(v: Number) => String */
function hzText(v) {
  return v >= 1000 ? `${fmt(v / 1000, 1)} kHz` : `${fmt(v, 0)} Hz`;
}

const GRIDS = [0, 0.125, 0.25, 0.5, 1, 1 / 6, 1 / 3];
const GRID_LABELS = ["Off", "1/32", "1/16", "1/8", "Beat", "1/16 T", "1/8 T"];

/** function settingsView(b: Builder) => Undefined */
function settingsView(b) {
  const s = voice.settings;
  const drums = voice.mode === "drums";
  b.open("div", "set", "voice-settings");

  if (!drums) {
    b.open("div", "notes", "voice-group");
    b.leaf("div", "t", "voice-group-title", "Notes");
    b.open("div", "row", "voice-row");
    const last = DETAILS.length - 1;
    dial(
      b,
      "detail",
      "Detail",
      s.detail >= 0 && s.detail <= last ? DETAILS[s.detail] : "",
      s.detail / last,
      "Smooth absorbs slides, scoops and ornaments into the notes around them; Every note keeps quick runs (and more blips)",
      (v) => {
        s.detail = Math.round(v * last);
      }
    );
    b.close();
    b.close();
  }

  b.open("div", "q", "voice-group");
  b.leaf("div", "t", "voice-group-title", "Quantize");
  b.open("div", "row", "voice-row");
  choice(
    b,
    "grid",
    "Grid",
    String(s.grid),
    GRIDS.map((g) => String(g)),
    GRID_LABELS,
    "Grid the notes snap to",
    (v) => {
      s.grid = Number(v);
    }
  );
  dial(b, "str", "Strength", `${Math.round(s.strength * 100)}%`, s.strength, "How far notes move onto the grid (0% keeps the feel of the take)", (v) => {
    s.strength = Math.round(v * 20) / 20;
  });
  if (!drums) {
    toggle(b, "ends", "Ends", s.lengths, "Quantize where notes end too", (v) => {
      s.lengths = v;
    });
    toggle(b, "legato", "Legato", s.legato, "Hold every note until the next one", (v) => {
      s.legato = v;
    });
  }
  choice(
    b,
    "bars",
    "Length",
    String(s.bars),
    ["0", "1", "2", "4", "8"],
    ["Auto", "1 bar", "2 bars", "4 bars", "8 bars"],
    "Pattern length (notes past it are left out)",
    (v) => {
      s.bars = Math.round(Number(v));
    }
  );
  b.close();
  b.close();

  if (!drums) {
    const k = resolveKey(voice.take, s);
    b.open("div", "tune", "voice-group");
    b.leaf("div", "t", "voice-group-title", "Auto-tune");
    b.open("div", "row", "voice-row");
    /** const keys: String[] */
    const keys = ["-1"];
    /** const keyLabels: String[] */
    const keyLabels = [voice.take.notes.length > 0 && s.key < 0 ? `Detect (${KEY_NAMES[k.key]})` : "Detect"];
    for (let i = 0; i < 12; i++) {
      keys.push(String(i));
      keyLabels.push(KEY_NAMES[i]);
    }
    choice(b, "key", "Key", String(s.key), keys, keyLabels, "Key the notes are snapped to (Detect finds it from the take)", (v) => {
      s.key = Math.round(Number(v));
    });
    choice(
      b,
      "scale",
      "Scale",
      s.scale,
      SCALES.map((x) => x.id),
      SCALES.map((x) => x.label),
      "Scale the notes are snapped to (Chromatic: the nearest semitone)",
      (v) => {
        s.scale = v;
      }
    );
    choice(b, "oct", "Octave", String(s.octave), ["-2", "-1", "0", "1", "2"], ["−2", "−1", "0", "+1", "+2"], "Move the notes by octaves", (v) => {
      s.octave = Math.round(Number(v));
    });
    toggle(b, "dyn", "Dynamics", s.dynamics, "Velocities follow how loud each note was sung", (v) => {
      s.dynamics = v;
    });
    b.close();
    b.close();
  } else {
    b.open("div", "sort", "voice-group");
    b.leaf("div", "t", "voice-group-title", "Hits");
    b.open("div", "row", "voice-row");
    dial(b, "sens", "Sensitivity", `${Math.round(s.sensitivity * 100)}%`, s.sensitivity, "Higher keeps quieter hits (ghost notes)", (v) => {
      s.sensitivity = Math.round(v * 50) / 50;
    });
    dial(b, "kick", "Kick below", hzText(s.kickBelow), hzUnit(s.kickBelow, 200, 3000), "Hits darker than this are kicks", (v) => {
      s.kickBelow = Math.min(unitHz(v, 200, 3000), s.hatAbove - 100);
    });
    dial(b, "hat", "Hat above", hzText(s.hatAbove), hzUnit(s.hatAbove, 1500, 12000), "Hits brighter than this are hats (in between: snares)", (v) => {
      s.hatAbove = Math.max(unitHz(v, 1500, 12000), s.kickBelow + 100);
    });
    toggle(b, "dyn", "Accents", s.dynamics, "Velocities follow how hard each hit was", (v) => {
      s.dynamics = v;
    });
    b.close();
    b.close();
  }
  b.close();
}

/** Where the result goes: channels and loop count. */
/** function targetView(b: Builder) => Undefined */
function targetView(b) {
  const drums = voice.mode === "drums";
  const chs = state.project.channels;
  b.open("div", "target", "voice-target");
  if (!drums) {
    /** const ids: String[] */
    const ids = ["auto", "new"];
    const sel = melodicSelection();
    /** const names: String[] */
    const names = [sel !== "" ? `Selected (${channelName(sel)})` : "Selected (new channel)", "New channel"];
    for (const c of chs) {
      if (c.instrument.type === "drum") continue;
      ids.push(c.id);
      names.push(c.name);
    }
    choice(b, "ch", "Channel", voice.melodyChannel, ids, names, "Channel that plays the notes", (v) => {
      voice.melodyChannel = v;
    });
  } else {
    for (const e of voice.drumChannels) {
      const found = drumChannel(e.key);
      /** const ids: String[] */
      const ids = [""];
      /** const names: String[] */
      const names = [found !== "" ? `Auto (${channelName(found)})` : "Auto (new Atelier)"];
      for (const c of chs) {
        ids.push(c.id);
        names.push(c.name);
      }
      choice(b, e.key, e.key === "kick" ? "Kick" : e.key === "snare" ? "Snare" : "Hat", e.value, ids, names, `Channel that plays the ${e.key} hits`, (v) => {
        e.value = v;
      });
    }
  }
  choice(b, "rep", "Loop", String(voice.repeat), ["1", "2", "4", "8"], ["×1", "×2", "×4", "×8"], "How many times the clip plays the pattern", (v) => {
    voice.repeat = Math.round(Math.max(1, Number(v)));
  });
  b.close();
}

// ------------------------------------------------------------------ preview

const DRUM_COLORS = ["#d08a93", "#46a58b", "#e8d5b0"];

/** function drumColor(kind: String) => String */
function drumColor(kind) {
  const i = DRUMS.indexOf(kind);
  return i >= 0 ? DRUM_COLORS[i] : "#a39780";
}

/** Tone (Hz) on the beatbox view's log axis. */
/** function toneY(tone: Number, h: Number) => Number */
function toneY(tone, h) {
  const t = Math.log(Math.max(60, Math.min(16000, tone)) / 60) / Math.log(16000 / 60);
  return 12 + (1 - t) * (h - 24);
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
  const t = voice.take;
  const s = voice.settings;
  const shift = 12 * s.octave;
  const t0 = takeStart(t, s.detail, voice.aligned);
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
    const x = xOf(voice.origin + (i * t.step - t0) * bps);
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
    const x = xOf(voice.origin + (i * t.step - t0) * bps);
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
    const hit = voice.take.hits[n.src];
    out.push({ x: (n.start / r.length) * w, y: toneY(hit.tone, h), rawX: (n.raw / r.length) * w, lane: n.lane, src: n.src, velocity: n.velocity });
  }
  return out;
}

/** function paintDrums(g: Ctx, w: Number, h: Number, r: VoiceResult) => Undefined */
function paintDrums(g, w, h, r) {
  const s = voice.settings;
  paintGrid(g, w, h, r.length);
  // The tone boundaries, with the drum each region makes.
  const yk = toneY(s.kickBelow, h);
  const yh = toneY(s.hatAbove, h);
  g.fillStyle = "rgba(208, 138, 147, 0.05)";
  g.fillRect(0, yk, w, h - yk);
  g.fillStyle = "rgba(232, 213, 176, 0.04)";
  g.fillRect(0, 0, w, yh);
  g.strokeStyle = "rgba(212, 175, 55, 0.4)";
  g.lineWidth = 1;
  g.setLineDash([4, 4]);
  for (const y of [yk, yh]) {
    g.beginPath();
    g.moveTo(0, Math.round(y) + 0.5);
    g.lineTo(w, Math.round(y) + 0.5);
    g.stroke();
  }
  g.setLineDash([]);
  g.font = "600 9px Manrope, sans-serif";
  g.fillStyle = drumColor("hat");
  g.fillText("HAT", 6, Math.max(12, yh - 6));
  g.fillStyle = drumColor("snare");
  g.fillText("SNARE", 6, (yh + yk) / 2 + 3);
  g.fillStyle = drumColor("kick");
  g.fillText("KICK", 6, Math.min(h - 4, yk + 14));
  // Hits the sensitivity drops, where they were played (as drumHits places them).
  const need = strengthNeeded(s.sensitivity);
  const t = voice.take;
  const bps = state.project.transport.bpm / 60;
  let t0 = 0;
  if (voice.aligned) {
    for (const hit of t.hits) {
      if (hit.strength >= need) {
        t0 = hit.time;
        break;
      }
    }
  }
  g.fillStyle = "rgba(163, 151, 128, 0.35)";
  for (const hit of t.hits) {
    if (hit.strength >= need) continue;
    const beat = voice.origin + (hit.time - t0) * bps;
    const x = (beat / r.length) * w;
    if (x < 0 || x > w) continue;
    g.beginPath();
    g.arc(x, toneY(hit.tone, h), 2.5, 0, Math.PI * 2);
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
    });
    b.on("pointerdown", (e) => clickHit(e));
    if (voice.take.mode === "drums") b.on("pointerenter", (e) => hint("Click a hit to make it the next drum (kick → snare → hat)"));
  } else {
    b.open("div", "empty", "voice-empty");
    glyph(b, "mic");
    const drums = voice.mode === "drums";
    b.leaf(
      "div",
      "t",
      "voice-empty-title",
      voice.status === "recording" ? (drums ? "Beatbox away…" : "Sing away…") : drums ? "Beatbox a loop" : "Sing, hum or whistle a melody"
    );
    b.leaf(
      "div",
      "d",
      "voice-empty-doc",
      drums
        ? "Kicks (a low “b” or “boom”), snares (“pf”, “k”) and hats (“ts”, “t”) become a drum loop on Atelier channels. Record a take, or open a recording."
        : "The notes come out on the beat grid, snapped to a key and scale — a pattern for the piano roll. Record a take, or open a recording."
    );
    b.close();
  }
  b.close();
}

// ------------------------------------------------------------------ view

/** function summary(r: VoiceResult) => String */
function summary(r) {
  if (voice.status === "analyzing") return "Listening to the take…";
  if (voice.take.mode === "") return "No take yet";
  const bars = r.length / state.project.transport.beatsPerBar;
  const barText = `${fmt(bars, Math.abs(bars - Math.round(bars)) < 1e-9 ? 0 : 2)} bar${bars === 1 ? "" : "s"}`;
  if (voice.take.mode === "drums") {
    const count = (lane) => r.notes.filter((n) => n.lane === lane).length;
    return `${count("kick")} kicks · ${count("snare")} snares · ${count("hat")} hats · ${barText}`;
  }
  const k = resolveKey(voice.take, voice.settings);
  const scale = voice.settings.scale === "chromatic" ? "chromatic" : `${KEY_NAMES[k.key]} ${scaleLabel(k.scale).toLowerCase()}`;
  return `${r.notes.length} notes · ${scale} · ${barText}`;
}

/** function voicePanel(b: Builder) => Undefined */
export function voicePanel(b) {
  const r = voiceResult();
  const rec = voice.status === "recording";
  b.open("div", "voice", `voice ${voice.mode}`);
  b.open("div", "grid", "voice-grid");

  b.open("div", "side", "voice-side");
  b.open("div", "mode", "seg");
  button(b, "melody", voice.mode === "melody" ? "small on" : "small", "Melody", "Sing, hum or whistle: notes for the piano roll", () => setVoiceMode("melody"));
  button(b, "drums", voice.mode === "drums" ? "small on" : "small", "Beatbox", "Vocal percussion: a kick / snare / hat loop", () => setVoiceMode("drums"));
  b.close();
  b.open("div", "rec", "voice-rec");
  iconButton(
    b,
    "btn",
    rec ? "voice-recbtn armed" : "voice-recbtn",
    rec ? "stop" : "record",
    rec ? "Stop and analyze the take" : "Record a take from the microphone",
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
    rec ? `${Math.floor(secs / 60)}:${String(Math.floor(secs % 60)).padStart(2, "0")}` : voice.status === "analyzing" ? "Analyzing…" : "Record"
  );
  b.close();
  choice(
    b,
    "along",
    "Play along",
    voice.playAlong,
    ["off", "pattern", "song"],
    ["Silent", "Pattern", "Song"],
    "Hear the pattern or the song while recording (the take keeps its place in time)",
    (v) => {
      voice.playAlong = v;
    }
  );
  /** const takes: String[] */
  const takes = [""];
  /** const takeLabels: String[] */
  const takeLabels = ["Earlier take…"];
  // The new take may not be in the samples list yet.
  const paths = state.samples.includes(voice.path) || voice.path === "" ? state.samples : state.samples.concat([voice.path]);
  for (const path of paths) {
    takes.push(path);
    takeLabels.push(path.split("/").pop() ?? path);
  }
  choice(b, "take", "Take", voice.path, takes, takeLabels, "Analyze an earlier recording (any sample of the project)", (v) => pickTake(v));
  b.open("div", "acts", "voice-actions");
  button(b, "open", "small", "Open a recording…", "Analyze an audio file from this device (it is added to the project's samples)", () => openRecording());
  if (voice.path !== "") {
    button(b, "again", "small", "Analyze again", "Run the analysis on this take again (in the current mode)", () => analyzeAgain());
    button(b, "listen", voice.playing ? "small on" : "small", voice.playing ? "Stop" : "Listen", "Play the recording", () => {
      if (voice.playing) {
        stopPreview();
        voice.playing = false;
        return undefined;
      }
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
    });
  }
  b.close();
  b.close();

  settingsView(b);

  b.open("div", "main", "voice-main");
  previewView(b, r);
  b.open("div", "foot", "voice-foot");
  b.leaf("div", "sum", "voice-summary", summary(r));
  targetView(b);
  button(
    b,
    "add",
    r.notes.length > 0 ? "gold" : "",
    "Add to song",
    "A new pattern with these notes, placed on the playlist at the playhead (Ctrl+Z undoes it)",
    () => addToSong()
  );
  b.close();
  b.close();

  b.close();
  b.close();
}

/** Dock tab tools: nothing to add beyond the panel's own. */
/** function voiceTools(b: Builder) => Undefined */
export function voiceTools(b) {
  b.leaf("span", "l", "label", voice.mode === "drums" ? "Beatbox → drum loop" : "Voice → notes");
}
