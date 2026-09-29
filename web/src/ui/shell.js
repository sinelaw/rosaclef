// The studio shell: composes every panel into one description tree.

import { drag } from "#platform";
import { state, showDock, invalidate, setFocus } from "../store.js";
import { topbar } from "./topbar.js";
import { browser } from "./browser.js";
import { rack, rackTools } from "./rack.js";
import { pianoRoll, pianoTools } from "./pianoroll.js";
import { playlist, playlistTools } from "./playlist.js";
import { mixer, mixerTools } from "./mixer.js";
import { agentPanel } from "./agent.js";
import { toastView } from "./toast.js";
import { glyph } from "./widgets.js";

export const layoutState = { dockH: 46, agentW: 460 };

/** function tab(b: Builder, id: String, label: String, icon: String, key: String) => Undefined */
function tab(b, id, label, icon, key) {
  b.open("button", id, state.dock === id ? "tab on" : "tab");
  b.on("click", (e) => showDock(id));
  glyph(b, icon);
  b.leaf("span", "l", "", label);
  b.leaf("kbd", "k", "", key);
  b.close();
  return undefined;
}

/** function studio(b: Builder) => Undefined */
export function studio(b) {
  b.open("div", "studio", "studio");
  b.style("--agent-w", `${layoutState.agentW}px`);
  b.style("--dock-h", `${layoutState.dockH}%`);
  topbar(b);
  browser(b);

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

  b.open("section", "top", "pane pane-top");
  b.on("pointerdown", (e) => setFocus("playlist"));
  b.open("div", "tabs", "tabs");
  b.open("div", "t", "tab on");
  glyph(b, "playlist");
  b.leaf("span", "l", "", "Playlist");
  b.close();
  b.open("div", "tools", "tools");
  playlistTools(b);
  b.close();
  b.close();
  b.open("div", "body", "dock-body");
  playlist(b);
  b.close();
  b.close();

  b.leaf("div", "split", "splitter", "");
  b.on("pointerdown", (e) => {
    e.preventDefault();
    const y0 = e.clientY;
    const h0 = layoutState.dockH;
    const total = Math.max(200, window.innerHeight - 86);
    drag(e, (m) => {
      layoutState.dockH = Math.max(18, Math.min(82, h0 + ((y0 - m.clientY) / total) * 100));
      invalidate();
      return undefined;
    }, (u) => undefined);
    return undefined;
  });

  b.open("section", "dock", "pane pane-dock");
  b.on("pointerdown", (e) => setFocus(state.dock === "piano" ? "piano roll" : state.dock === "mixer" ? "mixer" : "channel rack"));
  b.open("div", "tabs", "tabs");
  tab(b, "rack", "Channel Rack", "rack", "F6");
  tab(b, "piano", "Piano Roll", "piano", "F7");
  tab(b, "mixer", "Mixer", "mixer", "F9");
  b.open("div", "tools", "tools");
  if (state.dock === "rack") rackTools(b);
  else if (state.dock === "piano") pianoTools(b);
  else mixerTools(b);
  b.close();
  b.close();
  b.open("div", "body", "dock-body");
  if (state.dock !== "piano") state.viewport.prOn = false;
  if (state.dock === "rack") rack(b);
  else if (state.dock === "piano") pianoRoll(b);
  else mixer(b);
  b.close();
  b.close();
  b.close();

  agentPanel(b);
  b.leaf("div", "resize", "agent-resize", "");
  b.style("right", `${layoutState.agentW - 3}px`);
  b.style("left", "auto");
  b.style("position", "fixed");
  b.style("top", "60px");
  b.style("bottom", "26px");
  b.on("pointerdown", (e) => {
    e.preventDefault();
    const x0 = e.clientX;
    const w0 = layoutState.agentW;
    drag(e, (m) => {
      layoutState.agentW = Math.max(300, Math.min(900, w0 + (x0 - m.clientX)));
      invalidate();
      return undefined;
    }, (u) => undefined);
    return undefined;
  });

  b.open("footer", "hint", "hintbar");
  b.leaf("span", "h", "hint", state.hint !== "" ? state.hint : "Space plays · F6 rack · F7 piano roll · F9 mixer · Ctrl+Z undoes the agent too");
  b.open("span", "m1", "meta");
  b.leaf("span", "dot", state.connected ? "status-dot live" : "status-dot bad", "");
  b.leaf("span", "t", "", state.connected ? "Synced" : "Offline");
  b.close();
  b.leaf("span", "m2", "meta", state.output === "native" ? `Studio engine${state.nativeDevice !== "" ? " · " + state.nativeDevice : ""}` : state.audioReady ? "Browser engine · WebAssembly" : "Click anywhere to start audio");
  b.leaf("span", "m3", "meta", `rev ${state.rev}`);
  b.close();

  toastView(b);
  b.close();
  return undefined;
}
