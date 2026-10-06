// Mix check: a Maestro plugin showing what `rosaclef mixcheck` measures. It is
// another face of the same tool: the panel sends the request object the command
// line builds from its flags (POST /api/mixcheck — the server, or the
// browser-only studio's worker) and draws the report that comes back, the JSON
// an agent reads. Trying a suggestion is a `whatIf` request; applying a fix
// sends `apply` and commits the patched project (one undo step). Nothing here
// measures audio: every number is the report's.
//
// It reads like a mastering meter: loudness (integrated, short-term,
// momentary) against a delivery target's bracket, true peak, PLR and LRA,
// gain-reduction meters, phase correlation, a loudness history, the spectrum
// (against a level-matched reference), then the mix's parts with their
// audibility, and the findings ranked with their fixes.

import { getJson, sendJson, fmt } from "#platform";
import { state, commit, invalidate, fixSelection, selectChannel, selectInsert, hint } from "../store.js";
import { encodeProject, decodeProject } from "../model.js";
import { button, select, glyph } from "./widgets.js";
import { openDock, setTop, setView, isCompact } from "./panes.js";
import { revealBeat } from "./playlist.js";
import { toast } from "./toast.js";
import { insertIx } from "#brands";
import { t, tf, tk } from "../i18n.js";

/** The six bands' names and ranges: t() translates them where they show. */
const BANDS = [tk("Sub"), tk("Bass"), tk("Low mid"), tk("Mid"), tk("High mid"), tk("Air")];
const BAND_TIPS = [tk("under 60 Hz"), "60–250 Hz", "250–500 Hz", "500 Hz–2 kHz", "2–6 kHz", tk("over 6 kHz")];

const view = {
  /** "song", "bars" or "section". */
  scope: "song",
  from: 1,
  to: 8,
  section: "",
  target: localStorage.getItem("rosaclef.mixcheck.target") ?? "spotify",
  threshold: "normal",
  /** A sample in the project to compare with ("" = none). */
  reference: "",
  busy: false,
  error: "",
  /** The project version the report is about (-1: none yet). */
  edits: -1,
  report: emptyReport(),
  targets /*: MixTargetInfo[] */: [],
  catalogAsked: false,
  /** Sections folded away. */
  folded /*: String[] */: [],
  /** The element whose suggestions show. */
  open: "",
  /** A what-if's outcome, by the patch it tried. */
  tried /*: { patch: String, summary: String }[] */: [],
};

/** function emptyGr() => MixGr */
function emptyGr() {
  return { id: "", effect: 0, kind: "", max: NaN, mean: NaN, above3: NaN };
}

/** function emptyMaster() => MixMaster */
function emptyMaster() {
  return {
    integrated: NaN,
    shortMax: NaN,
    momentaryMax: NaN,
    truePeak: NaN,
    samplePeak: NaN,
    rms: NaN,
    preLimiter: NaN,
    preEffects: NaN,
    limGr: emptyGr(),
    compGr: emptyGr(),
    plr: NaN,
    crest: NaN,
    lra: NaN,
    corrMean: NaN,
    corrMin: NaN,
    monoLoss: NaN,
    spectrum: [],
  };
}

/** function emptyReport() => MixReport */
function emptyReport() {
  return {
    ok: false,
    fromBar: 1,
    toBar: 1,
    fromBeat: 0,
    toBeat: 0,
    seconds: 0,
    repeats: false,
    master: emptyMaster(),
    target: { id: "", name: "", lufs: NaN, truePeak: NaN, status: "", gain: NaN, notes: [] },
    reference: { file: "", levelMatch: NaN, spectrumDiff: [], summary: "", master: emptyMaster() },
    rows: [],
    elements: [],
    gr: [],
    findings: [],
    history: { step: 0.2, t: [], m: [], s: [], tp: [], gr: [], bars: [] },
    whatIf: "",
    cached: false,
    ms: 0,
    renders: 0,
    warnings: [],
  };
}

// ------------------------------------------------------------------ decoding

/** A number of the report, NaN when it is absent or null. */
/** function num<T>(x: T) => Number */
function num(x) {
  return x === undefined || x === null ? NaN : Number(x);
}

/** function str<T>(x: T) => String */
function str(x) {
  return x === undefined || x === null ? "" : String(x);
}

/** function int<T>(x: T) => Int */
function int(x) {
  return x === undefined || x === null ? 0 : Math.round(Number(x));
}

/** function nums<T>(x: T[]) => Number[] */
function nums(x) {
  return (x ?? []).map((v) => num(v));
}

/** function decodeGr<T>(g: T) => MixGr */
function decodeGr(g) {
  if (g === undefined || g === null) return emptyGr();
  return { id: str(g.id), effect: int(g.effect), kind: str(g.type), max: num(g.max), mean: num(g.mean), above3: num(g.pctTimeAbove3) };
}

/** function decodeMaster<T>(m: T) => MixMaster */
function decodeMaster(m) {
  const c = m.correlation;
  const hasC = c !== undefined && c !== null;
  const sp = m.spectrum;
  return {
    integrated: num(m.integratedLufs),
    shortMax: num(m.shortTermLufsMax),
    momentaryMax: num(m.momentaryLufsMax),
    truePeak: num(m.truePeakDbtp),
    samplePeak: num(m.samplePeakDbfs),
    rms: num(m.rmsDbfs),
    preLimiter: num(m.preLimiterPeakDbfs),
    preEffects: num(m.preEffectsPeakDbfs),
    limGr: decodeGr(m.limiterGainReductionDb),
    compGr: decodeGr(m.compressorGainReductionDb),
    plr: num(m.plrDb),
    crest: num(m.crestFactorDb),
    lra: num(m.lra),
    corrMean: hasC ? num(c.mean) : NaN,
    corrMin: hasC ? num(c.min) : NaN,
    monoLoss: hasC ? num(c.monoLossDb) : NaN,
    spectrum: sp === undefined || sp === null ? [] : nums(sp.db),
  };
}

/** function patchText<T>(p: T) => String */
function patchText(p) {
  return p === undefined || p === null || p.length === 0 ? "" : JSON.stringify(p);
}

