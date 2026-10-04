// The ink's drops: from where ink was written (a crisp mask drawn by
// web/src/pdf.js `inkPart`), how high it stands at each pixel — the height
// map the film's renderer raises and lights (web/lib/filmgl.js).
//
// Wet ink on paper stands as drops shaped by surface tension: across a
// stroke, its surface is an arc of a circle meeting the paper at a contact
// angle (a spherical cap, as a small drop on a surface is). So:
//
//  1. Each ink pixel's distance to the nearest paper (an exact Euclidean
//     distance transform, two 1D passes — Felzenszwalb & Huttenlocher).
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
/** The tallest a bead of ink stands (points), and the height a full 255 encodes (web/lib/filmgl.js reads it so). */
const DEEPEST = 0.8;
export const INK_UNIT = 1;

/** Squared distance transform of one line (Felzenszwalb & Huttenlocher), in place over `f` read through `get`/`set`. */
function dt1(f, n, v, z, d) {
  let k = 0;
  v[0] = 0;
  z[0] = -Infinity;
  z[1] = Infinity;
  for (let q = 1; q < n; q++) {
    let s;
    for (;;) {
      const p = v[k];
      s = (f[q] + q * q - (f[p] + p * p)) / (2 * q - 2 * p);
      if (s > z[k]) break;
      k--;
    }
    k++;
    v[k] = q;
    z[k] = s;
    z[k + 1] = Infinity;
  }
  k = 0;
  for (let q = 0; q < n; q++) {
    while (z[k + 1] < q) k++;
    const p = v[k];
    d[q] = (q - p) * (q - p) + f[p];
  }
}

/** Squared Euclidean distance from each pixel to the nearest pixel where `grid` is 0. */
function edt(grid, w, h) {
  const n = Math.max(w, h);
  const f = new Float64Array(n);
  const d = new Float64Array(n);
  const v = new Int32Array(n);
  const z = new Float64Array(n + 1);
  for (let x = 0; x < w; x++) {
    for (let y = 0; y < h; y++) f[y] = grid[y * w + x];
    dt1(f, h, v, z, d);
    for (let y = 0; y < h; y++) grid[y * w + x] = d[y];
  }
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) f[x] = grid[y * w + x];
    dt1(f, w, v, z, d);
    for (let x = 0; x < w; x++) grid[y * w + x] = d[x];
  }
  return grid;
}

/** Heights (points) of the drops over a mask of ink coverage (0..1), `ppt` pixels a point. */
export function drops(cover, w, h, ppt) {
  const INF = 1e20;
  const grid = new Float64Array(w * h);
  for (let i = 0; i < w * h; i++) grid[i] = cover[i] >= 0.5 ? INF : 0;
  edt(grid, w, h);
  // Distance to the edge, in pixels: from the pixel's centre to the boundary
  // between it and the nearest paper (the antialiased coverage nudges it).
  const dist = new Float32Array(w * h);
  let max = 0;
  for (let i = 0; i < w * h; i++) {
    if (grid[i] === 0) continue;
    const e = Math.sqrt(grid[i]) - 0.5 + (cover[i] - 0.5) * 0.5;
    dist[i] = Math.max(0.05, e);
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

/**
 * Rasterize the ink's mask (an SVG of `w` by `h` pixels, `ppt` pixels a
 * point) and shape its drops: a PNG's object URL, its red channel
 * 255 × (1 − height / INK_UNIT) (paper white, the deepest ink darkest).
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
    for (let i = 0; i < w * h; i++) {
      const v = Math.round(255 * (1 - Math.min(1, height[i] / INK_UNIT)));
      px.data[i * 4] = v;
      px.data[i * 4 + 1] = v;
      px.data[i * 4 + 2] = v;
      px.data[i * 4 + 3] = 255;
    }
    g.putImageData(px, 0, 0);
    const blob = await new Promise((done) => canvas.toBlob(done, "image/png"));
    if (!blob) throw new Error("the ink could not be drawn");
    return URL.createObjectURL(blob);
  } finally {
    URL.revokeObjectURL(url);
  }
}
