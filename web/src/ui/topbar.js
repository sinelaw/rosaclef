// The top bar: brand, song title, transport, tempo, output and export.

import { drag, fmt, sendJson, download } from "#platform";
import { state, begin, changed, commit, undo, redo, hint } from "../store.js";
import { barBeat } from "../model.js";
import { togglePlay, stop, record, setMode, setOutput } from "../audio.js";
import { iconButton, button, knobAt, meter } from "./widgets.js";
import { isAutomated, shownValue, openMenu } from "../automation.js";
import { toast } from "./toast.js";
import { projectsButton } from "./projects.js";
import { keyboard, toggleKeyboard } from "./keyboard.js";

/** function lcd(b: Builder, key: String, label: String, value: String, unit: String) => Undefined */
function lcd(b, key, label, value, unit) {
  b.leaf("span", "label", "lcd-label", label);
  b.open("span", "value", "lcd-value");
  b.text(value);
  if (unit !== "") b.leaf("small", "unit", "", unit);
  b.close();
}

let exporting = false;

function exportSong() {
  if (exporting) return;
  exporting = true;
  toast(
    "Rendering mixdown…",
    state.backend === "local" ? "The engine renders the song offline, in your browser." : "The native engine renders the song offline (plugins included).",
    "info"
  );
  sendJson("/api/render", "POST", { bits: 24 })
    .then((r) => {
      exporting = false;
      const path = String(r.path);
      toast("Mixdown ready", `${path}\n${fmt(Number(r.duration), 1)} s · peak ${fmt(Number(r.peakDb), 1)} dBFS`, "info");
      download(String(r.url), path.split("/").pop() ?? "mixdown.wav");
      return true;
    })
    .catch((e) => {
      exporting = false;
      toast("Export failed", String(e), "error");
      return false;
    });
}

/** function topbar(b: Builder) => Undefined */
export function topbar(b) {
  const p = state.project;
  b.open("header", "top", "topbar");

  b.open("div", "brand", "brand");
  b.open("div", "mark", "brand-mark");
  b.leaf("span", "r", "", "R");
  b.close();
  b.open("div", "text", "");
  b.leaf("div", "name", "brand-name", "Rosaclef");
  b.leaf("span", "sub", "brand-sub", "Studio · AI edition");
  b.close();
  b.close();

  projectsButton(b);
  b.leaf("div", "title", "song-title", p.meta.title);
  b.attr("title", state.folder);

  b.open("div", "transport", "transport");
  b.open("div", "mode", "seg");
  button(b, "pat", state.mode === "pattern" ? "on" : "", "PAT", "Pattern mode: loop the selected pattern (L)", () => setMode("pattern"));
  button(b, "song", state.mode === "song" ? "on" : "", "SONG", "Song mode: play the playlist arrangement (L)", () => setMode("song"));
  b.close();
  iconButton(b, "play", state.playing ? "play on" : "play", state.playing ? "pause" : "play", "Play / pause (Space)", () => {
    togglePlay();
  });
  iconButton(b, "stop", "stop", "stop", "Stop and rewind", () => {
    stop();
  });
  iconButton(b, "rec", state.recording ? "rec armed" : "rec", "record", "Record audio from the microphone onto the selected track (R)", () => {
    record();
  });

  b.open("div", "pos", "lcd static");
  lcd(b, "pos", state.mode === "song" ? "Song" : "Pattern", barBeat(state.position, p.transport), "");
  b.close();

  b.open("div", "bpm", isAutomated("tempo") ? "lcd automated" : "lcd");
  b.attr("title", "Tempo — drag up/down (Shift for fine), right-click to automate");
  b.on("pointerenter", (e) => hint("Tempo — drag up/down (Shift: fine steps) · right-click to automate it"));
  b.on("contextmenu", (e) => {
    e.preventDefault();
    openMenu("tempo", e.clientX, e.clientY);
  });
  b.on("pointerdown", (e) => {
    e.preventDefault();
    if (e.button === 2) return undefined;
    begin();
    const y0 = e.clientY;
    const bpm0 = p.transport.bpm;
    drag(
      e,
      (m) => {
        const step = m.shiftKey ? 0.05 : 0.5;
        const v = Math.round((bpm0 + (y0 - m.clientY) * step) * 100) / 100;
        state.project.transport.bpm = Math.max(20, Math.min(400, v));
        changed(true);
      },
      (u) => undefined
    );
  });
  lcd(b, "bpm", "Tempo", fmt(shownValue("tempo", p.transport.bpm), 2), "BPM");
  if (isAutomated("tempo")) b.leaf("i", "auto", "auto-dot", "");
  b.close();

  const swing = shownValue("swing", p.transport.swing);
  b.open("div", "swing", "lcd static");
  b.leaf("span", "label", "lcd-label", "Swing");
  knobAt(b, "k", "small", swing, "", `Swing ${Math.round(swing * 100)}%`, 0, "swing", (v) => {
    state.project.transport.swing = Math.round(v * 100) / 100;
  });
  b.close();
  b.close();

  b.leaf("div", "sp", "spacer", "");

  // The native engine exists only with a server that has an audio device.
  if (state.nativeAvailable) {
    b.open("div", "out", "seg");
    button(b, "browser", state.output === "browser" ? "on" : "", "Browser", "Play through the WebAssembly engine in this browser", () => setOutput("browser"));
    button(
      b,
      "native",
      state.output === "native" ? "on" : "",
      "Studio",
      "Play through the native engine on the server's audio device (plugins, lowest latency)",
      () => setOutput("native")
    );
    b.close();
  }

  const master = p.mixer.inserts.length > 0 ? p.mixer.inserts[0] : undefined;
  if (master) {
    const ml = state.meters.length > 1 ? state.meters[0] : 0;
    const mr = state.meters.length > 1 ? state.meters[1] : 0;
    b.open("div", "master", "master-mini");
    knobAt(b, "vol", "", shownValue("insert/0/volume", master.volume) / 1.25, "", "Master volume", 0.8, "insert/0/volume", (v) => {
      const ins = state.project.mixer.inserts[0];
      ins.volume = Math.round(v * 1.25 * 1000) / 1000;
    });
    meter(b, "meter", ml, mr);
    b.close();
  }

  iconButton(
    b,
    "keys",
    keyboard.shown ? "kb-toggle on" : "kb-toggle",
    "keys",
    keyboard.shown ? "Hide the on-screen piano" : "Show the on-screen piano (plays the selected channel)",
    () => {
      toggleKeyboard();
    }
  );
  iconButton(b, "undo", "", "undo", "Undo (Ctrl+Z) — includes the agent's edits", () => {
    undo();
  });
  iconButton(b, "redo", "", "redo", "Redo (Ctrl+Shift+Z)", () => {
    redo();
  });
  b.open("button", "export", "btn gold");
  b.attr("title", "Render the song to a WAV file");
  b.on("pointerenter", (e) => hint("Export: render the whole song offline to a 24-bit WAV (saved in renders/, and downloaded)"));
  b.on("click", (e) => {
    exportSong();
  });
  b.leaf("span", "t", "", "Export");
  b.close();
  b.close();
}
