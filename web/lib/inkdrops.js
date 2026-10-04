// The ink's drops: from where ink was written (a crisp mask drawn by
// web/src/pdf.js `inkPart`), how high it stands at each pixel — the height
// map the film's renderer raises and lights (web/lib/filmgl.js).
//
// Wet ink on paper stands as drops shaped by surface tension: across a
// stroke, its surface is an arc of a circle meeting the paper at a contact
// angle (a spherical cap, as a small drop on a surface is). So:
//
//  1. Each ink pixel's distance to the nearest paper: an anti-aliased
//     Euclidean distance transform (Gustavson & Strand), the edge placed
//     within its pixels by their coverage, so a slanting stroke's sides are
//     straight, not the staircase of its pixels.
//  2. The half-width of the stroke it belongs to: the distance on the
//     stroke's ridge, carried down to the edge (pixels taken from the
//     farthest in, each one takes the largest half-width of its neighbours
//     farther in) — so a stem joining a notehead rises into it.
//  3. The cap over that half-width at that distance from the edge: low and
//     beaded for a stem, a dome for a notehead. A wide mark's cap would stand
//     taller than a bead of ink does: the whole cap is lowered to the height it
//     can stand, still rounded to its top (a flat top would mirror the room as
//     one sheet seen low down; a dome catches the light as a glint).
//
// Not type-checked (canvas pixels are outside inty's library);
// web/types/platform.d.js types what it exports.

/** Contact angle of the ink on the paper (radians). */
const CONTACT = (58 * Math.PI) / 180;
/** The tallest a bead of ink stands (points), and the full scale of the heights kept (web/lib/filmgl.js reads them so). */
const DEEPEST = 0.8;
export const INK_UNIT = 1;

/**
 * How far a pixel's centre lies from the edge crossing it, given the edge's
 * direction (its gradient, gx, gy) and how much of the pixel the object
 * covers (`a`): positive outside the object. (Gustavson & Strand, "Anti-aliased
 * Euclidean distance transform", 2011.)
 */
function edgeOffset(gx, gy, a) {
  if (gx === 0 || gy === 0) return 0.5 - a;
  const len = Math.hypot(gx, gy);
  gx = Math.abs(gx / len);
  gy = Math.abs(gy / len);
  if (gx < gy) {
    const t = gx;
    gx = gy;
    gy = t;
  }
  const a1 = (0.5 * gy) / gx;
  if (a < a1) return 0.5 * (gx + gy) - Math.sqrt(2 * gx * gy * a);
  if (a < 1 - a1) return (0.5 - a) * gx;
  return -0.5 * (gx + gy) + Math.sqrt(2 * gx * gy * (1 - a));
}

/**
 * The anti-aliased Euclidean distance transform (Gustavson & Strand): for each
 * pixel, how far its centre is from the edge of the object `img` covers
 * (0..1 a pixel), the edge placed within its pixels by their coverage, so a
 * slanting edge is straight, not the staircase of its pixels (whose ripples a
 * drop's slope would catch as stripes of light). 0 inside the object.
 */
