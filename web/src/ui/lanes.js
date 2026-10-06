// Automation lanes in the playlist: an "Automation" section below the
// tracks with one row per lane. The rows share the playlist's horizontal
// geometry (zoom and scroll), and — like the clips — point positions are
// computed once (`laneBoxes`) and read by both rendering and hit-testing.
// The curve itself is a canvas leaf covering only the visible range; only
// visible points become DOM nodes.
//
// Also here: the small context menu that controls open on right-click
// ("Create automation lane", "Go to automation", "Remove automation").

import { drag, promptBox, fmt } from "#platform";
import { state, commit, begin, changed, invalidate, hint, reportContext } from "../store.js";
import { snapTo, dbText, panText, barBeat, PALETTE } from "../model.js";
import { auto, curveShape, laneValueAt, targetInfo, goToLane, removeLane, createLane, closeMenu, laneByTarget, selectedPoints } from "../automation.js";
import { iconButton, paramText, glyph } from "./widgets.js";
import { pointIx, pointIndex } from "#brands";
import { t, tf } from "../i18n.js";

/** Height of a lane row and of the section divider, in pixels. */
export const LANE_H = 64;
export const DIV_H = 24;
/** Vertical padding inside a lane (values 0 and 1 sit this far from the edges). */
const PAD = 9;

/** type LaneGeo = { zoom: Number, top: Number, x0: Number, x1: Number } */
/** type PtBox = { i: PointIx, x: Number, y: Number } */
/** type VRange = { lo: Number, hi: Number, exp: Boolean } */

// ------------------------------------------------------------------ values

/** The value range a lane draws: the target's range (log for frequencies);
 * tempo zooms to 60..200 BPM unless the points need more. */
/** function displayRange(info: TargetInfo, lane: AutomationLane) => VRange */
function displayRange(info, lane) {
  const s = info.spec;
  if (info.kind === "tempo" || info.open) {
    let lo = info.kind === "tempo" ? 60 : 0;
    let hi = info.kind === "tempo" ? 200 : 1;
    const margin = info.kind === "tempo" ? 10 : 0;
    for (const pt of lane.points) {
      lo = Math.min(lo, pt.value - margin);
      hi = Math.max(hi, pt.value + margin);
    }
    if (info.kind === "tempo") {
      lo = Math.max(20, lo);
      hi = Math.min(999, hi);
    }
    return { lo: lo, hi: Math.max(hi, lo + 0.000001), exp: false };
  }
  return { lo: s.min, hi: s.max, exp: s.curve === "exp" && s.min > 0 };
}

/** function unitOf(r: VRange, v: Number) => Number */
function unitOf(r, v) {
  const u = r.exp ? Math.log(Math.max(v, r.lo) / r.lo) / Math.log(r.hi / r.lo) : (v - r.lo) / (r.hi - r.lo);
  return Math.max(0, Math.min(1, u));
}

/** function valueOf(r: VRange, u: Number) => Number */
function valueOf(r, u) {
  const t = Math.max(0, Math.min(1, u));
  return r.exp ? r.lo * Math.pow(r.hi / r.lo, t) : r.lo + (r.hi - r.lo) * t;
}

/** Y (inside a lane) of a unit value. */
/** function yOf(u: Number) => Number */
function yOf(u) {
  return PAD + (1 - u) * (LANE_H - 2 * PAD);
}

/** Round a dragged value to something readable (integers stay integers). */
/** function tidy(info: TargetInfo, v: Number) => Number */
function tidy(info, v) {
  if (info.spec.integer) return Math.round(v);
  if (info.kind === "tempo") return Math.round(v * 10) / 10;
  if (v === 0) return 0;
  const step = Math.pow(10, Math.floor(Math.log10(Math.abs(v))) - 3);
  return Math.round(v / step) * step;
}

/** function formatValue(info: TargetInfo, v: Number) => String */
export function formatValue(info, v) {
  if (info.kind === "tempo") return `${fmt(v, 1)} BPM`;
  if (info.kind === "swing") return `${Math.round(v * 100)}%`;
  if (info.kind === "gain") return dbText(v);
  if (info.kind === "pan") return panText(v);
  if (info.open) return fmt(v, 3);
  return paramText(info.spec, v);
}

