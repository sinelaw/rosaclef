// Sheet music, part 2: engraving — the notation from `notation.js` placed on
// the page, the way engravers lay out a score:
//
//  - Spacing. Every moment where a note or rest starts in any staff is a
//    column, shared by all staves of a system. The space after a column grows
//    with the time until the next one, slowly (≈ duration^0.65): a spring.
//    What the column's symbols need (accidentals, noteheads, dots, flags) is a
//    rod: a minimum the spring may not shrink below.
//  - Line breaking. Systems are chosen for the whole piece at once (dynamic
//    programming over the measures, the way Knuth–Plass breaks paragraphs):
//    the cost of a system is how far it must stretch or squeeze. Each system
//    is then justified: one stretch factor for all of its springs, found by
//    bisection, so that it fills the line exactly.
//  - Symbols. Bravura glyphs (SMUFL) for clefs, noteheads, rests, flags,
//    accidentals and time signatures; stems, beams, ledger lines, bar lines
//    and ties are drawn with Bravura's engraving defaults (staff line 0.13
//    spaces, stem 0.12, beam 0.5 …). Beams follow the notes with a limited
//    slope snapped to quarter spaces and keep every stem at least 3¼ spaces;
//    accidentals in chords stack in zig-zag columns; seconds put a notehead
//    on the other side of the stem; ties curve away from the stems.
//  - Vertical spacing. Each staff's ink is measured (its skyline, reduced to
//    its top and bottom), and staves are pushed apart until nothing collides.
//
// Units are staff spaces (the distance between two staff lines); y grows
// downwards from a staff's top line. The result is plain data: each system
// is a few filled paths and glyph runs per color, plus what the view needs
// to hit-test notes and to map time to x.

import { G } from "./smufl.js";
import { NO_ACC, bottomStep, TPQ } from "./notation.js";

// Bravura's engraving defaults, in staff spaces.
const STAFF_LINE = 0.13;
const STEM = 0.12;
const BEAM = 0.5;
const BEAM_STEP = 0.75;
const LEDGER = 0.16;
const LEDGER_EXT = 0.4;
const THIN = 0.16;
const THICK = 0.5;
const STEM_LEN = 3.5;
/** Space between systems. */
const SYSTEM_GAP = 2.5;
/** The run holding the glints of the wet ink (drawn in light over the music, not in ink). */
export const GLOSS = "gloss";
/** The broad, faint sheen around the glints. */
export const SHEEN = "sheen";
/** The SMuFL stem anchor: stems meet a notehead this far from its middle. */
const STEM_Y = 0.168;

/** type Prim = { kind: Int, color: String, nums: Number[], ch: String } */
/** type Ink = { color: String, d: String, text: String, xs: String, ys: String } */
/** type Label = { x: Number, y: Number, text: String, cls: String, anchor: String, color: String } */
/** A brace: its glyph's origin (bottom left) and its scale (one staff high is 1). */
/** type Brace = { x: Number, y: Number, s: Number } */
/** type Band = { x: Number, y: Number, w: Number, h: Number, color: String, label: String, mark: Int, first: Boolean } */
/** A notehead on the page (system coordinates): what a click on it edits. */
/** type HeadBox = { x: Number, y: Number, w: Number, src: Int, staff: Int, glyph: String } */
/** type TimePt = { x: Number, tick: Int } */
/** A staff in a system: its top line's y and how to read pitches off it. */
/** type Row = { staff: Int, y: Number, clef: String, drum: Boolean } */
/** type Sys = { top: Number, height: Number, inks: Ink[], labels: Label[], braces: Brace[], bands: Band[], heads: HeadBox[], times: TimePt[], rows: Row[], first: Int, last: Int, start: Int, end: Int, x0: Number, x1: Number } */
/** type Page = { width: Number, height: Number, systems: Sys[] } */
/** `width`: of the page, in staff spaces; `hideEmpty`: leave out staves that only rest in a system. */
/** type EngraveOpts = { width: Number, hideEmpty: Boolean } */

// ------------------------------------------------------------------ painter

/** A list of primitives with their vertical extent; shifted into place later. */
/** type Inker = { prims: Prim[], top: Number, bottom: Number } */

/** function painter() => Inker */
function painter() {
  return { prims: [], top: 0, bottom: 4 };
}

/** function extend(p: Inker, y0: Number, y1: Number) => Undefined */
function extend(p, y0, y1) {
  if (y0 < p.top) p.top = y0;
  if (y1 > p.bottom) p.bottom = y1;
}

/** function rect(p: Inker, x: Number, y: Number, w: Number, h: Number, color: String) => Undefined */
function rect(p, x, y, w, h, color) {
  p.prims.push({ kind: 0, color: color, nums: [x, y, x + w, y, x + w, y + h, x, y + h], ch: "" });
  extend(p, y, y + h);
}

/** function quad(p: Inker, x0: Number, y0: Number, x1: Number, y1: Number, t: Number, color: String) => Undefined */
function quad(p, x0, y0, x1, y1, t, color) {
  p.prims.push({ kind: 0, color: color, nums: [x0, y0, x1, y1, x1, y1 + t, x0, y0 + t], ch: "" });
  extend(p, Math.min(y0, y1), Math.max(y0, y1) + t);
}

/** function glyph(p: Inker, g: Glyph, x: Number, y: Number, color: String) => Undefined */
function glyph(p, g, x, y, color) {
  p.prims.push({ kind: 1, color: color, nums: [x, y], ch: g.c });
  extend(p, y + g.top, y + g.bottom);
}

/** The gloss of a wet drop of ink: a small ellipse (centre, radii, tilt in degrees) where the light catches it. */
/** function glint(p: Inker, run: String, cx: Number, cy: Number, rx: Number, ry: Number, deg: Number) => Undefined */
function glint(p, run, cx, cy, rx, ry, deg) {
  p.prims.push({ kind: 4, color: run, nums: [cx, cy, rx, ry, deg], ch: "" });
}

/** Where the light catches a notehead: a broad soft sheen over the upper left of a
 * filled one with a small highlight at its heart; on a hollow one, the highlight on its rim. */
/** function headGlint(p: Inker, gl: Glyph, x: Number, y: Number) => Undefined */
function headGlint(p, gl, x, y) {
  if (gl.c === G.noteheadBlack.c) {
    glint(p, SHEEN, x + 0.45, y - 0.14, 0.34, 0.15, -22);
    glint(p, GLOSS, x + 0.37, y - 0.23, 0.1, 0.045, -22);
  } else if (gl.c === G.noteheadHalf.c) glint(p, GLOSS, x + 0.3, y - 0.32, 0.1, 0.035, -25);
  else if (gl.c === G.noteheadWhole.c) glint(p, GLOSS, x + 0.45, y - 0.37, 0.12, 0.035, -12);
}

