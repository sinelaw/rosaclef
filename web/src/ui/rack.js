// The channel rack and the instrument inspector.
//
// The rack lists the song's channels (one instrument each). For the selected
// pattern, each row shows that channel's part: as a step sequencer (sixteen
// squares a bar, one per 16th note: click to place a hit) when every note
// fits on a step at one pitch — drums, mostly — or else as a small picture
// of its notes that opens the piano roll. An instrument dragged from the
// browser onto a row replaces that channel's instrument in place.

import { drag, getJson } from "#platform";
import { state, commit, begin, changed, currentPattern, currentChannel, selectChannel, showDock, deviceSpec, invalidate, hint } from "../store.js";
import { getParam, setParam, getOption, setOption, presetDevice, PALETTE, copyArp, newDevice, noArp } from "../model.js";
import { preview } from "../audio.js";
import { knobAt, paramKnobAt, select, button, iconButton, led, textInput, glyph } from "./widgets.js";
import { shownValue, retargetLanes } from "../automation.js";
import { insertIx, insertIndex } from "#brands";
import { sampleCredit } from "./credits.js";
import { dragPick, replaceInstrument, addPick } from "./instruments.js";
import { browseInstrument } from "./browser.js";
import { showInsert } from "./panes.js";
import { t, tf, tk } from "../i18n.js";

const STEP = 0.25;
const EPS = 0.000001;

/** function stepPitch(ch: Channel) => Number */
function stepPitch(ch) {
  return 60;
}

/** Notes of `ch` that the step grid can represent (16ths, short, one pitch). */
/** function isStepChannel(pat: Pattern, ch: Channel) => Boolean */
function isStepChannel(pat, ch) {
  for (const n of pat.notes) {
    if (n.channel !== ch.id) continue;
    const s = n.start / STEP;
    if (Math.abs(s - Math.round(s)) > EPS) return false;
    if (n.pitch !== stepPitch(ch) && ch.instrument.type !== "drum") return false;
    if (n.length > STEP + EPS && ch.instrument.type !== "drum") return false;
  }
  return true;
}

/** function stepNote(pat: Pattern, ch: Channel, step: Number) => Int */
function stepNote(pat, ch, step) {
  const t = step * STEP;
  return pat.notes.findIndex((n) => n.channel === ch.id && Math.abs(n.start - t) < EPS);
}

/** function toggleStep(pat: Pattern, ch: Channel, step: Number) => Undefined */
function toggleStep(pat, ch, step) {
  const i = stepNote(pat, ch, step);
  commit(() => {
    if (i >= 0) pat.notes.splice(i, 1);
    else pat.notes.push({ channel: ch.id, pitch: stepPitch(ch), start: step * STEP, length: STEP, velocity: 0.8 });
  });
  if (i < 0) preview(ch.id, stepPitch(ch), 0.8);
}

/** Where step `s` (a 16th) falls: "bar 2, beat 3" or "bar 2, beat 3 + 1/16". */
/** function stepName(s: Int) => String */
function stepName(s) {
  const bpb = Math.max(1, state.project.transport.beatsPerBar);
  const beat = Math.floor(s / 4);
  const bar = Math.floor(beat / bpb) + 1;
  const sub = s % 4;
  const beatNo = String((beat % bpb) + 1);
  return sub === 0 ? tf("rack.step.position", [String(bar), beatNo]) : tf("rack.step.positionSub", [String(bar), beatNo, String(sub)]);
}

/** The count above the steps: bar numbers on their downbeats, beats between. */
/** function ruler(b: Builder, steps: Int) => Undefined */
function ruler(b, steps) {
  const bpb = Math.max(1, state.project.transport.beatsPerBar);
  b.open("div", "ruler", "rack-row rack-ruler");
  b.leaf("div", "lead", "rr-lead", t("term.channel"));
  b.open("div", "beats", "steps");
  const groups = Math.ceil(steps / 4);
  for (let g = 0; g < groups; g++) {
    const down = g % bpb === 0;
    b.leaf("div", `g${g}`, down ? "rr-beat bar" : "rr-beat", down ? String(g / bpb + 1) : `.${(g % bpb) + 1}`);
    b.attr("title", down ? tf("format.bar", [String(g / bpb + 1)]) : tf("rack.ruler.beat.title", [String((g % bpb) + 1)]));
  }
  b.close();
  b.close();
}

