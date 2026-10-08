// Application state, undo/redo and change notification.
//
// State flows down into the view functions; there are no observers. Every
// change calls `invalidate()`, which marks the UI tree; the next animation
// frame rebuilds all descriptions and reconciles them (see web/tree/tree.js).

import { debounce, nowIso } from "#platform";
import { decodeProject, emptyProject, projectJson, cloneProject, describeChange, defaultArpCatalog, newDevice, noArp } from "./model.js";
import { insertIx, insertIndex, trackIx, noteIndex, clipIndex } from "#brands";
import { dockInfo } from "./docks.js";
import { previewTakes } from "./preview.js";

export const state = {
  project /*: Project */: emptyProject(),
  rev: 0,
  /** Counts changes to the project (edits, undo, remote versions): views cache what they derive from it by this. */
  edits: 0,
  loaded: false,
  catalog /*: Catalog */: { devices: [], plugins: [], presets: [], arp: defaultArpCatalog(), collections: [] },
  agents /*: AgentPreset[] */: [],
  samples /*: String[] */: [],
  folder: "",
  pattern: "",
  channel: "",
  insert: insertIx(1),
  track: trackIx(0),
  dock: "rack",
  mode: "song",
  playing: false,
  position: 0,
  /** When `position` was last reported (ms, `now()`), to read the playhead between reports. */
  positionAt: 0,
  loopLength: 0,
  meters /*: Number[] */: [],
  chMeters /*: Number[] */: [],
  snap: 0.25,
  follow: true,
  /** Click on every beat while playing. */
  metronome: false,
  output: "browser",
  nativeAvailable: false,
  /** "server" (the Rosaclef server) or "local" (the browser-only studio, projects in browser storage). */
  backend: "server",
  nativeEnabled: false,
  nativeDevice: "",
  audioReady: false,
  connected: false,
  diskIssues /*: Issue[] */: [],
  recording: false,
  selection /*: NoteIx[] */: [],
  clipSelection /*: ClipIx[] */: [],
  focus: "playlist",
  /** The film on screen (Score view, Film mode), for the agent's context: the scene at the playhead. */
  film: { on: false, mode: "", shot: -1, selected: -1, start: 0, end: 0, frame: "", focus /*: String[] */: [], why: "" },
  viewport: { plStart: 0, plEnd: 0, plTrack0: 0, plTrack1: 0, prStart: 0, prEnd: 0, prLow: 0, prHigh: 0, prOn: false },
  recent /*: { at: String, summary: String }[] */: [],
  hint: "",
  /** An instrument tried out from the browser: while `on`, the piano plays it
   * instead of the selected channel. It is not part of the song: the engine
   * gets it as one more channel (`AUDITION`) that no pattern plays. `key`
   * names the browser item it came from. */
  audition: { on: false, key: "", name: "", color: "#d4af37", device: newDevice("") },
};

/** The engine's id for the instrument being tried out (never a song channel's: see `engineJson`). */
export const AUDITION = "__audition__";

// ------------------------------------------------------------------ redraw

/** Request a rebuild of the whole UI on the next frame (mark-and-flush). */
export function invalidate() {
  if (hooks.mark) hooks.mark();
}

/** Same as invalidate: descriptions are cheap, everything is rebuilt. */
export function tick() {
  if (hooks.mark) hooks.mark();
}

// ------------------------------------------------------------------ hooks

/** Set once, at start, by the network and audio layers (one each). Panels use
 * `claimPreview` (preview.js) and `onOpened` below instead. */
export const hooks = {
  /** @type {() => Undefined} */
  mark: null,
  /** @type {(String) => Undefined} */
  sync: null,
  /** @type {(String) => Undefined} */
  engine: null,
  /** @type {() => Undefined} */
  context: null,
  /** The tried-out instrument changed (audio.js tells the engine). */
  /** @type {() => Undefined} */
  audition: null,
};

// ------------------------------------------------------------------ opening

/** const openedListeners: (() => Undefined)[] */
const openedListeners = [];

/** Run `fn` whenever another project opens: panels drop what they measured
 * of the last one (the Mix check's report). */
/** function onOpened(fn: () => Undefined) => Undefined */
export function onOpened(fn) {
  openedListeners.push(fn);
}

/** Another project opened (net.js). */
export function projectOpened() {
  for (const fn of openedListeners) fn();
}

// ------------------------------------------------------------------ editing

/** const undoStack: String[] */
const undoStack = [];
/** const redoStack: String[] */
const redoStack = [];
const MAX_UNDO = 200;

/** The project as of the last edit-log entry (to describe what changed). */
const logged = { project: emptyProject() };

function logEdits() {
  const lines = describeChange(logged.project, state.project);
  logged.project = cloneProject(state.project);
  if (lines.length === 0) return;
  const at = nowIso();
  for (const l of lines) state.recent.push({ at: at, summary: l });
  while (state.recent.length > 12) state.recent.shift();
  reportContext();
}

const syncSoon = debounce(120, () => {
  if (hooks.sync) hooks.sync(projectJson(state.project));
  logEdits();
});

let engineQueued = false;

function pushToEngine() {
  if (engineQueued) return;
  engineQueued = true;
  setTimeout(() => {
    engineQueued = false;
    if (!previewTakes() && hooks.engine) hooks.engine(engineJson());
  }, 30);
}

/** The audition channel the engine plays the tried-out instrument on. */
/** function auditionChannel() => Channel */
export function auditionChannel() {
  const a = state.audition;
  return {
    id: AUDITION,
    name: a.name,
    color: a.color,
    instrument: a.device,
    volume: 0.8,
    pan: 0,
    mute: false,
    mixer: insertIx(0),
    arp: noArp(),
    layerOf: "",
  };
}

