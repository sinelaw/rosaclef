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
import { t, tf, tk } from "../i18n.js";

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
  { id: "terminal", label: tk("agent.tab.terminal.label"), icon: "terminal", tip: tk("agent.tab.terminal.title") },
  {
    id: "critic",
    label: tk("agent.tab.critic.label"),
    icon: "critic",
    tip: tk("agent.tab.critic.title"),
  },
  {
    id: "mixcheck",
    label: tk("panel.mixCheck"),
    icon: "meter",
    tip: tk("agent.tab.mixcheck.title"),
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
  const tm = term.length > 0 ? term[0] : undefined;
  if (tm) {
    tm.reset();
    tm.fit();
  }
  send({ t: "start", agent: id, cols: tm ? tm.cols() : 80, rows: tm ? tm.rows() : 24 });
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
  const tm = createTerm(el, (data) => {
    send({ t: "input", data: data });
  });
  term.push(tm);
  tm.fit();
  tm.writeText("\x1b[38;2;227;196;122m  ✦ Rosaclef · Maestro\x1b[0m\r\n\x1b[38;2;163;151;128m  " + t("agent.term.banner") + "\x1b[0m\r\n\r\n");
  connectTerm();
}

function fitTerm() {
  if (term.length === 0) return;
  const tm = term[0];
  tm.fit();
  send({ t: "resize", cols: tm.cols(), rows: tm.rows() });
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

/** The chips of what the agent sees: the pattern, the channel and the view
 * in front of you, the selected notes. */
/** function agentContext(b: Builder) => Undefined */
function agentContext(b) {
  const pat = currentPattern();
  const ch = currentChannel();
  b.leaf("span", "lbl", "agent-context-label", t("agent.context.label"));
  b.attr("title", t("agent.context.title"));
  b.open("span", "c1", "chip");
  b.text(t("term.pattern") + " ");
  b.leaf("b", "v", "", pat ? pat.name : "—");
  b.close();
  b.open("span", "c2", "chip");
  b.text(t("term.channel") + " ");
  b.leaf("b", "v", "", ch ? ch.name : "—");
  b.close();
  b.open("span", "c3", "chip");
  b.text(t("agent.context.view.label") + " ");
  b.leaf(
    "b",
    "v",
    "",
    state.dock === "piano"
      ? t("panel.pianoRoll")
      : state.dock === "mixer"
        ? tf("agent.context.view.mixer", [String(insertIndex(state.insert))])
        : state.dock === "voice"
          ? t("agent.context.view.voice")
          : state.dock === "drums"
            ? t("panel.drums")
            : t("agent.context.view.rack")
  );
  b.close();
  if (state.selection.length > 0) {
    b.open("span", "c4", "chip");
    b.leaf("b", "v", "", String(state.selection.length));
    b.text(" " + t("agent.context.notesSelected"));
    b.close();
  }
}

/** function agentPanel(b: Builder) => Undefined */
export function agentPanel(b) {
  b.open("aside", "agent", "agent");
  b.on("pointerdown", (e) => setFocus("agent"));

  b.open("div", "head", "agent-head");
  paneHeader(b, "agent");
  b.open("div", "mark", "agent-mark");
  glyph(b, "spark");
  b.close();
  b.open("div", "title", "agent-title");
  b.leaf("b", "t", "", "Maestro");
  b.leaf("span", "s", "", agent.running ? tf("agent.status.live", [agent.name]) : agent.connected ? t("agent.status.idle") : t("agent.status.connecting"));
  b.close();
  b.leaf("div", "sp", "spacer", "");
  b.leaf("div", "dot", agentDot(), "");
  const ids = state.agents.map((a) => a.id);
  const names = state.agents.map((a) => (a.available ? a.name : tf("agent.picker.notInstalled", [a.name])));
  if (ids.length > 0) {
    select(b, "pick", "", agent.id !== "" ? agent.id : preferred(), ids, names, t("agent.picker.title"), (v) => startAgent(v));
  }
  iconButton(b, "restart", "small", "restart", agent.running ? t("agent.restart.title") : t("agent.start.title"), () => {
    const id = agent.id !== "" ? agent.id : preferred();
    if (id === "") agent.choosing = true;
    else startAgent(id);
    invalidate();
  });
  if (agent.running) {
    iconButton(b, "stop", "small ghost", "stop", t("agent.stop.title"), () => {
      send({ t: "stop" });
    });
  }
  paneControls(b, "agent");
  b.close();

  const tab = PLUGINS.some((x) => x.id === maestro.tab) ? maestro.tab : "terminal";
  b.open("div", "tabs", "maestro-tabs");
  for (const x of PLUGINS) {
    b.open("button", x.id, tab === x.id ? "maestro-tab on" : "maestro-tab");
    b.attr("title", t(x.tip));
    b.on("pointerenter", (e) => hint(t(x.tip)));
    b.on("click", (e) => setTab(x.id));
    glyph(b, x.icon);
    b.leaf("span", "l", "", t(x.label));
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

  // What the agent can see right now (also written to .rosaclef/context.json):
  // the agent's context, so on its tab only; a broken project.json on every tab.
  if (tab === "terminal" || state.diskIssues.length > 0) {
    b.open("div", "ctx", "agent-context");
    if (tab === "terminal") agentContext(b);
    if (state.diskIssues.length > 0) {
      b.leaf("span", "c5", "chip warn", tf("agent.context.diskInvalid", [state.diskIssues[0].path]));
      b.attr("title", state.diskIssues.map((i) => `${i.path}: ${i.message}`).join("\n"));
    }
    b.close();
  }

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
    b.leaf("h2", "h", "", t("agent.chooser.title"));
    const intro = state.backend === "local" ? t("agent.chooser.intro.local") : t("agent.chooser.intro.native");
    b.leaf("p", "p", "", agent.error !== "" ? agent.error : agent.exitCode >= 0 ? tf("agent.chooser.exited", [String(agent.exitCode)]) : intro);
    b.open("div", "grid", "agent-grid");
    for (const a of state.agents) {
      b.open("button", a.id, a.available ? "agent-choice" : "agent-choice na");
      b.attr("title", a.available ? a.command.join(" ") : tf("agent.chooser.install.title", [a.hint]));
      b.on("click", (e) => {
        startAgent(a.id);
      });
      b.leaf("b", "n", "", a.name);
      b.leaf("span", "c", "", a.available ? a.command.join(" ") : t("agent.chooser.notInstalled"));
      b.close();
    }
    b.close();
    b.close();
  }
  b.close();

  b.open("div", "suggest", "suggest");
  for (const s of suggestions()) {
    b.leaf("span", s, "chip", s);
    b.attr("title", state.backend === "local" ? t("agent.suggest.shell.title") : t("agent.typeIntoPrompt"));
    b.on("pointerenter", (e) => hint(state.backend === "local" ? tf("agent.suggest.shell.hint", [s]) : tf("agent.suggest.agent.hint", [s])));
    b.on("click", (e) => {
      typeIntoAgent(s);
    });
  }
  b.close();
  b.close();

  b.close();
}