/** A tiny piano-roll preview of one channel's notes (canvas leaf). */
/** function miniRoll(b: Builder, pat: Pattern, ch: Channel, width: Number) => Undefined */
function miniRoll(b, pat, ch, width) {
  b.canvas("mini", "mini-roll", (g, w, h) => {
    const notes = pat.notes.filter((n) => n.channel === ch.id);
    if (notes.length === 0) return undefined;
    let lo = 127;
    let hi = 0;
    for (const n of notes) {
      lo = Math.min(lo, n.pitch);
      hi = Math.max(hi, n.pitch);
    }
    const span = Math.max(6, hi - lo + 1);
    const nh = Math.max(2, (h - 6) / span);
    g.fillStyle = ch.color;
    g.shadowColor = ch.color;
    g.shadowBlur = 6;
    for (const n of notes) {
      const x = (n.start / pat.length) * w;
      const y = h - 3 - (n.pitch - lo + 1) * nh;
      g.globalAlpha = 0.5 + n.velocity * 0.5;
      g.fillRect(x, y, Math.max(2, (n.length / pat.length) * w - 1), Math.max(2, nh - 1));
    }
  });
  b.style("width", `${width}px`);
  b.attr("title", tf("rack.row.melody.title", [ch.name]));
  b.on("pointerenter", (e) => hint(tf("rack.row.melody.hint", [ch.name])));
  b.on("click", (e) => {
    selectChannel(ch.id);
    showDock("piano");
  });
}

