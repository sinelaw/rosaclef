// The top bar: brand, song title, transport, tempo, output and export.

import { drag, fmt } from "#platform";
import { state, begin, changed, commit, undo, redo, hint, invalidate, projectEdit } from "../store.js";
import { barBeat, semitonesText } from "../model.js";
import { togglePlay, stop, record, setMode, setOutput, toggleMetronome } from "../audio.js";
import { iconButton, button, knob, meter, glyph } from "./widgets.js";
import { t, tf, tk, language, setLanguage, LANGUAGES } from "../i18n.js";
import { isAutomated, shownValue, openMenu } from "../automation.js";
import { projectsButton } from "./projects.js";
import { keyboard, toggleKeyboard } from "./keyboard.js";
import { meterLcd } from "./meter.js";
import { jobLabel, jobFraction } from "./progress.js";
import { exp, openExport, closeExport } from "./export.js";

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
    b.leaf("span", "label", "lcd-label", t("term.tempo"));
    b.open("span", "value", "lcd-value");
    b.leaf("input", "in", "lcd-input", "");
    b.attr("aria-label", t("topbar.tempo.input.aria"));
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
  b.attr("title", t("topbar.tempo.title"));
  b.on("pointerenter", (e) => hint(t("topbar.tempo.hint")));
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
        changed();
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
  lcd(b, "bpm", t("term.tempo"), fmt(shownValue("tempo", p.transport.bpm), 2), "BPM");
  if (isAutomated("tempo")) b.leaf("i", "auto", "auto-dot", "");
  b.close();
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

