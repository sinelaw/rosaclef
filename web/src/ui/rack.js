// The channel rack (step sequencer) and the instrument inspector.

import { drag, getJson } from "#platform";
import { state, commit, begin, changed, currentPattern, currentChannel, selectChannel, showDock, deviceSpec, invalidate, hint } from "../store.js";
import { getParam, setParam, getOption, setOption, presetDevice, PALETTE, copyArp } from "../model.js";
import { preview } from "../audio.js";
import { knobAt, paramKnobAt, select, button, iconButton, led, textInput, glyph } from "./widgets.js";
import { shownValue, retargetLanes } from "../automation.js";
import { insertIx, insertIndex } from "#brands";

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
  b.open("div", ch.id, ch.id === state.channel ? "rack-row sel" : "rack-row");

  b.leaf("div", "mute", ch.mute ? "ch-mute off" : "ch-mute", "");
  b.attr("title", ch.mute ? "Unmute channel" : "Mute channel");
  b.on("click", (e) => {
    commit(() => {
      ch.mute = !ch.mute;
    });
  });

  const pan = shownValue(`channel/${ch.id}/pan`, ch.pan);
  const vol = shownValue(`channel/${ch.id}/volume`, ch.volume);
  knobAt(b, "pan", "small", (pan + 1) / 2, "", `Pan ${Math.round(pan * 100)}`, 0.5, `channel/${ch.id}/pan`, (v) => {
    ch.pan = Math.round((v * 2 - 1) * 100) / 100;
  });
  knobAt(b, "vol", "small", vol / 1.25, "", `Volume ${Math.round(vol * 100)}%`, 0.64, `channel/${ch.id}/volume`, (v) => {
    ch.volume = Math.round(v * 125) / 100;
  });

  b.leaf("div", "ins", "ch-ins", insertIndex(ch.mixer) === 0 ? "M" : String(insertIndex(ch.mixer)));
  b.attr("title", "Mixer insert — drag up/down to reroute");
  b.on("pointerenter", (e) => hint("Mixer insert this channel plays through — drag to change, M = master"));
  b.on("pointerdown", (e) => {
    e.preventDefault();
    begin();
    const y0 = e.clientY;
    const i0 = insertIndex(ch.mixer);
    const max = state.project.mixer.inserts.length - 1;
    drag(
      e,
      (m) => {
        const i = Math.max(0, Math.min(max, i0 + Math.round((y0 - m.clientY) / 12)));
        ch.mixer = insertIx(i);
        changed(true);
      },
      (u) => undefined
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
  b.leaf("span", "n", "", ch.name);
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
      select(b, "sel", "", getOption(dev, o), o.choices, o.choices, o.doc, (v) => {
        commit(() => setOption(dev, o.key, v));
      });
      b.close();
    }
    b.close();
  }
  const params = spec.openParams ? pluginSpecs(dev) : spec.params;
  // Parameters named opN… (FM operators) are grouped per operator.
  /** const general: ParamSpec[] */
  const general = [];
  /** const groups: { title: String, params: ParamSpec[] }[] */
  const groups = [];
  for (const ps of params) {
    const digit = ps.key.charAt(2);
    if (!ps.key.startsWith("op") || digit < "1" || digit > "9") {
      general.push(ps);
      continue;
    }
    const title = `Operator ${digit}`;
    const g = groups.find((x) => x.title === title);
    if (g) g.params.push(ps);
    else groups.push({ title: title, params: [ps] });
  }
  b.open("div", "params", "params");
  for (const ps of general) {
    paramKnobAt(b, ps, getParam(dev, ps), target + ps.key, (v) => setParam(dev, ps.key, v));
  }
  b.close();
  for (const g of groups) {
    b.open("div", `grp-${g.title}`, "param-group");
    b.leaf("div", "t", "param-group-title", g.title);
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
  const labels = ["Factory presets…"].concat(presets.map((p) => `${p.name} — ${p.tags}`));
  b.open("div", "preset", "field");
  b.leaf("label", "l", "", "Preset");
  select(b, "sel", "", "", names, labels, "Load a factory preset into this channel", (v) => {
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
const ARP_RATE_NAMES = ["1/4", "1/8", "1/8 triplet", "1/16", "1/16 triplet", "1/32", "1/32 triplet", "1/64", "1/128"];
const ARP_DIRECTION_NAMES = [
  ["up", "Up"],
  ["down", "Down"],
  ["updown", "Up & down"],
  ["downup", "Down & up"],
  ["random", "Random"],
];
const ARP_GATES = [0.25, 0.5, 0.75, 1, 1.5];

/** A select over numbers; a value that is none of them is offered as itself. */
/** function numberSelect(b: Builder, key: String, label: String, value: Number, values: Number[], names: String[], unit: String, tip: String, onSet: (Number) => Undefined) => Undefined */
function numberSelect(b, key, label, value, values, names, unit, tip, onSet) {
  const vs = values.slice();
  const ns = names.slice();
  let at = vs.findIndex((v) => Math.abs(v - value) < 1e-6);
  if (at < 0) {
    vs.push(value);
    ns.push(`${Math.round(value * 1000) / 1000}${unit}`);
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
  b.leaf("div", "t", "param-group-title", "Arpeggio");
  button(b, "on", a.on ? "small gold" : "small", a.on ? "On" : "Off", "Play every note of this channel as a run of notes (the notes stay as written)", () => {
    commit(() => {
      a.on = !a.on;
    });
  });
  b.close();
  if (a.on) {
    b.open("div", "opts", "options");
    b.open("div", "chord", "option");
    b.leaf("label", "l", "", "Chord");
    select(b, "sel", "", a.chord, cat.chords, cat.chords, "Notes the run cycles through above each held note (octave = the note itself)", (v) => {
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
      "Octaves",
      Number(a.octaves),
      octaves,
      octaves.map((o) => String(o)),
      "",
      "Octaves the run spans",
      (v) => {
        commit(() => {
          a.octaves = Math.round(v);
        });
      }
    );
    numberSelect(b, "rate", "Rate", a.rate, ARP_RATES, ARP_RATE_NAMES, " beats", "Time from one note of the run to the next", (v) => {
      commit(() => {
        a.rate = v;
      });
    });
    b.open("div", "dir", "option");
    b.leaf("label", "l", "", "Direction");
    select(
      b,
      "sel",
      "",
      a.direction,
      ARP_DIRECTION_NAMES.map((d) => d[0]),
      ARP_DIRECTION_NAMES.map((d) => d[1]),
      "Order of the notes in the run",
      (v) => {
        commit(() => {
          a.direction = v;
        });
      }
    );
    b.close();
    numberSelect(b, "gate", "Gate", a.gate, ARP_GATES, ["25%", "50%", "75%", "100%", "150%"], "×", "Length of each note as a share of the rate", (v) => {
      commit(() => {
        a.gate = v;
      });
    });
    b.open("div", "mode", "option");
    b.leaf("label", "l", "", "Chords");
    select(
      b,
      "sel",
      "",
      a.mode,
      ["free", "sort"],
      ["Each note", "Take turns"],
      "Notes struck together: each runs its own arpeggio, or they take turns as one",
      (v) => {
        commit(() => {
          a.mode = v;
        });
      }
    );
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
    b.leaf("div", "none", "b-empty", "Select a channel");
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
  if (spec) b.leaf("div", "doc", "insp-doc", spec.doc);

  b.open("div", "name", "field");
  b.leaf("label", "l", "", "Channel name");
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

  b.open("div", "actions", "rack-add");
  button(b, "roll", "small", "Piano roll", "Edit this channel's notes (F7)", () => {
    showDock("piano");
  });
  button(b, "dup", "small", "Duplicate", "Duplicate this channel", () => {
    duplicateChannel(ch);
  });
  button(b, "del", "small danger", "Delete", "Delete this channel and its notes", () => {
    deleteChannel(ch);
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
    });
  });
  selectChannel(id);
}

/** function deleteChannel(ch: Channel) => Undefined */
function deleteChannel(ch) {
  const p = state.project;
  commit(() => {
    p.channels = p.channels.filter((c) => c.id !== ch.id);
    for (const pat of p.patterns) pat.notes = pat.notes.filter((n) => n.channel !== ch.id);
    retargetLanes((t) => (t.startsWith(`channel/${ch.id}/`) ? "" : t));
  });
}

/** function rack(b: Builder) => Undefined */
export function rack(b) {
  const pat = currentPattern();
  b.open("div", "rack", "rack");
  b.open("div", "list", "rack-list");
  if (!pat) {
    b.leaf("div", "none", "b-empty", "Create a pattern in the browser to start sequencing.");
  } else {
    let idx = 0;
    for (const ch of state.project.channels) {
      rackRow(b, pat, ch, idx);
      idx = idx + 1;
    }
    if (state.project.channels.length === 0) b.leaf("div", "empty", "b-empty", "Add an instrument from the browser — or ask the agent on the right.");
  }
  b.close();
  inspector(b);
  b.close();
}

/** function rackTools(b: Builder) => Undefined */
export function rackTools(b) {
  const pat = currentPattern();
  if (!pat) return undefined;
  b.leaf("span", "l", "label", "Length");
  const lengths = ["4", "8", "12", "16", "32", "64"];
  select(
    b,
    "len",
    "",
    String(pat.length),
    lengths,
    lengths.map((x) => `${Number(x) / 4} bar${x === "4" ? "" : "s"}`),
    "Pattern length",
    (v) => {
      commit(() => {
        pat.length = Number(v);
      });
    }
  );
}