function edtaa(img, w, h) {
  const n = w * h;
  const gx = new Float32Array(n);
  const gy = new Float32Array(n);
  const R2 = Math.SQRT2;
  // The edge's direction where it crosses a pixel (Sobel over the coverage).
  for (let y = 1; y < h - 1; y++)
    for (let x = 1; x < w - 1; x++) {
      const i = y * w + x;
      if (img[i] <= 0 || img[i] >= 1) continue;
      const ax = -img[i - w - 1] - R2 * img[i - 1] - img[i + w - 1] + img[i - w + 1] + R2 * img[i + 1] + img[i + w + 1];
      const ay = -img[i - w - 1] - R2 * img[i - w] - img[i - w + 1] + img[i + w - 1] + R2 * img[i + w] + img[i + w + 1];
      const len = Math.hypot(ax, ay);
      if (len > 0) {
        gx[i] = ax / len;
        gy[i] = ay / len;
      }
    }
  // Each pixel's offset to its nearest edge pixel, and its distance.
  const dx = new Int32Array(n);
  const dy = new Int32Array(n);
  const dist = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    if (img[i] <= 0) dist[i] = 1e6;
    else if (img[i] < 1) dist[i] = edgeOffset(gx[i], gy[i], img[i]);
    else dist[i] = 0;
  }
  /** The distance from a pixel to the edge in pixel `c`, `ox, oy` from it. */
  function via(c, ox, oy) {
    const a = Math.min(1, Math.max(0, img[c]));
    if (a === 0) return 1e6;
    const d = Math.hypot(ox, oy);
    return d + (d === 0 ? edgeOffset(gx[c], gy[c], a) : edgeOffset(ox, oy, a));
  }
  /** Try pixel `i` against its neighbour `j`, `sx, sy` from it toward `i`. */
  function relax(i, j, sx, sy) {
    const ox = dx[j] + sx;
    const oy = dy[j] + sy;
    const c = i - ox - oy * w;
    const d = via(c, ox, oy);
    if (d < dist[i] - 1e-3) {
      dx[i] = ox;
      dy[i] = oy;
      dist[i] = d;
      return true;
    }
    return false;
  }
  for (let changed = true, round = 0; changed && round < 8; round++) {
    changed = false;
    // Down the image: from above and the left, then from the right.
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        const i = y * w + x;
        if (dist[i] <= 0) continue;
        if (y > 0) {
          if (relax(i, i - w, 0, 1)) changed = true;
          if (x > 0 && relax(i, i - w - 1, 1, 1)) changed = true;
          if (x < w - 1 && relax(i, i - w + 1, -1, 1)) changed = true;
        }
        if (x > 0 && relax(i, i - 1, 1, 0)) changed = true;
      }
      for (let x = w - 2; x >= 0; x--) {
        const i = y * w + x;
        if (dist[i] > 0 && relax(i, i + 1, -1, 0)) changed = true;
      }
    }
    // Up the image: from below and the right, then from the left.
    for (let y = h - 1; y >= 0; y--) {
      for (let x = w - 1; x >= 0; x--) {
        const i = y * w + x;
        if (dist[i] <= 0) continue;
        if (y < h - 1) {
          if (relax(i, i + w, 0, -1)) changed = true;
          if (x < w - 1 && relax(i, i + w + 1, -1, -1)) changed = true;
          if (x > 0 && relax(i, i + w - 1, 1, -1)) changed = true;
        }
        if (x < w - 1 && relax(i, i + 1, -1, 0)) changed = true;
      }
      for (let x = 1; x < w; x++) {
        const i = y * w + x;
        if (dist[i] > 0 && relax(i, i - 1, 1, 0)) changed = true;
      }
    }
  }
  return dist;
}

/** Average `v` over the pixels within `r` (along rows, then columns), counting only those where `on` is above 0. */
function smoothOver(v, on, w, h, r) {
  const tmp = new Float32Array(w * h);
  for (let y = 0; y < h; y++)
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      if (on[i] <= 0) continue;
      let sum = 0;
      let n = 0;
      for (let k = Math.max(0, x - r); k <= Math.min(w - 1, x + r); k++)
        if (on[y * w + k] > 0) {
          sum += v[y * w + k];
          n++;
        }
      tmp[i] = sum / n;
    }
  for (let x = 0; x < w; x++)
    for (let y = 0; y < h; y++) {
      const i = y * w + x;
      if (on[i] <= 0) continue;
      let sum = 0;
      let n = 0;
      for (let k = Math.max(0, y - r); k <= Math.min(h - 1, y + r); k++)
        if (on[k * w + x] > 0) {
          sum += tmp[k * w + x];
          n++;
        }
      v[i] = sum / n;
    }
}

