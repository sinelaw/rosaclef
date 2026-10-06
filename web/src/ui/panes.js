// Panel window controls — minimize / maximize / restore for the browser
// (left), the playlist, the bottom dock and the agent panel (right).
//
// This module is the one place for the studio's layout state. It is
// persisted per browser and read by the shell, which turns it into grid
// column widths and a dock height (CSS transitions do the animation).
//
//  - Side panels (browser, agent) are "open", "min" (a slim rail that
//    restores on click) or "max" (much wider). Only one side can be
//    maximized; while it is, the other side shows as its rail.
//  - The playlist and the dock share the workspace column: `work` says which
//    of them fills it ("split" = both). Minimizing one is maximizing the
//    other; a collapsed pane keeps its header (tab strip).
//  - On a phone (a window too small for the three columns) the studio shows
//    one view at a time — browser, playlist, dock or agent — picked from a
//    bottom navigation bar; `view` says which.

import { loadPref, savePref } from "#platform";
import { invalidate, hint, showDock, selectInsert } from "../store.js";
import { insertIndex } from "#brands";
import { glyph } from "./widgets.js";
import { t, tf } from "../i18n.js";

/** Width of a collapsed side panel (its rail). */
export const RAIL = 36;
/** Width of the open browser. */
export const BROWSER_W = 236;
/** Height of a collapsed workspace pane: its tab strip (plus the dock's top border). */
export const STRIP = 38;
/** Narrowest workspace a side panel may leave. */
const MIN_WORK = 340;
/** Below this width (or height) the studio switches to its one-view phone layout. */
const COMPACT_W = 900;
const COMPACT_H = 480;

export const layoutState = {
  dockH: 46,
  agentW: 460,
  browser: "open",
  agent: "open",
  work: "split",
  view: "playlist",
  /** What the top pane shows: the "playlist" or the "score". */
  top: "playlist",
  dragging: false,
};

const PREFIX = "rosaclef.layout.";

/** function isMode(v: String) => Boolean */
function isMode(v) {
  return v === "open" || v === "min" || v === "max";
}

/** function isView(v: String) => Boolean */
function isView(v) {
  return v === "browser" || v === "playlist" || v === "dock" || v === "agent";
}

/** Read the persisted layout (ignores anything malformed). */
export function loadLayout() {
  const br = loadPref(PREFIX + "browser");
  if (isMode(br)) layoutState.browser = br;
  const ag = loadPref(PREFIX + "agent");
  if (isMode(ag)) layoutState.agent = ag;
  if (layoutState.browser === "max" && layoutState.agent === "max") layoutState.agent = "open";
  const w = loadPref(PREFIX + "work");
  if (w === "split" || w === "playlist" || w === "dock") layoutState.work = w;
  const dh = Number(loadPref(PREFIX + "dockH"));
  if (dh >= 18 && dh <= 82) layoutState.dockH = dh;
  const aw = Number(loadPref(PREFIX + "agentW"));
  if (aw >= 300 && aw <= 1600) layoutState.agentW = aw;
  const v = loadPref(PREFIX + "view");
  if (isView(v)) layoutState.view = v;
  const top = loadPref(PREFIX + "top");
  if (top === "playlist" || top === "score") layoutState.top = top;
}

export function saveLayout() {
  savePref(PREFIX + "browser", layoutState.browser);
  savePref(PREFIX + "agent", layoutState.agent);
  savePref(PREFIX + "work", layoutState.work);
  savePref(PREFIX + "dockH", String(Math.round(layoutState.dockH * 10) / 10));
  savePref(PREFIX + "agentW", String(Math.round(layoutState.agentW)));
  savePref(PREFIX + "view", layoutState.view);
  savePref(PREFIX + "top", layoutState.top);
}

/** Show the playlist or the score in the top pane (restoring the pane if it was minimized). */
/** function setTop(v: String) => Undefined */
export function setTop(v) {
  layoutState.top = v;
  if (workMode("playlist") === "min") layoutState.work = "split";
  changedLayout();
}

function changedLayout() {
  saveLayout();
  invalidate();
}

// ------------------------------------------------------------------ state

/** The stored mode of a side panel ("browser" or "agent"). */
/** function storedSide(id: String) => String */
function storedSide(id) {
  return id === "browser" ? layoutState.browser : layoutState.agent;
}

/** How a side panel shows: a maximized side turns the other one into its rail. */
/** function sideMode(id: String) => String */
export function sideMode(id) {
  const other = id === "browser" ? layoutState.agent : layoutState.browser;
  const own = storedSide(id);
  if (other === "max" && own !== "max") return "min";
  return own;
}

/** How a workspace pane ("playlist" or "dock") shows: "open", "min" or "max". */
/** function workMode(id: String) => String */
export function workMode(id) {
  if (layoutState.work === "split") return "open";
  return layoutState.work === id ? "max" : "min";
}