/** function rackRow(b: Builder, pat: Pattern, ch: Channel, idx: Int) => Undefined */
function rackRow(b, pat, ch, idx) {
  const steps = Math.min(64, Math.round(pat.length / STEP));
  const playStep = state.playing && state.mode === "pattern" ? Math.floor(state.position / STEP) : -1;
  const level = idx < state.chMeters.length ? state.chMeters[idx] : 0;
  let rowCls = ch.id === state.channel ? "rack-row sel" : "rack-row";
  if (dragPick.on && dragPick.over === ch.id) rowCls = `${rowCls} drop`;
  b.open("div", ch.id, rowCls);
  b.on("dragover", (e) => {
    if (!dragPick.on) return undefined;
    e.preventDefault();
    e.stopPropagation();
    if (dragPick.over !== ch.id) {
      dragPick.over = ch.id;
      hint(tf("rack.row.drop.replace.hint", [ch.name, dragPick.pick.name]));
      invalidate();
    }
  });
  b.on("drop", (e) => {
    if (!dragPick.on) return undefined;
    e.preventDefault();
    e.stopPropagation();
    dragPick.on = false;
    dragPick.over = "";
    hint("");
    replaceInstrument(ch, dragPick.pick);
  });

  b.leaf("div", "mute", ch.mute ? "ch-mute off" : "ch-mute", "");
  b.attr("title", ch.mute ? t("rack.row.mute.titleUnmute") : t("rack.row.mute.title"));
  b.on("click", (e) => {
    commit(() => {
      ch.mute = !ch.mute;
    });
  });

  const pan = shownValue(`channel/${ch.id}/pan`, ch.pan);
  const vol = shownValue(`channel/${ch.id}/volume`, ch.volume);
  knobAt(b, "pan", "small", (pan + 1) / 2, "", tf("rack.row.pan.title", [String(Math.round(pan * 100))]), 0.5, `channel/${ch.id}/pan`, (v) => {
    ch.pan = Math.round((v * 2 - 1) * 100) / 100;
  });
  knobAt(b, "vol", "small", vol / 1.25, "", tf("rack.row.volume.title", [String(Math.round(vol * 100))]), 0.64, `channel/${ch.id}/volume`, (v) => {
    ch.volume = Math.round(v * 125) / 100;
  });

  b.leaf("div", "ins", "ch-ins", insertIndex(ch.mixer) === 0 ? "M" : String(insertIndex(ch.mixer)));
  b.attr("title", t("rack.row.mixer.title"));
  b.on("pointerenter", (e) => hint(t("rack.row.mixer.hint")));
  b.on("pointerdown", (e) => {
    e.preventDefault();
    const y0 = e.clientY;
    const i0 = insertIndex(ch.mixer);
    const max = state.project.mixer.inserts.length - 1;
    let moved = false;
    drag(
      e,
      (m) => {
        const i = Math.max(0, Math.min(max, i0 + Math.round((y0 - m.clientY) / 12)));
        if (!moved && i === i0) return undefined;
        if (!moved) {
          moved = true;
          begin();
        }
        ch.mixer = insertIx(i);
        changed(true);
      },
      (u) => {
        // A click without a drag: go to the insert.
        if (!moved) showInsert(ch.mixer);
      }
    );
  });

  b.open("div", "name", "ch-name");
  b.on("click", (e) => {
    selectChannel(ch.id);
  });
  b.on("dblclick", (e) => {
    selectChannel(ch.id);
    showDock("piano");
  });
  b.leaf("i", "sw", "swatch", "");
  b.style("--c", ch.color);
  b.leaf("span", "n", "", ch.layerOf !== "" ? `↳ ${ch.name}` : ch.name);
  if (ch.layerOf !== "") b.attr("title", tf("rack.row.layer.title", [channelLabel(ch.layerOf)]));
  b.close();
  led(b, "led", level);

  if (isStepChannel(pat, ch)) {
    b.open("div", "steps", "steps");
    const groups = Math.ceil(steps / 4);
    for (let g = 0; g < groups; g++) {
      b.open("div", `g${g}`, g % 2 === 1 ? "step-group alt" : "step-group");
      for (let k = 0; k < 4 && g * 4 + k < steps; k++) {
        const s = g * 4 + k;
        const ni = stepNote(pat, ch, s);
        let cls = "step";
        if (ni >= 0) cls = `${cls} on`;
        if (s === playStep) cls = `${cls} play`;
        b.leaf("div", `s${s}`, cls, "");
        b.style("--c", ch.color);
        b.on("pointerenter", (e) => hint(ni >= 0 ? tf("rack.step.remove.title", [ch.name, stepName(s)]) : tf("rack.step.place.title", [ch.name, stepName(s)])));
        if (ni >= 0) b.style("--vel", String(pat.notes[ni].velocity));
        b.on("pointerdown", (e) => {
          e.preventDefault();
          if (e.button === 2) {
            if (ni >= 0) toggleStep(pat, ch, s);
            return undefined;
          }
          toggleStep(pat, ch, s);
        });
        b.on("contextmenu", (e) => {
          e.preventDefault();
        });
        b.on("wheel", (e) => {
          if (ni < 0) return undefined;
          e.preventDefault();
          begin();
          const n = pat.notes[ni];
          n.velocity = Math.max(0.05, Math.min(1, Math.round((n.velocity - e.deltaY / 1000) * 100) / 100));
          changed(true);
        });
      }
      b.close();
    }
    b.close();
  } else {
    miniRoll(b, pat, ch, steps * 25 + Math.ceil(steps / 4) * 5);
  }
  b.close();
}

// ------------------------------------------------------------------ inspector

/** const pluginParams: { key: String, params: ParamSpec[] }[] */
const pluginParams = [];
/** const pluginLoading: String[] */
const pluginLoading = [];

/** function pluginSpecs(dev: Device) => ParamSpec[] */
function pluginSpecs(dev) {
  const key = `${getOptionRaw(dev, "path")}#${getOptionRaw(dev, "id")}`;
  for (const e of pluginParams) if (e.key === key) return e.params;
  if (!pluginLoading.includes(key)) {
    pluginLoading.push(key);
    getJson(`/api/plugins/params?path=${encodeURIComponent(getOptionRaw(dev, "path"))}&id=${encodeURIComponent(getOptionRaw(dev, "id"))}`)
      .then((r) => {
        /** const specs: ParamSpec[] */
        const specs = [];
        for (const pp of r.params) {
          specs.push({
            key: String(pp.id),
            label: String(pp.name),
            min: Number(pp.min),
            max: Number(pp.max),
            default: Number(pp.default),
            unit: "",
            curve: "linear",
            integer: pp.stepped === true,
            doc: String(pp.module),
          });
        }
        pluginParams.push({ key: key, params: specs });
        invalidate();
        return true;
      })
      .catch((e) => false);
  }
  /** const none: ParamSpec[] */
  const none = [];
  return none;
}