/** A tie or slur: a crescent between two points bulging by `h` (negative: up). */
/** function tie(p: Inker, x0: Number, x1: Number, y: Number, h: Number, color: String) => Undefined */
function tie(p, x0, x1, y, h, color) {
  const len = x1 - x0;
  if (len < 0.3) return undefined;
  const a = x0 + len * 0.25;
  const b = x0 + len * 0.75;
  // Control points 4/3 of the bulge give the curve its height.
  const outer = y + h * 1.333;
  const inner = y + (h - Math.sign(h) * 0.17) * 1.333;
  p.prims.push({ kind: 2, color: color, nums: [x0, y, a, outer, b, outer, x1, y, b, inner, a, inner, x0, y], ch: "" });
  extend(p, Math.min(y, y + h), Math.max(y, y + h));
}

/** function num(v: Number) => String */
function num(v) {
  return String(Math.round(v * 100) / 100);
}

/** Primitives shifted down by `dy`, appended to per-color runs. */
/** type Run = { color: String, d: String[], text: String[], xs: String[], ys: String[] } */

/** function runFor(runs: Run[], color: String) => Run */
function runFor(runs, color) {
  const found = runs.find((r) => r.color === color);
  if (found) return found;
  const r = { color: color, d: [], text: [], xs: [], ys: [] };
  runs.push(r);
  return r;
}

/** function emit(runs: Run[], prims: Prim[], dy: Number) => Undefined */
function emit(runs, prims, dy) {
  for (const pr of prims) {
    const r = runFor(runs, pr.color);
    const n = pr.nums;
    if (pr.kind === 1) {
      r.text.push(pr.ch);
      r.xs.push(num(n[0]));
      r.ys.push(num(n[1] + dy));
    } else if (pr.kind === 4) {
      // An ellipse: two arcs between the ends of its tilted long axis.
      const a = (n[4] * Math.PI) / 180;
      const ex = n[2] * Math.cos(a);
      const ey = n[2] * Math.sin(a);
      const arc = `A${num(n[2])} ${num(n[3])} ${num(n[4])} 1 0`;
      r.d.push(`M${num(n[0] - ex)} ${num(n[1] - ey + dy)}${arc} ${num(n[0] + ex)} ${num(n[1] + ey + dy)}${arc} ${num(n[0] - ex)} ${num(n[1] - ey + dy)}Z`);
    } else if (pr.kind === 0) {
      let s = `M${num(n[0])} ${num(n[1] + dy)}`;
      for (let i = 2; i + 1 < n.length; i = i + 2) s = `${s}L${num(n[i])} ${num(n[i + 1] + dy)}`;
      r.d.push(`${s}Z`);
    } else {
      r.d.push(
        `M${num(n[0])} ${num(n[1] + dy)}C${num(n[2])} ${num(n[3] + dy)} ${num(n[4])} ${num(n[5] + dy)} ${num(n[6])} ${num(n[7] + dy)}` +
          `C${num(n[8])} ${num(n[9] + dy)} ${num(n[10])} ${num(n[11] + dy)} ${num(n[12])} ${num(n[13] + dy)}Z`
      );
    }
  }
}

/** function inks(runs: Run[]) => Ink[] */
function inks(runs) {
  return runs.map((r) => ({ color: r.color, d: r.d.join(""), text: r.text.join(""), xs: r.xs.join(" "), ys: r.ys.join(" ") }));
}

// ------------------------------------------------------------------ glyphs

/** function headGlyph(ev: NEv, h: Head) => Glyph */
function headGlyph(ev, h) {
  if (h.head === "x") return ev.base >= 192 ? G.noteheadXWhole : ev.base >= 96 ? G.noteheadXHalf : G.noteheadXBlack;
  if (h.head === "o") return G.noteheadCircleX;
  if (ev.base >= 192) return G.noteheadWhole;
  if (ev.base >= 96) return G.noteheadHalf;
  return G.noteheadBlack;
}

/** function restGlyph(base: Int) => Glyph */
function restGlyph(base) {
  if (base >= 192) return G.restWhole;
  if (base >= 96) return G.restHalf;
  if (base >= 48) return G.restQuarter;
  if (base >= 24) return G.rest8th;
  if (base >= 12) return G.rest16th;
  return G.rest32nd;
}

/** function accGlyph(a: Int) => Glyph */
function accGlyph(a) {
  if (a === 1) return G.accidentalSharp;
  if (a === -1) return G.accidentalFlat;
  if (a === 2) return G.accidentalDoubleSharp;
  if (a === -2) return G.accidentalDoubleFlat;
  return G.accidentalNatural;
}

/** function flagGlyph(base: Int, up: Boolean) => Glyph */
function flagGlyph(base, up) {
  if (base >= 24) return up ? G.flag8thUp : G.flag8thDown;
  if (base >= 12) return up ? G.flag16thUp : G.flag16thDown;
  return up ? G.flag32ndUp : G.flag32ndDown;
}

/** Beams a value has (an eighth one, a sixteenth two). */
/** function levels(base: Int) => Int */
function levels(base) {
  if (base >= 48) return 0;
  if (base >= 24) return 1;
  if (base >= 12) return 2;
  return 3;
}

const DIGITS = [G.timeSig0, G.timeSig1, G.timeSig2, G.timeSig3, G.timeSig4, G.timeSig5, G.timeSig6, G.timeSig7, G.timeSig8, G.timeSig9];

/** function digitsWidth(n: Int) => Number */
function digitsWidth(n) {
  let w = 0;
  for (const ch of String(n).split("")) w = w + DIGITS[Math.round(Number(ch))].x1 + 0.05;
  return w;
}

/** function timeSigWidth(m: Measure) => Number */
function timeSigWidth(m) {
  return Math.max(digitsWidth(m.num), digitsWidth(m.den));
}

/** function drawDigits(p: Inker, n: Int, cx: Number, y: Number, color: String) => Undefined */
function drawDigits(p, n, cx, y, color) {
  let x = cx - digitsWidth(n) / 2;
  for (const ch of String(n).split("")) {
    const g = DIGITS[Math.round(Number(ch))];
    glyph(p, g, x, y, color);
    x = x + g.x1 + 0.05;
  }
}

// Key signature positions (y from the top line) on a treble staff.
const SHARP_Y = [0, 1.5, -0.5, 1, 2.5, 0.5, 2];
const FLAT_Y = [2, 0.5, 2.5, 1, 3, 1.5, 3.5];

/** function keyShift(clef: String) => Number */
function keyShift(clef) {
  if (clef === "bass") return 1;
  if (clef === "alto") return 0.5;
  return 0;
}

/** function keyWidth(fifths: Int) => Number */
function keyWidth(fifths) {
  if (fifths === 0) return 0;
  const g = fifths > 0 ? G.accidentalSharp : G.accidentalFlat;
  return Math.abs(fifths) * (g.x1 + 0.12);
}

// ------------------------------------------------------------------ geometry

/** Per-event geometry that does not depend on x: stem direction, staff y of each head. */
/** type EvGeo = { up: Boolean, ys: Number[], left: Number, right: Number } */

/** The staff y of a step: the top line is 0, each step half a space down. */
/** function stepY(clef: String, step: Int) => Number */
export function stepY(clef, step) {
  return (bottomStep(clef) + 8 - step) / 2;
}