/** function decodeReport<T>(r: T) => MixReport */
function decodeReport(r) {
  const empty = emptyReport();
  const rg = r.range;
  const t = r.target;
  const ref = r.reference;
  const h = r.history;
  return {
    ok: true,
    fromBar: int(rg.fromBar),
    toBar: int(rg.toBar),
    fromBeat: num(rg.fromBeat),
    toBeat: num(rg.toBeat),
    seconds: num(rg.seconds),
    repeats: rg.repeats === true,
    master: decodeMaster(r.master),
    target:
      t === undefined || t === null
        ? empty.target
        : {
            id: str(t.id),
            name: str(t.name),
            lufs: num(t.lufs),
            truePeak: num(t.truePeakDbtp),
            status: str(t.status),
            gain: num(t.playbackGainDb),
            notes: (t.notes ?? []).map((x) => String(x)),
          },
    reference:
      ref === undefined || ref === null
        ? empty.reference
        : {
            file: str(ref.file),
            levelMatch: num(ref.levelMatchDb),
            spectrumDiff: nums(ref.delta.spectrumDbLevelMatched ?? []),
            summary: str(ref.summary),
            master: decodeMaster(ref.master),
          },
    rows: (r.perBar ?? []).map((x) => ({
      label: x.section !== undefined && x.section !== null ? String(x.section) : `${int(x.bar)}`,
      bar: int(x.bar),
      pass: int(x.pass),
      fromBeat: num(x.fromBeat),
      toBeat: num(x.toBeat),
      lufs: num(x.lufs),
      mMax: num(x.lufsMomentaryMax),
      sMax: num(x.lufsShortTermMax),
      peak: num(x.peakDbfs),
      truePeak: num(x.truePeakDbtp),
      preLimiter: num(x.preLimiterPeakDbfs),
      limGr: num(x.limiterGrMaxDb),
      corr: num(x.correlation),
      spectrum: nums(x.spectrumDb ?? []),
      top: (x.topContributors ?? []).map((c) => ({ id: String(c.id), pct: num(c.shareOfEnergyPct) })),
    })),
    elements: (r.elements ?? []).map((e) => {
      const a = e.audibility;
      const hasA = a !== undefined && a !== null;
      const dom = hasA ? a.dominantBandHz : null;
      return {
        id: str(e.id),
        name: str(e.name),
        kind: str(e.kind),
        role: str(e.role),
        lead: e.lead === true,
        buriedIn: (e.buriedIn ?? []).map((u) => ({ from: int(u.fromBar), to: int(u.toBar), rel: num(u.relativeToMixDb) })),
        insert: int(e.insert),
        rms: num(e.rmsDbfs),
        peak: num(e.peakDbfs),
        lufs: num(e.lufs),
        rel: num(e.relativeToMixDb),
        share: num(e.shareOfEnergyPct),
        active: num(e.activePct),
        corr: num(e.correlation),
        audible: hasA ? num(a.audibleFractionPct) : NaN,
        maskers: hasA ? (a.maskedBy ?? []).map((m) => ({ id: String(m.id), lo: num(m.bandHz[0]), hi: num(m.bandHz[1]), db: num(m.maskingDb) })) : [],
        domLo: dom === undefined || dom === null ? NaN : num(dom[0]),
        domHi: dom === undefined || dom === null ? NaN : num(dom[1]),
        fader: num(e.level.faderDb),
        verdict: str(e.verdict),
        suggestions: (e.suggestions ?? []).map((s) => ({
          why: str(s.why),
          patch: patchText(s.patch),
          expRel: num(s.expectedRelativeToMixDb),
          expAud: num(s.expectedAudibleFractionPct),
          verified: s.verified === undefined || s.verified === null ? "" : JSON.stringify(s.verified),
        })),
      };
    }),
    gr: (r.gainReduction ?? []).map((g) => decodeGr(g)),
    findings: (r.findings ?? []).map((f) => ({
      severity: str(f.severity),
      rule: str(f.rule),
      key: str(f.key),
      where: str(f.where),
      detail: str(f.detail),
      element: str(f.element),
      fromBar: int(f.fromBar),
      fromBeat: num(f.fromBeat),
      patch: patchText(f.fix),
      label: str(f.fixLabel),
    })),
    history:
      h === undefined || h === null
        ? empty.history
        : {
            step: num(h.stepSeconds),
            t: nums(h.t),
            m: nums(h.momentary),
            s: nums(h.shortTerm),
            tp: nums(h.truePeak),
            gr: nums(h.limiterGr),
            bars: (h.bars ?? []).map((x) => ({ t: num(x.t), bar: int(x.bar), pass: int(x.pass), beat: num(x.beat) })),
          },
    whatIf: r.whatIf === undefined || r.whatIf === null ? "" : str(r.whatIf.summary),
    cached: r.render.cached === true,
    ms: num(r.render.ms),
    renders: int(r.render.renders),
    warnings: (r.warnings ?? []).map((w) => String(w)),
  };
}

/** function errText<E>(e: E) => String */
function errText(e) {
  return String(e && e.message ? e.message : e);
}

// ------------------------------------------------------------------ requests

/** The song's named passages (labelled score marks, the drum part's sections). */
/** function sectionNames() => String[] */
function sectionNames() {
  /** const out: String[] */
  const out = [];
  for (const m of state.project.score.marks) {
    const l = m.label.trim();
    if (m.pattern === "" && l !== "" && !out.includes(l)) out.push(l);
  }
  if (state.project.drums.on) {
    for (const s of state.project.drums.sections) {
      const l = s.name.trim();
      if (l !== "" && !out.includes(l)) out.push(l);
    }
  }
  return out;
}

/** The request's settings: the same as `rosaclef mixcheck`'s flags. */
/** function request() => { range: String, section: String, target: String, threshold: String, reference: String } */
function request() {
  const sections = sectionNames();
  const section = view.scope === "section" ? (sections.includes(view.section) ? view.section : sections.length > 0 ? sections[0] : "") : "";
  return {
    range: view.scope === "bars" ? `${Math.max(1, Math.round(view.from))}:${Math.max(Math.round(view.from), Math.round(view.to))}` : "",
    section: section,
    target: view.target,
    threshold: view.threshold,
    reference: view.reference,
  };
}

function loadCatalog() {
  view.catalogAsked = true;
  getJson("/api/mixcheck")
    .then((r) => {
      view.targets = r.targets.map((t) => ({ id: String(t.id), name: String(t.name), lufs: num(t.lufs), truePeak: num(t.truePeakDbtp) }));
      invalidate();
      return true;
    })
    .catch((e) => false);
}

/** Measure the song (or the range) as it is now. */
export function runMixcheck() {
  if (view.busy) return undefined;
  const edits = state.edits;
  view.busy = true;
  view.error = "";
  invalidate();
  const q = request();
  sendJson("/api/mixcheck", "POST", {
    project: encodeProject(state.project),
    range: q.range,
    section: q.section,
    target: q.target,
    threshold: q.threshold,
    reference: q.reference,
    history: true,
  })
    .then((r) => {
      view.busy = false;
      view.edits = edits;
      view.report = decodeReport(r);
      view.tried = [];
      invalidate();
      return true;
    })
    .catch((e) => {
      view.busy = false;
      view.error = errText(e);
      invalidate();
      return false;
    });
}

