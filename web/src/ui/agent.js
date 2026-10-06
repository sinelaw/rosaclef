// The agent panel (Maestro): a terminal running the producer's own coding
// agent (Claude Code, Codex, Gemini CLI, ...) inside the project folder, where
// AGENTS.md / CLAUDE.md teach it the project format. The browser-only studio
// has no processes to run: there the terminal runs the Rosaclef shell (the
// studio's command line, compiled into the page — see crates/local).
//
// Plugins share the panel with the terminal, one tab each: the Critic
// (./critic.js) lints the project; the Mix check (./mixcheck.js) measures it. The terminal stays mounted while another
// tab shows, so the agent keeps running.

import { connectRaw, wsUrl, createTerm, getJson } from "#platform";
import { state, invalidate, currentPattern, currentChannel, hint, setFocus } from "../store.js";
import { iconButton, button, select, glyph } from "./widgets.js";
import { paneHeader, paneControls } from "./panes.js";
import { insertIndex } from "#brands";
import { criticPanel, criticCount } from "./critic.js";
import { mixcheckPanel, mixcheckCount } from "./mixcheck.js";

const agent = {
  running: false,
  id: "",
  name: "",
  exitCode: -1,
  error: "",
  connected: false,
  choosing: false,
  autostarted: false,
};

/** The panel's plugins: the terminal, the Critic and the Mix check. */
/** const PLUGINS: { id: String, label: String, icon: String, tip: String }[] */
const PLUGINS = [
  { id: "terminal", label: "Terminal", icon: "terminal", tip: "Your coding agent, in the project folder" },
  { id: "critic", label: "Critic", icon: "critic", tip: "Mechanical checks of the project against production best practice, with one-click fixes" },
  {
    id: "mixcheck",
    label: "Mix check",
    icon: "meter",
    tip: "Measure the mix from one render: loudness and true peak against a delivery target, the limiter, phase, spectrum, masking — the same as `rosaclef mixcheck`",
  },
];

const maestro = { tab: localStorage.getItem("rosaclef.maestro.tab") ?? "terminal" };

/** function setTab(id: String) => Undefined */
function setTab(id) {
  maestro.tab = id;
  localStorage.setItem("rosaclef.maestro.tab", id);
  invalidate();
}

/** const term: Term[] */
const term = [];
/** const sock: RawSock[] */
const sock = [];

/** function send<M>(m: M) => Undefined */
function send(m) {
  if (sock.length > 0) sock[0].send(JSON.stringify(m));
}

/** function preferred() => String */
function preferred() {
  return localStorage.getItem("rosaclef.agent") ?? "";
}

/** function start(id: String) => Undefined */
export function startAgent(id) {
  localStorage.setItem("rosaclef.agent", id);
  agent.choosing = false;
  agent.error = "";
  const t = term.length > 0 ? term[0] : undefined;
  if (t) {
    t.reset();
    t.fit();
  }
  send({ t: "start", agent: id, cols: t ? t.cols() : 80, rows: t ? t.rows() : 24 });
  invalidate();
}

function connectTerm() {
  const s = connectRaw(wsUrl("/ws/term"), {
    onOpen: () => {
      agent.connected = true;
      invalidate();
    },
    onText: (text) => {
      const m = JSON.parse(text);
      if (m.t === "status") {
        agent.running = m.running === true;
        agent.id = String(m.agent ?? "");
        agent.name = String(m.name ?? agent.name);
        agent.exitCode = m.exitCode === undefined || m.exitCode === null ? -1 : Number(m.exitCode);
        if (!agent.running && !agent.autostarted) {
          agent.autostarted = true;
          const pref = preferred();
          if (pref !== "" && state.agents.some((a) => a.id === pref && a.available)) startAgent(pref);
        }
        agent.autostarted = true;
      } else if (m.t === "error") {
        agent.error = String(m.message);
        agent.choosing = true;
      }
      invalidate();
    },
    onBinary: (bytes) => {
      if (term.length > 0) term[0].write(bytes);
    },
    onClose: () => {
      agent.connected = false;
      sock.length = 0;
      invalidate();
      setTimeout(() => {
        connectTerm();
      }, 1500);
    },
  });
  sock.length = 0;
  sock.push(s);
}

