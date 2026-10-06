// The top bar: brand, song title, transport, tempo, output and export.

import { drag, fmt, sendJson, download } from "#platform";
import { state, begin, changed, commit, undo, redo, hint, invalidate } from "../store.js";
import { barBeat, semitonesText } from "../model.js";
import { togglePlay, stop, record, setMode, setOutput, toggleMetronome } from "../audio.js";
import { iconButton, button, knobAt, meter, glyph } from "./widgets.js";
import { t, tf, tk, language, setLanguage, LANGUAGES } from "../i18n.js";
import { isAutomated, shownValue, openMenu } from "../automation.js";
import { toast } from "./toast.js";
import { projectsButton } from "./projects.js";
import { keyboard, toggleKeyboard } from "./keyboard.js";
import { meterLcd } from "./meter.js";

/** function lcd(b: Builder, key: String, label: String, value: String, unit: String) => Undefined */
function lcd(b, key, label, value, unit) {
  b.leaf("span", "label", "lcd-label", label);
  b.open("span", "value", "lcd-value");
  b.text(value);
  if (unit !== "") b.leaf("small", "unit", "", unit);
  b.close();
}

/** The tempo LCD turns into a text field on a click (a drag still sets it). */
const tempoField = { editing: false };

/** Set the tempo to what was typed (anything that is not a number: no change). */
/** function typeTempo(text: String) => Undefined */
function typeTempo(text) {
  if (!tempoField.editing) return undefined;
  tempoField.editing = false;
  const v = Number(text.trim().replace(",", "."));
  if (text.trim() !== "" && Number.isFinite(v)) {
    const bpm = Math.max(20, Math.min(400, Math.round(v * 100) / 100));
    if (bpm !== state.project.transport.bpm)
      commit(() => {
        state.project.transport.bpm = bpm;
      });
  }
  invalidate();
}

/** function tempoLcd(b: Builder) => Undefined */
function tempoLcd(b) {
  const p = state.project;
  const cls = isAutomated("tempo") ? "lcd tempo automated" : "lcd tempo";
  b.open("div", "bpm", tempoField.editing ? `${cls} editing` : cls);
  if (tempoField.editing) {
    b.leaf("span", "label", "lcd-label", t("Tempo"));
    b.open("span", "value", "lcd-value");
    b.leaf("input", "in", "lcd-input", "");
    b.attr("aria-label", t("Tempo in BPM"));
    b.attr("inputmode", "decimal");
    b.attr("spellcheck", "false");
    b.prop("value", fmt(p.transport.bpm, 2));
    b.prop("select", "true");
    b.on("keydown", (e) => {
      if (e.key === "Enter") typeTempo(e.value);
      else if (e.key === "Escape") {
        tempoField.editing = false;
        invalidate();
      }
    });
    b.on("blur", (e) => typeTempo(e.value));
    b.leaf("small", "unit", "", "BPM");
    b.close();
    b.close();
    return undefined;
  }
  b.attr("title", t("Tempo — click to type it, drag up/down (Shift for fine), right-click to automate"));
  b.on("pointerenter", (e) => hint(t("Tempo — click to type a tempo · drag up/down (Shift: fine steps) · right-click to automate it")));
  b.on("contextmenu", (e) => {
    e.preventDefault();
    openMenu("tempo", e.clientX, e.clientY);
  });
  b.on("pointerdown", (e) => {
    e.preventDefault();
    if (e.button === 2) return undefined;
    const y0 = e.clientY;
    const bpm0 = p.transport.bpm;
    let moved = false;
    drag(
      e,
      (m) => {
        if (!moved && Math.abs(m.clientY - y0) < 3) return undefined;
        if (!moved) begin();
        moved = true;
        const step = m.shiftKey ? 0.05 : 0.5;
        const v = Math.round((bpm0 + (y0 - m.clientY) * step) * 100) / 100;
        state.project.transport.bpm = Math.max(20, Math.min(400, v));
        changed(true);
      },
      (u) => {
        // A click without a drag: type the tempo.
        if (!moved) {
          tempoField.editing = true;
          invalidate();
        }
      }
    );
  });
  lcd(b, "bpm", t("Tempo"), fmt(shownValue("tempo", p.transport.bpm), 2), "BPM");
  if (isAutomated("tempo")) b.leaf("i", "auto", "auto-dot", "");
  b.close();
}

let exporting = false;

