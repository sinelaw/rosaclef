// Automation: target resolution, curve evaluation and lane editing.
//
// A lane drives one target over song time. Targets are short paths
// (see crates/core/src/automation.rs, which this mirrors):
//   tempo | swing | channel/<id>/volume | channel/<id>/pan |
//   channel/<id>/<instrument param> | insert/<n>/volume | insert/<n>/pan |
//   insert/<n>/effect/<k>/<effect param>
// `laneValueAt` is the same interpolation as the engine's, so the playlist
// lanes and the controls show exactly what plays.

import { state, commit, invalidate, reportContext, deviceSpec } from "./store.js";
import { songLength, uniqueId, getParam } from "./model.js";
import { pointIndex } from "#brands";

/** Strength of `curve = ±1` (the engine uses the same constant). */
const CURVE_STRENGTH = 6;

/** UI state of the automation editor. */
export const auto = {
  /** Selected lane id ("" = none). */
  lane: "",
  points /*: PointIx[] */: [],
  collapsed: false,
  /** Lane id to scroll into view on the next playlist render. */
  reveal: "",
  menu: { open: false, x: 0, y: 0, target: "" },
};

// ------------------------------------------------------------------ curves

/** Shape a segment position `t` (0..1) by `curve` (-1..1). */
/** function curveShape(t: Number, curve: Number) => Number */
export function curveShape(t, curve) {
  const u = Math.max(0, Math.min(1, t));
  if (Math.abs(curve) < 0.000001) return u;
  const k = Math.max(-1, Math.min(1, curve)) * CURVE_STRENGTH;
  return (Math.exp(k * u) - 1) / (Math.exp(k) - 1);
}

/** Value of a lane at `beat` (points sorted by beat). */
/** function laneValueAt(points: AutomationPoint[], beat: Number) => Number */
export function laneValueAt(points, beat) {
  const n = points.length;
  if (n === 0) return 0;
  // Number of points at or before `beat`.
  let lo = 0;
  let hi = n;
  while (lo < hi) {
    const mid = Math.floor((lo + hi) / 2);
    if (points[mid].beat <= beat) lo = mid + 1;
    else hi = mid;
  }
  if (lo === 0) return points[0].value;
  if (lo === n) return points[n - 1].value;
  const a = points[lo - 1];
  const b = points[lo];
  const span = b.beat - a.beat;
  if (span <= 0.000000000001) return b.value;
  return a.value + (b.value - a.value) * curveShape((beat - a.beat) / span, b.curve);
}

// ------------------------------------------------------------------ targets

/** function makeSpec(key: String, label: String, min: Number, max: Number, dflt: Number, unit: String) => ParamSpec */
function makeSpec(key, label, min, max, dflt, unit) {
  return { key: key, label: label, min: min, max: max, default: dflt, unit: unit, curve: "linear", integer: false, doc: "" };
}

/** function none(target: String) => TargetInfo */
function none(target) {
  return { ok: false, kind: "", spec: makeSpec("", target, 0, 1, 0, ""), base: 0, label: target, color: "#8a6bb0", open: false };
}

/** function isIndex(s: String) => Boolean */
function isIndex(s) {
  if (s === "") return false;
  for (let i = 0; i < s.length; i++) {
    const c = s.charAt(i);
    if (c < "0" || c > "9") return false;
  }
  return true;
}

/** A device parameter as a target: its catalog spec (plugins: open range). */
/** function paramTarget(dev: Device, category: String, key: String, owner: String, color: String) => TargetInfo */
function paramTarget(dev, category, key, owner, color) {
  const ds = deviceSpec(dev.type, category);
  if (!ds) return none(key);
  if (ds.openParams) {
    let base = 0;
    for (const e of dev.params) if (e.key === key) base = e.value;
    return { ok: true, kind: "param", spec: makeSpec(key, key, 0, 1, base, ""), base: base, label: `${owner} · ${key}`, color: color, open: true };
  }
  for (const ps of ds.params) {
    if (ps.key === key) return { ok: true, kind: "param", spec: ps, base: getParam(dev, ps), label: `${owner} · ${ps.label}`, color: color, open: false };
  }
  return none(key);
}