/** function stemUp(ys: Number[]) => Boolean */
function stemUp(ys) {
  let hi = 99;
  let lo = -99;
  for (const y of ys) {
    if (y < hi) hi = y;
    if (y > lo) lo = y;
  }
  // The note farthest from the middle line decides; on it, stems go down.
  return lo - 2 > 2 - hi;
}

/** Accidental columns for a chord: zig-zag from the outside notes in, each in the first column it does not collide in. */
/** type AccPos = { head: Int, col: Int } */

/** function accColumns(ev: NEv, ys: Number[]) => AccPos[] */
function accColumns(ev, ys) {
  /** const order: Int[] */
  const order = [];
  /** const marked: Int[] */
  const marked = [];
  for (let i = ev.heads.length - 1; i >= 0; i--) if (ev.heads[i].acc !== NO_ACC) marked.push(i);
  let a = 0;
  let b = marked.length - 1;
  while (a <= b) {
    order.push(marked[a]);
    if (a !== b) order.push(marked[b]);
    a = a + 1;
    b = b - 1;
  }
  /** const out: AccPos[] */
  const out = [];
  for (const i of order) {
    let col = 0;
    while (out.some((o) => o.col === col && Math.abs(ys[o.head] - ys[i]) < 3)) col = col + 1;
    out.push({ head: i, col: col });
  }
  return out;
}

/** Whether the heads have a second (adjacent steps): a head goes on the other side of the stem. */
/** function hasSecond(ev: NEv) => Boolean */
function hasSecond(ev) {
  for (let i = 1; i < ev.heads.length; i++) if (ev.heads[i].step - ev.heads[i - 1].step === 1) return true;
  return false;
}

/** Geometry of every event of a staff; beamed groups share a stem direction. */
/** function staffGeo(st: Staff) => EvGeo[] */
function staffGeo(st) {
  /** const out: EvGeo[] */
  const out = [];
  for (const ev of st.events) {
    const ys = ev.heads.map((h) => stepY(st.clef, h.step));
    out.push({ up: st.drum ? true : stemUp(ys), ys: ys, left: 0, right: 0 });
  }
  if (!st.drum) {
    let k = 0;
    while (k < st.events.length) {
      const id = st.events[k].beam;
      if (id < 0) {
        k = k + 1;
        continue;
      }
      let j = k;
      let sum = 0;
      let far = 0;
      while (j < st.events.length && st.events[j].beam === id) {
        for (const y of out[j].ys) {
          sum = sum + (y - 2);
          if (Math.abs(y - 2) > Math.abs(far)) far = y - 2;
        }
        j = j + 1;
      }
      const up = Math.abs(far) > 2.5 ? far > 0 : sum > 0;
      for (let i = k; i < j; i++) out[i].up = up;
      k = j;
    }
  }
  // Extents left and right of the column (for spacing rods).
  for (let i = 0; i < st.events.length; i++) {
    const ev = st.events[i];
    const g = out[i];
    if (ev.rest) {
      g.left = 0;
      g.right = ev.whole ? 0 : restGlyph(ev.base).x1 + (ev.dots > 0 ? 0.7 : 0);
      continue;
    }
    const hw = ev.heads.length > 0 ? headGlyph(ev, ev.heads[0]).x1 : 1.18;
    const second = hasSecond(ev);
    let right = hw + (second && (g.up || ev.base >= 192) ? hw - STEM : 0) + ev.dots * 0.55 + (ev.dots > 0 ? 0.25 : 0);
    if (g.up && ev.beam < 0 && levels(ev.base) > 0) right = Math.max(right, hw + 1.05);
    let left = second && !g.up && ev.base < 192 ? hw - STEM : 0;
    const accs = accColumns(ev, g.ys);
    let cols = 0;
    for (const a of accs) cols = Math.max(cols, a.col + 1);
    if (cols > 0) left = left + cols * 1.12 + 0.15;
    g.left = left;
    g.right = right;
  }
  return out;
}

// ------------------------------------------------------------------ spacing

/** The ideal space after a column, from the time to the next one. */
/** function spring(ticks: Int) => Number */
function spring(ticks) {
  return 1.4 + 2.5 * Math.pow(ticks / TPQ, 0.65);
}

/** One column of a measure: its tick, spring and rod to the next, left extent. */
/** type Col = { tick: Int, spring: Number, rod: Number, left: Number } */
/** type MCols = { cols: Col[], lead: Number, meterW: Number } */

/** Columns of every measure, over all staves. */
/** function measureCols(sc: Score, geos: EvGeo[][]) => MCols[] */
function measureCols(sc, geos) {
  /** const ticks: Int[][] */
  const ticks = sc.measures.map((_) => []);
  /** const lefts: Number[][] */
  const lefts = sc.measures.map((_) => []);
  /** const rights: Number[][] */
  const rights = sc.measures.map((_) => []);
  for (let s = 0; s < sc.staves.length; s++) {
    const evs = sc.staves[s].events;
    for (let i = 0; i < evs.length; i++) {
      const ev = evs[i];
      const t = ticks[ev.measure];
      let k = t.indexOf(ev.start);
      if (k < 0) {
        t.push(ev.start);
        lefts[ev.measure].push(0);
        rights[ev.measure].push(0);
        k = t.length - 1;
      }
      lefts[ev.measure][k] = Math.max(lefts[ev.measure][k], geos[s][i].left);
      rights[ev.measure][k] = Math.max(rights[ev.measure][k], geos[s][i].right);
    }
  }
  /** const out: MCols[] */
  const out = [];
  for (let m = 0; m < sc.measures.length; m++) {
    const ms = sc.measures[m];
    const order = ticks[m].map((t) => t).sort((a, b) => a - b);
    if (order.length === 0) order.push(ms.start);
    /** const cols: Col[] */
    const cols = [];
    for (let k = 0; k < order.length; k++) {
      const at = ticks[m].indexOf(order[k]);
      const next = k + 1 < order.length ? order[k + 1] : ms.start + ms.length;
      const nextAt = k + 1 < order.length ? ticks[m].indexOf(order[k + 1]) : -1;
      const right = at >= 0 ? rights[m][at] : 0;
      const nextLeft = nextAt >= 0 ? lefts[m][nextAt] : 0;
      // The last column's rod reaches the bar line; a bar of one rest is never a sliver.
      const rod = k + 1 < order.length ? right + nextLeft + 0.3 : Math.max(right + 1.1, order.length === 1 ? 7 : 0);
      cols.push({ tick: order[k], spring: spring(next - order[k]), rod: rod, left: at >= 0 ? lefts[m][at] : 0 });
    }
    // A multi-measure rest gets a fixed, generous width.
    if (ms.count > 1) {
      cols[0].spring = 11;
      cols[0].rod = 11;
    }
    const meterW = ms.meter && m > 0 ? timeSigWidth(ms) + 1.2 : 0;
    out.push({ cols: cols, lead: 1.1 + cols[0].left, meterW: meterW });
  }
  return out;
}