/** Whether the report is about the song as it is now: its fixes point at
 * notes and devices by their position, which an edit since may have moved. */
/** function fresh() => Boolean */
function fresh() {
  if (view.edits === state.edits) return true;
  toast(t("The song changed since this report"), t("Measure again first: its fixes point at notes and devices by position."), "info");
  return false;
}

/** Measure a change without making it (a what-if: the project is not touched). */
/** function tryPatch(patch: String) => Undefined */
function tryPatch(patch) {
  if (view.busy || !fresh()) return undefined;
  view.busy = true;
  invalidate();
  const q = request();
  sendJson("/api/mixcheck", "POST", { project: encodeProject(state.project), range: q.range, section: q.section, threshold: q.threshold, whatIf: patch })
    .then((r) => {
      view.busy = false;
      const summary = r.whatIf === undefined || r.whatIf === null ? "" : String(r.whatIf.summary);
      view.tried = view.tried.filter((x) => x.patch !== patch).concat([{ patch: patch, summary: summary }]);
      invalidate();
      return true;
    })
    .catch((e) => {
      view.busy = false;
      toast(t("Could not try it"), errText(e), "error");
      invalidate();
      return false;
    });
}

/** Apply a fix: the endpoint patches the project as it is and validates it;
 * one undo step. If the song changed meanwhile, nothing is applied. */
/** function applyPatch(patch: String, what: String) => Undefined */
function applyPatch(patch, what) {
  if (!fresh()) return undefined;
  const edits = state.edits;
  sendJson("/api/mixcheck", "POST", { project: encodeProject(state.project), apply: patch })
    .then((r) => {
      if (state.edits !== edits) {
        toast(t("The song changed meanwhile"), t("Nothing was applied; try again."), "info");
        return false;
      }
      const fixed = decodeProject(r.project);
      commit(() => {
        state.project = fixed;
      });
      fixSelection();
      toast(what, t("Ctrl+Z undoes it. Check the mix again to measure it."), "info");
      return true;
    })
    .catch((e) => {
      toast(t("Could not apply it"), errText(e), "error");
      return false;
    });
}

/** The number of warnings to show on the tab. */
/** function mixcheckCount() => Int */
export function mixcheckCount() {
  return view.report.findings.filter((f) => f.severity === "warn").length;
}

// ------------------------------------------------------------------ helpers

/** function db(x: Number, digits: Number) => String */
function db(x, digits) {
  if (!Number.isFinite(x)) return "—";
  return fmt(x, digits);
}

/** function signed(x: Number) => String */
function signed(x) {
  if (!Number.isFinite(x)) return "—";
  return `${x > 0 ? "+" : x < 0 ? "−" : ""}${fmt(Math.abs(x), 1)}`;
}

/** Traffic light: "ok", "warn" or "bad". */
/** function light(bad: Boolean, warn: Boolean) => String */
function light(bad, warn) {
  return bad ? "bad" : warn ? "warn" : "ok";
}

/** Where to look in the studio for an element of the report. */
/** function revealElement(id: String) => Undefined */
function revealElement(id) {
  if (id.startsWith("channel:")) {
    const ch = id.slice(8);
    selectChannel(ch);
    const c = state.project.channels.find((x) => x.id === ch);
    if (c) selectInsert(c.mixer);
    openDock("mixer");
  } else if (id.startsWith("insert:")) {
    const n = Math.round(Number(id.slice(7).split("/")[0]));
    selectInsert(insertIx(n));
    openDock("mixer");
  }
  invalidate();
}

/** Show the bar in the playlist. */
/** function revealBar(beat: Number) => Undefined */
function revealBar(beat) {
  setTop("playlist");
  if (isCompact(window.innerWidth, window.innerHeight)) setView("playlist");
  revealBeat(Math.max(0, beat));
  invalidate();
}

/** The name of a channel or insert id, for people. */
/** function nameOf(id: String) => String */
function nameOf(id) {
  for (const e of view.report.elements) if (e.id === id) return e.name;
  if (id.startsWith("channel:")) {
    const c = state.project.channels.find((x) => x.id === id.slice(8));
    if (c) return c.name;
  }
  const slash = id.indexOf("/");
  return slash >= 0 ? id.slice(slash + 1) : id;
}

/** function section(b: Builder, key: String, title: String, sub: String) => Boolean */
function section(b, key, title, sub) {
  const folded = view.folded.includes(key);
  b.open("div", `h-${key}`, folded ? "mx-head folded" : "mx-head");
  b.on("click", (e) => {
    if (folded) view.folded.splice(view.folded.indexOf(key), 1);
    else view.folded.push(key);
    invalidate();
  });
  b.leaf("span", "caret", "mx-caret", "▾");
  b.leaf("b", "t", "", title);
  if (sub !== "") b.leaf("span", "s", "mx-sub", sub);
  b.close();
  return !folded;
}

// ------------------------------------------------------------------ meters

/** A readout: label, value, unit, and a traffic light. */
/** function readout(b: Builder, key: String, label: String, value: String, unit: String, state_: String, tip: String) => Undefined */
function readout(b, key, label, value, unit, state_, tip) {
  b.open("div", key, `mx-readout ${state_}`);
  b.attr("title", tip);
  b.on("pointerenter", (e) => hint(tip));
  b.leaf("span", "l", "mx-readout-label", label);
  b.open("span", "v", "mx-readout-value");
  b.text(value);
  b.leaf("small", "u", "", unit);
  b.close();
  b.close();
}