/** Resolve a target against the current project. */
/** function targetInfo(target: String) => TargetInfo */
export function targetInfo(target) {
  const p = state.project;
  const parts = target.split("/");
  if (target === "tempo") {
    return {
      ok: true,
      kind: "tempo",
      spec: makeSpec("bpm", "Tempo", 20, 999, 120, "BPM"),
      base: p.transport.bpm,
      label: "Tempo",
      color: "#d4af37",
      open: false,
    };
  }
  if (target === "swing") {
    return { ok: true, kind: "swing", spec: makeSpec("swing", "Swing", 0, 1, 0, ""), base: p.transport.swing, label: "Swing", color: "#e8d5b0", open: false };
  }
  if (parts.length === 3 && parts[0] === "channel") {
    for (const ch of p.channels) {
      if (ch.id !== parts[1]) continue;
      const what = parts[2];
      if (what === "volume")
        return {
          ok: true,
          kind: "gain",
          spec: makeSpec("volume", "Volume", 0, 1.5, 0.8, ""),
          base: ch.volume,
          label: `${ch.name} · Volume`,
          color: ch.color,
          open: false,
        };
      if (what === "pan")
        return { ok: true, kind: "pan", spec: makeSpec("pan", "Pan", -1, 1, 0, ""), base: ch.pan, label: `${ch.name} · Pan`, color: ch.color, open: false };
      return paramTarget(ch.instrument, "instrument", what, ch.name, ch.color);
    }
    return none(target);
  }
  if (parts.length >= 3 && parts[0] === "insert" && isIndex(parts[1])) {
    const i = Math.round(Number(parts[1]));
    if (i >= p.mixer.inserts.length) return none(target);
    const ins = p.mixer.inserts[i];
    const color = i === 0 ? "#d4af37" : "#5b82c4";
    if (parts.length === 3 && parts[2] === "volume")
      return {
        ok: true,
        kind: "gain",
        spec: makeSpec("volume", "Volume", 0, 2, 1, ""),
        base: ins.volume,
        label: `${ins.name} · Volume`,
        color: color,
        open: false,
      };
    if (parts.length === 3 && parts[2] === "pan")
      return { ok: true, kind: "pan", spec: makeSpec("pan", "Pan", -1, 1, 0, ""), base: ins.pan, label: `${ins.name} · Pan`, color: color, open: false };
    if (parts.length === 5 && parts[2] === "effect" && isIndex(parts[3])) {
      const k = Math.round(Number(parts[3]));
      if (k >= ins.effects.length) return none(target);
      return paramTarget(ins.effects[k], "effect", parts[4], ins.name, color);
    }
  }
  return none(target);
}

/** A readable id for a new lane: "pad-cutoff", "insert3-fx0-mix". */
/** function laneSlug(target: String) => String */
function laneSlug(target) {
  const parts = target.split("/");
  if (parts[0] === "channel" && parts.length === 3) return `${parts[1]}-${parts[2]}`;
  if (parts[0] === "insert" && parts.length === 3) return `insert${parts[1]}-${parts[2]}`;
  if (parts[0] === "insert" && parts.length === 5) return `insert${parts[1]}-fx${parts[3]}-${parts[4]}`;
  return target;
}

// ------------------------------------------------------------------ lanes

/** function laneByTarget(target: String) => AutomationLane? */
export function laneByTarget(target) {
  return state.project.automation.find((l) => l.target === target);
}

/** Whether a target has an automation lane. */
/** function isAutomated(target: String) => Boolean */
export function isAutomated(target) {
  if (target === "") return false;
  for (const l of state.project.automation) if (l.target === target) return true;
  return false;
}

