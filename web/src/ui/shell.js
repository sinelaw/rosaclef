// The studio shell: composes every panel into one description tree.

import { drag, fmt, releaseFocus } from "#platform";
import { state, setFocus, dockName } from "../store.js";
import { scoreView, scoreTools, topScore, dockScore } from "./score.js";
import { topbar } from "./topbar.js";
import { browser } from "./browser.js";
import { rack, rackTools } from "./rack.js";
import { pianoRoll, pianoTools, mainPiano } from "./pianoroll.js";
import { playlist, playlistTools, mainPlaylist } from "./playlist.js";
import { mixer, mixerTools } from "./mixer.js";
import { voicePanel, voiceTools } from "./voice.js";
import { drumsPanel, drumsTools } from "./drums.js";
import { agentPanel, agentDot } from "./agent.js";
import { toastView } from "./toast.js";
import { automationMenu } from "./lanes.js";
import { meterMenu } from "./meter.js";
import { projectsOverlay } from "./projects.js";
import { creditsOverlay } from "./credits.js";
import { glyph } from "./widgets.js";
import { keyboard, keyboardStrip } from "./keyboard.js";
import {
  layoutState,
  sideMode,
  workMode,
  sideSizes,
  dockBasis,
  paneControls,
  paneHeader,
  paneRail,
  openDock,
  setWork,
  toggleWorkMax,
  startDockResize,
  resizeDock,
  endDockResize,
  startAgentResize,
  resizeAgent,
  endAgentResize,
  isCompact,
  setView,
  setTop,
} from "./panes.js";
import { t, tf } from "../i18n.js";
import { DOCKS, dockInfo } from "../docks.js";

/** A dock tab's views: what it shows, and its tools in the tab strip. */
/** type DockView = { id: String, body: (Builder) => Undefined, tools: (Builder) => Undefined } */

/** The views of each dock tab (the tabs themselves are listed in ../docks.js). */
/** const DOCK_VIEWS: DockView[] */
const DOCK_VIEWS = [
  { id: "rack", body: rack, tools: rackTools },
  { id: "piano", body: (b) => pianoRoll(b, mainPiano), tools: (b) => pianoTools(b, mainPiano) },
  { id: "voice", body: voicePanel, tools: voiceTools },
  { id: "drums", body: drumsPanel, tools: drumsTools },
  { id: "mixer", body: mixer, tools: mixerTools },
  { id: "score", body: (b) => scoreView(b, dockScore), tools: (b) => scoreTools(b, dockScore) },
];

/** The views of the dock tab with this id (the first's for an unknown one). */
/** function dockView(id: String) => DockView */
function dockView(id) {
  for (const v of DOCK_VIEWS) {
    if (v.id === id) return v;
  }
  return DOCK_VIEWS[0];
}

/** function px(v: Number) => String */
function px(v) {
  return `${fmt(v, 0)}px`;
}

/** A press in the playlist or the dock: that panel gets the studio's keys. Off a control (a field,
 * a menu, a button), it also takes the keyboard back from where it was (the agent's terminal, a
 * field), so Space plays and stops, Delete and the arrows edit, right away. */
/** function paneDown(e: Ev, name: String) => Undefined */
function paneDown(e, name) {
  setFocus(name);
  if (!e.typing && !e.onControl) releaseFocus();
}