/** The loudness scale (−36 … 0 LUFS) with the target's bracket and the song's readings. */
/** function loudnessScale(b: Builder, m: MixMaster, tg: MixTarget) => Undefined */
function loudnessScale(b, m, tg) {
  const lo = -36;
  const hi = 0;
  /** function at(x: Number) => String */
  const at = (x) => `${fmt(Math.max(0, Math.min(100, ((x - lo) / (hi - lo)) * 100)), 2)}%`;
  b.open("div", "scale", "mx-scale");
  b.attr("title", t("Loudness scale (LUFS): the bracket is the target's ±1 LU; ▼ integrated, the bar the short-term range"));
  b.leaf("div", "track", "mx-scale-track", "");
  if (Number.isFinite(tg.lufs)) {
    b.leaf("div", "zone", "mx-scale-zone", "");
    b.style("left", at(tg.lufs - 1));
    b.style("width", `${fmt((2 / (hi - lo)) * 100, 2)}%`);
  }
  if (Number.isFinite(m.integrated) && Number.isFinite(m.shortMax)) {
    const low = Math.min(m.integrated, m.shortMax) - (Number.isFinite(m.lra) ? m.lra : 0);
    b.leaf("div", "range", "mx-scale-range", "");
    b.style("left", at(low));
    b.style("width", `${fmt(Math.max(0.5, ((m.shortMax - low) / (hi - lo)) * 100), 2)}%`);
  }
  if (Number.isFinite(m.integrated)) {
    b.leaf("div", "i", "mx-scale-mark", "▼");
    b.style("left", at(m.integrated));
  }
  for (const x of [-36, -30, -24, -18, -14, -9, -6, 0]) {
    b.leaf("span", `t${x}`, "mx-scale-tick", `${x}`);
    b.style("left", at(x));
  }
  b.close();
}

/** A gain-reduction meter: a bar hanging from the top, 0 … 12 dB, its mean and its peak. */
/** function grMeter(b: Builder, key: String, label: String, g: MixGr) => Undefined */
function grMeter(b, key, label, g) {
  const tip = tf("{0}: gain reduction up to {1} dB, {2} dB on average, over 3 dB {3}% of the time", [label, db(g.max, 1), db(g.mean, 1), db(g.above3, 0)]);
  b.open("div", key, "mx-gr");
  b.attr("title", tip);
  b.on("pointerenter", (e) => hint(tip));
  b.open("div", "bar", "mx-gr-bar");
  b.leaf("div", "mean", g.mean >= 3 ? "mx-gr-mean hot" : "mx-gr-mean", "");
  b.style("height", `${fmt(Math.min(100, (Math.max(0, g.mean) / 12) * 100), 1)}%`);
  b.leaf("div", "max", "mx-gr-max", "");
  b.style("top", `${fmt(Math.min(100, (Math.max(0, g.max) / 12) * 100), 1)}%`);
  b.close();
  b.leaf("span", "l", "mx-gr-label", label);
  b.leaf("span", "v", "mx-gr-value", Number.isFinite(g.max) ? `−${fmt(g.max, 1)}` : "—");
  b.close();
}

/** The correlation meter: −1 … +1, the mean as a needle, the lowest as a tick. */
/** function correlationMeter(b: Builder, m: MixMaster) => Undefined */
function correlationMeter(b, m) {
  /** function at(x: Number) => String */
  const at = (x) => `${fmt(((Math.max(-1, Math.min(1, x)) + 1) / 2) * 100, 2)}%`;
  const tip = tf("Phase correlation: {0} on average, {1} at its lowest (+1 mono, 0 wide, below 0 the sides cancel in mono); mono fold-down {2} dB", [
    db(m.corrMean, 2),
    db(m.corrMin, 2),
    signed(m.monoLoss),
  ]);
  b.open("div", "corr", "mx-corr");
  b.attr("title", tip);
  b.on("pointerenter", (e) => hint(tip));
  b.leaf("span", "l", "mx-corr-end", "−1");
  b.open("div", "track", "mx-corr-track");
  b.leaf("div", "mid", "mx-corr-mid", "");
  if (Number.isFinite(m.corrMin)) {
    b.leaf("div", "min", m.corrMin < 0 ? "mx-corr-min bad" : "mx-corr-min", "");
    b.style("left", at(m.corrMin));
  }
  if (Number.isFinite(m.corrMean)) {
    b.leaf("div", "needle", m.corrMean < 0 ? "mx-corr-needle bad" : m.corrMean < 0.3 ? "mx-corr-needle warn" : "mx-corr-needle", "");
    b.style("left", at(m.corrMean));
  }
  b.close();
  b.leaf("span", "r", "mx-corr-end", "+1");
  b.close();
}

// ------------------------------------------------------------------ graphs

/** The loudness history: short-term (gold) and momentary (champagne) LUFS, the
 * target (dashed), true peaks over the limit (rose dots), limiter gain
 * reduction hanging from the top, bar numbers along the bottom. */
/** function paintHistory(g: Ctx, w: Number, h: Number, r: MixReport) => Undefined */
function paintHistory(g, w, h, r) {
  const hs = r.history;
  g.fillStyle = "#0d0b10";
  g.fillRect(0, 0, w, h);
  const n = hs.t.length;
  if (n === 0) return undefined;
  const top = 6;
  const bottom = h - 14;
  // The range the music covers (6 dB steps), like a meter's auto range.
  let vmin = 0;
  let vmax = -60;
  for (let i = 0; i < n; i++) {
    const s = hs.s[i];
    const m = hs.m[i];
    if (Number.isFinite(s) && s > -60) vmin = Math.min(vmin, s);
    if (Number.isFinite(m) && m > -60) vmax = Math.max(vmax, m);
  }
  if (Number.isFinite(r.target.lufs)) {
    vmin = Math.min(vmin, r.target.lufs);
    vmax = Math.max(vmax, r.target.lufs);
  }
  const lo = Math.max(-60, Math.floor((Math.min(vmin, vmax - 12) - 3) / 6) * 6);
  const hi = Math.min(0, Math.ceil((vmax + 2) / 6) * 6);
  /** function y(v: Number) => Number */
  const y = (v) => top + ((hi - Math.max(lo, Math.min(hi, v))) / (hi - lo)) * (bottom - top);
  /** function x(i: Number) => Number */
  const x = (i) => (n <= 1 ? 0 : (i / (n - 1)) * (w - 1));
  g.font = "9px JetBrains Mono, monospace";
  g.textBaseline = "middle";
  for (let v = lo + 6; v < hi; v += 6) {
    g.strokeStyle = "rgba(212, 175, 55, 0.08)";
    g.lineWidth = 1;
    g.beginPath();
    g.moveTo(0, Math.round(y(v)) + 0.5);
    g.lineTo(w, Math.round(y(v)) + 0.5);
    g.stroke();
    g.fillStyle = "rgba(163, 151, 128, 0.6)";
    g.fillText(`${v}`, 3, y(v) - 6);
  }
  // Gain reduction, from the top (0 … 12 dB over a quarter of the height).
  g.fillStyle = "rgba(208, 138, 147, 0.32)";
  for (let i = 0; i < n; i++) {
    const gr = hs.gr[i];
    if (Number.isFinite(gr) && gr > 0.05) g.fillRect(x(i) - 0.5, 0, Math.max(1.5, w / n), (Math.min(12, gr) / 12) * (h / 4));
  }
  if (Number.isFinite(r.target.lufs)) {
    g.strokeStyle = "rgba(70, 165, 139, 0.8)";
    g.setLineDash([4, 3]);
    g.beginPath();
    g.moveTo(0, y(r.target.lufs));
    g.lineTo(w, y(r.target.lufs));
    g.stroke();
    g.setLineDash([]);
  }
  /** function line(vs: Number[], color: String, width: Number) => Undefined */
  const line = (vs, color, width) => {
    g.strokeStyle = color;
    g.lineWidth = width;
    g.beginPath();
    let pen = false;
    for (let i = 0; i < n; i++) {
      const v = vs[i];
      if (!Number.isFinite(v) || v < -70) {
        pen = false;
        continue;
      }
      if (pen) g.lineTo(x(i), y(v));
      else g.moveTo(x(i), y(v));
      pen = true;
    }
    g.stroke();
  };
  line(hs.m, "rgba(232, 213, 176, 0.45)", 1);
  line(hs.s, "#d4af37", 1.8);
  // True peaks over the target's limit (or −1 dBTP).
  const tpMax = Number.isFinite(r.target.truePeak) ? r.target.truePeak : -1;
  g.fillStyle = "#e0707e";
  for (let i = 0; i < n; i++) {
    const v = hs.tp[i];
    if (Number.isFinite(v) && v > tpMax) {
      g.beginPath();
      g.arc(x(i), top + 2, 2, 0, 6.2832);
      g.fill();
    }
  }
  // Bar numbers.
  g.fillStyle = "rgba(163, 151, 128, 0.85)";
  g.textBaseline = "alphabetic";
  const span = n <= 1 ? 1 : hs.t[n - 1] - hs.t[0];
  const every = Math.max(1, Math.ceil(hs.bars.length / Math.max(1, w / 34)));
  for (let k = 0; k < hs.bars.length; k += every) {
    const m = hs.bars[k];
    const px = span > 0 ? ((m.t - hs.t[0]) / span) * (w - 1) : 0;
    g.fillRect(Math.round(px), bottom, 1, 4);
    g.fillText(m.pass > 1 ? `${m.bar}′` : `${m.bar}`, px + 2, h - 2);
  }
}

