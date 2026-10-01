// Paint: the primitives an engraving is drawn with. Painters collect primitives —
// filled shapes, glyphs, ties and the glints of wet ink — with their
// vertical extent, so a staff can be drawn in its own coordinates and
// shifted into place; `emit` then writes them as SVG path data and glyph
// runs, one run per color (engrave.js, underlay.js; pdf.js reads the runs).

/** The run holding the glints of the wet ink (drawn in light over the music, not in ink). */
export const GLOSS = "gloss";
/** The broad, faint sheen around the glints. */
export const SHEEN = "sheen";

/** type Prim = { kind: Int, color: String, nums: Number[], ch: String } */
/** type Ink = { color: String, d: String, text: String, xs: String, ys: String } */
/** type Label = { x: Number, y: Number, text: String, cls: String, anchor: String, color: String } */

/** A list of primitives with their vertical extent; shifted into place later. */
/** type Inker = { prims: Prim[], top: Number, bottom: Number } */

/** function painter() => Inker */
export function painter() {
  return { prims: [], top: 0, bottom: 4 };
}

/** function extend(p: Inker, y0: Number, y1: Number) => Undefined */
export function extend(p, y0, y1) {
  if (y0 < p.top) p.top = y0;
  if (y1 > p.bottom) p.bottom = y1;
}

/** function rect(p: Inker, x: Number, y: Number, w: Number, h: Number, color: String) => Undefined */
export function rect(p, x, y, w, h, color) {
  p.prims.push({ kind: 0, color: color, nums: [x, y, x + w, y, x + w, y + h, x, y + h], ch: "" });
  extend(p, y, y + h);
}

/** function quad(p: Inker, x0: Number, y0: Number, x1: Number, y1: Number, t: Number, color: String) => Undefined */
export function quad(p, x0, y0, x1, y1, t, color) {
  p.prims.push({ kind: 0, color: color, nums: [x0, y0, x1, y1, x1, y1 + t, x0, y0 + t], ch: "" });
  extend(p, Math.min(y0, y1), Math.max(y0, y1) + t);
}

/** function glyph(p: Inker, g: Glyph, x: Number, y: Number, color: String) => Undefined */
export function glyph(p, g, x, y, color) {
  p.prims.push({ kind: 1, color: color, nums: [x, y], ch: g.c });
  extend(p, y + g.top, y + g.bottom);
}

/** The gloss of a wet drop of ink: a small ellipse (centre, radii, tilt in degrees) where the light catches it. */
/** function glint(p: Inker, run: String, cx: Number, cy: Number, rx: Number, ry: Number, deg: Number) => Undefined */
export function glint(p, run, cx, cy, rx, ry, deg) {
  p.prims.push({ kind: 4, color: run, nums: [cx, cy, rx, ry, deg], ch: "" });
}

/** A tie or slur: a crescent between two points bulging by `h` (negative: up). */
/** function tie(p: Inker, x0: Number, x1: Number, y: Number, h: Number, color: String) => Undefined */
export function tie(p, x0, x1, y, h, color) {
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
export function emit(runs, prims, dy) {
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
export function inks(runs) {
  return runs.map((r) => ({ color: r.color, d: r.d.join(""), text: r.text.join(""), xs: r.xs.join(" "), ys: r.ys.join(" ") }));
}