/** function getOptionRaw(d: Device, key: String) => String */
function getOptionRaw(d, key) {
  for (const o of d.options) if (o.key === key) return o.value;
  return "";
}

/** Knobs and selects for a device (instrument or effect). `target` is the
 * automation target prefix of its parameters ("channel/<id>/",
 * "insert/<n>/effect/<k>/"). */
/** function deviceControls(b: Builder, dev: Device, spec: DeviceSpec, target: String) => Undefined */
export function deviceControls(b, dev, spec, target) {
  if (spec.options.length > 0) {
    b.open("div", "opts", "options");
    for (const o of spec.options) {
      if (o.choices.length === 0) {
        if (dev.type === "plugin") continue;
        b.open("div", o.key, "option");
        b.leaf("label", "l", "", o.label);
        textInput(b, "in", "", getOption(dev, o), o.doc, (v) => {
          commit(() => setOption(dev, o.key, v));
        });
        b.close();
        continue;
      }
      b.open("div", o.key, "option");
      b.leaf("label", "l", "", o.label);
      // The tooltip says what the chosen value does.
      const cur = getOption(dev, o);
      const at = o.choices.indexOf(cur);
      const tip = at >= 0 && at < o.choiceDocs.length ? `${o.doc} ${cur}: ${o.choiceDocs[at]}.` : o.doc;
      select(b, "sel", "", cur, o.choices, o.choices, tip, (v) => {
        commit(() => setOption(dev, o.key, v));
      });
      b.close();
    }
    b.close();
    // A sampled instrument names its samples and their license.
    const prog = spec.options.find((o) => o.key === "program");
    sampleCredit(b, dev.type, prog ? getOption(dev, prog) : "");
  }
  const params = spec.openParams ? pluginSpecs(dev) : spec.params;
  // Parameters named opN… (FM operators) are grouped per operator.
  /** const general: ParamSpec[] */
  const general = [];
  /** const groups: { digit: String, params: ParamSpec[] }[] */
  const groups = [];
  for (const ps of params) {
    const digit = ps.key.charAt(2);
    if (!ps.key.startsWith("op") || digit < "1" || digit > "9") {
      general.push(ps);
      continue;
    }
    const g = groups.find((x) => x.digit === digit);
    if (g) g.params.push(ps);
    else groups.push({ digit: digit, params: [ps] });
  }
  b.open("div", "params", "params");
  for (const ps of general) {
    paramKnobAt(b, ps, getParam(dev, ps), target + ps.key, (v) => setParam(dev, ps.key, v));
  }
  b.close();
  for (const g of groups) {
    b.open("div", `grp-Operator ${g.digit}`, "param-group");
    b.leaf("div", "t", "param-group-title", tf("rack.params.operator.label", [g.digit]));
    b.open("div", "params", "params");
    for (const ps of g.params) {
      paramKnobAt(b, ps, getParam(dev, ps), target + ps.key, (v) => setParam(dev, ps.key, v));
    }
    b.close();
    b.close();
  }
}

/** Preset picker for an instrument (applies params and options). */
/** function presetPicker(b: Builder, dev: Device) => Undefined */
function presetPicker(b, dev) {
  const presets = state.catalog.presets.filter((p) => p.type === dev.type);
  if (presets.length === 0) return undefined;
  const names = [""].concat(presets.map((p) => p.name));
  const labels = [t("rack.preset.factory.label")].concat(presets.map((p) => `${p.name} — ${p.tags}`));
  b.open("div", "preset", "field");
  b.leaf("label", "l", "", t("rack.preset.label"));
  select(b, "sel", "", "", names, labels, t("rack.preset.title"), (v) => {
    const pr = presets.find((p) => p.name === v);
    if (!pr) return undefined;
    const fresh = presetDevice(pr);
    commit(() => {
      // Keep file references (e.g. a sampler's sample) that presets do not set.
      for (const o of dev.options) {
        if (!fresh.options.some((f) => f.key === o.key) && o.key === "sample") fresh.options.push(o);
      }
      dev.params = fresh.params;
      dev.options = fresh.options;
    });
  });
  b.close();
}