/** The spectrum in six bands (dB), with the reference (level-matched) as outlines. */
/** function paintSpectrum(g: Ctx, w: Number, h: Number, r: MixReport) => Undefined */
function paintSpectrum(g, w, h, r) {
  g.fillStyle = "#0d0b10";
  g.fillRect(0, 0, w, h);
  const sp = r.master.spectrum;
  if (sp.length === 0) return undefined;
  const finite = sp.filter((v) => Number.isFinite(v));
  const top = finite.reduce((a, v) => Math.max(a, v), -120);
  const hi = Math.ceil((top + 3) / 6) * 6;
  const lo = hi - 42;
  const bottom = h - 14;
  /** function y(v: Number) => Number */
  const y = (v) => 4 + ((hi - Math.max(lo, Math.min(hi, v))) / (hi - lo)) * (bottom - 4);
  const bw = w / sp.length;
  g.font = "9px Manrope, sans-serif";
  g.textAlign = "center";
  for (let i = 0; i < sp.length; i++) {
    const x0 = i * bw + 4;
    const grad = g.createLinearGradient(0, bottom, 0, 4);
    grad.addColorStop(0, "rgba(125, 95, 34, 0.9)");
    grad.addColorStop(1, "rgba(245, 226, 166, 0.95)");
    g.fillGradient(grad);
    g.fillRect(x0, y(sp[i]), bw - 8, bottom - y(sp[i]));
    const diff = i < r.reference.spectrumDiff.length ? r.reference.spectrumDiff[i] : NaN;
    if (Number.isFinite(diff)) {
      // Where the reference would be at the mix's loudness.
      g.strokeStyle = "#5b82c4";
      g.lineWidth = 2;
      g.beginPath();
      g.moveTo(x0 - 1, y(sp[i] - diff));
      g.lineTo(x0 + bw - 7, y(sp[i] - diff));
      g.stroke();
    }
    g.fillStyle = "rgba(163, 151, 128, 0.9)";
    g.fillText(t(BANDS[i]), i * bw + bw / 2, h - 3);
    g.fillStyle = "rgba(246, 238, 221, 0.85)";
    g.fillText(Number.isFinite(sp[i]) ? fmt(sp[i], 0) : "", i * bw + bw / 2, Math.max(12, y(sp[i]) - 3));
  }
  g.textAlign = "left";
}

// ------------------------------------------------------------------ parts

/** function verdictText(v: String) => String */
function verdictText(v) {
  return v === "inaudible"
    ? t("inaudible")
    : v === "buried"
      ? t("buried")
      : v === "dominant"
        ? t("dominant")
        : v === "overloading"
          ? t("overloading")
          : t("ok");
}

