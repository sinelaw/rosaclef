// The top bar: brand, song title, transport, tempo, output and export.

import { drag, fmt, sendJson, download } from "#platform";
import { state, begin, changed, commit, undo, redo, hint } from "../store.js";
import { barBeat } from "../model.js";
import { togglePlay, stop, record, setMode, setOutput } from "../audio.js";
import { iconButton, button, knob, meter } from "./widgets.js";
import { toast } from "./toast.js";
import { insertIx } from "#brands";

/** function lcd(b: Builder, key: String, label: String, value: String, unit: String) => Undefined */
function lcd(b, key, label, value, unit) {
  b.leaf("span", "label", "lcd-label", label);
  b.open("span", "value", "lcd-value");
  b.text(value);
  if (unit !== "") b.leaf("small", "unit", "", unit);
  b.close();
  return undefined;
}

let exporting = false;

function exportSong() {
  if (exporting) return;
  exporting = true;
  toast("Rendering mixdown…", "The native engine renders the song offline (plugins included).", "info");
  sendJson("/api/render", "POST", { bits: 24 })
    .then((r) => {
      exporting = false;
      const path = String(r.path);
      toast("Mixdown ready", `${path}\n${fmt(Number(r.duration), 1)} s · peak ${fmt(Number(r.peakDb), 1)} dBFS`, "info");
      download(String(r.url), path.split("/").pop() ?? "mixdown.wav");
      return Promise.resolve(true);
    })
    .catch((e) => {
      exporting = false;
      toast("Export failed", String(e), "error");
      return Promise.resolve(false);
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

  b.leaf("div", "title", "song-title", p.meta.title);
  b.attr("title", state.folder);

  b.open("div", "transport", "transport");
  b.open("div", "mode", "seg");
  button(b, "pat", state.mode === "pattern" ? "on" : "", "PAT", "Pattern mode: loop the selected pattern (L)", () => setMode("pattern"));
  button(b, "song", state.mode === "song" ? "on" : "", "SONG", "Song mode: play the playlist arrangement (L)", () => setMode("song"));
  b.close();
  iconButton(b, "play", state.playing ? "play on" : "play", state.playing ? "pause" : "play", "Play / pause (Space)", () => {
    togglePlay();
    return undefined;
  });
  iconButton(b, "stop", "stop", "stop", "Stop and rewind", () => {
    stop();
    return undefined;
  });
  iconButton(b, "rec", state.recording ? "rec armed" : "rec", "record", "Record audio from the microphone onto the selected track (R)", () => {
    record();
    return undefined;
  });

  b.open("div", "pos", "lcd static");
  lcd(b, "pos", state.mode === "song" ? "Song" : "Pattern", barBeat(state.position, p.transport.beatsPerBar), "");
  b.close();

  b.open("div", "bpm", "lcd");
  b.attr("title", "Tempo — drag up/down (Shift for fine), double-click to type");
  b.on("pointerenter", (e) => hint("Tempo — drag up/down (Shift: fine steps), double-click to type a value"));
  b.on("pointerdown", (e) => {
    e.preventDefault();
    begin();
    const y0 = e.clientY;
    const bpm0 = p.transport.bpm;
    drag(e, (m) => {
      const step = m.shiftKey ? 0.05 : 0.5;
      const v = Math.round((bpm0 + (y0 - m.clientY) * step) * 100) / 100;
      state.project.transport.bpm = Math.max(20, Math.min(400, v));
      changed(true);
      return undefined;
    }, (u) => undefined);
    return undefined;
  });
  lcd(b, "bpm", "Tempo", fmt(p.transport.bpm, 2), "BPM");
  b.close();

  b.open("div", "swing", "lcd static");
  b.leaf("span", "label", "lcd-label", "Swing");
  knob(b, "k", "small", p.transport.swing, "", `Swing ${Math.round(p.transport.swing * 100)}%`, 0, (v) => {
    state.project.transport.swing = Math.round(v * 100) / 100;
    return undefined;
  });
  b.close();
  b.close();

  b.leaf("div", "sp", "spacer", "");

  b.open("div", "out", "seg");
  button(b, "browser", state.output === "browser" ? "on" : "", "Browser", "Play through the WebAssembly engine in this browser", () => setOutput("browser"));
  button(b, "native", state.output === "native" ? "on" : "", "Studio", "Play through the native engine on the server's audio device (plugins, lowest latency)", () => setOutput("native"));
  b.close();

  const master = p.mixer.inserts.length > 0 ? p.mixer.inserts[0] : undefined;
  if (master) {
    const ml = state.meters.length > 1 ? state.meters[0] : 0;
    const mr = state.meters.length > 1 ? state.meters[1] : 0;
    b.open("div", "master", "master-mini");
    knob(b, "vol", "", master.volume / 1.25, "", "Master volume", 0.8, (v) => {
      const ins = state.project.mixer.inserts[0];
      ins.volume = Math.round(v * 1.25 * 1000) / 1000;
      return undefined;
    });
    meter(b, "meter", ml, mr);
    b.close();
  }

  iconButton(b, "undo", "", "undo", "Undo (Ctrl+Z) — includes the agent's edits", () => {
    undo();
    return undefined;
  });
  iconButton(b, "redo", "", "redo", "Redo (Ctrl+Shift+Z)", () => {
    redo();
    return undefined;
  });
  b.open("button", "export", "btn gold");
  b.attr("title", "Render the song to a WAV file");
  b.on("pointerenter", (e) => hint("Export: render the whole song offline to a 24-bit WAV (saved in renders/)"));
  b.on("click", (e) => {
    exportSong();
    return undefined;
  });
  b.leaf("span", "t", "", "Export");
  b.close();
  b.close();
  return undefined;
}

/** function selectMaster() => Undefined */
export function selectMaster() {
  state.insert = insertIx(0);
  return undefined;
}
