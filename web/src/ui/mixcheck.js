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
import { followJob, jobLabel, jobFraction } from "./progress.js";
import { toast } from "./toast.js";
import { insertIx } from "#brands";
import { t, tf, tk } from "../i18n.js";

/** The six bands' names and ranges: t() translates them where they show. The
 * inner ranges are plain numbers, not keys: bandTip() shows them as they are. */
const BANDS = [
  tk("mixcheck.spectrum.band.sub.label"),
  tk("mixcheck.spectrum.band.bass.label"),
  tk("mixcheck.spectrum.band.lowMid.label"),
  tk("mixcheck.spectrum.band.mid.label"),
  tk("mixcheck.spectrum.band.highMid.label"),
  tk("mixcheck.spectrum.band.air.label"),
];
const BAND_TIPS = [tk("mixcheck.spectrum.band.sub.range"), "60–250 Hz", "250–500 Hz", "500 Hz–2 kHz", "2–6 kHz", tk("mixcheck.spectrum.band.air.range")];

/** A band's range as shown: the first and the last are keys, the others numbers. */
/** function bandTip(i: Int) => String */
function bandTip(i) {
  return i === 0 || i === BAND_TIPS.length - 1 ? t(BAND_TIPS[i]) : BAND_TIPS[i];
}

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
  /** The report before the last Measure (to compare with), and whether the
   * panel shows it instead. */
  previous: emptyReport(),
  showPrevious: false,
  targets /*: MixTargetInfo[] */: [],
  catalogAsked: false,
  /** Sections folded away. */
  folded /*: String[] */: [],
  /** The element whose suggestions show. */
  open: "",
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
  /** Which project the panel is about: a measure under way when another
   * one opens is dropped. */
  song: 0,
};

/** Another project is open: nothing measured of the last one stays. */
/** function resetMixcheck() => Undefined */
function resetMixcheck() {
  stopAudition();
  view.song = view.song + 1;
  view.busy = false;
  view.job = 0;
  view.error = "";
  view.edits = -1;
  view.report = emptyReport();
  view.previous = emptyReport();
  view.showPrevious = false;
  view.scope = "song";
  view.section = "";
  view.reference = "";
  view.open = "";
  view.applied = [];
  view.appliedEdits = -1;
  view.patched = [];
  view.excluded = [];
  invalidate();
}