/** "rgba(r, g, b, a)" from "#rrggbb". */
/** function rgba(hex: String, a: Number) => String */
function rgba(hex, a) {
  const digits = "0123456789abcdef";
  const h = hex.toLowerCase();
  /** const c: Int[] */
  const c = [];
  for (let k = 0; k < 3; k++) {
    const hi = digits.indexOf(h.charAt(1 + k * 2));
    const lo = digits.indexOf(h.charAt(2 + k * 2));
    c.push(Math.max(0, hi) * 16 + Math.max(0, lo));
  }
  return `rgba(${c[0]}, ${c[1]}, ${c[2]}, ${a})`;
}

// ------------------------------------------------------------------ layout

/** Height of the automation section. */
export function autoHeight() {
  return DIV_H + (auto.collapsed ? 0 : state.project.automation.length * LANE_H);
}

/** Top of lane `k` relative to the section. */
/** function laneTop(k: Int) => Number */
function laneTop(k) {
  return DIV_H + k * LANE_H;
}

/** Point positions of a lane in content coordinates. */
/** function laneBoxes(lane: AutomationLane, r: VRange, zoom: Number, top: Number) => PtBox[] */
function laneBoxes(lane, r, zoom, top) {
  /** const out: PtBox[] */
  const out = [];
  for (let i = 0; i < lane.points.length; i++) {
    const pt = lane.points[i];
    out.push({ i: pointIx(i), x: pt.beat * zoom, y: top + yOf(unitOf(r, pt.value)) });
  }
  return out;
}

/** function hitPoint(boxes: PtBox[], x: Number, y: Number) => Int */
function hitPoint(boxes, x, y) {
  let best = -1;
  let bestD = 64;
  for (let k = 0; k < boxes.length; k++) {
    const dx = boxes[k].x - x;
    const dy = boxes[k].y - y;
    const d = dx * dx + dy * dy;
    if (d <= bestD) {
      best = k;
      bestD = d;
    }
  }
  return best;
}

/** Index of the point that ends the segment containing `beat` (-1: none). */
/** function segmentEnd(lane: AutomationLane, beat: Number) => Int */
function segmentEnd(lane, beat) {
  for (let j = 1; j < lane.points.length; j++) {
    if (lane.points[j - 1].beat <= beat && beat <= lane.points[j].beat) return j;
  }
  return -1;
}

/** Scroll offset (inside the section) of a lane to reveal, or -1. */
export function revealOffset() {
  if (auto.reveal === "") return -1;
  const id = auto.reveal;
  auto.reveal = "";
  const lanes = state.project.automation;
  for (let k = 0; k < lanes.length; k++) {
    if (lanes[k].id === id) return laneTop(k);
  }
  return -1;
}

// ------------------------------------------------------------------ editing

/** function selectLane(lane: AutomationLane) => Undefined */
function selectLane(lane) {
  if (auto.lane !== lane.id) {
    auto.lane = lane.id;
    auto.points = [];
    reportContext();
    invalidate();
  }
}

/** function isSelPoint(lane: AutomationLane, k: Int) => Boolean */
function isSelPoint(lane, k) {
  if (auto.lane !== lane.id) return false;
  return selectedPoints(lane).includes(k);
}

/** Drag a point: time (snapped; Shift = free and fine) and value. The undo
 * step starts with the first movement unless the gesture already began one. */
/** function dragPoint(e: Ev, lane: AutomationLane, idx: Int, r: VRange, info: TargetInfo, zoom: Number, begun: Boolean) => Undefined */
function dragPoint(e, lane, idx, r, info, zoom, begun) {
  const pt = lane.points[idx];
  const b0 = pt.beat;
  const u0 = unitOf(r, pt.value);
  const x0 = e.clientX;
  const y0 = e.clientY;
  const gesture = { begun: begun };
  drag(
    e,
    (m) => {
      if (!gesture.begun) {
        if (Math.abs(m.clientX - x0) + Math.abs(m.clientY - y0) < 3) return undefined;
        gesture.begun = true;
        begin();
      }
      const fine = m.shiftKey;
      const lo = idx > 0 ? lane.points[idx - 1].beat : 0;
      const hi = idx + 1 < lane.points.length ? lane.points[idx + 1].beat : 1000000;
      let beat = b0 + ((m.clientX - x0) * (fine ? 0.25 : 1)) / zoom;
      if (!fine) beat = snapTo(beat, state.snap);
      pt.beat = Math.max(lo, Math.min(hi, Math.max(0, beat)));
      const u = u0 - ((m.clientY - y0) / (LANE_H - 2 * PAD)) * (fine ? 0.2 : 1);
      pt.value = tidy(info, valueOf(r, u));
      hint(tf("lanes.point.drag.hint", [info.label, formatValue(info, pt.value), barBeat(pt.beat, state.project.transport)]));
      changed(true);
    },
    (u) => {
      reportContext();
    }
  );
}