/** One part of the mix: its level against the mix, its audibility, who masks it, and what to do. */
/** function elementRow(b: Builder, e: MixElement) => Undefined */
function elementRow(b, e) {
  const open = view.open === e.id;
  b.open("div", e.id, `mx-el ${e.verdict}${open ? " open" : ""}`);
  b.open("div", "row", "mx-el-row");
  b.on("click", (ev) => {
    view.open = open ? "" : e.id;
    invalidate();
  });
  b.leaf("span", "v", `mx-verdict ${e.verdict}`, verdictText(e.verdict));
  b.leaf("b", "n", "mx-el-name", e.name);
  if (e.lead) {
    b.open("span", "role", "mx-el-role lead");
    b.attr("title", t("The song's lead: also buried when it sits far under the mix"));
    b.text(t("lead"));
    b.close();
  } else if (e.role !== "") b.leaf("span", "role", "mx-el-role", e.role);
  b.leaf("span", "sp", "spacer", "");
  // Level against the mix: −30 … +6 dB.
  const under = e.buriedIn.map((u) => tf("bars {0}–{1} ({2} dB)", [String(u.from), String(u.to), signed(u.rel)])).join(", ");
  const relTip =
    under !== ""
      ? tf("{0}: {1} dB against the mix (its loudness where it plays), {2}% of the mix's energy; under the mix in {3}", [
          e.name,
          signed(e.rel),
          db(e.share, 0),
          under,
        ])
      : tf("{0}: {1} dB against the mix (its loudness where it plays), {2}% of the mix's energy", [e.name, signed(e.rel), db(e.share, 0)]);
  b.open("span", "rel", "mx-el-rel");
  b.attr("title", relTip);
  b.leaf("span", "fill", "mx-el-relfill", "");
  b.style("width", `${fmt(Number.isFinite(e.rel) ? Math.max(2, Math.min(100, ((e.rel + 30) / 36) * 100)) : 0, 1)}%`);
  b.leaf("span", "t", "mx-el-reltext", `${signed(e.rel)} dB`);
  b.close();
  const audTip = tf("Audible {0}% of the time it plays (partial loudness in the mix)", [db(e.audible, 0)]);
  b.open("span", "aud", "mx-el-aud");
  b.attr("title", audTip);
  b.leaf("span", "fill", e.audible < 25 ? "mx-el-audfill bad" : e.audible < 60 ? "mx-el-audfill warn" : "mx-el-audfill", "");
  b.style("width", `${fmt(Number.isFinite(e.audible) ? Math.max(2, e.audible) : 0, 0)}%`);
  b.leaf("span", "t", "mx-el-audtext", Number.isFinite(e.audible) ? `${fmt(e.audible, 0)}%` : "—");
  b.close();
  b.close();
  if (open) {
    b.open("div", "more", "mx-el-more");
    const facts = [
      tf("RMS {0} dBFS", [db(e.rms, 1)]),
      tf("peak {0} dBFS", [db(e.peak, 1)]),
      `${db(e.lufs, 1)} LUFS`,
      tf("fader {0} dB", [signed(e.fader)]),
      tf("plays {0}% of the range", [db(e.active, 0)]),
    ];
    if (Number.isFinite(e.corr)) facts.push(tf("correlation {0}", [fmt(e.corr, 2)]));
    if (Number.isFinite(e.domLo)) facts.push(tf("mostly {0}–{1} Hz", [fmt(e.domLo, 0), fmt(e.domHi, 0)]));
    b.leaf("div", "facts", "mx-facts", facts.join(" · "));
    for (const m of e.maskers) {
      // One sentence, the masker's name ({0}) a button in it.
      const said = tf("masked by {0} at {1}–{2} Hz ({3} dB over it)", ["{0}", fmt(m.lo, 0), fmt(m.hi, 0), signed(m.db)]);
      const cut = said.indexOf("{0}") >= 0 ? said.indexOf("{0}") : said.length;
      b.open("div", `m-${m.id}`, "mx-masker");
      b.leaf("span", "l", "", said.slice(0, cut));
      b.open("button", "who", "mx-link");
      b.on("click", (ev) => revealElement(m.id));
      b.text(nameOf(m.id));
      b.close();
      b.leaf("span", "r", "", said.slice(Math.min(said.length, cut + 3)));
      b.close();
    }
    for (let i = 0; i < e.suggestions.length; i++) {
      const s = e.suggestions[i];
      b.open("div", `s${i}`, "mx-sugg");
      b.leaf("div", "w", "mx-sugg-why", s.why);
      /** const exp: String[] */
      const exp = [];
      if (Number.isFinite(s.expAud)) exp.push(tf("audible {0}%", [fmt(s.expAud, 0)]));
      if (Number.isFinite(s.expRel)) exp.push(tf("{0} dB against the mix", [signed(s.expRel)]));
      const tried = view.tried.find((x) => x.patch === s.patch);
      b.open("div", "acts", "mx-acts");
      if (exp.length > 0) b.leaf("span", "e", "mx-expect", tf("expected: {0}", [exp.join(", ")]));
      if (tried) b.leaf("span", "t", "mx-tried", tf("measured: {0}", [tried.summary]));
      b.leaf("span", "sp", "spacer", "");
      button(b, "try", "small ghost", t("Try"), t("Render with this change without making it (a what-if)"), () => tryPatch(s.patch));
      button(b, "apply", "small gold", t("Apply"), t("Make this change (one undo step)"), () => applyPatch(s.patch, `${e.name}: ${s.why}`));
      b.close();
      b.close();
    }
    b.open("div", "show", "mx-acts");
    b.leaf("span", "sp", "spacer", "");
    button(b, "mixer", "small ghost", t("Show in the mixer"), t("Select its channel and insert"), () => revealElement(e.id));
    b.close();
    b.close();
  }
  b.close();
}

// ------------------------------------------------------------------ the panel

