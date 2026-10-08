// The mixer: insert strips and the effect rack of the selected insert.

import { promptBox } from "#platform";
import { state, commit, selectInsert, deviceSpec, hint, projectEdit } from "../store.js";
import { newDevice, dbText, panText } from "../model.js";
import { fader, knob, meter, button, iconButton, select } from "./widgets.js";
import { deviceControls } from "./rack.js";
import { shownValue, remapEffects } from "../automation.js";
import { insertIx, insertIndex } from "#brands";
import { reveal, insertRevealed } from "./panes.js";
import { t, tf } from "../i18n.js";

/** Fader travel: 0..1 maps to -inf..+6 dB with a musical curve. */
/** function volToFader(v: Number) => Number */
function volToFader(v) {
  return Math.pow(v / 2, 1 / 3);
}

/** function faderToVol(t: Number) => Number */
function faderToVol(t) {
  return Math.round(Math.pow(t, 3) * 2 * 1000) / 1000;
}

/** function strip(b: Builder, ins: Insert, i: Int) => Undefined */
function strip(b, ins, i) {
  const ix = insertIx(i);
  const sel = insertIndex(state.insert) === i;
  let cls = i === 0 ? "strip master" : "strip";
  if (sel) cls = `${cls} sel`;
  const l = i * 2 + 1 < state.meters.length ? state.meters[i * 2] : 0;
  const r = i * 2 + 1 < state.meters.length ? state.meters[i * 2 + 1] : 0;
  const users = state.project.channels.filter((c) => insertIndex(c.mixer) === i).map((c) => c.name);

  b.open("div", `s${i}`, cls);
  // Asked for from a channel (the rack, the score): brought into view.
  if (reveal.insert === i) b.prop("reveal", "true");
  b.on("pointerdown", (e) => {
    if (!sel) selectInsert(ix);
  });
  b.on("pointerenter", (e) => hint(users.length > 0 ? tf("mixer.strip.fedBy.hint", [ins.name, users.join(", ")]) : `${ins.name}`));
  b.leaf("div", "num", "strip-num", i === 0 ? t("mixer.strip.master.label") : tf("mixer.strip.insert.label", [String(i)]));
  b.leaf("div", "name", "strip-name", ins.name);
  b.attr("title", t("common.doubleClickToRename"));
  b.on("dblclick", (e) => {
    const name = promptBox(t("mixer.strip.rename.prompt"), ins.name);
    if (name !== "") {
      commit(() => {
        ins.name = name;
      });
    }
  });

  b.open("div", "fx", "strip-fx");
  for (let k = 0; k < 4; k++) {
    if (k < ins.effects.length) {
      const fx = ins.effects[k];
      const spec = deviceSpec(fx.type, "effect");
      b.leaf("div", `f${k}`, fx.enabled ? "fx-slot on" : "fx-slot off", spec ? spec.label : fx.type);
    } else {
      b.leaf("div", `f${k}`, "fx-slot", "");
    }
  }
  if (ins.effects.length > 4) b.leaf("div", "more", "fx-slot", tf("mixer.strip.moreEffects", [String(ins.effects.length - 4)]));
  b.close();

  const pan = shownValue(`insert/${i}/pan`, ins.pan);
  const vol = shownValue(`insert/${i}/volume`, ins.volume);
  knob(b, {
    key: "pan",
    cls: "small",
    value: (pan + 1) / 2,
    label: "",
    tip: tf("mixer.strip.pan.title", [panText(pan)]),
    dflt: 0.5,
    target: `insert/${i}/pan`,
    edit: projectEdit,
    onSet: (v) => {
      ins.pan = Math.round((v * 2 - 1) * 100) / 100;
    },
  });

  b.open("div", "faders", "strip-faders");
  fader(b, {
    key: "vol",
    value: volToFader(vol),
    tip: `${ins.name}: ${dbText(vol)}`,
    dflt: volToFader(1),
    target: `insert/${i}/volume`,
    edit: projectEdit,
    onSet: (u) => {
      ins.volume = faderToVol(u);
    },
  });
  meter(b, "meter", l, r);
  b.close();
  b.leaf("div", "db", "strip-db", dbText(vol));

  b.open("div", "btns", "strip-btns");
  button(b, "m", ins.mute ? "small m on" : "small m", "M", t("common.mute"), () => {
    commit(() => {
      ins.mute = !ins.mute;
    });
  });
  if (i > 0) {
    button(b, "s", ins.solo ? "small s on" : "small s", "S", t("mixer.strip.solo.title"), () => {
      commit(() => {
        ins.solo = !ins.solo;
      });
    });
  }
  b.close();
  b.close();
}