/** Alt-drag: bend the segment that ends at point `idx` (drag up raises its middle). */
/** function dragCurve(e: Ev, lane: AutomationLane, idx: Int, r: VRange, info: TargetInfo) => Undefined */
function dragCurve(e, lane, idx, r, info) {
  begin();
  const pt = lane.points[idx];
  const rising = unitOf(r, pt.value) >= unitOf(r, lane.points[idx - 1].value);
  const c0 = pt.curve;
  const y0 = e.clientY;
  drag(
    e,
    (m) => {
      const d = (y0 - m.clientY) / 90;
      pt.curve = Math.round(Math.max(-1, Math.min(1, c0 + (rising ? -d : d))) * 100) / 100;
      hint(tf("lanes.point.curve.hint", [info.label, fmt(pt.curve, 2)]));
      changed(true);
    },
    (u) => undefined
  );
}

/** function deletePoint(lane: AutomationLane, idx: Int) => Undefined */
function deletePoint(lane, idx) {
  if (lane.points.length <= 1) {
    hint(t("lanes.point.deleteLast.hint"));
    return undefined;
  }
  commit(() => {
    lane.points.splice(idx, 1);
  });
  auto.points = [];
  reportContext();
}

/** function typeValue(lane: AutomationLane, idx: Int, info: TargetInfo) => Undefined */
function typeValue(lane, idx, info) {
  const pt = lane.points[idx];
  const ask =
    info.kind === "tempo"
      ? tf("lanes.point.value.tempo.prompt", [info.label])
      : info.kind === "gain"
        ? tf("lanes.point.value.gain.prompt", [info.label])
        : info.spec.unit !== ""
          ? tf("lanes.point.value.withUnit.prompt", [info.label, info.spec.unit])
          : tf("lanes.point.value.prompt", [info.label]);
  const text = promptBox(ask, String(tidy(info, pt.value)));
  if (text === "") return undefined;
  const v = Number(text);
  if (!(v > -1000000000 && v < 1000000000)) return undefined;
  const lo = info.open ? -1000000000 : info.spec.min;
  const hi = info.open ? 1000000000 : info.spec.max;
  commit(() => {
    pt.value = Math.max(lo, Math.min(hi, v));
  });
}

/** Pointer down in the automation section (content coordinates). */
/** function onAutoDown(e: Ev, lg: LaneGeo, x: Number, y: Number) => Undefined */
export function onAutoDown(e, lg, x, y) {
  const rel = y - lg.top;
  if (rel < DIV_H) {
    if (e.button === 0) {
      auto.collapsed = !auto.collapsed;
      invalidate();
    }
    return undefined;
  }
  const lanes = state.project.automation;
  const k = Math.floor((rel - DIV_H) / LANE_H);
  if (auto.collapsed || k < 0 || k >= lanes.length) return undefined;
  const lane = lanes[k];
  const info = targetInfo(lane.target);
  const r = displayRange(info, lane);
  const top = lg.top + laneTop(k);
  const boxes = laneBoxes(lane, r, lg.zoom, top);
  const hit = hitPoint(boxes, x, y);
  selectLane(lane);

  if (e.button === 2) {
    if (hit >= 0) deletePoint(lane, hit);
    return undefined;
  }
  if (e.button !== 0) return undefined;
  if (e.altKey) {
    const idx = hit > 0 ? hit : segmentEnd(lane, x / lg.zoom);
    if (idx > 0) dragCurve(e, lane, idx, r, info);
    return undefined;
  }
  if (hit >= 0) {
    if (e.shiftKey) {
      if (!isSelPoint(lane, hit)) auto.points = auto.points.concat([pointIx(hit)]);
    } else auto.points = [pointIx(hit)];
    reportContext();
    dragPoint(e, lane, hit, r, info, lg.zoom, false);
    invalidate();
    return undefined;
  }

  // Empty lane area: add a point and keep dragging it.
  let beat = Math.max(0, x / lg.zoom);
  if (!e.shiftKey) beat = Math.max(0, snapTo(beat, state.snap));
  const value = tidy(info, valueOf(r, 1 - (y - top - PAD) / (LANE_H - 2 * PAD)));
  let idx = 0;
  while (idx < lane.points.length && lane.points[idx].beat <= beat) idx = idx + 1;
  begin();
  lane.points.splice(idx, 0, { beat: beat, value: value, curve: 0 });
  auto.points = [pointIx(idx)];
  changed(true);
  reportContext();
  dragPoint(e, lane, idx, r, info, lg.zoom, true);
}