/** function setSide(id: String, mode: String) => Undefined */
export function setSide(id, mode) {
  // Showing a side un-maximizes the other one (it would hide this one).
  if (mode !== "min") {
    if (id === "browser" && layoutState.agent === "max") layoutState.agent = "open";
    if (id === "agent" && layoutState.browser === "max") layoutState.browser = "open";
  }
  if (id === "browser") layoutState.browser = mode;
  else layoutState.agent = mode;
  changedLayout();
}

/** Minimize ⇄ restore. */
/** function toggleSide(id: String) => Undefined */
export function toggleSide(id) {
  setSide(id, sideMode(id) === "min" ? "open" : "min");
}

/** Maximize ⇄ restore. */
/** function toggleSideMax(id: String) => Undefined */
export function toggleSideMax(id) {
  setSide(id, sideMode(id) === "max" ? "open" : "max");
}

/** Put a workspace pane in a mode; the other pane takes the complement. */
/** function setWork(id: String, mode: String) => Undefined */
export function setWork(id, mode) {
  const other = id === "playlist" ? "dock" : "playlist";
  if (mode === "max") layoutState.work = id;
  else if (mode === "min") layoutState.work = other;
  else layoutState.work = "split";
  changedLayout();
}

/** function toggleWorkMax(id: String) => Undefined */
export function toggleWorkMax(id) {
  setWork(id, workMode(id) === "max" ? "open" : "max");
}

/** Any panel: toggle maximize (double-click on a header). */
/** function toggleMax(id: String) => Undefined */
export function toggleMax(id) {
  if (id === "browser" || id === "agent") toggleSideMax(id);
  else toggleWorkMax(id);
}

/** Show a dock tab, restoring the dock if it was minimized (F6/F7/F9, tab clicks). */
/** function openDock(name: String) => Undefined */
export function openDock(name) {
  if (workMode("dock") === "min") setWork("dock", "open");
  revealDock(name);
}

/** Show a dock tab when asked to from another panel: on a phone this switches to the dock view. */
/** function revealDock(name: String) => Undefined */
export function revealDock(name) {
  if (layoutState.view !== "dock") {
    layoutState.view = "dock";
    saveLayout();
  }
  showDock(name);
}

/** The mixer insert whose strip scrolls into view on the mixer's next draw (-1: none). */
export const reveal = { insert: -1 };

/** Show an insert in the mixer (a channel's, from the rack or the score): select it, open the mixer and bring its strip into view. */
/** function showInsert(ix: InsertIx) => Undefined */
export function showInsert(ix) {
  selectInsert(ix);
  reveal.insert = insertIndex(ix);
  openDock("mixer");
}

/** Phone layout: show one view ("browser", "playlist", "dock" or "agent"). */
/** function setView(v: String) => Undefined */
export function setView(v) {
  layoutState.view = v;
  changedLayout();
}

/** Everything back to the default split (keeps the sizes). */
export function restoreAll() {
  layoutState.browser = "open";
  layoutState.agent = "open";
  layoutState.work = "split";
  changedLayout();
}

/** Ctrl+Alt+<key> panel shortcuts; `code` is a KeyboardEvent code. Returns true when handled. */
/** function paneShortcut(code: String) => Boolean */
export function paneShortcut(code) {
  if (code === "KeyB") toggleSide("browser");
  else if (code === "KeyA") toggleSide("agent");
  else if (code === "KeyP") toggleWorkMax("playlist");
  else if (code === "KeyD") toggleWorkMax("dock");
  else if (code === "Digit0") restoreAll();
  else return false;
  return true;
}

// ------------------------------------------------------------------ geometry

/** Whether a window of this size gets the one-view phone layout. */
/** function isCompact(winW: Number, winH: Number) => Boolean */
export function isCompact(winW, winH) {
  return winW < COMPACT_W || winH < COMPACT_H;
}

/** type SideSizes = { browserCol: Number, browserW: Number, agentCol: Number, agentW: Number } */

/**
 * Pixel widths for the side panels given the window width. `*Col` is the
 * grid column (animated); `*W` is the panel's own width, which stays put
 * while the column animates or is collapsed, so the terminal is not refitted
 * to a sliver (the column clips it instead).
 */
/** function sideSizes(winW: Number) => SideSizes */
export function sideSizes(winW) {
  const w = Math.max(720, winW);
  const br = sideMode("browser");
  const ag = sideMode("agent");
  const agentMin = ag === "min" ? RAIL : 300;
  const browserMax = Math.max(BROWSER_W, Math.min(w * 0.5, w - agentMin - MIN_WORK));
  const browserCol = br === "min" ? RAIL : br === "max" ? browserMax : BROWSER_W;
  // A hidden agent keeps the width it will have when restored (which un-maximizes the browser).
  const agentOpen = Math.max(300, Math.min(layoutState.agentW, w - (ag === "min" && br === "max" ? BROWSER_W : browserCol) - MIN_WORK));
  const agentMax = Math.max(agentOpen, Math.min(w * 0.7, w - browserCol - MIN_WORK));
  const agentCol = ag === "min" ? RAIL : ag === "max" ? agentMax : agentOpen;
  return {
    browserCol: browserCol,
    browserW: br === "max" ? browserMax : BROWSER_W,
    agentCol: agentCol,
    agentW: ag === "max" ? agentMax : agentOpen,
  };
}