/** Heights (points) of the drops over a mask of ink coverage (0..1), `ppt` pixels a point. */
export function drops(cover, w, h, ppt) {
  // Distance to the edge, in pixels: from the pixel's centre to the boundary
  // between the ink and the nearest paper (the paper is the object measured to).
  const paper = new Float32Array(w * h);
  for (let i = 0; i < w * h; i++) paper[i] = 1 - cover[i];
  const toPaper = edtaa(paper, w, h);
  const dist = new Float32Array(w * h);
  let max = 0;
  for (let i = 0; i < w * h; i++) {
    if (toPaper[i] <= 0 || toPaper[i] >= 1e5) continue;
    dist[i] = Math.max(0.05, toPaper[i]);
    if (dist[i] > max) max = dist[i];
  }
  // The stroke's half-width, from its ridge outward (bucketed by distance, farthest first).
  const buckets = Math.ceil(max) + 1;
  const count = new Int32Array(buckets + 1);
  for (let i = 0; i < w * h; i++) if (dist[i] > 0) count[Math.floor(dist[i])]++;
  const start = new Int32Array(buckets + 1);
  for (let b = buckets - 1, at = 0; b >= 0; b--) {
    start[b] = at;
    at += count[b];
  }
  const order = new Int32Array(w * h);
  const fill = start.slice();
  for (let i = 0; i < w * h; i++) if (dist[i] > 0) order[fill[Math.floor(dist[i])]++] = i;
  const total = fill[0];
  const half = new Float32Array(w * h);
  for (let k = 0; k < total; k++) {
    const i = order[k];
    const x = i % w;
    const y = (i - x) / w;
    let r = dist[i];
    for (let dy = -1; dy <= 1; dy++)
      for (let dx = -1; dx <= 1; dx++) {
        const xx = x + dx;
        const yy = y + dy;
        if ((dx === 0 && dy === 0) || xx < 0 || yy < 0 || xx >= w || yy >= h) continue;
        const j = yy * w + xx;
        if (dist[j] > dist[i] && half[j] > r) r = half[j];
      }
    half[i] = r;
  }
  // A stroke's half-width changes slowly along it, but read off its pixelated
  // ridge it wobbles by up to half a pixel where the stroke slants: averaged
  // over the ink a few pixels around (along rows, then columns), it is smooth.
  smoothOver(half, dist, w, h, 3);
  // The cap: an arc of radius rho = R / sin(contact) over the stroke's width.
  const out = new Float32Array(w * h);
  const sinC = Math.sin(CONTACT);
  const cosC = Math.cos(CONTACT);
  for (let i = 0; i < w * h; i++) {
    const d = dist[i];
    if (d <= 0) continue;
    const R = Math.max(half[i], d);
    const rho = R / sinC;
    const t = R - d;
    const hpx = Math.sqrt(Math.max(0, rho * rho - t * t)) - rho * cosC;
    const hpt = Math.max(0, hpx) / ppt;
    const peak = (rho * (1 - cosC)) / ppt;
    out[i] = peak > 0 ? hpt * ((DEEPEST * Math.tanh(peak / DEEPEST)) / peak) : 0;
  }
  return out;
}

/** The drops' heights, by key: kept here (not as an image, whose 8 bits a pixel would terrace a gentle dome into stripes of light). */
const heights = new Map();
let made = 0;

/** The heights a key names (`w` by `h`, 0..65535 for 1 − height / INK_UNIT: paper 65535), or undefined. */
export function inkHeights(key) {
  return heights.get(key);
}

/** Let go of the heights a key names. */
export function forgetInk(key) {
  heights.delete(key);
}

/**
 * Rasterize the ink's mask (an SVG of `w` by `h` pixels, `ppt` pixels a
 * point) and shape its drops: a key ("ink:…") to their heights, kept at 16
 * bits (inkHeights; web/lib/filmgl.js uploads them as a half-float texture).
 */
export async function rasterInk(svg, w, h, ppt) {
  const url = URL.createObjectURL(new Blob([svg], { type: "image/svg+xml" }));
  try {
    const img = new Image();
    img.src = url;
    await img.decode();
    const canvas = document.createElement("canvas");
    canvas.width = w;
    canvas.height = h;
    const g = canvas.getContext("2d", { willReadFrequently: true });
    g.fillStyle = "#fff";
    g.fillRect(0, 0, w, h);
    g.drawImage(img, 0, 0, w, h);
    const px = g.getImageData(0, 0, w, h);
    const cover = new Float32Array(w * h);
    for (let i = 0; i < w * h; i++) cover[i] = 1 - px.data[i * 4] / 255;
    const height = drops(cover, w, h, ppt);
    const data = new Uint16Array(w * h);
    for (let i = 0; i < w * h; i++) data[i] = Math.round(65535 * (1 - Math.min(1, height[i] / INK_UNIT)));
    const key = `ink:${++made}`;
    heights.set(key, { w, h, data });
    return key;
  } finally {
    URL.revokeObjectURL(url);
  }
}