/** function fxPanel(b: Builder) => Undefined */
function fxPanel(b) {
  const i = insertIndex(state.insert);
  const inserts = state.project.mixer.inserts;
  b.open("div", "panel", "fx-panel");
  if (i >= inserts.length) {
    b.close();
    return undefined;
  }
  const ins = inserts[i];
  b.open("div", "head", "insp-head");
  b.open("div", "t", "");
  b.leaf("div", "title", "insp-title", ins.name);
  b.leaf("div", "sub", "insp-sub", i === 0 ? t("mixer.fx.masterSubtitle") : tf("mixer.fx.insertSubtitle", [String(i)]));
  b.close();
  b.close();

  for (let k = 0; k < ins.effects.length; k++) {
    const fx = ins.effects[k];
    const spec = deviceSpec(fx.type, "effect");
    b.open("div", `fx${k}`, fx.enabled ? "fx-card" : "fx-card off");
    b.open("div", "head", "fx-card-head");
    b.leaf("div", "title", "fx-card-title", spec ? spec.label : fx.type);
    button(b, "on", fx.enabled ? "small on" : "small", fx.enabled ? t("common.on") : t("common.off"), t("mixer.fx.bypass.title"), () => {
      commit(() => {
        fx.enabled = !fx.enabled;
      });
    });
    iconButton(b, "up", "small ghost", "undo", t("mixer.fx.moveUp.title"), () => {
      if (k === 0) return undefined;
      commit(() => {
        const a = ins.effects[k - 1];
        ins.effects[k - 1] = ins.effects[k];
        ins.effects[k] = a;
        remapEffects(i, (j) => (j === k ? k - 1 : j === k - 1 ? k : j));
      });
    });
    iconButton(b, "del", "small ghost danger", "trash", t("mixer.fx.remove.title"), () => {
      commit(() => {
        ins.effects.splice(k, 1);
        remapEffects(i, (j) => (j === k ? -1 : j > k ? j - 1 : j));
      });
    });
    b.close();
    if (spec) deviceControls(b, fx, spec, `insert/${i}/effect/${k}/`);
    b.close();
  }

  /** const types: String[] */
  const types = [""];
  /** const labels: String[] */
  const labels = [t("mixer.fx.add.label")];
  for (const d of state.catalog.devices) {
    if (d.category === "effect" && d.type !== "plugin") {
      types.push(d.type);
      labels.push(d.label);
    }
  }
  for (const pl of state.catalog.plugins) {
    if (pl.effect) {
      types.push(`plugin:${pl.path}#${pl.id}`);
      labels.push(tf("format.clapPlugin", [pl.name]));
    }
  }
  select(b, "add", "", "", types, labels, t("mixer.fx.add.title"), (v) => {
    if (v === "") return undefined;
    commit(() => {
      if (v.startsWith("plugin:")) {
        const rest = v.slice(7);
        const hashAt = rest.lastIndexOf("#");
        const dev = newDevice("plugin");
        dev.options.push({ key: "format", value: "clap" });
        dev.options.push({ key: "path", value: rest.slice(0, hashAt) });
        dev.options.push({ key: "id", value: rest.slice(hashAt + 1) });
        ins.effects.push(dev);
      } else {
        ins.effects.push(newDevice(v));
      }
    });
  });
  b.close();
}

/** function mixer(b: Builder) => Undefined */
export function mixer(b) {
  b.open("div", "mixer", "mixer");
  b.open("div", "strips", "strips");
  const inserts = state.project.mixer.inserts;
  for (let i = 0; i < inserts.length; i++) strip(b, inserts[i], i);
  insertRevealed();
  b.close();
  fxPanel(b);
  b.close();
}

/** function mixerTools(b: Builder) => Undefined */
export function mixerTools(b) {
  iconButton(b, "add", "small ghost", "plus", t("mixer.addInsert.title"), () => {
    commit(() => {
      const n = state.project.mixer.inserts.length;
      state.project.mixer.inserts.push({ name: `Insert ${n}`, volume: 1, pan: 0, mute: false, solo: false, effects: [] });
    });
  });
}