/** Double-click in the automation section: type a point's value. */
/** function onAutoDblClick(e: Ev, lg: LaneGeo, x: Number, y: Number) => Undefined */
export function onAutoDblClick(e, lg, x, y) {
  const rel = y - lg.top;
  const lanes = state.project.automation;
  const k = Math.floor((rel - DIV_H) / LANE_H);
  if (rel < DIV_H || auto.collapsed || k < 0 || k >= lanes.length) return undefined;
  const lane = lanes[k];
  const info = targetInfo(lane.target);
  const hit = hitPoint(laneBoxes(lane, displayRange(info, lane), lg.zoom, lg.top + laneTop(k)), x, y);
  if (hit >= 0) typeValue(lane, hit, info);
}

/** Hint-bar text for the pointer over the automation section. */
/** function autoHint(lg: LaneGeo, x: Number, y: Number) => String */
export function autoHint(lg, x, y) {
  const rel = y - lg.top;
  if (rel < DIV_H)
    return auto.collapsed
      ? t("lanes.divider.collapsed.hint")
      : t("lanes.divider.expanded.hint");
  const lanes = state.project.automation;
  const k = Math.floor((rel - DIV_H) / LANE_H);
  if (auto.collapsed || k < 0 || k >= lanes.length) return "";
  const lane = lanes[k];
  const info = targetInfo(lane.target);
  const r = displayRange(info, lane);
  const boxes = laneBoxes(lane, r, lg.zoom, lg.top + laneTop(k));
  const hit = hitPoint(boxes, x, y);
  const tp = state.project.transport;
  if (hit >= 0) {
    const pt = lane.points[hit];
    return tf("lanes.point.hover.hint", [
      info.label,
      formatValue(info, pt.value),
      barBeat(pt.beat, tp),
    ]);
  }
  const beat = x / lg.zoom;
  return tf("lanes.lane.hover.hint", [
    info.label,
    formatValue(info, laneValueAt(lane.points, beat)),
    barBeat(beat, tp),
  ]);
}

// ------------------------------------------------------------------ render

/** Headers of the automation section (inside the playlist's track-header column). */
/** function autoHeads(b: Builder, top: Number) => Undefined */
export function autoHeads(b, top) {
  const lanes = state.project.automation;
  b.open("div", "auto-divh", auto.collapsed ? "auto-divhead folded" : "auto-divhead");
  b.style("top", `${top}px`);
  b.style("height", `${DIV_H}px`);
  b.on("click", (e) => {
    auto.collapsed = !auto.collapsed;
    invalidate();
  });
  b.leaf("span", "chev", "auto-chev", "▾");
  b.leaf("span", "t", "auto-divtitle", t("lanes.divider.label"));
  b.leaf("span", "n", "auto-count", String(lanes.length));
  b.close();
  if (auto.collapsed) return undefined;

  const at = state.mode === "song" ? state.position : 0;
  for (let k = 0; k < lanes.length; k++) {
    const lane = lanes[k];
    const info = targetInfo(lane.target);
    let cls = "auto-head";
    if (auto.lane === lane.id) cls = `${cls} sel`;
    if (lane.mute) cls = `${cls} muted`;
    if (!info.ok) cls = `${cls} bad`;
    b.open("div", `ah-${lane.id}`, cls);
    b.style("top", `${top + laneTop(k)}px`);
    b.style("height", `${LANE_H}px`);
    b.style("--c", lane.color);
    b.on("pointerdown", (e) => {
      selectLane(lane);
    });

    b.open("div", "r1", "auto-row");
    b.leaf("i", "sw", "swatch", "");
    b.style("--c", lane.color);
    b.style("cursor", "pointer");
    b.attr("title", t("lanes.lane.color.title"));
    b.on("click", (e) => {
      const at2 = PALETTE.indexOf(lane.color);
      commit(() => {
        lane.color = PALETTE[(at2 + 1) % PALETTE.length];
      });
    });
    b.leaf("span", "name", "auto-name", lane.name !== "" ? lane.name : lane.id);
    b.attr("title", tf("lanes.lane.name.title", [lane.name]));
    b.on("dblclick", (e) => {
      const name = promptBox(t("lanes.lane.rename.prompt"), lane.name);
      if (name !== "") {
        commit(() => {
          lane.name = name;
        });
      }
    });
    b.close();

    b.leaf("div", "target", "auto-target", lane.target);
    b.attr("title", info.ok ? lane.target : tf("lanes.lane.target.missing.title", [lane.target]));

    b.open("div", "r2", "auto-row");
    b.leaf("span", "val", "auto-val", info.ok && lane.points.length > 0 ? formatValue(info, laneValueAt(lane.points, at)) : "—");
    b.attr("title", t("lanes.lane.value.title"));
    b.leaf("div", "mute", lane.mute ? "ch-mute off" : "ch-mute", "");
    b.attr("title", lane.mute ? t("lanes.lane.unmute.title") : t("lanes.lane.mute.title"));
    b.on("click", (e) => {
      e.stopPropagation();
      commit(() => {
        lane.mute = !lane.mute;
      });
    });
    iconButton(b, "del", "small ghost danger", "trash", t("lanes.lane.delete.title"), () => {
      removeLane(lane.id);
    });
    b.close();
    b.close();
  }
}