/** The value a control should show: the lane's value while the song plays. */
/** function shownValue(target: String, base: Number) => Number */
export function shownValue(target, base) {
  if (target === "" || !state.playing || state.mode !== "song") return base;
  for (const l of state.project.automation) {
    if (l.target === target && !l.mute && l.points.length > 0) return laneValueAt(l.points, state.position);
  }
  return base;
}

/** function selectedLane() => AutomationLane? */
export function selectedLane() {
  return state.project.automation.find((l) => l.id === auto.lane);
}

/** Selected point indexes that exist in the selected lane. */
/** function selectedPoints(lane: AutomationLane) => Int[] */
export function selectedPoints(lane) {
  /** const out: Int[] */
  const out = [];
  for (const ix of auto.points) {
    const k = pointIndex(ix);
    if (k < lane.points.length) out.push(k);
  }
  return out;
}

/** Select a lane, expand the automation section and scroll to it. */
/** function goToLane(id: String) => Undefined */
export function goToLane(id) {
  if (auto.lane !== id) auto.points = [];
  auto.lane = id;
  auto.collapsed = false;
  auto.reveal = id;
  reportContext();
  invalidate();
}

/** FL-style: a lane for a control, spanning the song at its current value. */
/** function createLane(target: String) => Undefined */
export function createLane(target) {
  const info = targetInfo(target);
  if (!info.ok) return undefined;
  const existing = laneByTarget(target);
  if (existing) {
    goToLane(existing.id);
    return undefined;
  }
  const p = state.project;
  const end = Math.max(songLength(p), p.transport.beatsPerBar * 4);
  const id = uniqueId(
    laneSlug(target),
    p.automation.map((l) => l.id),
  );
  const v = info.base;
  commit(() => {
    p.automation.push({
      id: id,
      name: info.label,
      target: target,
      color: info.color,
      mute: false,
      points: [
        { beat: 0, value: v, curve: 0 },
        { beat: end, value: v, curve: 0 },
      ],
    });
  });
  goToLane(id);
}

/** function removeLane(id: String) => Undefined */
export function removeLane(id) {
  commit(() => {
    state.project.automation = state.project.automation.filter((l) => l.id !== id);
  });
  if (auto.lane === id) {
    auto.lane = "";
    auto.points = [];
  }
  reportContext();
}

/** Keep lanes pointing at the right things after a structural edit: `map`
 * returns a lane's new target, or "" to drop the lane. Call inside a commit. */
/** function retargetLanes(map: (String) => String) => Undefined */
export function retargetLanes(map) {
  const p = state.project;
  /** const keep: AutomationLane[] */
  const keep = [];
  for (const l of p.automation) {
    const t = map(l.target);
    if (t !== "") {
      l.target = t;
      keep.push(l);
    }
  }
  p.automation = keep;
}

/** Effects of insert `insert` were moved or removed: `slot(k)` is effect
 * k's new index (-1: removed). */
/** function remapEffects(insert: Int, slot: (Int) => Int) => Undefined */
export function remapEffects(insert, slot) {
  const prefix = `insert/${insert}/effect/`;
  retargetLanes((t) => {
    if (!t.startsWith(prefix)) return t;
    const rest = t.slice(prefix.length);
    const cut = rest.indexOf("/");
    if (cut < 0) return t;
    const k = slot(Math.round(Number(rest.slice(0, cut))));
    return k < 0 ? "" : `${prefix}${k}${rest.slice(cut)}`;
  });
}

// ------------------------------------------------------------------ menu

/** Open the automation context menu of a control. */
/** function openMenu(target: String, x: Number, y: Number) => Undefined */
export function openMenu(target, x, y) {
  auto.menu.open = true;
  auto.menu.x = x;
  auto.menu.y = y;
  auto.menu.target = target;
  invalidate();
}

export function closeMenu() {
  if (!auto.menu.open) return;
  auto.menu.open = false;
  invalidate();
}