/** Width of a measure at stretch `s` (its meter shows unless it starts the system, where the header has it). */
/** function measureWidth(mc: MCols, s: Number, atStart: Boolean) => Number */
function measureWidth(mc, s, atStart) {
  let w = mc.lead + (atStart ? 0 : mc.meterW);
  for (const c of mc.cols) w = w + Math.max(c.rod, c.spring * s);
  return w;
}

// ------------------------------------------------------------------ systems

/** function shortName(name: String) => String */
function shortName(name) {
  const w = name.split(" ")[0];
  return w.length <= 5 ? w : `${w.slice(0, 4)}.`;
}

/** Rough width of a staff name at 1.5 spaces (a serif face). */
/** function nameWidth(name: String) => Number */
function nameWidth(name) {
  return Math.min(24, name.length) * 0.68;
}

/** Width of a system's start: names, clef, key and (when it changes or first) the time signature. */
/** function headerWidth(sc: Score, m: Int, indent: Number) => Number */
function headerWidth(sc, m, indent) {
  const ms = sc.measures[m];
  let w = indent + 0.7 + 2.9 + 0.6;
  const kw = keyWidth(sc.fifths);
  if (kw > 0) w = w + kw + 0.5;
  if (ms.meter) w = w + timeSigWidth(ms) + 0.8;
  return w;
}

/** function indentFor(sc: Score, first: Boolean) => Number */
function indentFor(sc, first) {
  let w = 0;
  for (const st of sc.staves) {
    if (st.part !== 0) continue;
    w = Math.max(w, nameWidth(first ? st.name : shortName(st.name)));
  }
  const groups = sc.staves.filter((st) => st.part === 0).length;
  const brace = sc.staves.some((st) => st.part === 1);
  return w + (w > 0 ? 1.4 : 0) + (brace ? 2.1 : groups > 1 ? 1.6 : 0.4);
}

/** Choose the systems: measures [from, to) for each, minimizing how much they stretch or squeeze. */
/** function breakLines(sc: Score, mcs: MCols[], width: Number) => Int[] */
function breakLines(sc, mcs, width) {
  const n = mcs.length;
  /** const best: Number[] */
  const best = [0];
  /** const prev: Int[] */
  const prev = [0];
  const indent0 = indentFor(sc, true);
  const indent = indentFor(sc, false);
  for (let j = 1; j <= n; j++) {
    best.push(Infinity);
    prev.push(j - 1);
  }
  for (let i = 0; i < n; i++) {
    if (best[i] === Infinity) continue;
    const avail = width - headerWidth(sc, i, i === 0 ? indent0 : indent);
    let natural = 0;
    let tight = 0;
    for (let j = i + 1; j <= n && j - i <= 48; j++) {
      natural = natural + measureWidth(mcs[j - 1], 1, j - 1 === i);
      tight = tight + measureWidth(mcs[j - 1], 0.72, j - 1 === i);
      if (tight > avail && j - i > 1) break;
      const r = avail / Math.max(1, natural);
      let cost = r >= 1 ? (r - 1) * (r - 1) * 60 : (1 - r) * (1 - r) * 260;
      if (j === n && r >= 1) cost = 0;
      cost = cost + 3;
      if (best[i] + cost < best[j]) {
        best[j] = best[i] + cost;
        prev[j] = i;
      }
    }
  }
  /** const breaks: Int[] */
  const breaks = [];
  let j = n;
  while (j > 0) {
    breaks.push(j);
    j = prev[j];
  }
  breaks.push(0);
  return breaks.reverse();
}

/** The stretch factor that makes measures [a, b) fill `avail`. */
/** function stretchFor(mcs: MCols[], a: Int, b: Int, avail: Number) => Number */
function stretchFor(mcs, a, b, avail) {
  let lo = 0.2;
  let hi = 12;
  for (let it = 0; it < 32; it++) {
    const mid = (lo + hi) / 2;
    let w = 0;
    for (let m = a; m < b; m++) w = w + measureWidth(mcs[m], mid, m === a);
    if (w > avail) hi = mid;
    else lo = mid;
  }
  return lo;
}

// ------------------------------------------------------------------ engrave

/** The color of a note: the last mark covering it, or "" (ink). */
/** function noteColor(sc: Score, src: Int) => String */
function noteColor(sc, src) {
  const n = sc.notes[src];
  let c = "";
  for (const m of sc.marks) {
    if (n.start >= m.start - 1e-6 && n.start < m.end - 1e-6 && (m.channels.length === 0 || m.channels.includes(n.channel))) c = m.color;
  }
  return c;
}

/** Lay out a score on a page `opts.width` staff spaces wide. */
/** function engrave(sc: Score, opts: EngraveOpts) => Page */
export function engrave(sc, opts) {
  /** const systems: Sys[] */
  const systems = [];
  if (sc.empty || sc.measures.length === 0) return { width: opts.width, height: 0, systems: systems };
  const geos = sc.staves.map(staffGeo);
  const mcs = measureCols(sc, geos);
  const breaks = breakLines(sc, mcs, opts.width);
  // Each staff's events by measure, for quick slicing.
  /** const firstEv: Int[][] */
  const firstEv = sc.staves.map((st) => {
    /** const idx: Int[] */
    const idx = sc.measures.map((_) => -1);
    for (let i = st.events.length - 1; i >= 0; i--) idx[st.events[i].measure] = i;
    return idx;
  });
  let y = 0;
  for (let k = 0; k + 1 < breaks.length; k++) {
    const sys = engraveSystem(sc, opts, geos, mcs, firstEv, breaks[k], breaks[k + 1], k === 0, k + 2 === breaks.length);
    sys.top = y;
    y = y + sys.height + SYSTEM_GAP;
    systems.push(sys);
  }
  return { width: opts.width, height: y, systems: systems };
}

