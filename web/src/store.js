// Application state, undo/redo and change notification.
//
// State flows down into the view functions; there are no observers. Every
// change calls `invalidate()`, which marks the UI tree; the next animation
// frame rebuilds all descriptions and reconciles them (see ui/tree.js).

import { debounce } from "#platform";
import { decodeProject, emptyProject, projectJson } from "./model.js";
import { insertIx, insertIndex, trackIx, noteIndex } from "#brands";

export const state = {
  project /*: Project */: emptyProject(),
  rev /*: Number */: 0,
  loaded: false,
  catalog /*: Catalog */: { devices: [], plugins: [] },
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
  position /*: Number */: 0,
  loopLength /*: Number */: 0,
  meters /*: Number[] */: [],
  chMeters /*: Number[] */: [],
  snap /*: Number */: 0.25,
  follow: true,
  output: "browser",
  nativeAvailable: false,
  nativeEnabled: false,
  nativeDevice: "",
  audioReady: false,
  connected: false,
  diskIssues /*: Issue[] */: [],
  recording: false,
  selection /*: NoteIx[] */: [],
  hint: "",
};

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

/** Set by the network and audio layers. */
export const hooks = {
  /** @type {() => Undefined} */
  mark: null,
  /** @type {(String) => Undefined} */
  sync: null,
  /** @type {(String) => Undefined} */
  engine: null,
  /** @type {() => Undefined} */
  context: null,
};

// ------------------------------------------------------------------ editing

/** const undoStack: String[] */
const undoStack = [];
/** const redoStack: String[] */
const redoStack = [];
const MAX_UNDO = 200;

const syncSoon = debounce(120, () => {
  if (hooks.sync) hooks.sync(projectJson(state.project));
  return undefined;
});

let engineQueued = false;

function pushToEngine() {
  if (engineQueued) return;
  engineQueued = true;
  setTimeout(() => {
    engineQueued = false;
    if (hooks.engine) hooks.engine(projectJson(state.project));
    return undefined;
  }, 30);
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
  return undefined;
}

/** Start a gesture (drag): one undo step for many `change` calls. */
export function begin() {
  snapshot();
}

/** Report a mutation made during a gesture. `structural` re-renders views. */
/** function changed(structural: Boolean) => Undefined */
export function changed(structural) {
  syncSoon();
  pushToEngine();
  if (structural) invalidate();
  else tick();
  return undefined;
}

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
  fixSelection();
  pushToEngine();
  invalidate();
  return undefined;
}

/** Replace the project without an undo step (initial load). */
/** function load(p: Project) => Undefined */
export function load(p) {
  state.project = p;
  state.loaded = true;
  undoStack.length = 0;
  redoStack.length = 0;
  fixSelection();
  pushToEngine();
  invalidate();
  return undefined;
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
  return undefined;
}

/** function selectChannel(id: String) => Undefined */
export function selectChannel(id) {
  state.channel = id;
  reportContext();
  invalidate();
  return undefined;
}

/** function selectInsert(i: InsertIx) => Undefined */
export function selectInsert(i) {
  state.insert = i;
  reportContext();
  invalidate();
  return undefined;
}

/** function showDock(name: String) => Undefined */
export function showDock(name) {
  state.dock = name;
  reportContext();
  invalidate();
  return undefined;
}

const contextSoon = debounce(250, () => {
  if (hooks.context) hooks.context();
  return undefined;
});

export function reportContext() {
  contextSoon();
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
  return undefined;
}
