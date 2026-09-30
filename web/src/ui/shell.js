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
import { layoutState, sideMode, workMode, sideSizes, dockBasis, paneControls, paneHeader, paneRail, openDock, setWork, toggleWorkMax, saveLayout } from "./panes.js";

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
    return undefined;
  });
  glyph(b, icon);
  b.leaf("span", "l", "", label);
  b.leaf("kbd", "k", "", key);
  b.close();
  return undefined;
}

/** function studio(b: Builder) => Undefined */
export function studio(b) {
  const sizes = sideSizes(window.innerWidth);
  b.open("div", "studio", layoutState.dragging ? "studio dragging" : "studio");
  b.style("--browser-col", px(sizes.browserCol));
  b.style("--browser-w", px(sizes.browserW));
  b.style("--agent-col", px(sizes.agentCol));
  b.style("--agent-w", px(sizes.agentW));
  b.style("--dock-h", dockBasis());
  topbar(b);

  // Side panels sit in a clipping column; the rail shows when minimized.
  b.open("div", "browser-side", `side side-browser ${sideMode("browser")}`);
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
    b.leaf("b", "t", "", "Reconnecting to the Rosaclef server…");
    b.close();
  }

  const plMode = workMode("playlist");
  b.open("section", "top", `pane pane-top ${plMode}`);
  b.on("pointerdown", (e) => setFocus("playlist"));
  b.open("div", "tabs", "tabs");
  paneHeader(b, "playlist");
  b.open("div", "t", "tab on");
  b.attr("title", plMode === "min" ? "Playlist — click to restore" : "Playlist — double-click to maximize");
  b.on("click", (e) => {
    if (workMode("playlist") === "min") setWork("playlist", "open");
    return undefined;
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
      return undefined;
    }, (u) => {
      layoutState.dragging = false;
      saveLayout();
      invalidate();
      return undefined;
    });
    return undefined;
  });

  b.open("section", "dock", `pane pane-dock ${workMode("dock")}`);
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

  b.open("div", "agent-side", `side side-agent ${sideMode("agent")}`);
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
      return undefined;
    }, (u) => {
      layoutState.dragging = false;
      layoutState.agentW = sideSizes(window.innerWidth).agentW;
      saveLayout();
      invalidate();
      return undefined;
    });
    return undefined;
  });
  b.close();

  b.open("footer", "hint", "hintbar");
  b.leaf("span", "h", "hint", state.hint !== "" ? state.hint : "Space plays · F6 rack · F7 piano roll · F9 mixer · Ctrl+Z undoes the agent too · Ctrl+Alt+B/P/D/A folds the panels");
  b.open("span", "m1", "meta");
  b.leaf("span", "dot", state.connected ? "status-dot live" : "status-dot bad", "");
  b.leaf("span", "t", "", state.connected ? "Synced" : "Offline");
  b.close();
  b.leaf("span", "m2", "meta", state.output === "native" ? `Studio engine${state.nativeDevice !== "" ? " · " + state.nativeDevice : ""}` : state.audioReady ? "Browser engine · WebAssembly" : "Click anywhere to start audio");
  b.leaf("span", "m3", "meta", `rev ${state.rev}`);
  b.close();

  projectsOverlay(b);
  toastView(b);
  automationMenu(b);
  b.close();
  return undefined;
}