// Arpeggio rates (beats) as note values.
const ARP_RATES = [1, 0.5, 1 / 3, 0.25, 1 / 6, 0.125, 1 / 12, 0.0625, 0.03125];
/** Their names: a note value written as is ("1/4"), or a key to translate (key ≠ ""). */
/** const ARP_RATE_NAMES: { text: String, key: String }[] */
const ARP_RATE_NAMES = [
  { text: "1/4", key: "" },
  { text: "1/8", key: "" },
  { text: "", key: tk("rack.arp.rate.option.eighthTriplet") },
  { text: "1/16", key: "" },
  { text: "", key: tk("rack.arp.rate.option.sixteenthTriplet") },
  { text: "1/32", key: "" },
  { text: "", key: tk("rack.arp.rate.option.thirtySecondTriplet") },
  { text: "1/64", key: "" },
  { text: "1/128", key: "" },
];
const ARP_DIRECTION_NAMES = [
  ["up", tk("rack.arp.direction.option.up")],
  ["down", tk("rack.arp.direction.option.down")],
  ["updown", tk("rack.arp.direction.option.updown")],
  ["downup", tk("rack.arp.direction.option.downup")],
  ["random", tk("rack.arp.direction.option.random")],
];
const ARP_GATES = [0.25, 0.5, 0.75, 1, 1.5];

/** A select over numbers; a value that is none of them is offered as itself, written by `unit` (given the number). */
/** function numberSelect(b: Builder, key: String, label: String, value: Number, values: Number[], names: String[], unit: (String) => String, tip: String, onSet: (Number) => Undefined) => Undefined */
function numberSelect(b, key, label, value, values, names, unit, tip, onSet) {
  const vs = values.slice();
  const ns = names.slice();
  let at = vs.findIndex((v) => Math.abs(v - value) < 1e-6);
  if (at < 0) {
    vs.push(value);
    ns.push(unit(String(Math.round(value * 1000) / 1000)));
    at = vs.length - 1;
  }
  b.open("div", key, "option");
  b.leaf("label", "l", "", label);
  const keys = vs.map((v, i) => String(i));
  select(b, "sel", "", String(at), keys, ns, tip, (k) => onSet(vs[Math.round(Number(k))]));
  b.close();
}