/** function tab(b: Builder, id: String, label: String, icon: String, key: String) => Undefined */
function tab(b, id, label, icon, key) {
  b.open("button", id, state.dock === id ? "tab on" : "tab");
  b.attr("title", workMode("dock") === "min" ? tf("shell.dock.tab.restore.title", [label, key]) : tf("shell.dock.tab.maximize.title", [label, key]));
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

/** A tab of the top pane: the playlist or the score. */
/** function topTab(b: Builder, id: String, label: String, icon: String, mode: String) => Undefined */
function topTab(b, id, label, icon, mode) {
  const on = layoutState.top === id;
  b.open("button", `t-${id}`, on ? "tab on" : "tab");
  b.attr(
    "title",
    mode === "min"
      ? tf("shell.top.tab.restore.title", [label])
      : on
        ? tf("shell.top.tab.maximize.title", [label])
        : id === "score"
          ? t("shell.top.tab.score.title")
          : t("shell.top.tab.playlist.title")
  );
  b.on("click", (e) => setTop(id));
  glyph(b, icon);
  b.leaf("span", "l", "", label);
  b.close();
}

/** The phone layout's bottom navigation: one view at a time. */
/** function navBar(b: Builder) => Undefined */
function navBar(b) {
  const v = layoutState.view;
  b.open("nav", "nav", "navbar");
  navItem(b, "browser", t("panel.browser"), "folder", v === "browser", "", () => setView("browser"));
  navItem(b, "playlist", t("panel.playlist"), "playlist", v === "playlist", "", () => setView("playlist"));
  for (const d of DOCKS) {
    navItem(b, d.id, t(d.navLabel), d.icon, v === "dock" && state.dock === d.id, "", () => openDock(d.id));
  }
  navItem(b, "agent", t("panel.maestro"), "spark", v === "agent", agentDot(), () => setView("agent"));
  b.close();
}

/** function studio(b: Builder) => Undefined */
export function studio(b) {
  const sizes = sideSizes(window.innerWidth);
  // A phone shows one view at a time; its panels are never folded.
  const compact = isCompact(window.innerWidth, window.innerHeight);
  // On a phone the keys show under the editors that use them: not over the
  // browser or the terminal, nor under the mixer or the Voice panel.
  const keys = keyboard.shown && (!compact || layoutState.view === "playlist" || (layoutState.view === "dock" && dockInfo(state.dock).keys));
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
  paneRail(b, "browser", t("panel.browser"), "folder", "");
  b.close();

  b.open("main", "work", "workspace");
  if (state.diskIssues.length > 0) {
    const first = state.diskIssues[0];
    b.open("div", "banner", "banner");
    b.leaf("b", "t", "", t("shell.banner.diskErrors"));
    b.leaf("code", "c", "", `${first.path}: ${first.message}`);
    b.close();
  }
  if (!state.connected) {
    b.open("div", "offline", "banner");
    b.leaf("b", "t", "", state.loaded ? t("shell.banner.reconnecting") : t("shell.banner.starting"));
    b.close();
  }

  const plMode = compact ? "open" : workMode("playlist");
  const top = layoutState.top;
  b.open("section", "top", `pane pane-top ${plMode}`);
  b.on("pointerdown", (e) => paneDown(e, top === "score" ? "score" : "playlist"));
  b.open("div", "tabs", "tabs");
  paneHeader(b, "playlist");
  topTab(b, "playlist", t("panel.playlist"), "playlist", plMode);
  topTab(b, "score", t("panel.score"), "score", plMode);
  b.open("div", "tools", "tools");
  if (top === "score") scoreTools(b, topScore);
  else playlistTools(b);
  b.close();
  paneControls(b, "playlist");
  b.close();
  b.open("div", "body", "dock-body");
  if (top === "score") scoreView(b, topScore);
  else playlist(b, mainPlaylist);
  b.close();
  b.close();

  b.leaf("div", "split", layoutState.work === "split" ? "splitter" : "splitter off", "");
  b.on("pointerdown", (e) => {
    e.preventDefault();
    const y0 = e.clientY;
    const h0 = layoutState.dockH;
    const total = Math.max(200, window.innerHeight - 86);
    startDockResize();
    drag(
      e,
      (m) => resizeDock(h0 + ((y0 - m.clientY) / total) * 100),
      (u) => endDockResize()
    );
  });

  b.open("section", "dock", `pane pane-dock ${compact ? "open" : workMode("dock")}`);
  b.on("pointerdown", (e) => paneDown(e, dockName(state.dock)));
  b.open("div", "tabs", "tabs");
  paneHeader(b, "dock");
  for (const d of DOCKS) tab(b, d.id, t(d.label), d.icon, d.key);
  const view = dockView(state.dock);
  b.open("div", "tools", "tools");
  view.tools(b);
  b.close();
  paneControls(b, "dock");
  b.close();
  b.open("div", "body", "dock-body");
  if (state.dock !== "piano") state.viewport.prOn = false;
  view.body(b);
  b.close();
  b.close();
  b.close();

  b.open("div", "agent-side", `side side-agent ${compact ? "open" : sideMode("agent")}`);
  agentPanel(b);
  paneRail(b, "agent", t("panel.maestro"), "spark", agentDot());
  b.leaf("div", "resize", "agent-resize", "");
  b.attr("title", t("shell.agent.resize.title"));
  b.on("pointerdown", (e) => {
    e.preventDefault();
    const x0 = e.clientX;
    const w0 = sizes.agentCol;
    startAgentResize(w0);
    drag(
      e,
      (m) => resizeAgent(w0 + (x0 - m.clientX)),
      (u) => endAgentResize(window.innerWidth)
    );
  });
  b.close();

  if (keys) keyboardStrip(b, compact);
  if (compact) navBar(b);
  else hintBar(b);

  projectsOverlay(b);
  creditsOverlay(b);
  toastView(b);
  automationMenu(b);
  meterMenu(b);
  b.close();
}

/** The desktop's bottom line: hover hints and status. */
/** function hintBar(b: Builder) => Undefined */
function hintBar(b) {
  b.open("footer", "hint", "hintbar");
  b.leaf("span", "h", "hint", state.hint !== "" ? state.hint : t("shell.hintbar.default"));
  b.open("span", "m1", "meta");
  b.leaf("span", "dot", state.connected ? "status-dot live" : "status-dot bad", "");
  b.leaf(
    "span",
    "t",
    "",
    !state.connected ? t("shell.status.offline") : state.backend === "local" ? t("shell.status.savedInBrowser") : t("shell.status.synced")
  );
  b.close();
  b.leaf(
    "span",
    "m2",
    "meta",
    state.output === "native"
      ? state.nativeDevice !== ""
        ? tf("shell.engine.nativeDevice", [state.nativeDevice])
        : t("shell.engine.native")
      : state.audioReady
        ? t("shell.engine.browser")
        : t("shell.engine.clickToStart")
  );
  b.leaf("span", "m3", "meta", tf("shell.revision", [String(state.rev)]));
  b.close();
}