/** The project as the engine plays it: the song, and the instrument being
 * tried out (if any) on a channel of its own, after the song's channels. */
/** function engineJson() => String */
export function engineJson() {
  const p = state.project;
  if (!state.audition.on || p.channels.some((c) => c.id === AUDITION)) return projectJson(p);
  const song = p.channels;
  p.channels = song.concat([auditionChannel()]);
  const text = projectJson(p);
  p.channels = song;
  return text;
}

/** Give the engine the current project (and audition channel) soon. */
export function refreshEngine() {
  pushToEngine();
}

function snapshot() {
  undoStack.push(projectJson(state.project));
  if (undoStack.length > MAX_UNDO) undoStack.shift();
  redoStack.length = 0;
}

/** Mutate the project as one undoable step. */
/** function commit(fn: () => Undefined) => Undefined */
export function commit(fn) {
  snapshot();
  fn();
  changed(true);
}

/** Start a gesture (drag): one undo step for many `change` calls. */
export function begin() {
  snapshot();
}

/** Report a mutation made during a gesture. `structural` re-renders views. */
/** function changed(structural: Boolean) => Undefined */
export function changed(structural) {
  state.edits = state.edits + 1;
  syncSoon();
  pushToEngine();
  if (structural) invalidate();
  else tick();
}

/** How controls (widgets.js) record changes to the project: a gesture is one
 * undo step, and every change is synced, played and drawn. */
/** const projectEdit: Edit */
export const projectEdit = {
  begin: () => begin(),
  change: () => changed(true),
  commit: (fn) => commit(fn),
};

export function undo() {
  const prev = undoStack.pop();
  if (prev === undefined) return;
  redoStack.push(projectJson(state.project));
  state.project = decodeProject(JSON.parse(prev));
  fixSelection();
  changed(true);
}

export function redo() {
  const next = redoStack.pop();
  if (next === undefined) return;
  undoStack.push(projectJson(state.project));
  state.project = decodeProject(JSON.parse(next));
  fixSelection();
  changed(true);
}

/** A new version arrived from the server (the agent, the API, a recording). */
/** function applyRemote(p: Project) => Undefined */
export function applyRemote(p) {
  snapshot();
  state.project = p;
  state.edits = state.edits + 1;
  logged.project = cloneProject(p);
  fixSelection();
  pushToEngine();
  invalidate();
}

/** Replace the project without an undo step (initial load). */
/** function load(p: Project) => Undefined */
export function load(p) {
  state.project = p;
  state.edits = state.edits + 1;
  logged.project = cloneProject(p);
  state.loaded = true;
  undoStack.length = 0;
  redoStack.length = 0;
  fixSelection();
  pushToEngine();
  invalidate();
}

/** Keep selections pointing at things that exist. */
export function fixSelection() {
  const p = state.project;
  if (!p.patterns.some((x) => x.id === state.pattern)) {
    state.pattern = p.patterns.length > 0 ? p.patterns[0].id : "";
    state.selection = [];
  }
  if (!p.channels.some((c) => c.id === state.channel)) {
    state.channel = p.channels.length > 0 ? p.channels[0].id : "";
  }
  const nIns = p.mixer.inserts.length;
  if (insertIndex(state.insert) >= nIns) state.insert = insertIx(Math.max(0, nIns - 1));
  const pat = currentPattern();
  if (pat) state.selection = state.selection.filter((i) => noteIndex(i) < pat.notes.length);
  const nClips = p.playlist.clips.length;
  state.clipSelection = state.clipSelection.filter((i) => clipIndex(i) < nClips);
}

// ------------------------------------------------------------------ selection

/** function currentPattern() => Pattern? */
export function currentPattern() {
  return state.project.patterns.find((p) => p.id === state.pattern);
}

/** function currentChannel() => Channel? */
export function currentChannel() {
  return state.project.channels.find((c) => c.id === state.channel);
}

/** function selectPattern(id: String) => Undefined */
export function selectPattern(id) {
  if (state.pattern !== id) state.selection = [];
  state.pattern = id;
  reportContext();
  invalidate();
}

/** function selectChannel(id: String) => Undefined */
export function selectChannel(id) {
  state.channel = id;
  // The piano plays what was chosen last.
  if (state.audition.on) {
    state.audition.on = false;
    if (hooks.audition) hooks.audition();
  }
  reportContext();
  invalidate();
}

/** function selectInsert(i: InsertIx) => Undefined */
export function selectInsert(i) {
  state.insert = i;
  reportContext();
  invalidate();
}

/** The dock's editor as the agent's context names it. */
/** function dockName(dock: String) => String */
export function dockName(dock) {
  return dockInfo(dock).agentName;
}

/** function showDock(name: String) => Undefined */
export function showDock(name) {
  state.dock = name;
  reportContext();
  invalidate();
}

const contextSoon = debounce(250, () => {
  if (hooks.context) hooks.context();
});

export function reportContext() {
  contextSoon();
}

/** Which panel the producer is working in (reported to the agent). */
/** function setFocus(name: String) => Undefined */
export function setFocus(name) {
  if (state.focus !== name) {
    state.focus = name;
    reportContext();
  }
}

/** function deviceSpec(type: String, category: String) => DeviceSpec? */
export function deviceSpec(type, category) {
  return state.catalog.devices.find((d) => d.type === type && d.category === category);
}

/** function hint(text: String) => Undefined */
export function hint(text) {
  if (state.hint !== text) {
    state.hint = text;
    tick();
  }
}