/** The channel's arpeggiator: off, or its chord, range, rate, direction, gate and mode. */
/** function arpControls(b: Builder, ch: Channel) => Undefined */
function arpControls(b, ch) {
  const a = ch.arp;
  const cat = state.catalog.arp;
  b.open("div", "arp", "param-group arp");
  b.open("div", "head", "arp-head");
  b.leaf("div", "t", "param-group-title", t("rack.arp.label"));
  button(b, "on", a.on ? "small gold" : "small", a.on ? t("common.on") : t("common.off"), t("rack.arp.on.title"), () => {
    commit(() => {
      a.on = !a.on;
    });
  });
  b.close();
  if (a.on) {
    b.open("div", "opts", "options");
    b.open("div", "chord", "option");
    b.leaf("label", "l", "", t("rack.arp.chord.label"));
    select(b, "sel", "", a.chord, cat.chords, cat.chords, t("rack.arp.chord.title"), (v) => {
      commit(() => {
        a.chord = v;
      });
    });
    b.close();
    /** const octaves: Number[] */
    const octaves = [];
    for (let i = 1; i <= cat.octavesMax; i++) octaves.push(i);
    numberSelect(
      b,
      "octaves",
      t("rack.arp.octaves.label"),
      Number(a.octaves),
      octaves,
      octaves.map((o) => String(o)),
      (n) => n,
      t("rack.arp.octaves.title"),
      (v) => {
        commit(() => {
          a.octaves = Math.round(v);
        });
      }
    );
    numberSelect(
      b,
      "rate",
      t("rack.arp.rate.label"),
      a.rate,
      ARP_RATES,
      ARP_RATE_NAMES.map((n) => (n.key === "" ? n.text : t(n.key))),
      (n) => tf("format.beats", [n]),
      t("rack.arp.rate.title"),
      (v) => {
        commit(() => {
          a.rate = v;
        });
      }
    );
    b.open("div", "dir", "option");
    b.leaf("label", "l", "", t("rack.arp.direction.label"));
    select(
      b,
      "sel",
      "",
      a.direction,
      ARP_DIRECTION_NAMES.map((d) => d[0]),
      ARP_DIRECTION_NAMES.map((d) => t(d[1])),
      t("rack.arp.direction.title"),
      (v) => {
        commit(() => {
          a.direction = v;
        });
      }
    );
    b.close();
    numberSelect(
      b,
      "gate",
      t("rack.arp.gate.label"),
      a.gate,
      ARP_GATES,
      ["25%", "50%", "75%", "100%", "150%"],
      (n) => `${n}×`,
      t("rack.arp.gate.title"),
      (v) => {
        commit(() => {
          a.gate = v;
        });
      }
    );
    b.open("div", "mode", "option");
    b.leaf("label", "l", "", t("rack.arp.mode.label"));
    select(b, "sel", "", a.mode, ["free", "sort"], [t("rack.arp.mode.option.free"), t("rack.arp.mode.option.sort")], t("rack.arp.mode.title"), (v) => {
      commit(() => {
        a.mode = v;
      });
    });
    b.close();
    b.close();
  }
  b.close();
}

/** function inspector(b: Builder) => Undefined */
function inspector(b) {
  const ch = currentChannel();
  b.open("div", "insp", "inspector");
  if (!ch) {
    b.leaf("div", "none", "b-empty", t("rack.inspector.empty"));
    b.close();
    return undefined;
  }
  const spec = deviceSpec(ch.instrument.type, "instrument");
  b.open("div", "head", "insp-head");
  b.leaf("i", "sw", "swatch", "");
  b.style("--c", ch.color);
  b.open("div", "t", "");
  b.leaf("div", "title", "insp-title", spec ? spec.label : ch.instrument.type);
  b.leaf("div", "sub", "insp-sub", `${ch.name} · ${ch.instrument.type}`);
  b.close();
  b.close();
  instrumentChooser(b, ch);
  if (spec && spec.bestFor !== "") b.leaf("div", "best", "insp-doc", tf("rack.inspector.bestFor", [spec.bestFor]));
  if (spec) b.leaf("div", "doc", "insp-doc", spec.doc);

  b.open("div", "name", "field");
  b.leaf("label", "l", "", t("rack.inspector.name.label"));
  textInput(b, "in", "", ch.name, "", (v) => {
    commit(() => {
      ch.name = v;
    });
  });
  b.close();

  b.open("div", "colors", "options");
  for (const c of PALETTE) {
    b.leaf("i", c, "swatch", "");
    b.style("--c", c);
    b.style("cursor", "pointer");
    b.on("click", (e) => {
      commit(() => {
        ch.color = c;
      });
    });
  }
  b.close();

  presetPicker(b, ch.instrument);
  if (spec) deviceControls(b, ch.instrument, spec, `channel/${ch.id}/`);
  arpControls(b, ch);
  layerControls(b, ch);

  b.open("div", "actions", "rack-add");
  button(b, "roll", "small", t("panel.pianoRoll"), t("rack.inspector.roll.title"), () => {
    showDock("piano");
  });
  button(
    b,
    "mix",
    "small",
    insertIndex(ch.mixer) === 0 ? t("rack.inspector.mixer.labelMaster") : tf("rack.inspector.mixer.label", [String(insertIndex(ch.mixer))]),
    t("rack.inspector.mixer.title"),
    () => {
      showInsert(ch.mixer);
    }
  );
  button(b, "dup", "small", t("common.duplicate"), t("rack.inspector.duplicate.title"), () => {
    duplicateChannel(ch);
  });
  button(b, "del", "small danger", t("common.delete"), t("rack.inspector.delete.title"), () => {
    deleteChannel(ch);
  });
  b.close();
  b.close();
}