/** function engraveSystem(sc: Score, opts: EngraveOpts, geos: EvGeo[][], mcs: MCols[], firstEv: Int[][], a: Int, b: Int, first: Boolean, last: Boolean) => Sys */
function engraveSystem(sc, opts, geos, mcs, firstEv, a, b, first, last) {
  const indent = indentFor(sc, first);
  const hdr = headerWidth(sc, a, indent);
  const avail = opts.width - hdr;
  let natural = 0;
  for (let m = a; m < b; m++) natural = natural + measureWidth(mcs[m], 1, m === a);
  const s = last && natural < avail * 0.72 ? 1 : stretchFor(mcs, a, b, avail);
  const startTick = sc.measures[a].start;
  const endTick = sc.measures[b - 1].start + sc.measures[b - 1].length;

  // Which staves show: all, or (condensed) those with notes in this system.
  /** const shown: Int[] */
  const shown = [];
  for (let i = 0; i < sc.staves.length; i++) {
    const st = sc.staves[i];
    if (!opts.hideEmpty) {
      shown.push(i);
      continue;
    }
    let busy = false;
    for (let j = 0; j < sc.staves.length; j++) {
      if (sc.staves[j].group !== st.group) continue;
      for (const ev of sc.staves[j].events) if (!ev.rest && ev.start >= startTick && ev.start < endTick) busy = true;
    }
    if (busy) shown.push(i);
  }
  if (shown.length === 0) for (let i = 0; i < sc.staves.length; i++) shown.push(i);

  // Columns and bar lines along x.
  /** const colX: { tick: Int, x: Number }[] */
  const colX = [];
  /** const bars: Number[] */
  const bars = [];
  /** const meterAt: { x: Number, m: Int }[] */
  const meterAt = [];
  /** const times: TimePt[] */
  const times = [];
  let x = hdr;
  for (let m = a; m < b; m++) {
    const mc = mcs[m];
    const startX = x;
    times.push({ x: m === a ? hdr - 0.6 : x + 0.25, tick: sc.measures[m].start });
    if (m !== a && mc.meterW > 0) {
      meterAt.push({ x: x + 0.7, m: m });
      x = x + mc.meterW;
    }
    x = x + mc.lead;
    for (const c of mc.cols) {
      colX.push({ tick: c.tick, x: x });
      times.push({ x: x + 0.59, tick: c.tick });
      x = x + Math.max(c.rod, c.spring * s);
    }
    bars.push(x);
    if (x <= startX) x = startX + 1;
  }
  times.push({ x: x, tick: endTick });
  const x1 = x;

  /** function colAt(tick: Int) => Number */
  function colAt(tick) {
    let lo = 0;
    let hi = colX.length - 1;
    while (lo < hi) {
      const mid = Math.floor((lo + hi) / 2);
      if (colX[mid].tick < tick) lo = mid + 1;
      else hi = mid;
    }
    return colX[lo].x;
  }

  // ---- each staff, in its own coordinates
  /** const painters: Inker[] */
  const painters = [];
  /** const heads: HeadBox[][] */
  const heads = [];
  for (const si of shown) {
    const st = sc.staves[si];
    const p = painter();
    /** const hb: HeadBox[] */
    const hb = [];
    drawStaff(sc, st, si, geos[si], firstEv[si], a, b, p, hb, colAt, bars, meterAt, mcs, indent, hdr, x1, last && b === sc.measures.length);
    painters.push(p);
    heads.push(hb);
  }

  // ---- stack the staves: at least 6 spaces apart (5 within a grand staff), more if their ink needs it
  /** const rowsY: Number[] */
  const rowsY = [];
  let yy = Math.max(4.5, -painters[0].top + 1.6);
  for (let k = 0; k < shown.length; k++) {
    if (k > 0) {
      const prev = painters[k - 1];
      const same = sc.staves[shown[k]].group === sc.staves[shown[k - 1]].group;
      const gap = Math.max(same ? 5 : 6.5, prev.bottom - 4 + -painters[k].top + 1.4);
      yy = rowsY[k - 1] + 4 + gap;
    }
    rowsY.push(yy);
  }
  const lastY = rowsY[rowsY.length - 1];
  const height = lastY + Math.max(4 + 3.5, painters[painters.length - 1].bottom + 1.8);

  /** const runs: Run[] */
  const runs = [];
  /** const labels: Label[] */
  const labels = [];
  /** const braces: Brace[] */
  const braces = [];
  /** const allHeads: HeadBox[] */
  const allHeads = [];
  /** const rows: Row[] */
  const rows = [];
  for (let k = 0; k < shown.length; k++) {
    emit(runs, painters[k].prims, rowsY[k]);
    for (const h of heads[k]) allHeads.push({ x: h.x, y: h.y + rowsY[k], w: h.w, src: h.src, staff: h.staff, glyph: h.glyph });
    const st = sc.staves[shown[k]];
    rows.push({ staff: shown[k], y: rowsY[k], clef: st.clef, drum: st.drum });
  }

  // ---- system furniture: start line, bar lines through grand staves, brackets, braces, names, numbers
  const sysP = painter();
  const top = rowsY[0];
  const bottom = lastY + 4;
  if (shown.length > 1) rect(sysP, indent - THIN / 2, top, THIN, bottom - top, "");
  let groups = 0;
  let braced = false;
  for (let k = 0; k < shown.length; k++) {
    if (sc.staves[shown[k]].part === 0) groups = groups + 1;
    if (k + 1 < shown.length && sc.staves[shown[k + 1]].group === sc.staves[shown[k]].group) braced = true;
  }
  for (let k = 0; k < shown.length; k++) {
    const st = sc.staves[shown[k]];
    const next = k + 1 < shown.length ? sc.staves[shown[k + 1]] : undefined;
    if (next !== undefined && next.group === st.group) {
      const h = rowsY[k + 1] + 4 - rowsY[k];
      // Bravura's brace is one staff high: it scales as a whole, so it thickens as it grows.
      const bs = h / 4;
      braces.push({ x: indent - 0.35 - G.brace.x1 * bs, y: rowsY[k] + h, s: bs });
      for (let i = 0; i < bars.length; i++) {
        const bx = bars[i];
        if (i === bars.length - 1 && last && b === sc.measures.length) continue;
        rect(sysP, bx - THIN, rowsY[k] + 4, THIN, rowsY[k + 1] - rowsY[k] - 4, "");
      }
    }
    // Names: full on the first system, short after.
    const name = first ? st.name : shortName(st.name);
    if (st.part === 0 && name !== "") {
      const span = next !== undefined && next.group === st.group ? rowsY[k + 1] + 4 - rowsY[k] : 4;
      labels.push({
        x: indent - (groups > 1 || braced ? 1.7 : 0.6),
        y: rowsY[k] + span / 2 + 0.5,
        text: name,
        cls: first ? "sname" : "sname short",
        anchor: "end",
        color: "",
      });
    }
  }
  if (groups > 1 && !braced) {
    const bx = indent - 1.0;
    rect(sysP, bx, top - 0.5, THICK, bottom - top + 1, "");
    glyph(sysP, G.bracketTop, bx, top - 0.5, "");
    glyph(sysP, G.bracketBottom, bx, bottom + 0.5, "");
  }
  emit(runs, sysP.prims, 0);
  if (!first) labels.push({ x: indent + 0.1, y: top - 2.3, text: String(sc.measures[a].number), cls: "mnum", anchor: "start", color: "" });

  // ---- colored passages
  /** const bands: Band[] */
  const bands = [];
  for (const mk of sc.marks) {
    const t0 = Math.round(mk.start * TPQ);
    const t1 = Math.round(mk.end * TPQ);
    if (t1 <= startTick || t0 >= endTick) continue;
    let ya = Infinity;
    let yb = -Infinity;
    for (let k = 0; k < shown.length; k++) {
      const st = sc.staves[shown[k]];
      if (mk.channels.length > 0 && !st.channels.some((c) => mk.channels.includes(c))) continue;
      ya = Math.min(ya, rowsY[k] - 1.6);
      yb = Math.max(yb, rowsY[k] + 5.6);
    }
    if (ya === Infinity) continue;
    const bx0 = t0 <= startTick ? hdr - 0.8 : timeX(times, t0) - 0.9;
    const bx1 = t1 >= endTick ? x1 + 0.4 : timeX(times, t1) - 0.9;
    bands.push({ x: bx0, y: ya, w: Math.max(0.5, bx1 - bx0), h: yb - ya, color: mk.color, label: mk.label, mark: mk.mark, first: t0 >= startTick });
  }

  return {
    top: 0,
    height: height,
    inks: inks(runs),
    labels: labels,
    braces: braces,
    bands: bands,
    heads: allHeads,
    times: times,
    rows: rows,
    first: a,
    last: b,
    start: startTick,
    end: endTick,
    x0: hdr,
    x1: x1,
  };
}