/** Called once the terminal's DOM node exists. */
function mountTerm() {
  if (term.length > 0) return;
  const el = document.getElementById("agent-term");
  const t = createTerm(el, (data) => {
    send({ t: "input", data: data });
  });
  term.push(t);
  t.fit();
  t.writeText(
    "\x1b[38;2;227;196;122m  ✦ Rosaclef · Maestro\x1b[0m\r\n\x1b[38;2;163;151;128m  Your own coding agent, working on this project's files.\x1b[0m\r\n\r\n"
  );
  connectTerm();
}

function fitTerm() {
  if (term.length === 0) return;
  const t = term[0];
  t.fit();
  send({ t: "resize", cols: t.cols(), rows: t.rows() });
}

/** Type text into the agent's prompt (the producer presses Enter). */
/** function typeIntoAgent(text: String) => Undefined */
export function typeIntoAgent(text) {
  send({ t: "input", data: text });
  if (term.length > 0) term[0].focus();
}

export function loadAgents() {
  getJson("/api/agents")
    .then((r) => {
      state.agents = r.agents;
      invalidate();
      return true;
    })
    .catch((e) => false);
}

/** function suggestions() => String[] */
function suggestions() {
  if (state.backend === "local") {
    const pat = currentPattern();
    return [
      "help",
      "summary",
      "set /transport/bpm 128",
      pat ? `get /patterns/${Math.max(0, state.project.patterns.indexOf(pat))}/name` : "get /meta",
      "presets additive",
      "render",
      "context",
    ];
  }
  const pat = currentPattern();
  const ch = currentChannel();
  const patName = pat ? `the "${pat.name}" pattern` : "a new pattern";
  const chName = ch ? ch.name : "the selected channel";
  return [
    `Write a 4-bar groove for ${patName} that fits the rest of the song.`,
    `Give ${chName} a more expressive part — vary velocities and rhythm.`,
    "Arrange a full song structure (intro, build, drop, break, outro) on the playlist.",
    "Mix the project: balance levels and pans, render, and report peak/RMS.",
    "Sound-design a lush new pad channel and add it to the arrangement.",
    "Explain what's in this project and suggest three improvements.",
  ];
}

/** The agent's status light (also shown on its collapsed rail). */
/** function agentDot() => String */
export function agentDot() {
  return agent.running ? "status-dot live" : agent.connected ? "status-dot warn" : "status-dot bad";
}