/** Swap this channel's instrument for another, keeping its notes (the
 * browser's ⇄ and dragging onto the rack do the same with presets). */
/** function instrumentChooser(b: Builder, ch: Channel) => Undefined */
function instrumentChooser(b, ch) {
  /** const kinds: String[] */
  const kinds = [];
  /** const labels: String[] */
  const labels = [];
  for (const d of state.catalog.devices) {
    if (d.category !== "instrument" || d.type === "plugin") continue;
    kinds.push(d.type);
    labels.push(d.label);
  }
  if (!kinds.includes(ch.instrument.type)) {
    kinds.push(ch.instrument.type);
    labels.push(ch.instrument.type);
  }
  b.open("div", "swap", "field insp-swap");
  b.leaf("label", "l", "", t("rack.inspector.instrument.label"));
  b.open("div", "row", "insp-swap-row");
  select(b, "sel", "", ch.instrument.type, kinds, labels, t("rack.inspector.instrument.title"), (v) => {
    if (v === ch.instrument.type) return undefined;
    const spec = deviceSpec(v, "instrument");
    replaceInstrument(ch, { key: `dev-${v}`, name: spec ? spec.label : v, device: newDevice(v) });
  });
  button(b, "browse", "small", t("rack.inspector.sounds.label"), t("rack.inspector.sounds.title"), () => {
    browseInstrument(ch.instrument.type);
    hint(tf("rack.inspector.sounds.hint", [ch.name]));
  });
  b.close();
  b.close();
}

/** function duplicateChannel(ch: Channel) => Undefined */
function duplicateChannel(ch) {
  const p = state.project;
  let n = 2;
  while (p.channels.some((c) => c.id === `${ch.id}-${n}`)) n = n + 1;
  const id = `${ch.id}-${n}`;
  commit(() => {
    p.channels.push({
      id: id,
      name: `${ch.name} ${n}`,
      color: ch.color,
      instrument: {
        type: ch.instrument.type,
        enabled: true,
        params: ch.instrument.params.map((x) => ({ key: x.key, value: x.value })),
        options: ch.instrument.options.map((x) => ({ key: x.key, value: x.value })),
      },
      volume: ch.volume,
      pan: ch.pan,
      mute: false,
      mixer: ch.mixer,
      arp: copyArp(ch.arp),
      layerOf: ch.layerOf,
    });
  });
  selectChannel(id);
}

/** The name of a channel by id (the id itself when it is gone). */
/** function channelLabel(id: String) => String */
function channelLabel(id) {
  for (const c of state.project.channels) if (c.id === id) return `“${c.name}”`;
  return `“${id}”`;
}

/** Layering: a layer plays the notes of another channel through its own
 * instrument (one level: a layer cannot be layered on). */
/** function layerControls(b: Builder, ch: Channel) => Undefined */
function layerControls(b, ch) {
  const p = state.project;
  b.open("div", "layer", "param-group layer");
  b.leaf("div", "t", "param-group-title", t("rack.layer.label"));
  b.open("div", "opts", "options");
  // Which channel's notes this one also plays.
  const hosts = p.channels.filter((c) => c.id !== ch.id && c.layerOf === "");
  const layered = p.channels.some((c) => c.layerOf === ch.id);
  if (!layered) {
    b.open("div", "of", "option");
    b.leaf("label", "l", "", t("rack.layer.alsoPlays.label"));
    select(
      b,
      "sel",
      "",
      ch.layerOf,
      [""].concat(hosts.map((c) => c.id)),
      [t("rack.layer.alsoPlays.option.none")].concat(hosts.map((c) => tf("rack.layer.alsoPlays.option.channel", [c.name]))),
      t("rack.layer.alsoPlays.title"),
      (v) => {
        commit(() => {
          ch.layerOf = v;
        });
      }
    );
    b.close();
  }
  if (ch.layerOf === "") {
    // Add a layer: a new channel playing this one's notes.
    /** const kinds: String[] */
    const kinds = [];
    /** const labels: String[] */
    const labels = [];
    for (const d of state.catalog.devices) {
      if (d.category !== "instrument" || d.type === "plugin" || d.type === "sampler") continue;
      kinds.push(d.type);
      labels.push(d.label);
    }
    b.open("div", "add", "option");
    b.leaf("label", "l", "", t("rack.layer.add.label"));
    select(b, "sel", "", "", [""].concat(kinds), [t("rack.layer.add.option.none")].concat(labels), t("rack.layer.add.title"), (v) => {
      if (v !== "") addLayer(ch, v);
    });
    b.close();
  }
  b.close();
  b.close();
}