const TRANSPOSE_TIP = tk("topbar.transpose.title");

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
          changed();
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
  b.leaf("span", "label", "lcd-label", t("topbar.transpose.label"));
  b.open("span", "row", "transpose-row");
  b.leaf("button", "down", "transpose-step", "‹");
  b.attr("title", t("topbar.transpose.down.title"));
  b.attr("aria-label", t("topbar.transpose.down.aria"));
  b.on("pointerdown", (e) => e.stopPropagation());
  b.on("click", (e) => setTranspose(state.project.transport.transpose - 1));
  b.open("span", "value", "lcd-value");
  b.text(semitonesText(tr));
  b.leaf("small", "unit", "", t("topbar.transpose.unit"));
  b.close();
  b.leaf("button", "up", "transpose-step", "›");
  b.attr("title", t("topbar.transpose.up.title"));
  b.attr("aria-label", t("topbar.transpose.up.aria"));
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
  b.leaf("span", "sub", "brand-sub", t("topbar.brand.subtitle"));
  b.close();
  b.close();

  projectsButton(b);
  b.leaf("div", "title", "song-title", p.meta.title);
  b.attr("title", state.folder);

  b.open("div", "transport", "transport");
  b.open("div", "mode", "seg");
  button(b, "pat", state.mode === "pattern" ? "on" : "", t("topbar.mode.pattern.label"), t("topbar.mode.pattern.title"), () => setMode("pattern"));
  button(b, "song", state.mode === "song" ? "on" : "", t("topbar.mode.song.label"), t("topbar.mode.song.title"), () => setMode("song"));
  b.close();
  iconButton(b, "play", state.playing ? "play on" : "play", state.playing ? "pause" : "play", t("topbar.play.title"), () => {
    togglePlay();
  });
  iconButton(b, "stop", "stop", "stop", t("topbar.stop.title"), () => {
    stop();
  });
  iconButton(b, "rec", state.recording ? "rec armed" : "rec", "mic", t("topbar.record.title"), () => {
    record();
  });

  iconButton(b, "metro", state.metronome ? "metro on" : "metro", "metronome", t("topbar.metronome.title"), () => {
    toggleMetronome();
  });

  b.open("div", "pos", "lcd static");
  // During a count-in: the beats left before it starts.
  if (state.playing && state.position < 0) lcd(b, "pos", t("term.countIn"), String(Math.ceil(-state.position - 1e-6)), "");
  else lcd(b, "pos", state.mode === "song" ? t("term.song") : t("term.pattern"), barBeat(state.position, p.transport), "");
  b.close();

  tempoLcd(b);
  meterLcd(b);

  transposeLcd(b);

  const swing = shownValue("swing", p.transport.swing);
  b.open("div", "swing", "lcd static");
  b.leaf("span", "label", "lcd-label", t("term.swing"));
  knob(b, {
    key: "k",
    cls: "small",
    value: swing,
    label: "",
    tip: tf("format.swingPercent", [String(Math.round(swing * 100))]),
    dflt: 0,
    target: "swing",
    edit: projectEdit,
    onSet: (v) => {
      state.project.transport.swing = Math.round(v * 100) / 100;
    },
  });
  b.close();
  b.close();

  b.leaf("div", "sp", "spacer", "");

  // The native engine exists only with a server that has an audio device.
  if (state.nativeAvailable) {
    b.open("div", "out", "seg");
    button(b, "browser", state.output === "browser" ? "on" : "", t("topbar.output.browser.label"), t("topbar.output.browser.title"), () =>
      setOutput("browser")
    );
    button(b, "native", state.output === "native" ? "on" : "", t("topbar.output.native.label"), t("topbar.output.native.title"), () => setOutput("native"));
    b.close();
  }

  const master = p.mixer.inserts.length > 0 ? p.mixer.inserts[0] : undefined;
  if (master) {
    const ml = state.meters.length > 1 ? state.meters[0] : 0;
    const mr = state.meters.length > 1 ? state.meters[1] : 0;
    b.open("div", "master", "master-mini");
    knob(b, {
      key: "vol",
      cls: "",
      value: shownValue("insert/0/volume", master.volume) / 1.25,
      label: "",
      tip: t("topbar.masterVolume.title"),
      dflt: 0.8,
      target: "insert/0/volume",
      edit: projectEdit,
      onSet: (v) => {
        const ins = state.project.mixer.inserts[0];
        ins.volume = Math.round(v * 1.25 * 1000) / 1000;
      },
    });
    meter(b, "meter", ml, mr);
    b.close();
  }

  iconButton(
    b,
    "keys",
    keyboard.shown ? "kb-toggle on" : "kb-toggle",
    "keys",
    keyboard.shown ? t("topbar.keyboard.hide.title") : t("topbar.keyboard.show.title"),
    () => {
      toggleKeyboard();
    }
  );
  iconButton(b, "undo", "", "undo", t("topbar.undo.title"), () => {
    undo();
  });
  iconButton(b, "redo", "", "redo", t("topbar.redo.title"), () => {
    redo();
  });
  const exporting = exp.rendering;
  b.open("button", "export", exporting ? "btn gold export busy" : exp.open ? "btn gold export on" : "btn gold export");
  b.attr("title", exporting ? jobLabel(exp.job) : t("topbar.export.dialog.title"));
  b.attr("aria-haspopup", "dialog");
  b.on("pointerenter", (e) => hint(t("topbar.export.dialog.hint")));
  b.on("click", (e) => {
    if (exp.open) closeExport();
    else openExport();
  });
  // While it renders: how far into the song, as a fill behind the label.
  b.style("--done", `${Math.round(jobFraction(exp.job) * 1000) / 10}%`);
  b.leaf("span", "t", "", exporting ? tf("topbar.export.progress.label", [String(Math.round(jobFraction(exp.job) * 100))]) : t("topbar.export.label"));
  b.close();
  languageSwitch(b);
  b.close();
}

/** The language switcher, at the right end of the top bar: the language's code over a menu of every language, each in its own name. */
/** function languageSwitch(b: Builder) => Undefined */
function languageSwitch(b) {
  const code = language();
  // In another language, the English word too, so that whoever cannot read it still finds the switch.
  const tip = code === "en" ? t("topbar.language.title") : `${t("topbar.language.title")} · Language`;
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
