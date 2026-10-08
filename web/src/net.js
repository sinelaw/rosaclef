// Connection to the Rosaclef server: project sync, native engine status.
//
// The server keeps a project per tab: the tab asks it to open the project
// its address names (projects.js) and gets the project's key, which scopes
// the tab's socket and requests to that project (`setScope`). Choosing
// another project (the Projects window, Back) connects the tab again.

import { connectRaw, wsUrl, loadPref, savePref, now, backendMode, sendJson, setScope, isOffline } from "#platform";
import { state, hooks, load, applyRemote, invalidate, currentPattern, currentChannel, dockName, projectOpened } from "./store.js";
import { toast } from "./ui/toast.js";
import { insertIndex, noteIndex, clipIndex, trackIndex } from "#brands";
import { decodeProject, encodeClipWire, barBeat } from "./model.js";
import { selectedLane, selectedPoints, laneValueAt } from "./automation.js";
import { projectSwitched, followAddress, addressedFolder, addressRefused } from "./ui/projects.js";
import { t } from "./i18n.js";

/** const sock: RawSock[] */
const sock = [];
let retry = 500;

const link = {
  /** Counts connections: a socket replaced by a newer one is let go quietly. */
  gen: 0,
  /** The project's key on the server ("" before the first welcome). */
  session: "",
  /** The next welcome is for a project the tab chose (not the one it had). */
  switching: false,
};

/** function send<M>(m: M) => Undefined */
export function send(m) {
  if (sock.length > 0) sock[0].send(JSON.stringify(m));
}

/** function issueText(issues: Issue[]) => String */
function issueText(issues) {
  return issues
    .slice(0, 3)
    .map((i) => (i.path === "" ? i.message : `${i.path}: ${i.message}`))
    .join("\n");
}