function exportSong() {
  if (exporting) return;
  exporting = true;
  toast(
    t("Rendering mixdown…"),
    state.backend === "local" ? t("The engine renders the song offline, in your browser.") : t("The native engine renders the song offline (plugins included)."),
    "info"
  );
  sendJson("/api/render", "POST", { bits: 24 })
    .then((r) => {
      exporting = false;
      const path = String(r.path);
      toast(t("Mixdown ready"), path + "\n" + tf("{0} s · peak {1} dBFS", [fmt(Number(r.duration), 1), fmt(Number(r.peakDb), 1)]), "info");
      download(String(r.url), path.split("/").pop() ?? "mixdown.wav");
      return true;
    })
    .catch((e) => {
      exporting = false;
      toast(t("Export failed"), String(e), "error");
      return false;
    });
}

/** Set the master transpose (semitones, -12..12). */
/** function setTranspose(n: Int) => Undefined */
function setTranspose(n) {
  const v = Math.max(-12, Math.min(12, n));
  if (v !== state.project.transport.transpose)
    commit(() => {
      state.project.transport.transpose = v;
    });
}

const TRANSPOSE_TIP = tk(
  "Transpose — shift every pitched instrument up or down by semitones, to suit a voice (drums and audio clips stay). Drag up/down, scroll or use the arrows; double-click: back to 0"
);

/** The master transpose, beside the tempo: what is written stays, what plays is shifted. */
/** function transposeLcd(b: Builder) => Undefined */
function transposeLcd(b) {
  const tr = state.project.transport.transpose;
  b.open("div", "transpose", tr !== 0 ? "lcd transpose shifted" : "lcd transpose");
  b.attr("title", t(TRANSPOSE_TIP));
  b.on("pointerenter", (e) => hint(t(TRANSPOSE_TIP)));
  b.on("pointerdown", (e) => {
    if (e.button !== 0) return undefined;
    e.preventDefault();
    begin();
    const y0 = e.clientY;
    const t0 = state.project.transport.transpose;
    drag(
      e,
      (m) => {
        const v = Math.max(-12, Math.min(12, t0 + Math.round((y0 - m.clientY) / 14)));
        if (v !== state.project.transport.transpose) {
          state.project.transport.transpose = v;
          changed(true);
        }
      },
      (u) => undefined
    );
  });
  b.on("wheel", (e) => {
    e.preventDefault();
    setTranspose(state.project.transport.transpose + (e.deltaY < 0 ? 1 : -1));
  });
  b.on("dblclick", (e) => setTranspose(0));
  b.leaf("span", "label", "lcd-label", t("Transpose"));
  b.open("span", "row", "transpose-row");
  b.leaf("button", "down", "transpose-step", "‹");
  b.attr("title", t("A semitone lower"));
  b.attr("aria-label", t("Transpose a semitone lower"));
  b.on("pointerdown", (e) => e.stopPropagation());
  b.on("click", (e) => setTranspose(state.project.transport.transpose - 1));
  b.open("span", "value", "lcd-value");
  b.text(semitonesText(tr));
  b.leaf("small", "unit", "", t("st"));
  b.close();
  b.leaf("button", "up", "transpose-step", "›");
  b.attr("title", t("A semitone higher"));
  b.attr("aria-label", t("Transpose a semitone higher"));
  b.on("pointerdown", (e) => e.stopPropagation());
  b.on("click", (e) => setTranspose(state.project.transport.transpose + 1));
  b.close();
  b.close();
}