/** function agentPanel(b: Builder) => Undefined */
export function agentPanel(b) {
  const pat = currentPattern();
  const ch = currentChannel();
  b.open("aside", "agent", "agent");
  b.on("pointerdown", (e) => setFocus("agent"));

  b.open("div", "head", "agent-head");
  paneHeader(b, "agent");
  b.open("div", "mark", "agent-mark");
  glyph(b, "spark");
  b.close();
  b.open("div", "title", "agent-title");
  b.leaf("b", "t", "", "Maestro");
  b.leaf("span", "s", "", agent.running ? `${agent.name} · live` : agent.connected ? "agent idle" : "connecting…");
  b.close();
  b.leaf("div", "sp", "spacer", "");
  b.leaf("div", "dot", agentDot(), "");
  const ids = state.agents.map((a) => a.id);
  const names = state.agents.map((a) => (a.available ? a.name : `${a.name} (not installed)`));
  if (ids.length > 0) {
    select(b, "pick", "", agent.id !== "" ? agent.id : preferred(), ids, names, "Which agent runs in the terminal", (v) => startAgent(v));
  }
  iconButton(b, "restart", "small", "restart", agent.running ? "Restart the agent" : "Start the agent", () => {
    const id = agent.id !== "" ? agent.id : preferred();
    if (id === "") agent.choosing = true;
    else startAgent(id);
    invalidate();
  });
  if (agent.running) {
    iconButton(b, "stop", "small ghost", "stop", "Stop the agent", () => {
      send({ t: "stop" });
    });
  }
  paneControls(b, "agent");
  b.close();

  const tab = PLUGINS.some((x) => x.id === maestro.tab) ? maestro.tab : "terminal";
  b.open("div", "tabs", "maestro-tabs");
  for (const x of PLUGINS) {
    b.open("button", x.id, tab === x.id ? "maestro-tab on" : "maestro-tab");
    b.attr("title", x.tip);
    b.on("pointerenter", (e) => hint(x.tip));
    b.on("click", (e) => setTab(x.id));
    glyph(b, x.icon);
    b.leaf("span", "l", "", x.label);
    if (x.id === "critic") {
      const n = criticCount();
      if (n > 0) b.leaf("span", "n", "maestro-count", String(n));
    }
    if (x.id === "mixcheck") {
      const n = mixcheckCount();
      if (n > 0) b.leaf("span", "n", "maestro-count", String(n));
    }
    b.close();
  }
  b.close();

  // What the agent can see right now (also written to .rosaclef/context.json).
  b.open("div", "ctx", "agent-context");
  b.open("span", "c1", "chip");
  b.text("Pattern ");
  b.leaf("b", "v", "", pat ? pat.name : "—");
  b.close();
  b.open("span", "c2", "chip");
  b.text("Channel ");
  b.leaf("b", "v", "", ch ? ch.name : "—");
  b.close();
  b.open("span", "c3", "chip");
  b.text("View ");
  b.leaf(
    "b",
    "v",
    "",
    state.dock === "piano"
      ? "Piano roll"
      : state.dock === "mixer"
        ? `Mixer · ${insertIndex(state.insert)}`
        : state.dock === "voice"
          ? "Voice to notes"
          : state.dock === "drums"
            ? "Drums"
            : "Channel rack"
  );
  b.close();
  if (state.selection.length > 0) {
    b.open("span", "c4", "chip");
    b.leaf("b", "v", "", String(state.selection.length));
    b.text(" notes selected");
    b.close();
  }
  if (state.diskIssues.length > 0) {
    b.leaf("span", "c5", "chip warn", `project.json invalid: ${state.diskIssues[0].path}`);
    b.attr("title", state.diskIssues.map((i) => `${i.path}: ${i.message}`).join("\n"));
  }
  b.close();

  // A plugin covers the terminal rather than replacing it: the terminal keeps its size.
  b.open("div", "body", "maestro-body");
  if (tab === "critic") {
    criticPanel(b, (text) => {
      setTab("terminal");
      typeIntoAgent(text);
    });
  }
  if (tab === "mixcheck") {
    mixcheckPanel(b, (text) => {
      setTab("terminal");
      typeIntoAgent(text);
    });
  }

  b.open("div", "wrap", "term-wrap");
  b.on("resize", (e) => {
    fitTerm();
  });
  b.leaf("div", "term", "term", "");
  b.attr("id", "agent-term");
  b.on("mount", (e) => {
    mountTerm();
  });

  const showChooser = agent.connected && !agent.running && (agent.choosing || preferred() === "" || agent.exitCode >= 0 || agent.error !== "");
  if (showChooser) {
    b.open("div", "empty", "term-empty");
    b.leaf("h2", "h", "", "Your maestro awaits");
    const intro =
      state.backend === "local"
        ? "In the browser studio this panel runs the Rosaclef shell — the studio's command line: inspect and edit the song, render, browse presets. To bring your own coding agent, download the project (Projects → .zip) and open it in the native studio."
        : "Bring your own coding agent. It runs in this project's folder and edits the song live — you hear every change.";
    b.leaf(
      "p",
      "p",
      "",
      agent.error !== "" ? agent.error : agent.exitCode >= 0 ? `The agent exited (code ${agent.exitCode}). Start it again or pick another.` : intro
    );
    b.open("div", "grid", "agent-grid");
    for (const a of state.agents) {
      b.open("button", a.id, a.available ? "agent-choice" : "agent-choice na");
      b.attr("title", a.available ? a.command.join(" ") : `Install: ${a.hint}`);
      b.on("click", (e) => {
        startAgent(a.id);
      });
      b.leaf("b", "n", "", a.name);
      b.leaf("span", "c", "", a.available ? a.command.join(" ") : "not installed");
      b.close();
    }
    b.close();
    b.close();
  }
  b.close();

  b.open("div", "suggest", "suggest");
  for (const s of suggestions()) {
    b.leaf("span", s, "chip", s);
    b.attr("title", state.backend === "local" ? "Type this into the shell" : "Type this into the agent's prompt");
    b.on("pointerenter", (e) =>
      hint(
        state.backend === "local" ? `Type “${s}” into the shell (press Enter to run it)` : `Suggest to the agent: “${s}” (press Enter in the terminal to send)`
      )
    );
    b.on("click", (e) => {
      typeIntoAgent(s);
    });
  }
  b.close();
  b.close();

  b.close();
}