/** x of a tick in a system (linear between the known points). */
/** function timeX(times: TimePt[], tick: Number) => Number */
export function timeX(times, tick) {
  if (times.length === 0) return 0;
  if (tick <= times[0].tick) return times[0].x;
  for (let i = 1; i < times.length; i++) {
    const p = times[i];
    if (tick <= p.tick) {
      const q = times[i - 1];
      const span = p.tick - q.tick;
      return span > 0 ? q.x + ((p.x - q.x) * (tick - q.tick)) / span : p.x;
    }
  }
  return times[times.length - 1].x;
}

/** Tick at an x of a system (the inverse of timeX). */
/** function xTick(times: TimePt[], x: Number) => Number */
export function xTick(times, x) {
  if (times.length === 0) return 0;
  if (x <= times[0].x) return times[0].tick;
  for (let i = 1; i < times.length; i++) {
    const p = times[i];
    if (x <= p.x) {
      const q = times[i - 1];
      const span = p.x - q.x;
      return span > 0 ? q.tick + ((p.tick - q.tick) * (x - q.x)) / span : p.tick;
    }
  }
  return times[times.length - 1].tick;
}

// ------------------------------------------------------------------ a staff

/** function drawStaff(sc: Score, st: Staff, si: Int, geo: EvGeo[], firstEv: Int[], a: Int, b: Int, p: Inker, hb: HeadBox[], colAt: (Int) => Number, bars: Number[], meterAt: { x: Number, m: Int }[], mcs: MCols[], indent: Number, hdr: Number, x1: Number, final: Boolean) => Undefined */
function drawStaff(sc, st, si, geo, firstEv, a, b, p, hb, colAt, bars, meterAt, mcs, indent, hdr, x1, final) {
  // Lines.
  for (let l = 0; l < 5; l++) rect(p, indent, l - STAFF_LINE / 2, x1 - indent, STAFF_LINE, "staff");

  // Clef, key, time.
  let x = indent + 0.7;
  if (st.clef === "bass") glyph(p, G.fClef, x, 1, "");
  else if (st.clef === "alto") glyph(p, G.cClef, x, 2, "");
  else if (st.clef === "percussion") glyph(p, G.unpitchedPercussionClef1, x + 0.6, 2, "");
  else if (st.clef === "treble8vb") glyph(p, G.gClef8vb, x, 3, "");
  else glyph(p, G.gClef, x, 3, "");
  x = x + 2.9 + 0.6;
  if (sc.fifths !== 0 && !st.drum) {
    const g = sc.fifths > 0 ? G.accidentalSharp : G.accidentalFlat;
    const ys = sc.fifths > 0 ? SHARP_Y : FLAT_Y;
    for (let i = 0; i < Math.abs(sc.fifths); i++) {
      glyph(p, g, x, ys[i] + keyShift(st.clef), "");
      x = x + g.x1 + 0.12;
    }
  }
  if (sc.fifths !== 0) x = indent + 0.7 + 2.9 + 0.6 + keyWidth(sc.fifths) + 0.5;
  const ms0 = sc.measures[a];
  if (ms0.meter) {
    const w = timeSigWidth(ms0);
    drawDigits(p, ms0.num, x + w / 2, 1, "");
    drawDigits(p, ms0.den, x + w / 2, 3, "");
  }
  for (const mt of meterAt) {
    const ms = sc.measures[mt.m];
    const w = timeSigWidth(ms);
    drawDigits(p, ms.num, mt.x + w / 2, 1, "");
    drawDigits(p, ms.den, mt.x + w / 2, 3, "");
  }

  // Bar lines (the last one of the piece: thin + thick).
  for (let i = 0; i < bars.length; i++) {
    const bx = bars[i];
    if (final && i === bars.length - 1) {
      rect(p, bx - THICK, 0, THICK, 4, "");
      rect(p, bx - THICK - 0.4 - THIN, 0, THIN, 4, "");
    } else rect(p, bx - THIN, 0, THIN, 4, "");
  }

  // Events of the measures in this system.
  let i0 = -1;
  for (let m = a; m < b && i0 < 0; m++) i0 = firstEv[m];
  if (i0 < 0) return undefined;
  const evs = st.events;
  let i1 = evs.length;
  for (let i = i0; i < evs.length; i++) {
    if (evs[i].measure >= b) {
      i1 = i;
      break;
    }
  }
  /** const tips: Number[] */
  const tips = evs.map((_) => 0);
  // Beams first: they decide the stems of their notes.
  let k = i0;
  while (k < i1) {
    const id = evs[k].beam;
    if (id < 0 || evs[k].rest) {
      k = k + 1;
      continue;
    }
    let j = k;
    while (j < i1 && evs[j].beam === id) j = j + 1;
    drawBeam(sc, st, geo, evs, k, j, p, colAt, tips);
    k = j;
  }
  for (let i = i0; i < i1; i++) {
    const ev = evs[i];
    const cx = colAt(ev.start);
    if (ev.rest) {
      drawRest(p, ev, cx, mcs[ev.measure], bars[ev.measure - a], sc.measures[ev.measure].count);
      continue;
    }
    tips[i] = drawChord(sc, st, si, geo[i], ev, cx, tips[i], p, hb);
    // Ties to the next event (or off the end of the system).
    for (let h = 0; h < ev.heads.length; h++) {
      const head = ev.heads[h];
      if (!head.tieOut) continue;
      const gy = geo[i].ys[h];
      const dir = tieDir(ev, geo[i], h);
      const hw = headGlyph(ev, head).x1;
      const tx0 = cx + hw + 0.12;
      const tx1 = i + 1 < i1 ? colAt(evs[i + 1].start) - 0.12 : x1 - 0.3;
      tie(p, tx0, tx1, gy + dir * 0.32, dir * Math.min(1.1, 0.42 + (tx1 - tx0) * 0.04), noteColor(sc, head.src));
    }
    // A tie that arrives from the previous system.
    if (i === i0 && i0 > 0) {
      for (let h = 0; h < ev.heads.length; h++) {
        if (!ev.heads[h].tieIn) continue;
        const gy = geo[i].ys[h];
        const dir = tieDir(ev, geo[i], h);
        tie(p, hdr - 1.2, cx - 0.12, gy + dir * 0.32, dir * 0.5, noteColor(sc, ev.heads[h].src));
      }
    }
  }
  drawTuplets(st, geo, evs, i0, i1, p, colAt, tips);
}