/** Draw a lane's curve over the visible range (canvas coordinates). */
/** function paintCurve(g2: Ctx, w: Number, h: Number, lane: AutomationLane, r: VRange, zoom: Number, x0: Number) => Undefined */
function paintCurve(g2, w, h, lane, r, zoom, x0) {
  const pts = lane.points;
  const n = pts.length;
  if (n === 0) return undefined;
  /** function X(beat: Number) => Number */
  function X(beat) {
    return beat * zoom - x0;
  }
  /** function Y(v: Number) => Number */
  function Y(v) {
    return yOf(unitOf(r, v));
  }
  /** function trace() => Undefined */
  function trace() {
    g2.beginPath();
    g2.moveTo(-2, Y(laneValueAt(pts, x0 / zoom)));
    for (let j = 1; j < n; j++) {
      const a = pts[j - 1];
      const c = pts[j];
      const xa = X(a.beat);
      const xc = X(c.beat);
      if (xc < -2) continue;
      if (xa > w + 2) break;
      if (xa >= -2) g2.lineTo(xa, Y(a.value));
      const span = c.beat - a.beat;
      if (span > 0) {
        for (let x = Math.max(xa, -2) + 3; x < Math.min(xc, w + 2); x = x + 3) {
          const t = ((x + x0) / zoom - a.beat) / span;
          g2.lineTo(x, Y(a.value + (c.value - a.value) * curveShape(t, c.curve)));
        }
      }
      if (xc <= w + 2) g2.lineTo(xc, Y(c.value));
    }
    g2.lineTo(w + 2, Y(laneValueAt(pts, (x0 + w + 2) / zoom)));
  }
  const alpha = lane.mute ? 0.35 : 1;
  trace();
  g2.lineTo(w + 2, h);
  g2.lineTo(-2, h);
  g2.closePath();
  const grad = g2.createLinearGradient(0, 0, 0, h);
  grad.addColorStop(0, rgba(lane.color, 0.34 * alpha));
  grad.addColorStop(1, rgba(lane.color, 0.03 * alpha));
  g2.fillGradient(grad);
  g2.fill();
  trace();
  g2.globalAlpha = alpha;
  g2.strokeStyle = lane.color;
  g2.lineWidth = 1.8;
  g2.lineJoin = "round";
  g2.shadowColor = lane.color;
  g2.shadowBlur = 6;
  g2.stroke();
}

