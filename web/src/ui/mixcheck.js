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

import { getJson, sendJson, fmt, audioPost, now } from "#platform";
import { state, commit, invalidate, fixSelection, selectChannel, selectInsert, hint, hooks, engineJson, currentPattern } from "../store.js";
import { encodeProject, decodeProject, meterMap } from "../model.js";
import { startAudio, seek, livePosition } from "../audio.js";
import { button, select, glyph } from "./widgets.js";
import { openDock, setTop, setView, isCompact, layoutState } from "./panes.js";
import { revealBeat } from "./playlist.js";
import { revealScoreBeat } from "./score.js";
import { newJob, watchJob, jobLabel, jobFraction } from "./progress.js";
import { toast } from "./toast.js";
import { insertIx } from "#brands";

const BANDS = ["Sub", "Bass", "Low mid", "Mid", "High mid", "Air"];
const BAND_TIPS = ["under 60 Hz", "60–250 Hz", "250–500 Hz", "500 Hz–2 kHz", "2–6 kHz", "over 6 kHz"];

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
  /** The job id of the measuring under way (its progress). */
  job: 0,
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
  /** Fixes applied since the report (by finding key, with the settings they
   * changed), and the project version after the last: the report's other
   * fixes still apply while nothing else changed and they touch other
   * settings. */
  applied /*: { key: String, paths: String[] }[] */: [],
  appliedEdits: -1,
  /** Before / after: which fix plays (its key), the song with it for the
   * engine, which half plays, over which beats; the patched songs, by patch. */
  audition: { key: "", json: "", after: false, on: false, start: 0, end: 0, at: 0, seq: 0 },
  patched /*: { patch: String, json: String }[] */: [],
  /** Fixes left out of Quick fix (by finding key): all are in by default. */
  excluded /*: String[] */: [],
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
      toBar: int(f.toBar),
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
  view.job = newJob();
  invalidate();
  watchJob(view.job, () => view.busy);
  const q = request();
  sendJson(`/api/mixcheck?job=${view.job}`, "POST", {
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
      view.applied = [];
      view.appliedEdits = -1;
      view.excluded = [];
      view.patched = [];
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
  if (view.edits === state.edits || view.appliedEdits === state.edits) return true;
  toast("The song changed since this report", "Measure again first: its fixes point at notes and devices by position.", "info");
  return false;
}

/** The settings a patch writes (its JSON Pointers). */
/** function pathsOf(patch: String) => String[] */
function pathsOf(patch) {
  /** const out: String[] */
  const out = [];
  for (const m of patch.matchAll(/"path":\s*"([^"]*)"/g)) out.push(m[1]);
  return out;
}

/** An applied fix that already changed one of the settings `patch` writes. */
/** function clash(patch: String) => String */
function clash(patch) {
  const ps = pathsOf(patch);
  for (const a of view.applied) {
    if (a.paths.some((x) => ps.includes(x))) return a.key;
  }
  return "";
}

/** Whether a finding's fix was applied since the report. */
/** function isApplied(key: String) => Boolean */
function isApplied(key) {
  return view.applied.some((a) => a.key === key);
}

/** Every fix not applied yet, as one patch: in the findings' order, the
 * first to set a setting keeps it. */
/** function allFixes(r: MixReport) => { patch: String, n: Int } */
function allFixes(r) {
  /** const seen: String[] */
  const seen = [];
  /** const ops: String[] */
  const ops = [];
  let n = 0;
  for (const f of r.findings) {
    if (f.patch === "" || isApplied(f.key) || view.excluded.includes(f.key) || clash(f.patch) !== "") continue;
    const list = JSON.parse(f.patch);
    let used = false;
    for (const op of list) {
      const p = String(op.path);
      if (seen.includes(p)) continue;
      seen.push(p);
      ops.push(JSON.stringify(op));
      used = true;
    }
    if (used) n += 1;
  }
  return { patch: `[${ops.join(",")}]`, n: n };
}

/** The song beat where bar `bar` (from 1) starts. */
/** function barBeat(bar: Int) => Number */
function barBeat(bar) {
  const map = meterMap(state.project.transport);
  let s = map[0];
  for (const m of map) if (m.bar <= bar - 1) s = m;
  return s.beat + (bar - 1 - s.bar) * s.barBeats;
}

/** A finding about some bars of the report's range (not all of it). */
/** function local(r: MixReport, f: MixFinding) => Boolean */
function local(r, f) {
  return f.fromBar > 0 && f.toBar >= f.fromBar && (f.fromBar > r.fromBar || f.toBar < r.toBar);
}

/** Measure a change without making it (a what-if: the project is not touched). */
/** A local finding is tried over its bars and one either side (a shorter
 * render); `range` "" tries it over the report's range. */
/** function tryPatch(patch: String, range: String) => Undefined */
function tryPatch(patch, range) {
  if (view.busy || !fresh()) return undefined;
  view.busy = true;
  view.job = newJob();
  invalidate();
  watchJob(view.job, () => view.busy);
  const q = request();
  sendJson(`/api/mixcheck?job=${view.job}`, "POST", {
    project: encodeProject(state.project),
    range: range !== "" ? range : q.range,
    section: range !== "" ? "" : q.section,
    threshold: q.threshold,
    whatIf: patch,
  })
    .then((r) => {
      view.busy = false;
      const summary = r.whatIf === undefined || r.whatIf === null ? "" : String(r.whatIf.summary);
      view.tried = view.tried.filter((x) => x.patch !== patch).concat([{ patch: patch, summary: summary }]);
      invalidate();
      return true;
    })
    .catch((e) => {
      view.busy = false;
      toast("Could not try it", errText(e), "error");
      invalidate();
      return false;
    });
}

/** Apply a fix: the endpoint patches the project as it is and validates it;
 * one undo step. If the song changed meanwhile, nothing is applied. */
/** function applyPatch(patch: String, what: String, keys: String[]) => Undefined */
function applyPatch(patch, what, keys) {
  if (!fresh()) return undefined;
  const by = clash(patch);
  if (by !== "") {
    toast("A fix you applied changed that already", `${by.split("|")[0].replace(/-/g, " ")} set the same setting: measure again to see what is left.`, "info");
    return undefined;
  }
  stopAudition();
  const edits = state.edits;
  sendJson("/api/mixcheck", "POST", { project: encodeProject(state.project), apply: patch })
    .then((r) => {
      if (state.edits !== edits) {
        toast("The song changed meanwhile", "Nothing was applied; try again.", "info");
        return false;
      }
      const fixed = decodeProject(r.project);
      const fresh_ = view.edits === state.edits || view.appliedEdits === state.edits;
      commit(() => {
        state.project = fixed;
      });
      fixSelection();
      // The report's other fixes still apply (they write other settings).
      if (fresh_) {
        const paths = pathsOf(patch);
        for (const k of keys) view.applied.push({ key: k, paths: paths });
        view.appliedEdits = state.edits;
      }
      toast(what, "Ctrl+Z undoes it. The other fixes still apply; measure again to check.", "info");
      return true;
    })
    .catch((e) => {
      toast("Could not apply it", errText(e), "error");
      return false;
    });
}

// ------------------------------------------------------------------ before/after

/** The song with `patch` applied, for the engine (asked once per patch). */
/** function patchedJson(patch: String) => Promise<String> */
function patchedJson(patch) {
  const hit = view.patched.find((x) => x.patch === patch);
  if (hit) return Promise.resolve(hit.json);
  return sendJson("/api/mixcheck", "POST", { project: encodeProject(state.project), apply: patch }).then((r) => {
    const json = JSON.stringify(r.project);
    view.patched.push({ patch: patch, json: json });
    return json;
  });
}

/** How many bars "Before / after" plays of each. */
const LISTEN_BARS = 4;

/** Before / after: from the playhead (put it anywhere, as usual, or with
 * Show), LISTEN_BARS bars as the song is, a click, the same bars with the
 * fix, then stop. The project is not touched. */
/** function listen(key: String, patch: String) => Promise<Boolean> */
async function listen(key, patch) {
  const was = view.audition.on && view.audition.key === key;
  stopAudition();
  if (was) return false;
  if (state.output !== "browser") {
    toast("Before / after plays in the browser engine", "Switch the output to the browser to compare.", "info");
    return false;
  }
  await startAudio();
  view.audition.seq = view.audition.seq + 1;
  const seq = view.audition.seq;
  let json = "";
  try {
    json = await patchedJson(patch);
  } catch (e) {
    toast("Could not prepare it", errText(e), "error");
    return false;
  }
  if (seq !== view.audition.seq) return false;
  const start = Math.max(0, state.playing ? livePosition() : state.position);
  const a = view.audition;
  a.key = key;
  a.json = json;
  a.on = true;
  a.after = false;
  a.start = start;
  a.end = start + LISTEN_BARS * state.project.transport.beatsPerBar;
  a.at = now();
  // An edit meanwhile ends it (and gives the engine the song back).
  hooks.preview = stopAudition;
  hooks.previewing = true;
  audioPost({ t: "metronome", on: false });
  audioPost({ t: "project", json: engineJson() });
  audioPost({ t: "mode", pattern: "" });
  seek(start);
  audioPost({ t: "play" });
  invalidate();
  listenTick();
  return true;
}

/** Before, then a click and after, then stop. */
/** function listenTick() => Undefined */
function listenTick() {
  const a = view.audition;
  if (!a.on) return undefined;
  const late = now() - a.at > 1500;
  if (late && !state.playing) {
    stopAudition();
    return undefined;
  }
  if (late && livePosition() >= a.end) {
    if (a.after) {
      stopAudition();
      return undefined;
    }
    // The same bars with the fix, after one click of count-in.
    audioPost({ t: "stop" });
    audioPost({ t: "project", json: a.json });
    seek(a.start);
    audioPost({ t: "play", countIn: 1 });
    a.after = true;
    a.at = now();
    invalidate();
  }
  setTimeout(() => listenTick(), 50);
}

/** Stop before / after and give the engine the song back (the playhead
 * back where it was). */
/** function stopAudition() => Undefined */
function stopAudition() {
  const a = view.audition;
  if (!a.on) return undefined;
  a.on = false;
  a.seq = a.seq + 1;
  hooks.previewing = false;
  // A pause keeps the place (a stop goes back to the start).
  audioPost({ t: "pause" });
  audioPost({ t: "project", json: engineJson() });
  audioPost({ t: "metronome", on: state.metronome });
  const pat = currentPattern();
  audioPost({ t: "mode", pattern: state.mode === "pattern" && pat ? pat.id : "" });
  seek(a.start);
  // The engine's last report of where it played may still be on its way.
  setTimeout(() => {
    if (!a.on && !state.playing) seek(a.start);
  }, 250);
  invalidate();
}

/** The Before / after button of a fix. */
/** function listenButton(b: Builder, key: String, patch: String) => Undefined */
function listenButton(b, key, patch) {
  const a = view.audition;
  const mine = a.on && a.key === key;
  button(
    b,
    "listen",
    mine ? "small on" : "small ghost",
    mine ? (a.after ? "After… (stop)" : "Before… (stop)") : "Before / after",
    `From the playhead: ${LISTEN_BARS} bars as it is, a click, then the same bars with the fix`,
    () => listen(key, patch)
  );
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
  const b = Math.max(0, beat);
  if (layoutState.top === "score") revealScoreBeat(b);
  else {
    setTop("playlist");
    if (isCompact(window.innerWidth, window.innerHeight)) setView("playlist");
    revealBeat(b);
  }
  // The playhead there too: Play starts where the problem is.
  if (!state.playing) seek(b);
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
/** function loudnessScale(b: Builder, m: MixMaster, t: MixTarget) => Undefined */
function loudnessScale(b, m, t) {
  const lo = -36;
  const hi = 0;
  /** function at(x: Number) => String */
  const at = (x) => `${fmt(Math.max(0, Math.min(100, ((x - lo) / (hi - lo)) * 100)), 2)}%`;
  b.open("div", "scale", "mx-scale");
  b.attr("title", "Loudness scale (LUFS): the bracket is the target's ±1 LU; ▼ integrated, the bar the short-term range");
  b.leaf("div", "track", "mx-scale-track", "");
  if (Number.isFinite(t.lufs)) {
    b.leaf("div", "zone", "mx-scale-zone", "");
    b.style("left", at(t.lufs - 1));
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
  const tip = `${label}: gain reduction up to ${db(g.max, 1)} dB, ${db(g.mean, 1)} dB on average, over 3 dB ${db(g.above3, 0)}% of the time`;
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
  const tip = `Phase correlation: ${db(m.corrMean, 2)} on average, ${db(m.corrMin, 2)} at its lowest (+1 mono, 0 wide, below 0 the sides cancel in mono); mono fold-down ${signed(m.monoLoss)} dB`;
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
    g.fillText(BANDS[i], i * bw + bw / 2, h - 3);
    g.fillStyle = "rgba(246, 238, 221, 0.85)";
    g.fillText(Number.isFinite(sp[i]) ? fmt(sp[i], 0) : "", i * bw + bw / 2, Math.max(12, y(sp[i]) - 3));
  }
  g.textAlign = "left";
}

// ------------------------------------------------------------------ parts

/** function verdictText(v: String) => String */
function verdictText(v) {
  return v === "inaudible" ? "inaudible" : v === "buried" ? "buried" : v === "dominant" ? "dominant" : v === "overloading" ? "overloading" : "ok";
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
    b.attr("title", "The song's lead: also buried when it sits far under the mix");
    b.text("lead");
    b.close();
  } else if (e.role !== "") b.leaf("span", "role", "mx-el-role", e.role);
  b.leaf("span", "sp", "spacer", "");
  // Level against the mix: −30 … +6 dB.
  const under = e.buriedIn.map((u) => `bars ${u.from}–${u.to} (${signed(u.rel)} dB)`).join(", ");
  const relTip = `${e.name}: ${signed(e.rel)} dB against the mix (its loudness where it plays), ${db(e.share, 0)}% of the mix's energy${under !== "" ? `; under the mix in ${under}` : ""}`;
  b.open("span", "rel", "mx-el-rel");
  b.attr("title", relTip);
  b.leaf("span", "fill", "mx-el-relfill", "");
  b.style("width", `${fmt(Number.isFinite(e.rel) ? Math.max(2, Math.min(100, ((e.rel + 30) / 36) * 100)) : 0, 1)}%`);
  b.leaf("span", "t", "mx-el-reltext", `${signed(e.rel)} dB`);
  b.close();
  const audTip = `Audible ${db(e.audible, 0)}% of the time it plays (partial loudness in the mix)`;
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
      `RMS ${db(e.rms, 1)} dBFS`,
      `peak ${db(e.peak, 1)} dBFS`,
      `${db(e.lufs, 1)} LUFS`,
      `fader ${signed(e.fader)} dB`,
      `plays ${db(e.active, 0)}% of the range`,
    ];
    if (Number.isFinite(e.corr)) facts.push(`correlation ${fmt(e.corr, 2)}`);
    if (Number.isFinite(e.domLo)) facts.push(`mostly ${fmt(e.domLo, 0)}–${fmt(e.domHi, 0)} Hz`);
    b.leaf("div", "facts", "mx-facts", facts.join(" · "));
    for (const m of e.maskers) {
      b.open("div", `m-${m.id}`, "mx-masker");
      b.leaf("span", "l", "", "masked by ");
      b.open("button", "who", "mx-link");
      b.on("click", (ev) => revealElement(m.id));
      b.text(nameOf(m.id));
      b.close();
      b.leaf("span", "r", "", ` at ${fmt(m.lo, 0)}–${fmt(m.hi, 0)} Hz (${signed(m.db)} dB over it)`);
      b.close();
    }
    for (let i = 0; i < e.suggestions.length; i++) {
      const s = e.suggestions[i];
      b.open("div", `s${i}`, "mx-sugg");
      b.leaf("div", "w", "mx-sugg-why", s.why);
      /** const exp: String[] */
      const exp = [];
      if (Number.isFinite(s.expAud)) exp.push(`audible ${fmt(s.expAud, 0)}%`);
      if (Number.isFinite(s.expRel)) exp.push(`${signed(s.expRel)} dB against the mix`);
      const tried = view.tried.find((x) => x.patch === s.patch);
      b.open("div", "acts", "mx-acts");
      if (exp.length > 0) b.leaf("span", "e", "mx-expect", `expected: ${exp.join(", ")}`);
      if (tried) b.leaf("span", "t", "mx-tried", `measured: ${tried.summary}`);
      b.leaf("span", "sp", "spacer", "");
      button(b, "try", "small ghost", "Try", "Render with this change without making it (a what-if)", () => tryPatch(s.patch, ""));
      button(b, "apply", "small gold", "Apply", "Make this change (one undo step)", () => applyPatch(s.patch, `${e.name}: ${s.why}`, []));
      b.close();
      b.close();
    }
    b.open("div", "show", "mx-acts");
    b.leaf("span", "sp", "spacer", "");
    button(b, "mixer", "small ghost", "Show in the mixer", "Select its channel and insert", () => revealElement(e.id));
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
    ["Whole song", "Bars", "Section"],
    "What to measure",
    (v) => {
      view.scope = v;
      invalidate();
    }
  );
  if (view.scope === "bars") {
    for (const x of [
      { k: "from", v: view.from, tip: "First bar (as the playlist counts them)" },
      { k: "to", v: view.to, tip: "Last bar (included)" },
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
    select(b, "section", "mx-select", cur, sections, sections, "The section (labelled score marks, drum part sections)", (v) => {
      view.section = v;
      invalidate();
    });
  }
  const ids = [""].concat(view.targets.map((t) => t.id));
  const names = ["No target"].concat(view.targets.map((t) => `${t.name} ${fmt(t.lufs, 0)} LUFS`));
  select(b, `target${view.targets.length}`, "mx-select", view.target, ids, names, "Delivery target: its loudness and true-peak limit", (v) => {
    view.target = v;
    localStorage.setItem("rosaclef.mixcheck.target", v);
    invalidate();
  });
  b.leaf("span", "sp", "spacer", "");
  b.open("button", "run", view.busy ? "btn small gold busy" : stale || !r.ok ? "btn small gold" : "btn small");
  b.attr("title", "Render the range once and measure everything (the same as `rosaclef mixcheck`)");
  b.on("click", (e) => runMixcheck());
  glyph(b, "meter");
  b.leaf("span", "l", "", view.busy ? "Measuring…" : r.ok ? "Measure again" : "Measure");
  b.close();
  b.close();

  b.open("div", "bar2", "mx-bar mx-bar2");
  select(
    b,
    "threshold",
    "mx-select small",
    view.threshold,
    ["strict", "normal", "loose"],
    ["Strict", "Normal", "Loose"],
    "How readily problems are reported",
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
    ["No reference"].concat(audio.map((s) => s.split("/").pop() ?? s)),
    "A reference track (in samples/): compared level-matched",
    (v) => {
      view.reference = v;
      invalidate();
    }
  );
  b.leaf("span", "sp", "spacer", "");
  if (r.ok) {
    const where = r.fromBar === r.toBar ? `bar ${r.fromBar}` : `bars ${r.fromBar}–${r.toBar}`;
    b.leaf(
      "span",
      "st",
      stale ? "mx-status stale" : "mx-status",
      `${where} · ${fmt(r.seconds, 1)} s${r.repeats ? " · every pass" : ""} · ${r.cached ? "cached" : `${fmt(r.ms / 1000, 1)} s`}${
        stale ? (view.appliedEdits === state.edits ? " · fixes applied: measure again to check" : " · the song changed") : ""
      }`
    );
  }
  b.close();

  if (view.busy) {
    const f = jobFraction(view.job);
    b.open("div", "prog", "mx-progress");
    b.attr("role", "progressbar");
    b.attr("aria-valuemin", "0");
    b.attr("aria-valuemax", "100");
    b.attr("aria-valuenow", String(Math.round(f * 100)));
    b.leaf("div", "fill", "mx-progress-fill", "");
    b.style("width", `${Math.round(f * 1000) / 10}%`);
    b.close();
    b.leaf("div", "progl", "mx-progress-label", jobLabel(view.job));
  }

  b.open("div", "scroll", "mx-scroll");
  if (view.error !== "") b.leaf("div", "err", "mx-error", view.error);
  if (!r.ok) {
    if (!view.busy) {
      // Where to start: an arrow up to the Measure button.
      b.open("div", "hint", "mx-hint");
      b.leaf("span", "t", "mx-hint-text", "Start here");
      b.open("svg", "a", "mx-hint-arrow");
      b.attr("viewBox", "0 0 80 60");
      b.attr("aria-hidden", "true");
      b.leaf("path", "p", "", "");
      b.attr("d", "M6 54 C 30 54, 58 44, 66 10");
      b.leaf("path", "h", "", "");
      b.attr("d", "M56 18 L66 6 L74 20");
      b.close();
      b.close();
    }
    b.open("div", "empty", "mx-empty");
    glyph(b, "meter");
    b.leaf("h3", "h", "", view.busy ? "Listening…" : "Measure the mix");
    b.leaf(
      "p",
      "p",
      "",
      "One render of the song (or a range): loudness and true peak against a delivery target, the limiter's work, phase, the spectrum, how audible each part is under the others. The fixes touch the mixer only. The agent gets the same numbers from `rosaclef mixcheck`."
    );
    b.close();
  } else {
    masterView(b, r);
    findingsView(b, r, ask);
    if (section(b, "history", "Loudness history", "short-term · momentary · true peak · limiter")) {
      b.canvas("history", "mx-canvas mx-history", (g, w, h) => paintHistory(g, w, h, r));
      b.on("pointerdown", (e) => {
        const hs = r.history;
        if (hs.bars.length === 0 || e.targetWidth <= 0) return undefined;
        const n = hs.t.length;
        const t = hs.t[0] + (e.offsetX / e.targetWidth) * (n <= 1 ? 0 : hs.t[n - 1] - hs.t[0]);
        let best = hs.bars[0];
        for (const m of hs.bars) if (m.t <= t) best = m;
        revealBar(best.beat);
      });
      b.on("pointerenter", (e) => hint("Click to show that bar in the playlist"));
    }
    if (section(b, "spectrum", "Spectrum", r.reference.file !== "" ? `blue: ${r.reference.file.split("/").pop() ?? ""}, level-matched` : "")) {
      b.canvas("spectrum", "mx-canvas mx-spectrum", (g, w, h) => paintSpectrum(g, w, h, r));
      const tip = r.master.spectrum.map((v, i) => `${BANDS[i]} (${BAND_TIPS[i]}) ${fmt(v, 1)} dB`).join(" · ");
      b.attr("title", tip);
      if (r.reference.summary !== "") b.leaf("p", "ref", "mx-note", `Against the reference: ${r.reference.summary}.`);
    }
    if (section(b, "parts", "Parts", `${r.elements.filter((e) => e.verdict !== "ok").length} need attention`)) {
      b.open("div", "legend", "mx-legend");
      b.leaf("span", "a", "", "level against the mix");
      b.leaf("span", "b", "", "audible");
      b.close();
      const order = r.elements.slice().sort((x, y) => {
        const rank = (v) => (v === "inaudible" ? 0 : v === "buried" ? 1 : v === "overloading" ? 2 : v === "dominant" ? 3 : 4);
        return rank(x.verdict) - rank(y.verdict) || y.share - x.share;
      });
      for (const e of order) elementRow(b, e);
    }
    if (section(b, "rows", "Bars", "momentary max · pre-limiter peak · limiter")) barsView(b, r);
    for (const w of r.warnings) b.leaf("div", `w-${w}`, "mx-warning", w);
  }
  b.close();
  b.close();
}

/** The master: loudness, peaks, dynamics, the limiter, phase. */
/** function masterView(b: Builder, r: MixReport) => Undefined */
function masterView(b, r) {
  const m = r.master;
  const t = r.target;
  b.open("div", "master", "mx-master");
  b.open("div", "lufs", "mx-lufs");
  const iState = Number.isFinite(t.lufs) && Number.isFinite(m.integrated) ? light(false, Math.abs(m.integrated - t.lufs) > 2) : "ok";
  b.open("div", "big", `mx-big ${iState}`);
  b.attr("title", "Integrated loudness (ITU-R BS.1770 / EBU R128): the average a streaming service normalizes");
  b.leaf("span", "v", "mx-big-value", db(m.integrated, 1));
  b.leaf("span", "u", "mx-big-unit", "LUFS integrated");
  b.close();
  b.open("div", "st", "mx-lufs-side");
  readout(b, "s", "Short-term max", db(m.shortMax, 1), "LUFS", "ok", "The loudest 3-second window");
  readout(b, "m", "Momentary max", db(m.momentaryMax, 1), "LUFS", "ok", "The loudest 400 ms window");
  b.close();
  if (t.id !== "") {
    b.open("div", "target", `mx-target ${t.status}`);
    b.attr("title", t.notes.join("; "));
    b.leaf("span", "chip", `mx-chip ${t.status}`, t.status === "pass" ? "PASS" : t.status === "warn" ? "CHECK" : "FAIL");
    b.leaf("span", "n", "", `${t.name} · ${fmt(t.lufs, 0)} LUFS · ${fmt(t.truePeak, 0)} dBTP`);
    if (Number.isFinite(t.gain)) b.leaf("span", "g", "mx-target-gain", `plays at ${signed(t.gain)} dB`);
    b.close();
  }
  b.close();
  loudnessScale(b, m, t);
  b.open("div", "tiles", "mx-tiles");
  const tpLimit = Number.isFinite(t.truePeak) ? t.truePeak : -1;
  readout(
    b,
    "tp",
    "True peak",
    db(m.truePeak, 1),
    "dBTP",
    light(m.truePeak > tpLimit, m.truePeak > tpLimit - 0.5),
    `Inter-sample peak (4× oversampled); keep it under ${fmt(tpLimit, 1)} dBTP`
  );
  readout(
    b,
    "pl",
    "Pre-limiter",
    signed(m.preLimiter),
    "dBFS",
    light(m.preLimiter > 3, m.preLimiter > 0),
    "Peak at the master limiter's input: over 0 the limiter is working"
  );
  readout(b, "plr", "PLR", db(m.plr, 1), "dB", light(m.plr < 6, m.plr < 8), "Peak-to-loudness ratio: under about 8 dB the mix is squashed");
  readout(b, "lra", "LRA", db(m.lra, 1), "LU", light(false, m.lra < 2), "Loudness range (EBU Tech 3342): how much the loudness moves");
  readout(b, "crest", "Crest", db(m.crest, 1), "dB", "ok", "Sample peak minus RMS");
  readout(b, "mono", "Mono", signed(m.monoLoss), "dB", light(m.monoLoss < -6, m.monoLoss < -4), "Loudness lost when the mix is folded to mono");
  b.close();
  b.open("div", "dyn", "mx-dyn");
  b.open("div", "grs", "mx-grs");
  grMeter(b, "lim", "Limiter", m.limGr);
  if (Number.isFinite(m.compGr.max)) grMeter(b, "comp", "Bus comp", m.compGr);
  for (const g of r.gr.filter((x) => !x.id.startsWith("insert:0/") && x.max >= 1).slice(0, 4)) {
    grMeter(b, `${g.id}-${g.effect}`, nameOf(g.id), g);
  }
  b.close();
  correlationMeter(b, m);
  b.close();
  if (r.whatIf !== "") b.leaf("div", "wi", "mx-note", `What-if: ${r.whatIf}`);
  b.close();
}

/** The findings, ranked, each with its fix. */
/** function findingsView(b: Builder, r: MixReport, ask: (String) => Undefined) => Undefined */
function findingsView(b, r, ask) {
  if (!section(b, "findings", "Findings", r.findings.length === 0 ? "none" : `${r.findings.length}`)) return undefined;
  if (r.findings.length === 0) {
    b.leaf("p", "none", "mx-note", "Nothing to report at this threshold.");
    return undefined;
  }
  quickFix(b, r);
  for (const f of r.findings) {
    const done = isApplied(f.key);
    const near = local(r, f);
    // A local finding is heard and tried over its bars, one either side.
    const lo = Math.max(1, f.fromBar - 1);
    const hi = f.toBar + 1;
    const where = near ? `bars ${lo}–${hi}` : "the range";
    b.open("div", f.key, `mx-find ${f.severity}${done ? " applied" : ""}`);
    b.leaf("span", "dot", "mx-dot", "");
    b.open("div", "body", "mx-find-body");
    b.open("div", "t", "mx-find-title");
    if (f.patch !== "" && !done) {
      b.leaf("input", "in", "mx-include", "");
      b.attr("type", "checkbox");
      b.attr("title", "Include this fix in Quick fix");
      b.prop("checked", view.excluded.includes(f.key) ? "" : "true");
      b.on("change", (e) => {
        view.excluded = view.excluded.includes(f.key) ? view.excluded.filter((k) => k !== f.key) : view.excluded.concat([f.key]);
        invalidate();
      });
    }
    b.leaf("b", "r", "", f.rule.replace(/-/g, " "));
    b.open("button", "w", "mx-link");
    b.attr("title", "Show it");
    b.on("click", (e) => {
      if (f.element !== "") revealElement(f.element);
      else if (Number.isFinite(f.fromBeat)) revealBar(f.fromBeat);
    });
    b.text(f.where);
    b.close();
    b.close();
    b.leaf("div", "d", "mx-find-detail", f.detail);
    b.open("div", "acts", "mx-acts");
    if (near) {
      const span = f.fromBar === f.toBar ? `bar ${f.fromBar}` : `bars ${f.fromBar}–${f.toBar}`;
      button(b, "show", "small ghost", "Show", `Show ${span} in the ${layoutState.top === "score" ? "score" : "playlist"} (and put the playhead there)`, () =>
        revealBar(barBeat(f.fromBar))
      );
    }
    if (f.patch !== "" && !done) listenButton(b, f.key, f.patch);
    b.leaf("span", "sp", "spacer", "");
    if (state.backend !== "local")
      button(b, "ask", "small ghost", "Ask Maestro", "Type this into the agent's prompt", () =>
        ask(`Mix check (${f.where}): ${f.detail} Please look into it (rosaclef mixcheck reports it as ${f.key}).`)
      );
    if (f.patch !== "" && done) {
      b.leaf("span", "ok", "mx-applied", "Applied ✓");
      b.close();
      if (f.label !== "") b.leaf("div", "fl", "mx-fixlabel", `Fix: ${f.label}`);
    } else if (f.patch !== "") {
      const tried = view.tried.find((x) => x.patch === f.patch);
      button(b, "try", "small ghost", "Try", `Measure ${where} with the fix without making it (a what-if)`, () => tryPatch(f.patch, near ? `${lo}:${hi}` : ""));
      button(b, "fix", "small gold", "Apply fix", `Apply: ${f.label} (one undo step)`, () => applyPatch(f.patch, f.label, [f.key]));
      b.close();
      if (f.label !== "") b.leaf("div", "fl", "mx-fixlabel", `Fix: ${f.label}`);
      if (tried) b.leaf("div", "tried", "mx-tried", `Measured with it: ${tried.summary}`);
    } else b.close();
    b.close();
    b.close();
  }
}

/** Every fix at once: try them, hear them, apply them (one undo step). */
/** function quickFix(b: Builder, r: MixReport) => Undefined */
function quickFix(b, r) {
  const all = allFixes(r);
  if (!r.findings.some((f) => f.patch !== "" && !isApplied(f.key))) return undefined;
  b.open("div", "quick", "mx-quick");
  b.open("div", "t", "mx-quick-title");
  b.leaf("b", "h", "", "Quick fix");
  b.leaf(
    "span",
    "n",
    "mx-quick-sub",
    all.n === 0
      ? "tick the fixes to include"
      : `${all.n} ticked fix${all.n === 1 ? "" : "es"} together${view.applied.length > 0 ? ` (${view.applied.length} applied already)` : ""}`
  );
  b.close();
  b.open("div", "acts", "mx-acts");
  listenButton(b, "*all", all.patch);
  b.leaf("span", "sp", "spacer", "");
  button(b, "try", "small ghost", "Try all", "Measure the range with every fix (a what-if: nothing changes)", () => tryPatch(all.patch, ""));
  button(b, "fix", "small gold", "Apply all", "Apply every fix (one undo step); where two set the same thing, the higher-ranked one wins", () =>
    applyPatch(
      all.patch,
      `${all.n} fixes applied`,
      r.findings.filter((f) => f.patch !== "" && !isApplied(f.key) && !view.excluded.includes(f.key) && clash(f.patch) === "").map((f) => f.key)
    )
  );
  b.close();
  const tried = view.tried.find((x) => x.patch === all.patch);
  if (tried) b.leaf("div", "tried", "mx-tried", `Measured with them all: ${tried.summary}`);
  b.close();
}

/** Every row: a compact table, the loud and the hot rows marked. */
/** function barsView(b: Builder, r: MixReport) => Undefined */
function barsView(b, r) {
  b.open("div", "table", "mx-rows");
  b.open("div", "head", "mx-rowline head");
  for (const h of ["", "M max", "S max", "pre-lim", "GR", "top"]) b.leaf("span", `h${h}`, "", h);
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
