// Connection to the Rosaclef server: project sync, native engine status.

import { connectRaw, wsUrl } from "platform";
import { decodeProject } from "./model.js";
import { state, hooks, load, applyRemote, invalidate, currentPattern, currentChannel } from "./store.js";
import { toast } from "./ui/toast.js";

/** const sock: RawSock[] */
const sock = [];
let retry = 500;

/** function send<M>(m: M) => Undefined */
export function send(m) {
  if (sock.length > 0) sock[0].send(JSON.stringify(m));
  return undefined;
}

/** function issueText(issues: Issue[]) => String */
function issueText(issues) {
  return issues.slice(0, 3).map((i) => (i.path === "" ? i.message : `${i.path}: ${i.message}`)).join("\n");
}

/** function onMessage(text: String) => Undefined */
function onMessage(text) {
  const m = JSON.parse(text);
  const t = String(m.t);
  if (t === "welcome") {
    state.folder = String(m.folder);
    state.samples = m.samples;
    state.rev = Number(m.rev);
    state.nativeAvailable = m.native.available === true;
    state.nativeEnabled = m.native.enabled === true;
    load(decodeProject(m.project));
  } else if (t === "project") {
    state.rev = Number(m.rev);
    state.diskIssues = [];
    applyRemote(decodeProject(m.project));
    const origin = String(m.origin);
    if (origin === "disk") toast("The agent updated the project", "Undo with Ctrl+Z", "agent");
    else if (origin === "api") toast("Project updated through the API", "", "agent");
  } else if (t === "ack") {
    state.rev = Number(m.rev);
  } else if (t === "rejected") {
    /** const issues: Issue[] */
    const issues = m.issues;
    toast("The server rejected an edit", issueText(issues), "error");
  } else if (t === "invalid") {
    state.diskIssues = m.issues;
    toast("project.json on disk is invalid", issueText(state.diskIssues), "error");
    invalidate();
  } else if (t === "samples") {
    state.samples = m.samples;
    invalidate();
  } else if (t === "native") {
    state.nativeEnabled = m.status.enabled === true;
    state.nativeDevice = String(m.status.device ?? "");
    if (m.status.error) toast("Studio audio device unavailable", String(m.status.error), "error");
    if (!state.nativeEnabled && state.output === "native") state.output = "browser";
    invalidate();
  } else if (t === "native.meters") {
    if (state.output === "native") {
      state.position = Number(m.position);
      state.playing = m.playing === true;
      state.loopLength = Number(m.loopLength);
      state.recording = m.recording === true;
      /** const ins: Number[][] */
      const ins = m.inserts;
      /** const flat: Number[] */
      const flat = [];
      for (const p of ins) {
        flat.push(p[0]);
        flat.push(p[1]);
      }
      state.meters = flat;
      state.chMeters = m.channels;
      invalidate();
    }
  } else if (t === "recorded") {
    toast("Recording saved", String(m.path), "info");
  } else if (t === "error") {
    toast("Server error", String(m.message), "error");
  }
  return undefined;
}

export function connect() {
  const s = connectRaw(wsUrl("/ws"), {
    onOpen: () => {
      state.connected = true;
      retry = 500;
      invalidate();
      return undefined;
    },
    onText: onMessage,
    onBinary: (b) => undefined,
    onClose: () => {
      state.connected = false;
      sock.length = 0;
      invalidate();
      setTimeout(() => {
        connect();
        return undefined;
      }, retry);
      retry = Math.min(8000, retry * 2);
      return undefined;
    },
  });
  sock.length = 0;
  sock.push(s);
}

/** Tell the server (and so the agent) what the producer is looking at. */
export function sendContext() {
  const pat = currentPattern();
  const ch = currentChannel();
  /** const selected: Note[] */
  const selected = [];
  if (pat) {
    for (const i of state.selection) {
      if (i < pat.notes.length) selected.push(pat.notes[i]);
    }
  }
  send({
    t: "context",
    context: {
      view: state.dock === "piano" ? "piano roll" : state.dock === "mixer" ? "mixer" : "channel rack",
      playMode: state.mode,
      selectedPattern: pat ? { id: pat.id, name: pat.name, length: pat.length } : null,
      selectedChannel: ch ? { id: ch.id, name: ch.name, instrument: ch.instrument.type } : null,
      selectedInsert: state.insert,
      selectedNotes: selected,
      playheadBeat: Math.round(state.position * 1000) / 1000,
      snap: state.snap,
      hint: "Indexes refer to arrays in project.json; notes are listed in full.",
    },
  });
}

export function installSync() {
  hooks.sync = (json) => {
    send({ t: "put", project: JSON.parse(json) });
    return undefined;
  };
  hooks.context = sendContext;
}