/** The Mix check panel (in the Maestro panel, over the terminal). */
/** function mixcheckPanel(b: Builder, ask: (String) => Undefined) => Undefined */
export function mixcheckPanel(b, ask) {
  if (!view.catalogAsked && state.loaded) loadCatalog();
  const r = view.report;
  const stale = r.ok && view.edits !== state.edits;
  b.open("div", "mixcheck", view.busy ? "mixcheck busy" : "mixcheck");

  // What to measure.
  b.open("div", "bar", "mx-bar");
  const sections = sectionNames();
  select(
    b,
    "scope",
    "mx-select",
    view.scope,
    sections.length > 0 ? ["song", "bars", "section"] : ["song", "bars"],
    [t("Whole song"), t("Bars"), t("Section")],
    t("What to measure"),
    (v) => {
      view.scope = v;
      invalidate();
    }
  );
  if (view.scope === "bars") {
    for (const x of [
      { k: "from", v: view.from, tip: t("First bar (as the playlist counts them)") },
      { k: "to", v: view.to, tip: t("Last bar (included)") },
    ]) {
      b.leaf("input", x.k, "mx-num", "");
      b.attr("type", "number");
      b.attr("min", "1");
      b.attr("title", x.tip);
      b.prop("value", `${x.v}`);
      b.on("change", (e) => {
        const v = Math.max(1, Math.round(Number(e.value)));
        if (x.k === "from") view.from = v;
        else view.to = v;
        invalidate();
      });
    }
  } else if (view.scope === "section") {
    const cur = sections.includes(view.section) ? view.section : sections.length > 0 ? sections[0] : "";
    select(b, "section", "mx-select", cur, sections, sections, t("The section (labelled score marks, drum part sections)"), (v) => {
      view.section = v;
      invalidate();
    });
  }
  const ids = [""].concat(view.targets.map((x) => x.id));
  const names = [t("No target")].concat(view.targets.map((x) => `${x.name} ${fmt(x.lufs, 0)} LUFS`));
  select(b, `target${view.targets.length}`, "mx-select", view.target, ids, names, t("Delivery target: its loudness and true-peak limit"), (v) => {
    view.target = v;
    localStorage.setItem("rosaclef.mixcheck.target", v);
    invalidate();
  });
  b.leaf("span", "sp", "spacer", "");
  b.open("button", "run", view.busy ? "btn small gold busy" : stale || !r.ok ? "btn small gold" : "btn small");
  b.attr("title", t("Render the range once and measure everything (the same as `rosaclef mixcheck`)"));
  b.on("click", (e) => runMixcheck());
  glyph(b, "meter");
  b.leaf("span", "l", "", view.busy ? t("Measuring…") : r.ok ? t("Measure again") : t("Measure"));
  b.close();
  b.close();

  b.open("div", "bar2", "mx-bar mx-bar2");
  select(
    b,
    "threshold",
    "mx-select small",
    view.threshold,
    ["strict", "normal", "loose"],
    [t("Strict"), t("Normal"), t("Loose")],
    t("How readily problems are reported"),
    (v) => {
      view.threshold = v;
      invalidate();
    }
  );
  const audio = state.samples.filter((s) => /\.(wav|flac|mp3|ogg|m4a|aac)$/i.test(s));
  select(
    b,
    "ref",
    "mx-select small",
    view.reference,
    [""].concat(audio),
    [t("No reference")].concat(audio.map((s) => s.split("/").pop() ?? s)),
    t("A reference track (in samples/): compared level-matched"),
    (v) => {
      view.reference = v;
      invalidate();
    }
  );
  b.leaf("span", "sp", "spacer", "");
  if (r.ok) {
    const where = r.fromBar === r.toBar ? tf("bar {0}", [String(r.fromBar)]) : tf("bars {0}–{1}", [String(r.fromBar), String(r.toBar)]);
    /** const facts: String[] */
    const facts = [where, `${fmt(r.seconds, 1)} s`];
    if (r.repeats) facts.push(t("every pass"));
    facts.push(r.cached ? t("cached") : `${fmt(r.ms / 1000, 1)} s`);
    if (stale) facts.push(t("the song changed"));
    b.leaf("span", "st", stale ? "mx-status stale" : "mx-status", facts.join(" · "));
  }
  b.close();

  b.open("div", "scroll", "mx-scroll");
  if (view.error !== "") b.leaf("div", "err", "mx-error", view.error);
  if (!r.ok) {
    b.open("div", "empty", "mx-empty");
    glyph(b, "meter");
    b.leaf("h3", "h", "", view.busy ? t("Listening…") : t("Measure the mix"));
    b.leaf(
      "p",
      "p",
      "",
      t(
        "One render of the song (or a range): loudness and true peak against a delivery target, the limiter's work, phase, the spectrum, how audible each part is under the others. The fixes touch the mixer only. The agent gets the same numbers from `rosaclef mixcheck`."
      )
    );
    b.close();
  } else {
    masterView(b, r);
    findingsView(b, r, ask);
    if (section(b, "history", t("Loudness history"), t("short-term · momentary · true peak · limiter"))) {
      b.canvas("history", "mx-canvas mx-history", (g, w, h) => paintHistory(g, w, h, r));
      b.on("pointerdown", (e) => {
        const hs = r.history;
        if (hs.bars.length === 0 || e.targetWidth <= 0) return undefined;
        const n = hs.t.length;
        const time = hs.t[0] + (e.offsetX / e.targetWidth) * (n <= 1 ? 0 : hs.t[n - 1] - hs.t[0]);
        let best = hs.bars[0];
        for (const m of hs.bars) if (m.t <= time) best = m;
        revealBar(best.beat);
      });
      b.on("pointerenter", (e) => hint(t("Click to show that bar in the playlist")));
    }
    if (section(b, "spectrum", t("Spectrum"), r.reference.file !== "" ? tf("blue: {0}, level-matched", [r.reference.file.split("/").pop() ?? ""]) : "")) {
      b.canvas("spectrum", "mx-canvas mx-spectrum", (g, w, h) => paintSpectrum(g, w, h, r));
      const tip = r.master.spectrum.map((v, i) => `${t(BANDS[i])} (${t(BAND_TIPS[i])}) ${fmt(v, 1)} dB`).join(" · ");
      b.attr("title", tip);
      if (r.reference.summary !== "") b.leaf("p", "ref", "mx-note", tf("Against the reference: {0}.", [r.reference.summary]));
    }
    if (section(b, "parts", t("Parts"), tf("{0} need attention", [String(r.elements.filter((e) => e.verdict !== "ok").length)]))) {
      b.open("div", "legend", "mx-legend");
      b.leaf("span", "a", "", t("level against the mix"));
      b.leaf("span", "b", "", t("audible"));
      b.close();
      const order = r.elements.slice().sort((x, y) => {
        const rank = (v) => (v === "inaudible" ? 0 : v === "buried" ? 1 : v === "overloading" ? 2 : v === "dominant" ? 3 : 4);
        return rank(x.verdict) - rank(y.verdict) || y.share - x.share;
      });
      for (const e of order) elementRow(b, e);
    }
    if (section(b, "rows", t("Bars"), t("momentary max · pre-limiter peak · limiter"))) barsView(b, r);
    for (const w of r.warnings) b.leaf("div", `w-${w}`, "mx-warning", w);
  }
  b.close();
  b.close();
}