hooks.opened = resetMixcheck;

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
        anchor: str(e.anchor),
        range: nums(e.rangeDb),
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
  const song = view.song;
  view.busy = true;
  view.error = "";
  view.job = 0;
  invalidate();
  const q = request();
  followJob(
    "mixcheck",
    {
      project: encodeProject(state.project),
      range: q.range,
      section: q.section,
      target: q.target,
      threshold: q.threshold,
      reference: q.reference,
      history: true,
    },
    (id) => {
      view.job = id;
    }
  )
    .then((r) => {
      if (view.song !== song) return false;
      view.busy = false;
      view.edits = edits;
      if (view.report.ok) view.previous = view.report;
      view.showPrevious = false;
      view.report = decodeReport(r);
      view.applied = [];
      view.appliedEdits = -1;
      view.excluded = [];
      view.patched = [];
      invalidate();
      return true;
    })
    .catch((e) => {
      if (view.song !== song) return false;
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
  toast(t("mixcheck.stale.toast.title"), t("mixcheck.stale.toast.body"), "info");
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

/** Apply a fix: the endpoint patches the project as it is and validates it;
 * one undo step. If the song changed meanwhile, nothing is applied. */
/** function applyPatch(patch: String, what: String, keys: String[]) => Undefined */
function applyPatch(patch, what, keys) {
  if (!fresh()) return undefined;
  const by = clash(patch);
  if (by !== "") {
    toast(t("mixcheck.clash.toast.title"), tf("mixcheck.clash.toast.body", [by.split("|")[0].replace(/-/g, " ")]), "info");
    return undefined;
  }
  stopAudition();
  const edits = state.edits;
  sendJson("/api/mixcheck", "POST", { project: encodeProject(state.project), apply: patch })
    .then((r) => {
      if (state.edits !== edits) {
        toast(t("fix.songChanged"), t("fix.nothingApplied"), "info");
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
      toast(what, t("mixcheck.apply.doneMore.body"), "info");
      return true;
    })
    .catch((e) => {
      toast(t("mixcheck.apply.failed.title"), errText(e), "error");
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
    toast(t("mixcheck.listen.browserOnly.toast.title"), t("mixcheck.listen.browserOnly.toast.body"), "info");
    return false;
  }
  await startAudio();
  view.audition.seq = view.audition.seq + 1;
  const seq = view.audition.seq;
  let json = "";
  try {
    json = await patchedJson(patch);
  } catch (e) {
    toast(t("mixcheck.listen.failed.title"), errText(e), "error");
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
    mine ? (a.after ? t("mixcheck.listen.after.label") : t("mixcheck.listen.before.label")) : t("mixcheck.listen.label"),
    tf("mixcheck.listen.title", [String(LISTEN_BARS)]),
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
/** function loudnessScale(b: Builder, m: MixMaster, tg: MixTarget) => Undefined */
function loudnessScale(b, m, tg) {
  const lo = -36;
  const hi = 0;
  /** function at(x: Number) => String */
  const at = (x) => `${fmt(Math.max(0, Math.min(100, ((x - lo) / (hi - lo)) * 100)), 2)}%`;
  b.open("div", "scale", "mx-scale");
  b.attr("title", t("mixcheck.master.scale.title"));
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
  const tip = tf("mixcheck.master.gr.title", [label, db(g.max, 1), db(g.mean, 1), db(g.above3, 0)]);
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
  const tip = tf("mixcheck.master.correlation.title", [db(m.corrMean, 2), db(m.corrMin, 2), signed(m.monoLoss)]);
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
    ? t("mixcheck.parts.verdict.inaudible")
    : v === "weak"
      ? t("mixcheck.parts.verdict.weak")
      : v === "buried"
        ? t("mixcheck.parts.verdict.buried")
        : v === "dominant"
          ? t("mixcheck.parts.verdict.dominant")
          : v === "overloading"
            ? t("mixcheck.parts.verdict.overloading")
            : t("mixcheck.parts.verdict.ok");
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
    b.attr("title", t("mixcheck.parts.lead.title"));
    b.text(t("mixcheck.parts.lead.label"));
    b.close();
  } else if (e.anchor !== "" && e.range.length === 2) {
    b.open("span", "role", "mx-el-role anchor");
    b.attr("title", tf("mixcheck.parts.anchor.title", [e.name, fmt(e.range[0], 0), fmt(e.range[1], 0)]));
    b.text(e.anchor === "kick" ? t("mixcheck.parts.anchor.kick.label") : t("mixcheck.parts.anchor.bass.label"));
    b.close();
  } else if (e.role !== "") b.leaf("span", "role", "mx-el-role", e.role);
  b.leaf("span", "sp", "spacer", "");
  // Level against the mix: −30 … +6 dB.
  const under = e.buriedIn.map((u) => tf("mixcheck.parts.rel.buriedRange", [String(u.from), String(u.to), signed(u.rel)])).join(", ");
  const relTip =
    under !== ""
      ? tf("mixcheck.parts.rel.title.withBuried", [e.name, signed(e.rel), db(e.share, 0), under])
      : tf("mixcheck.parts.rel.title.plain", [e.name, signed(e.rel), db(e.share, 0)]);
  b.open("span", "rel", "mx-el-rel");
  b.attr("title", relTip);
  b.leaf("span", "fill", "mx-el-relfill", "");
  b.style("width", `${fmt(Number.isFinite(e.rel) ? Math.max(2, Math.min(100, ((e.rel + 30) / 36) * 100)) : 0, 1)}%`);
  b.leaf("span", "t", "mx-el-reltext", `${signed(e.rel)} dB`);
  b.close();
  const audTip = tf("mixcheck.parts.audible.title", [db(e.audible, 0)]);
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
      tf("mixcheck.parts.fact.rms", [db(e.rms, 1)]),
      tf("mixcheck.parts.fact.peak", [db(e.peak, 1)]),
      `${db(e.lufs, 1)} LUFS`,
      tf("mixcheck.parts.fact.fader", [signed(e.fader)]),
      tf("mixcheck.parts.fact.active", [db(e.active, 0)]),
    ];
    if (Number.isFinite(e.corr)) facts.push(tf("mixcheck.parts.fact.correlation", [fmt(e.corr, 2)]));
    if (Number.isFinite(e.domLo)) facts.push(tf("mixcheck.parts.fact.dominantRange", [fmt(e.domLo, 0), fmt(e.domHi, 0)]));
    b.leaf("div", "facts", "mx-facts", facts.join(" · "));
    for (const m of e.maskers) {
      // One sentence, the masker's name ({0}) a button in it.
      const said = tf("mixcheck.parts.masker.text", ["{0}", fmt(m.lo, 0), fmt(m.hi, 0), signed(m.db)]);
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
      if (Number.isFinite(s.expAud)) exp.push(tf("mixcheck.parts.suggestion.expect.audible", [fmt(s.expAud, 0)]));
      if (Number.isFinite(s.expRel)) exp.push(tf("mixcheck.parts.suggestion.expect.rel", [signed(s.expRel)]));
      b.open("div", "acts", "mx-acts");
      if (exp.length > 0) b.leaf("span", "e", "mx-expect", tf("mixcheck.parts.suggestion.expected", [exp.join(", ")]));
      b.leaf("span", "sp", "spacer", "");
      button(b, "apply", "small gold", t("mixcheck.parts.suggestion.apply.label"), t("mixcheck.parts.suggestion.apply.title"), () =>
        applyPatch(s.patch, `${e.name}: ${s.why}`, [])
      );
      b.close();
      b.close();
    }
    b.open("div", "show", "mx-acts");
    b.leaf("span", "sp", "spacer", "");
    button(b, "mixer", "small ghost", t("mixcheck.parts.showInMixer.label"), t("mixcheck.parts.showInMixer.title"), () => revealElement(e.id));
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
  const latest = view.report;
  const old = view.showPrevious && view.previous.ok;
  const r = old ? view.previous : latest;
  const stale = latest.ok && view.edits !== state.edits;
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
    [t("term.wholeSong"), t("term.bars"), t("mixcheck.scope.option.section")],
    t("mixcheck.scope.title"),
    (v) => {
      view.scope = v;
      invalidate();
    }
  );
  if (view.scope === "bars") {
    for (const x of [
      { k: "from", v: view.from, tip: t("mixcheck.scope.from.title") },
      { k: "to", v: view.to, tip: t("mixcheck.scope.to.title") },
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
    select(b, "section", "mx-select", cur, sections, sections, t("mixcheck.scope.section.title"), (v) => {
      view.section = v;
      invalidate();
    });
  }
  const ids = [""].concat(view.targets.map((x) => x.id));
  const names = [t("mixcheck.target.option.none")].concat(view.targets.map((x) => `${x.name} ${fmt(x.lufs, 0)} LUFS`));
  select(b, `target${view.targets.length}`, "mx-select", view.target, ids, names, t("mixcheck.target.title"), (v) => {
    view.target = v;
    localStorage.setItem("rosaclef.mixcheck.target", v);
    invalidate();
  });
  b.leaf("span", "sp", "spacer", "");
  b.open("button", "run", view.busy ? "btn small gold busy" : stale || !r.ok ? "btn small gold" : "btn small");
  b.attr("title", t("mixcheck.run.title"));
  b.on("click", (e) => runMixcheck());
  glyph(b, "meter");
  b.leaf("span", "l", "", view.busy ? t("mixcheck.run.busy.label") : latest.ok ? t("mixcheck.run.again.label") : t("mixcheck.run.label"));
  b.close();
  b.close();

  b.open("div", "bar2", "mx-bar mx-bar2");
  select(
    b,
    "threshold",
    "mx-select small",
    view.threshold,
    ["strict", "normal", "loose"],
    [t("mixcheck.threshold.option.strict"), t("mixcheck.threshold.option.normal"), t("mixcheck.threshold.option.loose")],
    t("mixcheck.threshold.title"),
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
    [t("mixcheck.reference.option.none")].concat(audio.map((s) => s.split("/").pop() ?? s)),
    t("mixcheck.reference.title"),
    (v) => {
      view.reference = v;
      invalidate();
    }
  );
  b.leaf("span", "sp", "spacer", "");
  if (r.ok) {
    const where = r.fromBar === r.toBar ? tf("format.barLower", [String(r.fromBar)]) : tf("format.barRange", [String(r.fromBar), String(r.toBar)]);
    /** const facts: String[] */
    const facts = [where, `${fmt(r.seconds, 1)} s`];
    if (r.repeats) facts.push(t("mixcheck.status.everyPass"));
    facts.push(r.cached ? t("mixcheck.status.cached") : `${fmt(r.ms / 1000, 1)} s`);
    if (old) facts.push(t("mixcheck.compare.showing"));
    else if (stale) facts.push(view.appliedEdits === state.edits ? t("mixcheck.status.fixesApplied") : t("mixcheck.status.stale"));
    b.leaf("span", "st", stale ? "mx-status stale" : "mx-status", facts.join(" · "));
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
      b.leaf("span", "t", "mx-hint-text", t("mixcheck.empty.hint"));
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
    b.leaf("h3", "h", "", view.busy ? t("common.listening") : t("mixcheck.empty.label"));
    b.leaf("p", "p", "", t("mixcheck.empty.body"));
    b.close();
  } else {
    if (view.previous.ok && !view.busy) compareView(b, view.previous, latest);
    masterView(b, r);
    findingsView(b, r, ask, !old);
    if (section(b, "history", t("mixcheck.history.label"), t("mixcheck.history.legend"))) {
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
      b.on("pointerenter", (e) => hint(t("mixcheck.history.hint")));
    }
    if (
      section(
        b,
        "spectrum",
        t("mixcheck.spectrum.label"),
        r.reference.file !== "" ? tf("mixcheck.spectrum.legend.reference", [r.reference.file.split("/").pop() ?? ""]) : ""
      )
    ) {
      b.canvas("spectrum", "mx-canvas mx-spectrum", (g, w, h) => paintSpectrum(g, w, h, r));
      const tip = r.master.spectrum.map((v, i) => `${t(BANDS[i])} (${bandTip(i)}) ${fmt(v, 1)} dB`).join(" · ");
      b.attr("title", tip);
      if (r.reference.summary !== "") b.leaf("p", "ref", "mx-note", tf("mixcheck.spectrum.reference.summary", [r.reference.summary]));
    }
    if (section(b, "parts", t("term.parts"), tf("mixcheck.parts.needAttention", [String(r.elements.filter((e) => e.verdict !== "ok").length)]))) {
      b.open("div", "legend", "mx-legend");
      b.leaf("span", "a", "", t("mixcheck.parts.legend.rel"));
      b.leaf("span", "b", "", t("mixcheck.parts.legend.audible"));
      b.close();
      const order = r.elements.slice().sort((x, y) => {
        const rank = (v) => (v === "inaudible" ? 0 : v === "weak" ? 1 : v === "buried" ? 2 : v === "overloading" ? 3 : v === "dominant" ? 4 : 5);
        return rank(x.verdict) - rank(y.verdict) || y.share - x.share;
      });
      for (const e of order) elementRow(b, e);
    }
    if (section(b, "rows", t("term.bars"), t("mixcheck.bars.legend"))) barsView(b, r);
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
  b.attr("title", t("mixcheck.master.integrated.title"));
  b.leaf("span", "v", "mx-big-value", db(m.integrated, 1));
  b.leaf("span", "u", "mx-big-unit", t("mixcheck.master.integrated.unit"));
  b.close();
  b.open("div", "st", "mx-lufs-side");
  readout(b, "s", t("mixcheck.master.shortMax.label"), db(m.shortMax, 1), "LUFS", "ok", t("mixcheck.master.shortMax.title"));
  readout(b, "m", t("mixcheck.master.momentaryMax.label"), db(m.momentaryMax, 1), "LUFS", "ok", t("mixcheck.master.momentaryMax.title"));
  b.close();
  if (tg.id !== "") {
    b.open("div", "target", `mx-target ${tg.status}`);
    b.attr("title", tg.notes.join("; "));
    b.leaf(
      "span",
      "chip",
      `mx-chip ${tg.status}`,
      tg.status === "pass" ? t("mixcheck.target.status.pass") : tg.status === "warn" ? t("mixcheck.target.status.warn") : t("mixcheck.target.status.fail")
    );
    b.leaf("span", "n", "", `${tg.name} · ${fmt(tg.lufs, 0)} LUFS · ${fmt(tg.truePeak, 0)} dBTP`);
    if (Number.isFinite(tg.gain)) b.leaf("span", "g", "mx-target-gain", tf("mixcheck.target.gain", [signed(tg.gain)]));
    b.close();
  }
  b.close();
  loudnessScale(b, m, tg);
  b.open("div", "tiles", "mx-tiles");
  const tpLimit = Number.isFinite(tg.truePeak) ? tg.truePeak : -1;
  readout(
    b,
    "tp",
    t("mixcheck.master.truePeak.label"),
    db(m.truePeak, 1),
    "dBTP",
    light(m.truePeak > tpLimit, m.truePeak > tpLimit - 0.5),
    tf("mixcheck.master.truePeak.title", [fmt(tpLimit, 1)])
  );
  readout(
    b,
    "pl",
    t("mixcheck.master.preLimiter.label"),
    signed(m.preLimiter),
    "dBFS",
    light(m.preLimiter > 3, m.preLimiter > 0),
    t("mixcheck.master.preLimiter.title")
  );
  readout(b, "plr", "PLR", db(m.plr, 1), "dB", light(m.plr < 6, m.plr < 8), t("mixcheck.master.plr.title"));
  readout(b, "lra", "LRA", db(m.lra, 1), "LU", light(false, m.lra < 2), t("mixcheck.master.lra.title"));
  readout(b, "crest", t("mixcheck.master.crest.label"), db(m.crest, 1), "dB", "ok", t("mixcheck.master.crest.title"));
  readout(b, "mono", t("mixcheck.master.mono.label"), signed(m.monoLoss), "dB", light(m.monoLoss < -6, m.monoLoss < -4), t("mixcheck.master.mono.title"));
  b.close();
  b.open("div", "dyn", "mx-dyn");
  b.open("div", "grs", "mx-grs");
  grMeter(b, "lim", t("mixcheck.master.gr.limiter.label"), m.limGr);
  if (Number.isFinite(m.compGr.max)) grMeter(b, "comp", t("mixcheck.master.gr.busComp.label"), m.compGr);
  for (const g of r.gr.filter((x) => !x.id.startsWith("insert:0/") && x.max >= 1).slice(0, 4)) {
    grMeter(b, `${g.id}-${g.effect}`, nameOf(g.id), g);
  }
  b.close();
  correlationMeter(b, m);
  b.close();
  if (r.whatIf !== "") b.leaf("div", "wi", "mx-note", tf("mixcheck.master.whatIf", [r.whatIf]));
  b.close();
}

/** The findings, ranked, each with its fix. */
/** `live`: the latest report (the previous one, shown to compare, has no actions). */
/** function findingsView(b: Builder, r: MixReport, ask: (String) => Undefined, live: Boolean) => Undefined */
function findingsView(b, r, ask, live) {
  if (!section(b, "findings", t("mixcheck.findings.label"), r.findings.length === 0 ? t("mixcheck.findings.none") : `${r.findings.length}`)) return undefined;
  if (r.findings.length === 0) {
    b.leaf("p", "none", "mx-note", t("mixcheck.findings.empty"));
    return undefined;
  }
  if (live) quickFix(b, r);
  for (const f of r.findings) {
    const done = isApplied(f.key);
    const near = local(r, f);
    b.open("div", f.key, `mx-find ${f.severity}${done ? " applied" : ""}`);
    b.leaf("span", "dot", "mx-dot", "");
    b.open("div", "body", "mx-find-body");
    b.open("div", "t", "mx-find-title");
    if (live && f.patch !== "" && !done) {
      b.leaf("input", "in", "mx-include", "");
      b.attr("type", "checkbox");
      b.attr("title", t("mixcheck.findings.include.title"));
      b.prop("checked", view.excluded.includes(f.key) ? "" : "true");
      b.on("change", (e) => {
        view.excluded = view.excluded.includes(f.key) ? view.excluded.filter((k) => k !== f.key) : view.excluded.concat([f.key]);
        invalidate();
      });
    }
    b.leaf("b", "r", "", f.rule.replace(/-/g, " "));
    b.open("button", "w", "mx-link");
    b.attr("title", t("common.showIt"));
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
      const span = f.fromBar === f.toBar ? tf("format.barLower", [String(f.fromBar)]) : tf("format.barRange", [String(f.fromBar), String(f.toBar)]);
      const tip = layoutState.top === "score" ? tf("mixcheck.findings.show.score.title", [span]) : tf("mixcheck.findings.show.playlist.title", [span]);
      button(b, "show", "small ghost", t("mixcheck.findings.show.label"), tip, () => revealBar(barBeat(f.fromBar)));
    }
    if (live && f.patch !== "" && !done) listenButton(b, f.key, f.patch);
    b.leaf("span", "sp", "spacer", "");
    if (live && state.backend !== "local")
      button(b, "ask", "small ghost", t("agent.askMaestro"), t("agent.typeIntoPrompt"), () =>
        ask(`Mix check (${f.where}): ${f.detail} Please look into it (rosaclef mixcheck reports it as ${f.key}).`)
      );
    if (f.patch !== "" && done) {
      b.leaf("span", "ok", "mx-applied", t("mixcheck.findings.applied"));
      b.close();
      if (f.label !== "") b.leaf("div", "fl", "mx-fixlabel", tf("mixcheck.findings.fix.text", [f.label]));
    } else if (live && f.patch !== "") {
      button(b, "fix", "small gold", t("mixcheck.findings.fix.label"), tf("fix.applyOneUndo", [f.label]), () => applyPatch(f.patch, f.label, [f.key]));
      b.close();
      if (f.label !== "") b.leaf("div", "fl", "mx-fixlabel", tf("mixcheck.findings.fix.text", [f.label]));
    } else b.close();
    b.close();
    b.close();
  }
}

/** The last Measure against the one before: the headline numbers, the
 * findings gone and new; and a switch to show the previous report whole. */
/** function compareView(b: Builder, before: MixReport, now: MixReport) => Undefined */
function compareView(b, before, now) {
  const a = before.master;
  const z = now.master;
  b.open("div", "cmp", "mx-compare");
  b.open("div", "h", "mx-compare-head");
  b.leaf("b", "t", "", t("mixcheck.compare.label"));
  b.leaf("span", "sp", "spacer", "");
  if (view.showPrevious)
    button(b, "back", "small gold", t("mixcheck.compare.back.label"), t("mixcheck.compare.back.title"), () => {
      view.showPrevious = false;
      invalidate();
    });
  else
    button(b, "prev", "small ghost", t("mixcheck.compare.show.label"), t("mixcheck.compare.show.title"), () => {
      view.showPrevious = true;
      invalidate();
    });
  b.close();
  /** const facts: String[] */
  const facts = [
    tf("mixcheck.compare.integrated", [db(a.integrated, 1), db(z.integrated, 1)]),
    tf("mixcheck.compare.truePeak", [db(a.truePeak, 1), db(z.truePeak, 1)]),
    tf("mixcheck.compare.plr", [db(a.plr, 1), db(z.plr, 1)]),
    tf("mixcheck.compare.findings", [String(before.findings.length), String(now.findings.length)]),
  ];
  b.leaf("div", "n", "mx-compare-line", facts.join(" · "));
  const keys = (r) => r.findings.map((f) => f.key);
  const gone = before.findings.filter((f) => !keys(now).includes(f.key)).map((f) => f.rule.replace(/-/g, " "));
  const added = now.findings.filter((f) => !keys(before).includes(f.key)).map((f) => f.rule.replace(/-/g, " "));
  if (gone.length > 0) b.leaf("div", "g", "mx-compare-line gone", tf("mixcheck.compare.gone", [gone.join(", ")]));
  if (added.length > 0) b.leaf("div", "a", "mx-compare-line added", tf("mixcheck.compare.added", [added.join(", ")]));
  b.close();
}

/** Every fix at once: hear them, apply them (one undo step). */
/** function quickFix(b: Builder, r: MixReport) => Undefined */
function quickFix(b, r) {
  const all = allFixes(r);
  if (!r.findings.some((f) => f.patch !== "" && !isApplied(f.key))) return undefined;
  b.open("div", "quick", "mx-quick");
  b.open("div", "t", "mx-quick-title");
  b.leaf("b", "h", "", t("mixcheck.quick.label"));
  /** const facts: String[] */
  const facts = [all.n === 0 ? t("mixcheck.quick.none") : all.n === 1 ? t("mixcheck.quick.ticked.one") : tf("mixcheck.quick.ticked.other", [String(all.n)])];
  if (view.applied.length > 0) facts.push(tf("mixcheck.quick.appliedAlready", [String(view.applied.length)]));
  b.leaf("span", "n", "mx-quick-sub", facts.join(" · "));
  b.close();
  b.open("div", "acts", "mx-acts");
  listenButton(b, "*all", all.patch);
  b.leaf("span", "sp", "spacer", "");
  button(b, "fix", "small gold", t("mixcheck.quick.applyAll.label"), t("mixcheck.quick.applyAll.title"), () =>
    applyPatch(
      all.patch,
      all.n === 1 ? t("mixcheck.quick.applied.toast.one") : tf("mixcheck.quick.applied.toast.other", [String(all.n)]),
      r.findings.filter((f) => f.patch !== "" && !isApplied(f.key) && !view.excluded.includes(f.key) && clash(f.patch) === "").map((f) => f.key)
    )
  );
  b.close();
  b.close();
}

/** Every row: a compact table, the loud and the hot rows marked. */
/** function barsView(b: Builder, r: MixReport) => Undefined */
function barsView(b, r) {
  b.open("div", "table", "mx-rows");
  b.open("div", "head", "mx-rowline head");
  const heads = [
    "",
    t("mixcheck.bars.head.momentaryMax"),
    t("mixcheck.bars.head.shortMax"),
    t("mixcheck.bars.head.preLimiter"),
    t("mixcheck.bars.head.gainReduction"),
    t("mixcheck.bars.head.top"),
  ];
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