/** function topbar(b: Builder) => Undefined */
export function topbar(b) {
  const p = state.project;
  b.open("header", "top", "topbar");

  b.open("div", "brand", "brand");
  b.open("div", "mark", "brand-mark");
  b.leaf("span", "r", "", "R");
  b.close();
  b.open("div", "text", "brand-text");
  b.leaf("div", "name", "brand-name", "Rosaclef");
  b.leaf("span", "sub", "brand-sub", t("Studio · AI edition"));
  b.close();
  b.close();

  projectsButton(b);
  b.leaf("div", "title", "song-title", p.meta.title);
  b.attr("title", state.folder);

  b.open("div", "transport", "transport");
  b.open("div", "mode", "seg");
  button(b, "pat", state.mode === "pattern" ? "on" : "", t("PAT"), t("Pattern mode: loop the selected pattern (Shift+L)"), () => setMode("pattern"));
  button(b, "song", state.mode === "song" ? "on" : "", t("SONG"), t("Song mode: play the playlist arrangement (Shift+L)"), () => setMode("song"));
  b.close();
  iconButton(b, "play", state.playing ? "play on" : "play", state.playing ? "pause" : "play", t("Play / pause (Space)"), () => {
    togglePlay();
  });
  iconButton(b, "stop", "stop", "stop", t("Stop and rewind"), () => {
    stop();
  });
  iconButton(b, "rec", state.recording ? "rec armed" : "rec", "mic", t("Record audio from the microphone onto the selected track (Shift+R)"), () => {
    record();
  });

  iconButton(b, "metro", state.metronome ? "metro on" : "metro", "metronome", t("Metronome: click every beat while playing (Shift+M)"), () => {
    toggleMetronome();
  });

  b.open("div", "pos", "lcd static");
  // During a count-in: the beats left before it starts.
  if (state.playing && state.position < 0) lcd(b, "pos", t("Count-in"), String(Math.ceil(-state.position - 1e-6)), "");
  else lcd(b, "pos", state.mode === "song" ? t("Song") : t("Pattern"), barBeat(state.position, p.transport), "");
  b.close();

  tempoLcd(b);
  meterLcd(b);

  transposeLcd(b);

  const swing = shownValue("swing", p.transport.swing);
  b.open("div", "swing", "lcd static");
  b.leaf("span", "label", "lcd-label", t("Swing"));
  knobAt(b, "k", "small", swing, "", tf("Swing {0}%", [String(Math.round(swing * 100))]), 0, "swing", (v) => {
    state.project.transport.swing = Math.round(v * 100) / 100;
  });
  b.close();
  b.close();

  b.leaf("div", "sp", "spacer", "");

  // The native engine exists only with a server that has an audio device.
  if (state.nativeAvailable) {
    b.open("div", "out", "seg");
    button(b, "browser", state.output === "browser" ? "on" : "", t("Browser"), t("Play through the WebAssembly engine in this browser"), () => setOutput("browser"));
    button(
      b,
      "native",
      state.output === "native" ? "on" : "",
      t("Studio"),
      t("Play through the native engine on the server's audio device (plugins, lowest latency)"),
      () => setOutput("native")
    );
    b.close();
  }

  const master = p.mixer.inserts.length > 0 ? p.mixer.inserts[0] : undefined;
  if (master) {
    const ml = state.meters.length > 1 ? state.meters[0] : 0;
    const mr = state.meters.length > 1 ? state.meters[1] : 0;
    b.open("div", "master", "master-mini");
    knobAt(b, "vol", "", shownValue("insert/0/volume", master.volume) / 1.25, "", t("Master volume"), 0.8, "insert/0/volume", (v) => {
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
    keyboard.shown ? t("Hide the on-screen piano") : t("Show the on-screen piano (plays the selected channel, or the instrument picked in the browser)"),
    () => {
      toggleKeyboard();
    }
  );
  iconButton(b, "undo", "", "undo", t("Undo (Ctrl+Z) — includes the agent's edits"), () => {
    undo();
  });
  iconButton(b, "redo", "", "redo", t("Redo (Ctrl+Shift+Z)"), () => {
    redo();
  });
  b.open("button", "export", "btn gold");
  b.attr("title", t("Render the song to a WAV file"));
  b.on("pointerenter", (e) => hint(t("Export: render the whole song offline to a 24-bit WAV (saved in renders/, and downloaded)")));
  b.on("click", (e) => {
    exportSong();
  });
  b.leaf("span", "t", "", t("Export"));
  b.close();
  languageSwitch(b);
  b.close();
}

/** The language switcher, at the right end of the top bar: the language's code over a menu of every language, each in its own name. */
/** function languageSwitch(b: Builder) => Undefined */
function languageSwitch(b) {
  const code = language();
  const tip = code === "en" ? "Language" : `${t("Language")} · Language`;
  b.open("label", "lang", "lang-switch");
  b.attr("title", tip);
  b.on("pointerenter", (e) => hint(tip));
  glyph(b, "globe");
  b.leaf("span", "code", "lang-code", code.split("-")[0].toUpperCase());
  b.open("select", "sel", "lang-select");
  b.attr("aria-label", tip);
  b.prop("value", code);
  b.on("change", (e) => {
    setLanguage(e.value, () => invalidate());
  });
  for (const l of LANGUAGES) {
    b.leaf("option", l.code, "", l.code === "en" || code === "en" ? l.name : `${l.name} — ${t(l.english)}`);
    b.attr("value", l.code);
    b.attr("lang", l.code);
  }
  b.close();
  b.close();
}