/** function addLayer(ch: Channel, type: String) => Undefined */
function addLayer(ch, type) {
  const p = state.project;
  const base = `${ch.id.slice(0, 52)}-layer`;
  let id = base;
  let n = 2;
  while (p.channels.some((c) => c.id === id)) {
    id = `${base}${n}`;
    n = n + 1;
  }
  const spec = deviceSpec(type, "instrument");
  commit(() => {
    p.channels.push({
      id: id,
      name: `${ch.name} · ${spec ? spec.label : type}`,
      color: ch.color,
      instrument: newDevice(type),
      volume: ch.volume,
      pan: ch.pan,
      mute: false,
      mixer: ch.mixer,
      arp: noArp(),
      layerOf: ch.id,
    });
  });
  selectChannel(id);
}

/** function deleteChannel(ch: Channel) => Undefined */
function deleteChannel(ch) {
  const p = state.project;
  commit(() => {
    p.channels = p.channels.filter((c) => c.id !== ch.id);
    // Its layers keep their own notes and stop following it.
    for (const c of p.channels) if (c.layerOf === ch.id) c.layerOf = "";
    for (const pat of p.patterns) pat.notes = pat.notes.filter((n) => n.channel !== ch.id);
    retargetLanes((t) => (t.startsWith(`channel/${ch.id}/`) ? "" : t));
  });
}

/** function rack(b: Builder) => Undefined */
export function rack(b) {
  const pat = currentPattern();
  b.open("div", "rack", "rack");
  b.open("div", "list", dragPick.on && dragPick.over === "+" ? "rack-list drop" : "rack-list");
  b.on("dragover", (e) => {
    if (!dragPick.on) return undefined;
    e.preventDefault();
    if (dragPick.over !== "+") {
      dragPick.over = "+";
      hint(tf("rack.drop.add.hint", [dragPick.pick.name]));
      invalidate();
    }
  });
  b.on("drop", (e) => {
    if (!dragPick.on) return undefined;
    e.preventDefault();
    dragPick.on = false;
    dragPick.over = "";
    hint("");
    addPick(dragPick.pick);
  });
  if (!pat) {
    b.leaf("div", "none", "b-empty", t("rack.empty.noPattern"));
  } else {
    b.open("div", "guide", "rack-guide");
    b.leaf("b", "t", "", `${pat.name}`);
    b.leaf("span", "d", "", t("rack.guide.label"));
    b.close();
    if (state.project.channels.length > 0) ruler(b, Math.min(64, Math.round(pat.length / STEP)));
    let idx = 0;
    for (const ch of state.project.channels) {
      rackRow(b, pat, ch, idx);
      idx = idx + 1;
    }
    if (state.project.channels.length === 0) b.leaf("div", "empty", "b-empty", t("rack.empty.noChannels"));
  }
  b.close();
  inspector(b);
  b.close();
}

/** function rackTools(b: Builder) => Undefined */
export function rackTools(b) {
  const pat = currentPattern();
  if (!pat) return undefined;
  b.leaf("span", "l", "label", t("term.length"));
  const lengths = ["4", "8", "12", "16", "32", "64"];
  select(
    b,
    "len",
    "",
    String(pat.length),
    lengths,
    lengths.map((x) => (x === "4" ? t("format.barsOne") : tf("format.barsMany", [String(Number(x) / 4)]))),
    t("rack.length.title"),
    (v) => {
      commit(() => {
        pat.length = Number(v);
      });
    }
  );
}