/** function onMessage(text: String) => Undefined */
function onMessage(text) {
  const m = JSON.parse(text);
  const kind = String(m.t);
  if (kind === "welcome" || kind === "switched") {
    // Another project (opened, imported, moved, or found on reconnecting):
    // what was measured of the last one goes.
    if (kind === "switched" || (state.loaded && state.folder !== String(m.folder))) projectOpened();
    const first = !state.loaded;
    const chose = link.switching;
    link.switching = false;
    const key = String(m.session ?? "");
    const fresh = first || key !== link.session;
    link.session = key;
    setScope(key);
    state.folder = String(m.folder);
    state.name = String(m.name ?? "");
    state.samples = m.samples;
    state.rev = Number(m.rev);
    state.nativeAvailable = m.native.available === true;
    state.nativeEnabled = m.native.enabled === true;
    state.backend = m.backend === "local" ? "local" : "server";
    load(decodeProject(m.project));
    followAddress(first, kind === "switched");
    if (fresh && hooks.session) hooks.session();
    if (kind === "switched" || chose) projectSwitched();
    else if (state.backend === "local" && loadPref("rosaclef.localIntro") === "") {
      savePref("rosaclef.localIntro", "shown");
      toast(t("net.localIntro.toast.title"), t("net.localIntro.toast.body"), "info");
    }
  } else if (kind === "project") {
    state.rev = Number(m.rev);
    state.diskIssues = [];
    applyRemote(decodeProject(m.project));
    const origin = String(m.origin);
    if (origin === "disk") toast(t("net.project.agentUpdated.toast.title"), t("net.project.agentUpdated.toast.body"), "agent");
    else if (origin === "api") toast(t("net.project.apiUpdated.toast.title"), "", "agent");
  } else if (kind === "ack") {
    state.rev = Number(m.rev);
  } else if (kind === "rejected") {
    /** const issues: Issue[] */
    const issues = m.issues;
    toast(t("net.edit.rejected.toast.title"), issueText(issues), "error");
  } else if (kind === "invalid") {
    state.diskIssues = m.issues;
    toast(t("net.project.invalid.toast.title"), issueText(state.diskIssues), "error");
    invalidate();
  } else if (kind === "samples") {
    state.samples = m.samples;
    invalidate();
  } else if (kind === "native") {
    state.nativeEnabled = m.status.enabled === true;
    state.nativeDevice = String(m.status.device ?? "");
    if (m.status.error) toast(t("net.native.deviceUnavailable.toast.title"), String(m.status.error), "error");
    if (!state.nativeEnabled && state.output === "native") state.output = "browser";
    // A freshly started native engine has not heard of the tried-out instrument.
    if (state.nativeEnabled && state.audition.on && hooks.audition) hooks.audition();
    invalidate();
  } else if (kind === "native.meters") {
    if (state.output === "native") {
      state.position = Number(m.position);
      state.positionAt = now();
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
  } else if (kind === "recorded") {
    toast(t("net.recorded.toast.title"), String(m.path), "info");
  } else if (kind === "error") {
    toast(state.backend === "local" ? t("net.error.studio.toast.title") : t("net.error.server.toast.title"), String(m.message), "error");
  } else if (kind === "notice") {
    toast(String(m.title), String(m.message), "error");
  }
}

/** Connect to the back end: on the server, to the project the address
 * names (the server's own when it names none). */
export function connect() {
  const gen = link.gen + 1;
  link.gen = gen;
  backendMode()
    .then((mode) => {
      if (gen !== link.gen) return false;
      const want = mode === "server" ? addressedFolder() : "";
      if (want === "") {
        // The server's own project (its key comes with the welcome).
        setScope("");
        openSocket(gen);
        return true;
      }
      sendJson("/api/projects/open", "POST", { path: want })
        .then((r) => {
          if (gen !== link.gen) return false;
          setScope(String(r.session));
          openSocket(gen);
          return true;
        })
        .catch((e) => {
          if (gen !== link.gen) return false;
          if (isOffline(e)) {
            // Down or restarting: try again (the address still names the project).
            retryLater(gen);
            return false;
          }
          // Gone (deleted, renamed elsewhere) or not the server's to open.
          addressRefused(String(e));
          setScope("");
          openSocket(gen);
          return false;
        });
      return true;
    })
    .catch((e) => false);
}

/** function retryLater(gen: Int) => Undefined */
function retryLater(gen) {
  setTimeout(() => {
    if (gen === link.gen) connect();
  }, retry);
  retry = Math.min(8000, retry * 2);
}

/** function openSocket(gen: Int) => Undefined */
function openSocket(gen) {
  const s = connectRaw(wsUrl("/ws"), {
    onOpen: () => {
      if (gen !== link.gen) return undefined;
      state.connected = true;
      retry = 500;
      invalidate();
    },
    onText: (text) => {
      if (gen === link.gen) onMessage(text);
    },
    onBinary: (b) => undefined,
    onClose: () => {
      if (gen !== link.gen) return undefined;
      state.connected = false;
      sock.length = 0;
      invalidate();
      retryLater(gen);
    },
  });
  sock.length = 0;
  sock.push(s);
}

/** Connect this tab to the project its address names now (the Projects window, Back, a typed address). */
function reopen() {
  link.switching = true;
  const old = sock.length > 0 ? sock[0] : undefined;
  sock.length = 0;
  connect();
  if (old) old.close();
}

/** Tell the server (and so the agent) what the producer is looking at.
 * Written to .rosaclef/context.json (schema: .rosaclef/context.schema.json). */
export function sendContext() {
  const p = state.project;
  const pat = currentPattern();
  const ch = currentChannel();
  const vp = state.viewport;
  /** const notes: { index: Int, note: Note }[] */
  const notes = [];
  if (pat) {
    for (const i of state.selection) {
      const n = noteIndex(i);
      if (n < pat.notes.length) notes.push({ index: n, note: pat.notes[n] });
    }
  }
  const clips = [];
  for (const c of state.clipSelection) {
    const i = clipIndex(c);
    if (i < p.playlist.clips.length) clips.push({ index: i, clip: encodeClipWire(p.playlist.clips[i]) });
  }
  const ins = insertIndex(state.insert);
  const tr = trackIndex(state.track);
  const lane = selectedLane();
  let laneAt = -1;
  /** const points: { index: Int, point: AutomationPoint }[] */
  const points = [];
  if (lane) {
    for (let i = 0; i < p.automation.length; i++) if (p.automation[i].id === lane.id) laneAt = i;
    for (const k of selectedPoints(lane)) {
      const pt = lane.points[k];
      points.push({ index: k, point: { beat: pt.beat, value: pt.value, curve: pt.curve } });
    }
  }
  // (Nested ifs: inty wants both operands of && to have one type.)
  const roll = [];
  if (vp.prOn) {
    if (pat) {
      if (ch) roll.push({ pattern: pat.id, channel: ch.id, startBeat: vp.prStart, endBeat: vp.prEnd, lowPitch: vp.prLow, highPitch: vp.prHigh });
    }
  }
  const fm = state.film;
  send({
    t: "context",
    context: {
      focus: state.focus,
      dock: dockName(state.dock),
      transport: {
        playing: state.playing,
        mode: state.mode,
        positionBeats: Math.round(state.position * 1000) / 1000,
        position: barBeat(state.position, p.transport),
        bpm: p.transport.bpm,
      },
      selection: {
        pattern: pat ? { id: pat.id, name: pat.name, length: pat.length, noteCount: pat.notes.length } : null,
        channel: ch ? { id: ch.id, name: ch.name, instrument: ch.instrument.type, mixer: insertIndex(ch.mixer) } : null,
        insert: ins < p.mixer.inserts.length ? { index: ins, name: p.mixer.inserts[ins].name } : null,
        track: tr < p.playlist.tracks.length ? { index: tr, name: p.playlist.tracks[tr].name } : null,
        notes: notes,
        clips: clips,
        automation: lane
          ? {
              index: laneAt,
              id: lane.id,
              name: lane.name,
              target: lane.target,
              pointCount: lane.points.length,
              points: points,
              valueAtPlayhead: Math.round(laneValueAt(lane.points, state.mode === "song" ? state.position : 0) * 1000) / 1000,
            }
          : null,
      },
      visible: {
        playlist: { startBeat: vp.plStart, endBeat: vp.plEnd, firstTrack: vp.plTrack0, lastTrack: vp.plTrack1 },
        pianoRoll: roll.length > 0 ? roll[0] : null,
        film: fm.on
          ? {
              mode: fm.mode,
              shot: fm.shot >= 0 ? fm.shot : null,
              selectedShot: fm.selected >= 0 ? fm.selected : null,
              startBeat: Math.round(fm.start * 1000) / 1000,
              endBeat: Math.round(fm.end * 1000) / 1000,
              frame: fm.frame,
              focus: fm.focus,
              why: fm.why,
            }
          : null,
      },
      recentEdits: state.recent,
    },
  });
}

export function installSync() {
  hooks.reopen = reopen;
  hooks.sync = (json) => {
    send({ t: "put", folder: state.folder, project: JSON.parse(json) });
  };
  hooks.context = sendContext;
}