/** The master: loudness, peaks, dynamics, the limiter, phase. */
/** function masterView(b: Builder, r: MixReport) => Undefined */
function masterView(b, r) {
  const m = r.master;
  const tg = r.target;
  b.open("div", "master", "mx-master");
  b.open("div", "lufs", "mx-lufs");
  const iState = Number.isFinite(tg.lufs) && Number.isFinite(m.integrated) ? light(false, Math.abs(m.integrated - tg.lufs) > 2) : "ok";
  b.open("div", "big", `mx-big ${iState}`);
  b.attr("title", t("Integrated loudness (ITU-R BS.1770 / EBU R128): the average a streaming service normalizes"));
  b.leaf("span", "v", "mx-big-value", db(m.integrated, 1));
  b.leaf("span", "u", "mx-big-unit", t("LUFS integrated"));
  b.close();
  b.open("div", "st", "mx-lufs-side");
  readout(b, "s", t("Short-term max"), db(m.shortMax, 1), "LUFS", "ok", t("The loudest 3-second window"));
  readout(b, "m", t("Momentary max"), db(m.momentaryMax, 1), "LUFS", "ok", t("The loudest 400 ms window"));
  b.close();
  if (tg.id !== "") {
    b.open("div", "target", `mx-target ${tg.status}`);
    b.attr("title", tg.notes.join("; "));
    b.leaf("span", "chip", `mx-chip ${tg.status}`, tg.status === "pass" ? t("PASS") : tg.status === "warn" ? t("CHECK") : t("FAIL"));
    b.leaf("span", "n", "", `${tg.name} · ${fmt(tg.lufs, 0)} LUFS · ${fmt(tg.truePeak, 0)} dBTP`);
    if (Number.isFinite(tg.gain)) b.leaf("span", "g", "mx-target-gain", tf("plays at {0} dB", [signed(tg.gain)]));
    b.close();
  }
  b.close();
  loudnessScale(b, m, tg);
  b.open("div", "tiles", "mx-tiles");
  const tpLimit = Number.isFinite(tg.truePeak) ? tg.truePeak : -1;
  readout(
    b,
    "tp",
    t("True peak"),
    db(m.truePeak, 1),
    "dBTP",
    light(m.truePeak > tpLimit, m.truePeak > tpLimit - 0.5),
    tf("Inter-sample peak (4× oversampled); keep it under {0} dBTP", [fmt(tpLimit, 1)])
  );
  readout(
    b,
    "pl",
    t("Pre-limiter"),
    signed(m.preLimiter),
    "dBFS",
    light(m.preLimiter > 3, m.preLimiter > 0),
    t("Peak at the master limiter's input: over 0 the limiter is working")
  );
  readout(b, "plr", "PLR", db(m.plr, 1), "dB", light(m.plr < 6, m.plr < 8), t("Peak-to-loudness ratio: under about 8 dB the mix is squashed"));
  readout(b, "lra", "LRA", db(m.lra, 1), "LU", light(false, m.lra < 2), t("Loudness range (EBU Tech 3342): how much the loudness moves"));
  readout(b, "crest", t("Crest"), db(m.crest, 1), "dB", "ok", t("Sample peak minus RMS"));
  readout(b, "mono", t("Mono"), signed(m.monoLoss), "dB", light(m.monoLoss < -6, m.monoLoss < -4), t("Loudness lost when the mix is folded to mono"));
  b.close();
  b.open("div", "dyn", "mx-dyn");
  b.open("div", "grs", "mx-grs");
  grMeter(b, "lim", t("Limiter"), m.limGr);
  if (Number.isFinite(m.compGr.max)) grMeter(b, "comp", t("Bus comp"), m.compGr);
  for (const g of r.gr.filter((x) => !x.id.startsWith("insert:0/") && x.max >= 1).slice(0, 4)) {
    grMeter(b, `${g.id}-${g.effect}`, nameOf(g.id), g);
  }
  b.close();
  correlationMeter(b, m);
  b.close();
  if (r.whatIf !== "") b.leaf("div", "wi", "mx-note", tf("What-if: {0}", [r.whatIf]));
  b.close();
}

/** The findings, ranked, each with its fix. */
/** function findingsView(b: Builder, r: MixReport, ask: (String) => Undefined) => Undefined */
function findingsView(b, r, ask) {
  if (!section(b, "findings", t("Findings"), r.findings.length === 0 ? t("none") : `${r.findings.length}`)) return undefined;
  if (r.findings.length === 0) {
    b.leaf("p", "none", "mx-note", t("Nothing to report at this threshold."));
    return undefined;
  }
  for (const f of r.findings) {
    b.open("div", f.key, `mx-find ${f.severity}`);
    b.leaf("span", "dot", "mx-dot", "");
    b.open("div", "body", "mx-find-body");
    b.open("div", "t", "mx-find-title");
    b.leaf("b", "r", "", f.rule.replace(/-/g, " "));
    b.open("button", "w", "mx-link");
    b.attr("title", t("Show it"));
    b.on("click", (e) => {
      if (f.element !== "") revealElement(f.element);
      else if (Number.isFinite(f.fromBeat)) revealBar(f.fromBeat);
    });
    b.text(f.where);
    b.close();
    b.close();
    b.leaf("div", "d", "mx-find-detail", f.detail);
    b.open("div", "acts", "mx-acts");
    b.leaf("span", "sp", "spacer", "");
    if (state.backend !== "local")
      button(b, "ask", "small ghost", t("Ask Maestro"), t("Type this into the agent's prompt"), () =>
        ask(`Mix check (${f.where}): ${f.detail} Please look into it (rosaclef mixcheck reports it as ${f.key}).`)
      );
    if (f.patch !== "") {
      const tried = view.tried.find((x) => x.patch === f.patch);
      button(b, "try", "small ghost", t("Try"), t("Render with the fix without making it (a what-if)"), () => tryPatch(f.patch));
      button(b, "fix", "small gold", t("Apply fix"), tf("Apply: {0} (one undo step)", [f.label]), () => applyPatch(f.patch, f.label));
      b.close();
      if (f.label !== "") b.leaf("div", "fl", "mx-fixlabel", tf("Fix: {0}", [f.label]));
      if (tried) b.leaf("div", "tried", "mx-tried", tf("Measured with it: {0}", [tried.summary]));
    } else b.close();
    b.close();
    b.close();
  }
}

/** Every row: a compact table, the loud and the hot rows marked. */
/** function barsView(b: Builder, r: MixReport) => Undefined */
function barsView(b, r) {
  b.open("div", "table", "mx-rows");
  b.open("div", "head", "mx-rowline head");
  const heads = ["", t("M max"), t("S max"), t("pre-lim"), t("GR"), t("top")];
  for (let i = 0; i < heads.length; i++) b.leaf("span", `h${i}`, "", heads[i]);
  b.close();
  const loudest = r.rows.reduce((a, x) => (Number.isFinite(x.mMax) ? Math.max(a, x.mMax) : a), -99);
  for (let i = 0; i < r.rows.length; i++) {
    const x = r.rows[i];
    b.open("div", `r${i}`, `mx-rowline${x.preLimiter > 0 ? " hot" : ""}${x.mMax >= loudest - 0.05 ? " loudest" : ""}`);
    b.on("click", (e) => revealBar(x.fromBeat));
    b.leaf("span", "k", "mx-rowkey", x.pass > 1 ? `${x.label}′${x.pass}` : x.label);
    b.leaf("span", "m", "", db(x.mMax, 1));
    b.leaf("span", "s", "", db(x.sMax, 1));
    b.leaf("span", "p", x.preLimiter > 0 ? "hot" : "", signed(x.preLimiter));
    b.leaf("span", "g", "", Number.isFinite(x.limGr) && x.limGr > 0.05 ? `−${fmt(x.limGr, 1)}` : "");
    b.leaf("span", "c", "mx-rowtop", x.top.length > 0 ? `${nameOf(x.top[0].id)} ${fmt(x.top[0].pct, 0)}%` : "");
    b.close();
  }
  b.close();
}