/** Triplets: a 3 over (or under) the beam of a beamed group, else in a bracket. */
/** function drawTuplets(st: Staff, geo: EvGeo[], evs: NEv[], i0: Int, i1: Int, p: Inker, colAt: (Int) => Number, tips: Number[]) => Undefined */
function drawTuplets(st, geo, evs, i0, i1, p, colAt, tips) {
  let k = i0;
  while (k < i1) {
    const id = evs[k].tuplet;
    if (id < 0) {
      k = k + 1;
      continue;
    }
    let j = k;
    while (j < i1 && evs[j].tuplet === id) j = j + 1;
    // Which side: the stems' side of the notes (above when they point up).
    let up = 0;
    let beam = evs[k].beam;
    for (let i = k; i < j; i++) {
      if (!evs[i].rest) up = up + (geo[i].up ? 1 : -1);
      if (evs[i].beam !== beam || evs[i].rest) beam = -1;
    }
    const above = st.drum || up >= 0;
    const x0 = colAt(evs[k].start);
    const last = evs[j - 1];
    const x1 = colAt(last.start) + (last.heads.length > 0 ? headGlyph(last, last.heads[0]).x1 : 1.1);
    const g = G.tuplet3;
    const cx = (x0 + x1) / 2;
    // The farthest ink of the group on that side.
    let edge = above ? 0 : 4;
    for (let i = k; i < j; i++) {
      const ev = evs[i];
      for (const y of geo[i].ys) edge = above ? Math.min(edge, y - 0.8) : Math.max(edge, y + 0.8);
      if (!ev.rest && ev.base < 192 && geo[i].up === above) edge = above ? Math.min(edge, tips[i]) : Math.max(edge, tips[i]);
    }
    if (beam >= 0 && j - k >= 2) {
      // On the beam's side, a 3 alone.
      const y = above ? edge - 0.55 : edge + 0.55 + (g.bottom - g.top);
      glyph(p, g, cx - (g.x1 - g.x0) / 2, y, "");
    } else {
      const y = above ? edge - 1.1 : edge + 1.1;
      const half = (g.x1 - g.x0) / 2 + 0.3;
      const hook = above ? 0.55 : -0.55;
      const t = 0.1;
      rect(p, x0 - 0.2, y - t / 2, cx - half - (x0 - 0.2), t, "");
      rect(p, cx + half, y - t / 2, x1 + 0.2 - (cx + half), t, "");
      rect(p, x0 - 0.2, Math.min(y, y + hook), t, Math.abs(hook), "");
      rect(p, x1 + 0.2 - t, Math.min(y, y + hook), t, Math.abs(hook), "");
      glyph(p, g, cx - (g.x1 - g.x0) / 2, y + (g.bottom - g.top) / 2 - 0.05, "");
    }
    k = j;
  }
}

/** Ties curve away from the stem; in a chord, the upper half up and the lower half down. */
/** function tieDir(ev: NEv, g: EvGeo, h: Int) => Number */
function tieDir(ev, g, h) {
  if (ev.heads.length === 1) return g.up && ev.base < 192 ? 1 : -1;
  return h >= ev.heads.length / 2 ? -1 : 1;
}

/** function drawRest(p: Inker, ev: NEv, cx: Number, mc: MCols, bar: Number, count: Int) => Undefined */
function drawRest(p, ev, cx, mc, bar, count) {
  if (ev.whole && count > 1) {
    // The H-bar, with its count above the staff.
    const left = cx - mc.cols[0].left + 0.6;
    const right = bar - 1.4;
    rect(p, left, 1.55, right - left, 0.9, "");
    rect(p, left, 1, THIN, 2, "");
    rect(p, right - THIN, 1, THIN, 2, "");
    drawDigits(p, count, (left + right) / 2, -1.6, "");
    return undefined;
  }
  if (ev.whole) {
    const g = G.restWhole;
    const left = cx - mc.cols[0].left;
    glyph(p, g, (left + bar) / 2 - g.x1 / 2 - 0.3, 1, "");
    return undefined;
  }
  const g = restGlyph(ev.base);
  const y = ev.base >= 192 ? 1 : 2;
  glyph(p, g, cx, y, "");
  if (ev.dots > 0) glyph(p, G.augmentationDot, cx + g.x1 + 0.3, 1.5, "");
}

/** Draw a note or chord; returns where its stem ends (its outer head without one). */
/** function drawChord(sc: Score, st: Staff, si: Int, g: EvGeo, ev: NEv, cx: Number, tip: Number, p: Inker, hb: HeadBox[]) => Number */
function drawChord(sc, st, si, g, ev, cx, tip, p, hb) {
  const n = ev.heads.length;
  if (n === 0) return 2;
  const color = noteColor(sc, ev.heads[0].src);
  const hw = headGlyph(ev, ev.heads[0]).x1;
  const up = g.up;
  const stemmed = ev.base < 192;
  // Heads: seconds go to the other side of the stem.
  /** const hx: Number[] */
  const hx = ev.heads.map((_) => cx);
  if (up || !stemmed) {
    let prev = -99;
    let prevMoved = false;
    for (let i = 0; i < n; i++) {
      const moved = ev.heads[i].step - prev === 1 && !prevMoved;
      if (moved) hx[i] = cx + hw - (stemmed ? STEM : 0);
      prevMoved = moved;
      prev = ev.heads[i].step;
    }
  } else {
    let prev = 999;
    let prevMoved = false;
    for (let i = n - 1; i >= 0; i--) {
      const moved = prev - ev.heads[i].step === 1 && !prevMoved;
      if (moved) hx[i] = cx - hw + STEM;
      prevMoved = moved;
      prev = ev.heads[i].step;
    }
  }
  let hi = 99;
  let lo = -99;
  for (const y of g.ys) {
    hi = Math.min(hi, y);
    lo = Math.max(lo, y);
  }
  // Ledger lines.
  let lx0 = cx;
  let lx1 = cx + hw;
  for (let i = 0; i < n; i++) {
    lx0 = Math.min(lx0, hx[i]);
    lx1 = Math.max(lx1, hx[i] + hw);
  }
  for (let ly = -1; ly >= hi - 0.01; ly = ly - 1) rect(p, lx0 - LEDGER_EXT, ly - LEDGER / 2, lx1 - lx0 + 2 * LEDGER_EXT, LEDGER, "");
  for (let ly = 5; ly <= lo + 0.01; ly = ly + 1) rect(p, lx0 - LEDGER_EXT, ly - LEDGER / 2, lx1 - lx0 + 2 * LEDGER_EXT, LEDGER, "");
  // Heads.
  for (let i = 0; i < n; i++) {
    const h = ev.heads[i];
    const gl = headGlyph(ev, h);
    const c = noteColor(sc, h.src);
    glyph(p, gl, hx[i], g.ys[i], c);
    headGlint(p, gl, hx[i], g.ys[i]);
    hb.push({ x: hx[i], y: g.ys[i], w: gl.x1, src: h.src, staff: si, glyph: gl.c });
  }
  // Accidentals, in zig-zag columns left of the heads.
  const accs = accColumns(ev, g.ys);
  if (accs.length > 0) {
    /** const colW: Number[] */
    const colW = [];
    for (const a of accs) {
      const w = accGlyph(ev.heads[a.head].acc).x1;
      while (colW.length <= a.col) colW.push(0);
      colW[a.col] = Math.max(colW[a.col], w);
    }
    for (const a of accs) {
      let ax = lx0 - 0.22;
      for (let c = 0; c < a.col; c++) ax = ax - colW[c] - 0.12;
      const gl = accGlyph(ev.heads[a.head].acc);
      glyph(p, gl, ax - gl.x1, g.ys[a.head], noteColor(sc, ev.heads[a.head].src));
    }
  }
  // Dots.
  if (ev.dots > 0) {
    let dx = cx + hw + 0.3;
    for (let i = 0; i < n; i++) if (hx[i] > cx) dx = Math.max(dx, hx[i] + hw + 0.3);
    /** const done: Number[] */
    const done = [];
    for (let i = 0; i < n; i++) {
      const y = g.ys[i];
      const dy = Math.abs(y - Math.round(y)) < 0.01 ? y - 0.5 : y;
      if (done.includes(dy)) continue;
      done.push(dy);
      for (let d = 0; d < ev.dots; d++) {
        glyph(p, G.augmentationDot, dx + d * 0.5, dy, color);
        glint(p, GLOSS, dx + d * 0.5 + 0.14, dy - 0.07, 0.04, 0.028, -30);
      }
    }
  }
  if (!stemmed) return up ? hi : lo;
  // Stem and flag.
  const sx = up ? cx + hw - STEM : cx;
  const beamed = ev.beam >= 0;
  const lv = levels(ev.base);
  if (up) {
    const end = beamed ? tip : Math.min(hi - STEM_LEN - (lv >= 2 ? (lv - 1) * 0.5 : 0), 2);
    rect(p, sx, end, STEM, lo - STEM_Y - end, color);
    if (!beamed && lv > 0) glyph(p, flagGlyph(ev.base, true), sx, end, color);
    return end;
  }
  const end = beamed ? tip : Math.max(lo + STEM_LEN + (lv >= 2 ? (lv - 1) * 0.5 : 0), 2);
  rect(p, sx, hi + STEM_Y, STEM, end - hi - STEM_Y, color);
  if (!beamed && lv > 0) glyph(p, flagGlyph(ev.base, false), sx, end, color);
  return end;
}