/** Lane rows in the playlist content (below the tracks). */
/** function autoBody(b: Builder, lg: LaneGeo) => Undefined */
export function autoBody(b, lg) {
  const lanes = state.project.automation;
  b.leaf("div", "auto-div", auto.collapsed ? "auto-divider folded" : "auto-divider", "");
  b.style("top", `${lg.top}px`);
  b.style("height", `${DIV_H}px`);
  if (auto.collapsed) return undefined;
  const tp = state.project.transport;
  const left = Math.max(0, lg.x0);
  const width = Math.max(1, lg.x1 - left);
  for (let k = 0; k < lanes.length; k++) {
    const lane = lanes[k];
    const info = targetInfo(lane.target);
    const r = displayRange(info, lane);
    const top = lg.top + laneTop(k);
    let cls = "auto-lane";
    if (auto.lane === lane.id) cls = `${cls} sel`;
    if (lane.mute) cls = `${cls} muted`;
    b.open("div", `al-${lane.id}`, cls);
    b.style("top", `${top}px`);
    b.style("height", `${LANE_H}px`);
    b.style("--c", lane.color);
    b.canvas("curve", "auto-curve", (g2, w, h) => paintCurve(g2, w, h, lane, r, lg.zoom, left));
    b.style("left", `${left}px`);
    b.style("width", `${width}px`);
    for (const box of laneBoxes(lane, r, lg.zoom, top)) {
      if (box.x < lg.x0 - 8 || box.x > lg.x1 + 8) continue;
      const k2 = pointIndex(box.i);
      const sel = isSelPoint(lane, k2);
      b.leaf("div", `p${k2}`, sel ? "auto-pt sel" : "auto-pt", "");
      b.style("left", `${box.x}px`);
      b.style("top", `${box.y - top}px`);
      if (sel) {
        const pt = lane.points[k2];
        b.leaf("div", `tag${k2}`, box.y - top < LANE_H / 2 ? "auto-tag below" : "auto-tag", `${formatValue(info, pt.value)} · ${barBeat(pt.beat, tp)}`);
        b.style("left", `${box.x}px`);
        b.style("top", `${box.y - top}px`);
      }
    }
    b.close();
  }
}

// ------------------------------------------------------------------ menu

/** function menuItem(b: Builder, key: String, icon: String, label: String, onClick: () => Undefined) => Undefined */
function menuItem(b, key, icon, label, onClick) {
  b.open("button", key, "auto-menu-item");
  b.on("click", (e) => {
    closeMenu();
    onClick();
  });
  glyph(b, icon);
  b.leaf("span", "l", "", label);
  b.close();
}

/** The right-click menu of automatable controls (rendered by the shell). */
/** function automationMenu(b: Builder) => Undefined */
export function automationMenu(b) {
  // A stable container: changing the shell's own children would re-append
  // (and so reset the scroll of) every panel.
  b.open("div", "auto-menu-root", "auto-menu-root");
  automationMenuBody(b);
  b.close();
}

/** function automationMenuBody(b: Builder) => Undefined */
function automationMenuBody(b) {
  const m = auto.menu;
  if (!m.open) return undefined;
  const target = m.target;
  const info = targetInfo(target);
  const lane = laneByTarget(target);
  b.leaf("div", "auto-backdrop", "auto-backdrop", "");
  b.on("pointerdown", (e) => {
    e.preventDefault();
    closeMenu();
  });
  b.on("contextmenu", (e) => {
    e.preventDefault();
    closeMenu();
  });
  b.open("div", "auto-menu", "auto-menu");
  b.style("left", `min(${m.x}px, calc(100vw - 250px))`);
  b.style("top", `min(${m.y}px, calc(100vh - 170px))`);
  b.on("contextmenu", (e) => {
    e.preventDefault();
  });
  b.leaf("div", "t", "auto-menu-title", info.label);
  b.leaf("div", "s", "auto-menu-sub", target);
  if (!info.ok) {
    b.leaf("div", "x", "auto-menu-note", t("lanes.menu.notAutomatable"));
  } else if (!lane) {
    menuItem(b, "create", "draw", t("lanes.menu.create.label"), () => createLane(target));
  } else {
    const id = lane.id;
    const muted = lane.mute;
    menuItem(b, "go", "playlist", t("lanes.menu.goTo.label"), () => goToLane(id));
    menuItem(b, "mute", "mute", muted ? t("lanes.menu.unmute.label") : t("lanes.menu.mute.label"), () => {
      commit(() => {
        for (const l of state.project.automation) if (l.id === id) l.mute = !muted;
      });
    });
    menuItem(b, "rm", "trash", t("lanes.menu.remove.label"), () => removeLane(id));
  }
  b.close();
}
