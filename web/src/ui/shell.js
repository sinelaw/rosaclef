// The studio shell: composes every panel into one description tree.

import { drag, fmt } from "#platform";
import { state, invalidate, setFocus } from "../store.js";
import { topbar } from "./topbar.js";
import { browser } from "./browser.js";
import { rack, rackTools } from "./rack.js";
import { pianoRoll, pianoTools } from "./pianoroll.js";
import { playlist, playlistTools } from "./playlist.js";
import { mixer, mixerTools } from "./mixer.js";
import { agentPanel, agentDot } from "./agent.js";
import { toastView } from "./toast.js";
import { automationMenu } from "./lanes.js";
import { projectsOverlay } from "./projects.js";
import { glyph } from "./widgets.js";
import { keyboard, keyboardStrip } from "./keyboard.js";
import { layoutState, sideMode, workMode, sideSizes, dockBasis, paneControls, paneHeader, paneRail, openDock, setWork, toggleWorkMax, saveLayout, isCompact, setView } from "./panes.js";

/** function px(v: Number) => String */
function px(v) {
  return `${fmt(v, 0)}px`;
}

/** function tab(b: Builder, id: String, label: String, icon: String, key: String) => Undefined */
function tab(b, id, label, icon, key) {
  b.open("button", id, state.dock === id ? "tab on" : "tab");
  b.attr("title", workMode("dock") === "min" ? `${label} (${key}) — restores the dock` : `${label} (${key}) — double-click to maximize the dock`);
  b.on("click", (e) => openDock(id));
  b.on("dblclick", (e) => {
    e.stopPropagation();
    toggleWorkMax("dock");
  });
  glyph(b, icon);
  b.leaf("span", "l", "", label);
  b.leaf("kbd", "k", "", key);
  b.close();
}

/** One button of the phone layout's bottom navigation bar. */
/** function navItem(b: Builder, key: String, label: String, icon: String, on: Boolean, dot: String, onClick: () => Undefined) => Undefined */
function navItem(b, key, label, icon, on, dot, onClick) {
  b.open("button", key, on ? "nav-item on" : "nav-item");
  b.attr("aria-label", label);
  if (on) b.attr("aria-current", "page");
  b.on("click", (e) => onClick());
  glyph(b, icon);
  if (dot !== "") b.leaf("span", "dot", dot, "");
  b.leaf("span", "l", "", label);
  b.close();
}

/** The phone layout's bottom navigation: one view at a time. */
/** function navBar(b: Builder) => Undefined */
function navBar(b) {
  const v = layoutState.view;
  b.open("nav", "nav", "navbar");
  navItem(b, "browser", "Browser", "folder", v === "browser", "", () => setView("browser"));
  navItem(b, "playlist", "Playlist", "playlist", v === "playlist", "", () => setView("playlist"));
  navItem(b, "rack", "Rack", "rack", v === "dock" && state.dock === "rack", "", () => openDock("rack"));
  navItem(b, "piano", "Piano", "piano", v === "dock" && state.dock === "piano", "", () => openDock("piano"));
  navItem(b, "mixer", "Mixer", "mixer", v === "dock" && state.dock === "mixer", "", () => openDock("mixer"));
  navItem(b, "agent", "Maestro", "spark", v === "agent", agentDot(), () => setView("agent"));
  b.close();
}