/** The dock's flex basis (CSS length) for the current workspace mode. */
/** function dockBasis() => String */
export function dockBasis() {
  if (layoutState.work === "playlist") return `${STRIP + 1}px`;
  if (layoutState.work === "dock") return `calc(100% - ${STRIP}px)`;
  return `${Math.round(layoutState.dockH * 100) / 100}%`;
}

// ------------------------------------------------------------------ views

/** function keyOf(id: String) => String */
function keyOf(id) {
  if (id === "browser") return "Ctrl+Alt+B";
  if (id === "agent") return "Ctrl+Alt+A";
  if (id === "playlist") return "Ctrl+Alt+P";
  return "Ctrl+Alt+D";
}

/** One small gold-line window button. */
/** function winButton(b: Builder, key: String, icon: String, tip: String, onClick: () => Undefined) => Undefined */
function winButton(b, key, icon, tip, onClick) {
  b.open("button", key, `winbtn win-${icon}`);
  b.attr("title", tip);
  b.attr("aria-label", tip);
  b.on("pointerenter", (e) => hint(tip));
  b.on("click", (e) => {
    e.stopPropagation();
    onClick();
    // The button under the pointer now does something else; drop the stale tip.
    hint("");
  });
  // A quick double click on a button must not also maximize via the header.
  b.on("dblclick", (e) => {
    e.stopPropagation();
  });
  glyph(b, icon);
  b.close();
}

/** Minimize / maximize / restore buttons for a panel header. */
/** function paneControls(b: Builder, id: String) => Undefined */
export function paneControls(b, id) {
  const side = id === "browser" || id === "agent";
  const mode = side ? sideMode(id) : workMode(id);
  const key = keyOf(id);
  const br = id === "browser";
  const pl = id === "playlist";
  b.open("div", "winctl", "winctl");
  if (side) {
    const minTip = br ? tf("panes.browser.minimize.title", [key]) : tf("panes.agent.minimize.title", [key]);
    winButton(b, "min", "minimize", minTip, () => setSide(id, "min"));
    if (mode === "max") {
      const tip = br ? t("panes.browser.restore.title") : t("panes.agent.restore.title");
      winButton(b, "max", "restore", tip, () => setSide(id, "open"));
    } else {
      const tip = br ? t("panes.browser.maximize.title") : t("panes.agent.maximize.title");
      winButton(b, "max", "maximize", tip, () => setSide(id, "max"));
    }
  } else {
    if (mode === "min") {
      const tip = pl ? t("panes.playlist.restore.title") : t("panes.dock.restore.title");
      winButton(b, "min", "restore", tip, () => setWork(id, "open"));
    } else {
      const tip = pl ? t("panes.playlist.minimize.title") : t("panes.dock.minimize.title");
      winButton(b, "min", "minimize", tip, () => setWork(id, "min"));
    }
    if (mode === "max") {
      const tip = pl ? tf("panes.playlist.restoreShortcut.title", [key]) : tf("panes.dock.restoreShortcut.title", [key]);
      winButton(b, "max", "restore", tip, () => setWork(id, "open"));
    } else {
      const tip = pl ? tf("panes.playlist.maximize.title", [key]) : tf("panes.dock.maximize.title", [key]);
      winButton(b, "max", "maximize", tip, () => setWork(id, "max"));
    }
  }
  b.close();
}

/** Double-clicking a header (not one of its controls) toggles maximize. Call right after opening the header. */
/** function paneHeader(b: Builder, id: String) => Undefined */
export function paneHeader(b, id) {
  b.on("dblclick", (e) => {
    if (!e.onControl) toggleMax(id);
  });
}

/** The slim rail a minimized side panel collapses to; clicking it restores the panel. */
/** function paneRail(b: Builder, id: String, title: String, icon: String, dot: String) => Undefined */
export function paneRail(b, id, title, icon, dot) {
  const tip = id === "browser" ? tf("panes.browser.rail.title", [keyOf(id)]) : tf("panes.agent.rail.title", [keyOf(id)]);
  b.open("div", "rail", `rail rail-${id}`);
  b.attr("title", tip);
  b.on("pointerenter", (e) => hint(tip));
  b.on("click", (e) => {
    setSide(id, "open");
    hint("");
  });
  b.open("div", "ctl", "winctl");
  winButton(b, "restore", "restore", tip, () => setSide(id, "open"));
  b.close();
  b.open("div", "mark", "rail-mark");
  glyph(b, icon);
  b.close();
  if (dot !== "") b.leaf("span", "dot", dot, "");
  b.leaf("span", "l", "rail-label", title);
  b.leaf("span", "line", "rail-line", "");
  b.close();
}