/** Beam a group of events [k, j): a line over the stems, sloped with the
 * notes but at most a space, snapped to quarter spaces, keeping every stem
 * long enough; then secondary beams and hooks. Writes each stem's end into `tips`. */
/** function drawBeam(sc: Score, st: Staff, geo: EvGeo[], evs: NEv[], k: Int, j: Int, p: Inker, colAt: (Int) => Number, tips: Number[]) => Undefined */
function drawBeam(sc, st, geo, evs, k, j, p, colAt, tips) {
  const up = geo[k].up;
  /** const xs: Number[] */
  const xs = [];
  /** const refs: Number[] */
  const refs = [];
  let deepest = 1;
  for (let i = k; i < j; i++) {
    const ev = evs[i];
    const hw = ev.heads.length > 0 ? headGlyph(ev, ev.heads[0]).x1 : 1.18;
    const cx = colAt(ev.start);
    xs.push(up ? cx + hw - STEM : cx);
    let hi = 99;
    let lo = -99;
    for (const y of geo[i].ys) {
      hi = Math.min(hi, y);
      lo = Math.max(lo, y);
    }
    refs.push(up ? hi : lo);
    deepest = Math.max(deepest, levels(ev.base));
  }
  const n = xs.length;
  const span = Math.max(0.01, xs[n - 1] - xs[0]);
  // Slope: follow the outer notes, flat when an inner note sticks out (a concave group).
  let rise = refs[n - 1] - refs[0];
  for (let i = 1; i < n - 1; i++) {
    const inner = refs[i];
    if (up ? inner < Math.min(refs[0], refs[n - 1]) : inner > Math.max(refs[0], refs[n - 1])) rise = 0;
  }
  const maxRise = Math.min(1, span * 0.18);
  rise = Math.max(-maxRise, Math.min(maxRise, rise * 0.5));
  rise = Math.round(rise * 4) / 4;
  const slope = rise / span;
  // Height: every stem at least 3¼ spaces (more with more beams), and reaching the middle line.
  const minStem = 3.25 + (deepest - 1) * 0.5;
  let y0 = up ? Infinity : -Infinity;
  for (let i = 0; i < n; i++) {
    const need = up ? refs[i] - minStem : refs[i] + minStem;
    const at = need - slope * (xs[i] - xs[0]);
    y0 = up ? Math.min(y0, at) : Math.max(y0, at);
  }
  const mid = y0 + slope * (span / 2);
  if (up && mid > 2) y0 = y0 - (mid - 2);
  if (!up && mid < 2) y0 = y0 + (2 - mid);
  y0 = up ? Math.floor(y0 * 4) / 4 : Math.ceil(y0 * 4) / 4;
  /** function yAt(x: Number) => Number */
  function yAt(x) {
    return y0 + slope * (x - xs[0]);
  }
  const color = noteColor(sc, evs[k].heads.length > 0 ? evs[k].heads[0].src : 0);
  let same = true;
  for (let i = k; i < j; i++) if (evs[i].heads.length > 0 && noteColor(sc, evs[i].heads[0].src) !== color) same = false;
  const bc = same ? color : "";
  const t = up ? 0 : -BEAM;
  // Primary beam.
  quad(p, xs[0], yAt(xs[0]) + t, xs[n - 1] + STEM, yAt(xs[n - 1] + STEM) + t, BEAM, bc);
  // Secondary beams and hooks.
  for (let lv = 2; lv <= deepest; lv++) {
    const off = (lv - 1) * BEAM_STEP * (up ? 1 : -1);
    let r = 0;
    while (r < n) {
      if (levels(evs[k + r].base) < lv) {
        r = r + 1;
        continue;
      }
      let q = r;
      while (q + 1 < n && levels(evs[k + q + 1].base) >= lv) q = q + 1;
      if (q > r) quad(p, xs[r], yAt(xs[r]) + t + off, xs[q] + STEM, yAt(xs[q] + STEM) + t + off, BEAM, bc);
      else {
        const hook = Math.min(1.15, (span / Math.max(1, n - 1)) * 0.6);
        const left = r === n - 1 || (r > 0 && evs[k + r - 1].dots > 0);
        const hx0 = left ? xs[r] - hook : xs[r];
        const hx1 = left ? xs[r] + STEM : xs[r] + hook;
        quad(p, hx0, yAt(hx0) + t + off, hx1, yAt(hx1) + t + off, BEAM, bc);
      }
      r = q + 1;
    }
  }
  for (let i = 0; i < n; i++) tips[k + i] = yAt(xs[i] + STEM / 2);
}