/** function studio(b: Builder) => Undefined */
export function studio(b) {
  const sizes = sideSizes(window.innerWidth);
  // A phone shows one view at a time; its panels are never folded.
  const compact = isCompact(window.innerWidth, window.innerHeight);
  // On a phone the keys show under the editors, not over the browser or the terminal.
  const keys = keyboard.shown && (!compact || layoutState.view === "playlist" || layoutState.view === "dock");
  const base = compact ? `studio compact v-${layoutState.view}` : "studio";
  const cls = keys ? `${base} has-keys` : base;
  b.open("div", "studio", layoutState.dragging ? `${cls} dragging` : cls);
  b.style("--browser-col", px(sizes.browserCol));
  b.style("--browser-w", px(sizes.browserW));
  b.style("--agent-col", px(sizes.agentCol));
  b.style("--agent-w", px(sizes.agentW));
  b.style("--dock-h", dockBasis());
  topbar(b);

  // Side panels sit in a clipping column; the rail shows when minimized.
  b.open("div", "browser-side", `side side-browser ${compact ? "open" : sideMode("browser")}`);
  browser(b);
  paneRail(b, "browser", "Browser", "folder", "");
  b.close();

  b.open("main", "work", "workspace");
  if (state.diskIssues.length > 0) {
    const first = state.diskIssues[0];
    b.open("div", "banner", "banner");
    b.leaf("b", "t", "", "project.json on disk has errors — playing the last valid version:");
    b.leaf("code", "c", "", `${first.path}: ${first.message}`);
    b.close();
  }
  if (!state.connected) {
    b.open("div", "offline", "banner");
    b.leaf("b", "t", "", state.loaded ? "Reconnecting to the Rosaclef server…" : "Starting the studio…");
    b.close();
  }

  const plMode = compact ? "open" : workMode("playlist");
  b.open("section", "top", `pane pane-top ${plMode}`);
  b.on("pointerdown", (e) => setFocus("playlist"));
  b.open("div", "tabs", "tabs");
  paneHeader(b, "playlist");
  b.open("div", "t", "tab on");
  b.attr("title", plMode === "min" ? "Playlist — click to restore" : "Playlist — double-click to maximize");
  b.on("click", (e) => {
    if (workMode("playlist") === "min") setWork("playlist", "open");
  });
  glyph(b, "playlist");
  b.leaf("span", "l", "", "Playlist");
  b.close();
  b.open("div", "tools", "tools");
  playlistTools(b);
  b.close();
  paneControls(b, "playlist");
  b.close();
  b.open("div", "body", "dock-body");
  playlist(b);
  b.close();
  b.close();

  b.leaf("div", "split", layoutState.work === "split" ? "splitter" : "splitter off", "");
  b.on("pointerdown", (e) => {
    e.preventDefault();
    const y0 = e.clientY;
    const h0 = layoutState.dockH;
    const total = Math.max(200, window.innerHeight - 86);
    layoutState.dragging = true;
    drag(e, (m) => {
      layoutState.dockH = Math.max(18, Math.min(82, h0 + ((y0 - m.clientY) / total) * 100));
      invalidate();
    }, (u) => {
      layoutState.dragging = false;
      saveLayout();
      invalidate();
    });
  });

  b.open("section", "dock", `pane pane-dock ${compact ? "open" : workMode("dock")}`);
  b.on("pointerdown", (e) => setFocus(state.dock === "piano" ? "piano roll" : state.dock === "mixer" ? "mixer" : "channel rack"));
  b.open("div", "tabs", "tabs");
  paneHeader(b, "dock");
  tab(b, "rack", "Channel Rack", "rack", "F6");
  tab(b, "piano", "Piano Roll", "piano", "F7");
  tab(b, "mixer", "Mixer", "mixer", "F9");
  b.open("div", "tools", "tools");
  if (state.dock === "rack") rackTools(b);
  else if (state.dock === "piano") pianoTools(b);
  else mixerTools(b);
  b.close();
  paneControls(b, "dock");
  b.close();
  b.open("div", "body", "dock-body");
  if (state.dock !== "piano") state.viewport.prOn = false;
  if (state.dock === "rack") rack(b);
  else if (state.dock === "piano") pianoRoll(b);
  else mixer(b);
  b.close();
  b.close();
  b.close();

  b.open("div", "agent-side", `side side-agent ${compact ? "open" : sideMode("agent")}`);
  agentPanel(b);
  paneRail(b, "agent", "Maestro", "spark", agentDot());
  b.leaf("div", "resize", "agent-resize", "");
  b.attr("title", "Drag to resize the agent panel");
  b.on("pointerdown", (e) => {
    e.preventDefault();
    const x0 = e.clientX;
    // Dragging a maximized panel resizes it from where it is.
    const w0 = sizes.agentCol;
    if (layoutState.agent === "max") layoutState.agent = "open";
    layoutState.agentW = w0;
    layoutState.dragging = true;
    drag(e, (m) => {
      layoutState.agentW = Math.max(300, Math.min(1600, w0 + (x0 - m.clientX)));
      invalidate();
    }, (u) => {
      layoutState.dragging = false;
      layoutState.agentW = sideSizes(window.innerWidth).agentW;
      saveLayout();
      invalidate();
    });
  });
  b.close();

  if (keys) keyboardStrip(b, compact);
  if (compact) navBar(b);
  else hintBar(b);

  projectsOverlay(b);
  toastView(b);
  automationMenu(b);
  b.close();
}

/** The desktop's bottom line: hover hints and status. */
/** function hintBar(b: Builder) => Undefined */
function hintBar(b) {
  b.open("footer", "hint", "hintbar");
  b.leaf("span", "h", "hint", state.hint !== "" ? state.hint : "Space plays · F6 rack · F7 piano roll · F9 mixer · Ctrl+Z undoes the agent too · Ctrl+Alt+B/P/D/A folds the panels");
  b.open("span", "m1", "meta");
  b.leaf("span", "dot", state.connected ? "status-dot live" : "status-dot bad", "");
  b.leaf("span", "t", "", !state.connected ? "Offline" : state.backend === "local" ? "Saved in this browser" : "Synced");
  b.close();
  b.leaf("span", "m2", "meta", state.output === "native" ? `Studio engine${state.nativeDevice !== "" ? " · " + state.nativeDevice : ""}` : state.audioReady ? "Browser engine · WebAssembly" : "Click anywhere to start audio");
  b.leaf("span", "m3", "meta", `rev ${state.rev}`);
  b.close();
}
