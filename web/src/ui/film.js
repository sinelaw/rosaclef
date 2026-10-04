// The Film view (the Score view's Film mode): the score's pages lie on a desk
// and a camera in 3D space above them plays the song — following the music,
// zooming in on a part, pulling back to the whole band, leaning and turning so
// the music runs diagonally across the picture. film.js directs it; this
// prepares its frames, edits its shots and exports it as a video.
//
// Drawing. Each frame is plain data (GlFrame) drawn by the WebGL renderer
// (web/lib/filmgl.js): the desk, the pages as bitmaps of the printed score on
// paper (pdf.js draws them as SVG, the platform layer rasterizes them once),
// and with each page a bitmap of its ink, from which the renderer raises the
// ink off the paper and lights it — glossy wet ink catching the lamp as the
// camera moves. Where the camera comes close, sharper bitmaps of the system
// it looks at are drawn over the page. Bitmaps are made one at a time, the
// ones the camera needs first, and only made again when the music changes
// (not when the shots do).
//
// Editing. The timeline under the picture shows the scenes: the director's
// (auto) and the project's shots (manual). Click to play from a point, select
// a shot to edit it in the panel, drag its body or edges to move it, drag on
// the picture to frame it (Shift: turn and lean), wheel to zoom. Everything is
// written to `project.animation`, so the agent sees it, and is undoable.
//
// Export. "MP4" renders the mixdown, then films the song frame by frame at
// 1920×1080 (its repeats played as they sound) and encodes it with its sound.

import {
  fmt,
  now,
  rasterSvg,
  dropUrl,
  toggleFullscreen,
  drag,
  pixelRatio,
  paperSize,
  filmDraw,
  filmLive,
  filmForget,
  encodeFilm,
  renderStill,
  sendJson,
  decodeAudioUrl,
  download,
  loadPref,
  savePref,
  fastGraphics,
} from "#platform";
import { state, commit, begin, changed, invalidate, hint, reportContext } from "../store.js";
import { livePosition, togglePlay } from "../audio.js";
import {
  makeFilm,
  replan,
  cameraAt,
  mixCam,
  sceneAt,
  sparks,
  bake,
  systemAt,
  channelsOf,
  onDesk,
  performance,
  writtenAt,
  FRAMES,
  ROLES,
  TRANSITIONS,
  EASES,
  EFFECTS,
  SURFACES,
  ease,
  defaultTilt,
} from "../film.js";
import { pagePart } from "../pdf.js";
import { surface, PAPER } from "../ink.js";
import { decodeShot, musicJson, noMove } from "../model.js";
import { TPQ } from "../notation.js";
import { select, iconButton, glyph, textInput } from "./widgets.js";
import { toast } from "./toast.js";

// ------------------------------------------------------------------ state

/** A bitmap (made or being made): what it shows, its object URL ("" while drawing, "-" if it failed), its scale (pixels a point), when last shown (ms), its page (-1: the desk) and the part of the page it shows ([x, y, w, h], points). */
/** type Bitmap = { key: String, url: String, scale: Number, used: Number, page: Int, box: Number[] } */
/** A change of view made by hand while paused (not part of any shot): moved by fractions of the picture, zoomed, turned, leaned. */
/** type Look = { dx: Number, dy: Number, zoom: Number, turn: Number, tilt: Number } */
/** What the film is made of, from the score view: its notation, how it is drawn, its title block, beats in a bar, whether the playhead is this score's, and how to seek and go back to the paper. */
/** type FilmInput = { sc: Score, hide: Boolean, look: PageLook, info: PdfInfo, bar: Number, showing: Boolean, seek: (Number) => Undefined, back: () => Undefined } */
/**
 * A film view: its id and size, panel, selection (`shot`: an index in
 * animation.shots; `auto`: the start of a selected director's scene), the film
 * (0 or 1), what it was made from (`sc`, `sig`: the music; `plan`: the shots
 * and aspect), its bitmaps (pages, sharper tiles, the desk), whether one is
 * being made, the camera last shown (for gliding over jumps), the look by
 * hand, an export under way (its progress, 0..1; -1: none), and what the
 * picture on screen leaves out to draw faster (DRAWING keys).
 */
/** type FilmView = { id: String, width: Number, height: Number, side: Boolean, cinema: Boolean, shot: Int, auto: Number, films: Film[], sc: Score[], sig: String, plan: String, pageKeys: String[], pages: Bitmap[], tiles: Bitmap[], desk: Bitmap[], busy: Boolean, last: Cam[], lastBeat: Number, from: Cam[], fromAt: Number, look: Look, gesture: Boolean, export: Number, off: String[], ticking: Boolean, shown: Film[], shownInp: FilmInput[] } */

/** function newFilmView(id: String) => FilmView */
export function newFilmView(id) {
  return {
    id: id,
    width: 900,
    height: 500,
    side: true,
    cinema: false,
    shot: -1,
    auto: NaN,
    films: [],
    sc: [],
    sig: "",
    plan: "",
    pageKeys: [],
    pages: [],
    tiles: [],
    desk: [],
    busy: false,
    last: [],
    lastBeat: 0,
    from: [],
    fromAt: 0,
    look: { dx: 0, dy: 0, zoom: 1, turn: 0, tilt: 0 },
    gesture: false,
    export: -1,
    off: drawingOff(),
    ticking: false,
    shown: [],
    shownInp: [],
  };
}

/**
 * What the picture on screen draws, each of which can be left out to draw
 * faster: [key, name, what it is]. Quality draws them all, Performance none.
 * Only the screen, in this browser: the film, its frames and its exports keep
 * everything.
 */
const DRAWING = [
  ["glow", "Note glow", "The notes playing light up"],
  ["spotlight", "Spotlight", "The pool of light on the framed staves"],
  ["vignette", "Vignette", "The picture darkening toward its edges"],
  ["finish", "Finish", "The lamp's warmth, soft highlights and film grain over the picture"],
  [
    "ink",
    "Ink look",
    "The ink as on the paper view: its soft edges and the wet ink's glints (off: crisp plain ink, drawn many times faster — in Firefox above all)",
  ],
  ["paper", "Paper texture", "The paper's tooth, formation, grain and toned edges (off: plain paper)"],
  ["sharp", "Full resolution", "As many pixels as the screen has (off: one a CSS pixel — a quarter of them on a high-density screen)"],
  ["detail", "Sharp close-ups", "Close up, the page as sharp as the screen shows it (off: half as sharp, a quarter of the drawing)"],
];
/** Where what the screen leaves out is remembered: DRAWING keys, by commas ("none": nothing). */
const DRAWING_PREF = "rosaclef.film.off";

/** What the screen leaves out: as chosen in this browser; until then, everything (Performance) unless the film draws smoothly here (Chrome on a GPU). */
/** function drawingOff() => String[] */
function drawingOff() {
  const saved = loadPref(DRAWING_PREF);
  if (saved === "") return fastGraphics() ? [] : DRAWING.map((x) => x[0]);
  const off = saved.split(",").filter((k) => DRAWING.some((x) => x[0] === k));
  // Performance as saved before the ink's look could be left out: still Performance.
  const before = DRAWING.filter((x) => x[0] !== "ink");
  if (off.length === before.length && before.every((x) => off.includes(x[0]))) return off.concat(["ink"]);
  return off;
}

/** Pixels a point of the pages' bitmaps. */
const PAGE_SCALE = 1.6;
/** Where the camera comes closer, tiles of TILE_PX pixels square at one of these scales (pixels a point) are drawn over the page: up close, a staff space is a hundred pixels. */
const TILE_LEVELS = [2.5, 5, 10, 20, 40, 80];
const TILE_PX = 1024;
/** Tiles kept at once (the ones shown longest ago go). */
const TILES_MAX = 64;
/** The exported video. */
const VIDEO_W = 1920;
const VIDEO_H = 1080;
const VIDEO_FPS = 30;

// ------------------------------------------------------------------ helpers

/** A short hash of a text (to tell bitmaps apart by what they show). */
/** function hashText(s: String) => String */
function hashText(s) {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h = h ^ s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return `${s.length}.${h >>> 0}`;
}

/** The project's film, written into the project from now on (the first edit makes it). */
/** function anim() => Animation */
function anim() {
  const a = state.project.animation;
  a.on = true;
  return a;
}

/** Bars and beats of a song beat: "12.3". */
/** function barBeat(beat: Number, bar: Number) => String */
function barBeat(beat, bar) {
  const b = Math.floor(beat / bar + 1e-9);
  return `${b + 1}.${Math.floor(beat - b * bar + 1e-9) + 1}`;
}

/** function clampNum(x: Number, lo: Number, hi: Number) => Number */
function clampNum(x, lo, hi) {
  return Math.max(lo, Math.min(hi, x));
}

// ------------------------------------------------------------------ the film

/** The film for the view's current input (made again only when the music, the shots or the aspect change). */
/** function filmFor(fv: FilmView, inp: FilmInput) => Film */
function filmFor(fv, inp) {
  const a = state.project.animation;
  const aspect = Math.max(0.3, fv.width / Math.max(1, fv.height));
  const planKey = `${JSON.stringify(a)}|${fmt(aspect, 2)}`;
  if (fv.films.length > 0 && fv.sc.length > 0 && fv.sc[0] === inp.sc && fv.plan === planKey) return fv.films[0];
  // The score was written again: is it different music, or only the same with another film?
  const sig = `${musicJson(state.project)}|${inp.hide}|${inp.info.title}`;
  if (fv.films.length > 0 && fv.sig === sig) {
    const f = replan(fv.films[0], a, aspect);
    fv.films = [f];
    fv.sc = [inp.sc];
    fv.plan = planKey;
    return f;
  }
  const f = makeFilm(a, inp.sc, paperSize(), inp.hide, aspect, inp.bar);
  fv.films = [f];
  fv.sc = [inp.sc];
  fv.sig = sig;
  fv.plan = planKey;
  return f;
}

/** The look of the pages as a key. */
/** function lookKey(inp: FilmInput) => String */
function lookKey(inp) {
  return `${inp.look.wet}|${fmt(inp.look.gloss, 2)}|${fmt(inp.look.shine, 2)}`;
}

/** The pages' color: the paper view's ink, without its painted gloss (the film's ink is lit in 3D instead); plain ink where the screen leaves the ink's look out. */
/** function paperLook(fv: FilmView, inp: FilmInput) => PageLook */
function paperLook(fv, inp) {
  return { wet: inp.look.wet, gloss: 0, shine: inp.look.shine, filters: !fv.off.includes("ink") };
}

/** What each page shows (its SVG's hash): bitmaps are kept while it stays the same. */
/** function pageKeys(fv: FilmView, f: Film, inp: FilmInput) => String[] */
function pageKeys(fv, f, inp) {
  const key = `${fv.sig}|${lookKey(inp)}|${fv.off.includes("ink") ? "plain" : "inked"}`;
  if (fv.pageKeys.length === f.lay.pages.length + 1 && fv.pageKeys[0] === key) return fv.pageKeys;
  /** const keys: String[] */
  const keys = [key];
  for (let i = 0; i < f.lay.pages.length; i++) keys.push(hashText(pagePart(f.lay, i, inp.info, paperLook(fv, inp), 1, [0, 0, f.lay.w, f.lay.h], false)));
  fv.pageKeys = keys;
  return keys;
}

/** function drop(url: String) => Undefined */
function drop(url) {
  if (url === "" || url === "-") return undefined;
  filmForget(url);
  dropUrl(url);
}

/** A bitmap is drawn (or could not be). */
/** function done(b: Bitmap) => Boolean */
function done(b) {
  return b.url !== "";
}

/**
 * The next bitmap to draw: the desk, the pages in view, the tiles wanted now,
 * the other pages, the tiles wanted soon. `need` is [page, level, tx, ty,
 * later] (level -1: the whole page). Undefined: nothing to do. Bitmaps that
 * no longer show anything go.
 */
/** function nextJob(fv: FilmView, f: Film, inp: FilmInput, need: Number[][], all: Boolean) => Bitmap | Undefined */
function nextJob(fv, f, inp, need, all) {
  const keys = pageKeys(fv, f, inp);
  for (const pg of fv.pages)
    if (!keys.includes(pg.key)) {
      drop(pg.url);
    }
  fv.pages = fv.pages.filter((pg) => keys.includes(pg.key));
  for (const t of fv.tiles)
    if (!keys.includes(t.key.split("#")[0])) {
      drop(t.url);
    }
  fv.tiles = fv.tiles.filter((t) => keys.includes(t.key.split("#")[0]));
  const surf = state.project.animation.surface;
  if (fv.desk.length === 0 || fv.desk[0].key !== surf) {
    for (const d of fv.desk) drop(d.url);
    const d = { key: surf, url: "", scale: 1, used: now(), page: -1, box: [] };
    fv.desk = [d];
    return d;
  }
  if (!done(fv.desk[0])) return undefined;
  for (const b of fv.pages) if (!done(b)) return b;
  for (const b of fv.tiles) if (!done(b)) return b;
  /** function page(pass: Int) => Bitmap | Undefined */
  function page(pass) {
    for (let i = 0; i < f.lay.pages.length; i++) {
      const k = keys[i + 1];
      if (fv.pages.some((pg) => pg.key === k)) continue;
      if (pass === 0 && !need.some((nd) => nd[0] === i)) continue;
      const b = { key: k, url: "", scale: PAGE_SCALE, used: now(), page: i, box: [0, 0, f.lay.w, f.lay.h] };
      fv.pages.push(b);
      return b;
    }
    return undefined;
  }
  const inView = page(0);
  if (inView) return inView;
  const now0 = tile(
    fv,
    f,
    keys,
    need.filter((nd) => nd[4] < 0.5),
    all
  );
  if (now0) return now0;
  const rest = page(1);
  if (rest) return rest;
  return tile(fv, f, keys, need, all);
}

/** The next tile to draw for some needs (undefined: they are all there). */
/** function tile(fv: FilmView, f: Film, keys: String[], need: Number[][], all: Boolean) => Bitmap | Undefined */
function tile(fv, f, keys, need, all) {
  for (const nd of need) {
    const level = nd[1];
    if (level < 0) continue;
    const page = Math.round(nd[0]);
    const key = `${keys[page + 1]}#${level}/${nd[2]}/${nd[3]}`;
    const have = fv.tiles.find((t) => t.key === key);
    if (have) {
      have.used = now();
      continue;
    }
    // While playing, only what is needed now.
    if (!all && state.playing && nd[4] > 0.5) continue;
    while (fv.tiles.length >= TILES_MAX) {
      let old = 0;
      for (let i = 1; i < fv.tiles.length; i++) if (fv.tiles[i].used < fv.tiles[old].used) old = i;
      drop(fv.tiles[old].url);
      fv.tiles.splice(old, 1);
    }
    const size = TILE_PX / level;
    const x = nd[2] * size;
    const y = nd[3] * size;
    const b = { key: key, url: "", scale: level, used: now(), page: page, box: [x, y, Math.min(size, f.lay.w - x), Math.min(size, f.lay.h - y)] };
    fv.tiles.push(b);
    return b;
  }
  return undefined;
}

/** Draw a bitmap. */
/** function drawBitmap(fv: FilmView, f: Film, inp: FilmInput, b: Bitmap) => Promise<Boolean> */
async function drawBitmap(fv, f, inp, b) {
  const lay = f.lay;
  let svg = "";
  let w = 0;
  let h = 0;
  if (b.page < 0) {
    const t = surface(b.key);
    svg = t.svg;
    w = t.size;
    h = t.size;
  } else {
    const page = b.page;
    const box = b.box;
    // On plain paper: the renderer lays the paper's texture over it.
    svg = pagePart(lay, page, inp.info, paperLook(fv, inp), b.scale, box, false);
    w = Math.round(box[2] * b.scale);
    h = Math.round(box[3] * b.scale);
  }
  try {
    b.url = await rasterSvg(svg, w, h, "image/jpeg");
    return true;
  } catch (e) {
    b.url = "-";
    return false;
  }
}

/** Make the next bitmap the view needs, in the background. */
/** function schedule(fv: FilmView, f: Film, inp: FilmInput, need: Number[][]) => Undefined */
function schedule(fv, f, inp, need) {
  if (fv.busy || fv.export >= 0) return undefined;
  const b = nextJob(fv, f, inp, need, false);
  if (b === undefined) return undefined;
  fv.busy = true;
  drawBitmap(fv, f, inp, b)
    .then((ok) => {
      fv.busy = false;
      invalidate();
      return ok;
    })
    .catch((e) => {
      fv.busy = false;
      invalidate();
      return false;
    });
}

/** Make every bitmap a frame needs, now (exporting). */
/** function ensure(fv: FilmView, f: Film, inp: FilmInput, need: Number[][]) => Promise<Boolean> */
async function ensure(fv, f, inp, need) {
  for (let i = 0; i < 400; i++) {
    const b = nextJob(fv, f, inp, need, true);
    if (b === undefined) return true;
    await drawBitmap(fv, f, inp, b);
  }
  return true;
}

/** The lens's vertical field of view (radians), as web/lib/filmgl.js draws with it. */
const FOV = (32 * Math.PI) / 180;

/** The bitmap level that draws `px` pixels a point sharply: -1 for the page's own bitmap, else a tile level. */
/** function levelFor(px: Number) => Number */
function levelFor(px) {
  const need = px * 1.15;
  if (need <= PAGE_SCALE * 1.15) return -1;
  for (const l of TILE_LEVELS) if (l >= need) return l;
  return TILE_LEVELS[TILE_LEVELS.length - 1];
}

/** A convex polygon (x, y pairs) cut to the side of an axis-aligned line where `sign` · (coordinate − `at`) ≥ 0 (`axis` 0: x, 1: y). */
/** function cut(poly: Number[][], axis: Int, at: Number, sign: Number) => Number[][] */
function cut(poly, axis, at, sign) {
  /** const out: Number[][] */
  const out = [];
  for (let i = 0; i < poly.length; i++) {
    const p = poly[i];
    const q = poly[(i + 1) % poly.length];
    const dp = sign * (p[axis] - at);
    const dq = sign * (q[axis] - at);
    if (dp >= 0) out.push(p);
    if (dp >= 0 !== dq >= 0) {
      const u = dp / (dp - dq);
      out.push([p[0] + (q[0] - p[0]) * u, p[1] + (q[1] - p[1]) * u]);
    }
  }
  return out;
}

/** A polygon's area. */
/** function area(poly: Number[][]) => Number */
function area(poly) {
  let a = 0;
  for (let i = 0; i < poly.length; i++) {
    const p = poly[i];
    const q = poly[(i + 1) % poly.length];
    a = a + (p[0] * q[1] - q[0] * p[1]);
  }
  return Math.abs(a) / 2;
}

/** The part of a convex polygon inside the rectangle [x0, x1] × [y0, y1]. */
/** function within(poly: Number[][], x0: Number, y0: Number, x1: Number, y1: Number) => Number[][] */
function within(poly, x0, y0, x1, y1) {
  return cut(cut(cut(cut(poly, 0, x0, 1), 0, x1, -1), 1, y0, 1), 1, y1, -1);
}

/**
 * What a camera needs drawn: the pages it sees ([page, -1, 0, 0, later]), and
 * over them the tiles of each part in view at the level its own nearness
 * calls for ([page, level, tx, ty, later]). The camera is the renderer's
 * (web/lib/filmgl.js `camera`): leaning back, the near part of the picture is
 * closer than the far, so every part gets a bitmap as sharp as it shows —
 * near and far drawn alike, never the far part from the page's coarse one.
 */
/** function wants(f: Film, cam: Cam, w: Number, h: Number, later: Number, ratio: Number) => Number[][] */
function wants(f, cam, w, h, later, ratio) {
  /** const out: Number[][] */
  const out = [];
  const d = f.desk;
  const aspect = w / Math.max(1, h);
  const t = (cam.tilt * Math.PI) / 180;
  const r = (cam.turn * Math.PI) / 180;
  const dist = cam.span / 2 / Math.tan(FOV / 2) / aspect;
  const dx = -Math.sin(r);
  const dy = Math.cos(r);
  // The eye (z into the desk: above it is negative), where it looks, and the picture's axes.
  const eye = [cam.x + dx * dist * Math.sin(t), cam.y + dy * dist * Math.sin(t), -dist * Math.cos(t)];
  const fwd = [(cam.x - eye[0]) / dist, (cam.y - eye[1]) / dist, -eye[2] / dist];
  // Right: across the picture (level on the desk); up: the picture's up.
  const right = [Math.cos(r), Math.sin(r), 0];
  const up = [right[1] * fwd[2] - right[2] * fwd[1], right[2] * fwd[0] - right[0] * fwd[2], right[0] * fwd[1] - right[1] * fwd[0]];
  const ty = Math.tan(FOV / 2) * 1.1;
  const tx = ty * aspect;
  const far = dist * 20;
  // The picture on the desk: where the rays through its corners (a little beyond) meet it, cut at the far plane.
  /** const quad: Number[][] */
  const quad = [];
  for (const c of [
    [-1, -1],
    [1, -1],
    [1, 1],
    [-1, 1],
  ]) {
    const ray = [fwd[0] + right[0] * c[0] * tx + up[0] * c[1] * ty, fwd[1] + right[1] * c[0] * tx + up[1] * c[1] * ty, fwd[2] + up[2] * c[1] * ty];
    const reach = ray[2] > 1e-6 ? Math.min(far, -eye[2] / ray[2]) : far;
    quad.push([eye[0] + ray[0] * reach, eye[1] + ray[1] * reach]);
  }
  /** Pixels a point at a point of the desk (across the picture, where nothing is foreshortened). */
  /** function sharpness(x: Number, y: Number) => Number */
  function sharpness(x, y) {
    const z = (x - eye[0]) * fwd[0] + (y - eye[1]) * fwd[1] - eye[2] * fwd[2];
    return z > 1e-6 ? (h * ratio) / 2 / (z * Math.tan(FOV / 2)) : 0;
  }
  /** const tiles: Number[][] */
  const tiles = [];
  for (let p = 0; p < d.pages.length; p++) {
    const pg = d.pages[p];
    const a = (-pg.rot * Math.PI) / 180;
    const ca = Math.cos(a);
    const sa = Math.sin(a);
    // The picture in the page's points, and back.
    /** const local: Number[][] */
    const local = [];
    for (const c of quad) {
      const qx = c[0] - pg.x;
      const qy = c[1] - pg.y;
      local.push([qx * ca - qy * sa + d.pw / 2, qx * sa + qy * ca + d.ph / 2]);
    }
    /** function nearness(lx: Number, ly: Number) => Number */
    function nearness(lx, ly) {
      const ux = lx - d.pw / 2;
      const uy = ly - d.ph / 2;
      return sharpness(pg.x + ux * ca + uy * sa, pg.y - ux * sa + uy * ca);
    }
    /** The finest and coarsest levels a part of the page (a convex polygon in its points) calls for: depth is linear over the desk, so its corners tell. */
    /** function levels(poly: Number[][]) => Number[] */
    function levels(poly) {
      let lo = Infinity;
      let hi = 0;
      for (const v of poly) {
        const s = nearness(v[0], v[1]);
        lo = Math.min(lo, s);
        hi = Math.max(hi, s);
      }
      return [levelFor(lo), levelFor(hi)];
    }
    const seen = within(local, 0, 0, d.pw, d.ph);
    if (seen.length < 3) continue;
    out.push([p, -1, 0, 0, later]);
    const span = levels(seen);
    if (span[1] < 0) continue;
    for (const level of TILE_LEVELS) {
      if (level < span[0] || level > span[1]) continue;
      const size = TILE_PX / level;
      let x0 = Infinity;
      let y0 = Infinity;
      let x1 = -Infinity;
      let y1 = -Infinity;
      for (const v of seen) {
        x0 = Math.min(x0, v[0]);
        y0 = Math.min(y0, v[1]);
        x1 = Math.max(x1, v[0]);
        y1 = Math.max(y1, v[1]);
      }
      // The tiles of the page only (the view, cut to the page, can stray a rounding error past its edge).
      for (let tyi = Math.max(0, Math.floor(y0 / size)); tyi * size < Math.min(y1, d.ph); tyi++)
        for (let txi = Math.max(0, Math.floor(x0 / size)); txi * size < Math.min(x1, d.pw); txi++) {
          // A tile is drawn where some part of it in view (not a mere sliver along its edge) calls for its level.
          const part = within(seen, txi * size, tyi * size, (txi + 1) * size, (tyi + 1) * size);
          if (part.length < 3 || area(part) < size * size * 1e-6) continue;
          const lv = levels(part);
          if (level < lv[0] || level > lv[1]) continue;
          let near = 0;
          for (const v of part) near = Math.max(near, nearness(v[0], v[1]));
          tiles.push([p, level, txi, tyi, later, near]);
        }
    }
  }
  // The coarser first (the whole picture soon), then the nearest.
  tiles.sort((x, y) => x[1] - y[1] || y[5] - x[5]);
  for (const t of tiles.slice(0, later > 0.5 ? 6 : TILES_MAX - 8)) out.push([t[0], t[1], t[2], t[3], t[4]]);
  return out;
}

// ------------------------------------------------------------------ the camera

/** The camera to show at a beat: the film's, with the look by hand (paused), gliding over jumps of the playhead. */
/** function presented(fv: FilmView, f: Film, beat: Number) => Cam */
function presented(fv, f, beat) {
  const cam = cameraAt(f, beat);
  const lk = fv.look;
  if (!state.playing && (lk.dx !== 0 || lk.dy !== 0 || lk.zoom !== 1 || lk.turn !== 0 || lk.tilt !== 0)) {
    const th = (cam.turn * Math.PI) / 180;
    const dx = lk.dx * cam.span;
    const dy = (lk.dy * cam.span) / f.aspect;
    cam.x = cam.x + dx * Math.cos(th) - dy * Math.sin(th);
    cam.y = cam.y + dx * Math.sin(th) + dy * Math.cos(th);
    cam.span = cam.span / lk.zoom;
    cam.turn = cam.turn + lk.turn;
    cam.tilt = clampNum(cam.tilt + lk.tilt, 0, 75);
  }
  // A jump (a seek, a repeat): glide there in a moment instead of cutting.
  const k = f.scenes[Math.max(0, sceneAt(f.scenes, beat))];
  const cut = f.scenes.length > 0 && k.transition === "cut" && beat - k.start >= 0 && beat - k.start < 0.25;
  if (fv.last.length > 0 && Math.abs(beat - fv.lastBeat) > 0.6 && !cut) {
    fv.from = [fv.last[0]];
    fv.fromAt = now();
  }
  let out = cam;
  if (fv.from.length > 0) {
    const u = (now() - fv.fromAt) / 450;
    if (u >= 1) fv.from = [];
    else out = mixCam(fv.from[0], cam, ease("smooth", u));
  }
  fv.last = [out];
  fv.lastBeat = beat;
  return out;
}

// ------------------------------------------------------------------ editing

/** The shot selected, if it still exists. */
/** function selShot(fv: FilmView) => Shot[] */
function selShot(fv) {
  const shots = state.project.animation.shots;
  return fv.shot >= 0 && fv.shot < shots.length ? [shots[fv.shot]] : [];
}

/** A new shot from a scene of the film (the director's, or a shot already written). */
/** function shotOf(f: Film, i: Int) => Shot */
function shotOf(f, i) {
  return bake(f.sc, [f.scenes[i]])[0];
}

/** Write a new shot (manual mode) and select it. */
/** function addShot(fv: FilmView, s: Shot) => Undefined */
function addShot(fv, s) {
  commit(() => {
    const a = anim();
    a.mode = "manual";
    a.shots.push(s);
    a.shots.sort((x, y) => x.start - y.start);
  });
  fv.shot = state.project.animation.shots.indexOf(s);
  fv.auto = NaN;
}

/** A new shot at a beat: the bars around it, framed as the director frames them. */
/** function shotAt(fv: FilmView, f: Film, beat: Number) => Undefined */
function shotAt(fv, f, beat) {
  const i = sceneAt(f.scenes, beat);
  const bar = f.bar;
  const start = Math.floor(beat / bar) * bar;
  const s = i >= 0 ? shotOf(f, i) : decodeShot(JSON.parse(`{ "start": 0, "end": 4 }`));
  s.start = start;
  s.end = start + bar * 2;
  s.label = "";
  s.transition = "";
  s.glide = NaN;
  addShot(fv, s);
}

/** Change the selected shot in a gesture (one undo step until it ends). */
/** function editShot(fv: FilmView, fn: (Shot) => Undefined) => Undefined */
function editShot(fv, fn) {
  const sel = selShot(fv);
  if (sel.length === 0) return undefined;
  if (!fv.gesture) {
    begin();
    fv.gesture = true;
  }
  anim();
  fn(sel[0]);
  changed(true);
}

/** End a gesture. */
/** function endEdit(fv: FilmView) => Undefined */
function endEdit(fv) {
  fv.gesture = false;
}

/** Change a film setting (one undo step). */
/** function editFilm(fn: (Animation) => Undefined) => Undefined */
function editFilm(fn) {
  commit(() => fn(anim()));
}

/** Set an effect's amount in a list of effects. */
/** function setEffect(list: FilmEffect[], type: String, amount: Number) => Undefined */
function setEffect(list, type, amount) {
  const e = list.find((x) => x.type === type);
  if (e) e.amount = amount;
  else list.push({ type: type, amount: amount });
}

// ------------------------------------------------------------------ pointer

/** Drag on the picture: frame the selected shot (paused, manual), or look about by hand. Shift (or the right button): turn and lean. */
/** function onStageDown(e: Ev, fv: FilmView, f: Film, beat: Number) => Undefined */
function onStageDown(e, fv, f, beat) {
  e.preventDefault();
  const x0 = e.clientX;
  const y0 = e.clientY;
  const orbit = e.shiftKey || e.button === 2;
  const sel = selShot(fv);
  const editing = sel.length > 0 && !state.playing && state.project.animation.mode === "manual";
  const lk0 = { dx: fv.look.dx, dy: fv.look.dy, zoom: fv.look.zoom, turn: fv.look.turn, tilt: fv.look.tilt };
  const s0 = editing ? sel[0] : decodeShot(JSON.parse(`{ "start": 0, "end": 1 }`));
  const off0 = s0.offset.length === 2 ? [s0.offset[0], s0.offset[1]] : [0, 0];
  const to0 = s0.to.offset.length === 2 ? [s0.to.offset[0], s0.to.offset[1]] : [];
  const k = editing ? f.scenes.find((t) => t.src === fv.shot) : undefined;
  const turn0 = k ? k.turn : 0;
  const tilt0 = k ? k.tilt : 0;
  const turn1 = k ? k.turn1 : 0;
  const tilt1 = k ? k.tilt1 : 0;
  drag(
    e,
    (m) => {
      const dx = (m.clientX - x0) / Math.max(1, fv.width);
      const dy = (m.clientY - y0) / Math.max(1, fv.height);
      if (!editing) {
        if (orbit) {
          fv.look.turn = lk0.turn - dx * 120;
          fv.look.tilt = lk0.tilt + dy * 80;
        } else {
          fv.look.dx = lk0.dx - dx / fv.look.zoom;
          fv.look.dy = lk0.dy - dy / fv.look.zoom;
        }
        hint("Looking about by hand — double-click to go back to the film's camera. Select a shot (Manual) to frame it.");
        invalidate();
        return undefined;
      }
      editShot(fv, (s) => {
        if (orbit) {
          s.turn = Math.round((turn0 - dx * 120) * 10) / 10;
          s.tilt = clampNum(Math.round((tilt0 + dy * 80) * 10) / 10, 0, 75);
          if (Number.isFinite(s.to.turn)) s.to.turn = Math.round((turn1 - dx * 120) * 10) / 10;
          if (Number.isFinite(s.to.tilt)) s.to.tilt = clampNum(Math.round((tilt1 + dy * 80) * 10) / 10, 0, 75);
        } else {
          const z = Number.isFinite(s.zoom) ? s.zoom : 1;
          s.offset = [clampNum(Math.round((off0[0] - dx / z) * 1000) / 1000, -2, 2), clampNum(Math.round((off0[1] - dy / z) * 1000) / 1000, -2, 2)];
          if (to0.length === 2)
            s.to.offset = [clampNum(Math.round((to0[0] - dx / z) * 1000) / 1000, -2, 2), clampNum(Math.round((to0[1] - dy / z) * 1000) / 1000, -2, 2)];
        }
      });
      hint(orbit ? "Turning and leaning the shot's camera" : "Moving the shot's frame — Shift-drag turns and leans it, the wheel zooms");
    },
    (u) => endEdit(fv)
  );
}

/** The wheel on the picture: zoom the selected shot (paused, manual), or look closer by hand. */
/** function onStageWheel(e: Ev, fv: FilmView) => Undefined */
function onStageWheel(e, fv) {
  e.preventDefault();
  const r = e.deltaY < 0 ? 1.1 : 1 / 1.1;
  const sel = selShot(fv);
  if (sel.length > 0 && !state.playing && state.project.animation.mode === "manual") {
    editShot(fv, (s) => {
      s.zoom = clampNum(Math.round((Number.isFinite(s.zoom) ? s.zoom : 1) * r * 1000) / 1000, 0.1, 10);
      if (Number.isFinite(s.to.zoom)) s.to.zoom = clampNum(Math.round(s.to.zoom * r * 1000) / 1000, 0.1, 10);
    });
    endEdit(fv);
    return undefined;
  }
  fv.look.zoom = clampNum(fv.look.zoom * r, 0.2, 12);
  invalidate();
}

/** Press on the timeline: seek (empty), select a scene, move or stretch a shot. */
/** function onStripDown(e: Ev, fv: FilmView, f: Film, inp: FilmInput, i: Int, part: String) => Undefined */
function onStripDown(e, fv, f, inp, i, part) {
  e.preventDefault();
  e.stopPropagation();
  const total = Math.max(1, f.end);
  const w = Math.max(1, fv.width);
  const k = f.scenes[i];
  if (k.src < 0) {
    fv.shot = -1;
    fv.auto = k.start;
    fv.side = true;
    const px = e.clientX - e.targetLeft;
    inp.seek(k.start + (px / Math.max(1, e.targetWidth)) * (k.end - k.start));
    invalidate();
    return undefined;
  }
  fv.shot = k.src;
  fv.auto = NaN;
  fv.side = true;
  const s = state.project.animation.shots[k.src];
  const a0 = s.start;
  const z0 = s.end;
  const x0 = e.clientX;
  let moved = false;
  drag(
    e,
    (m) => {
      const db = ((m.clientX - x0) / w) * total;
      if (!moved && Math.abs(m.clientX - x0) < 4) return undefined;
      moved = true;
      // By beats (Alt: free), by bars with Shift.
      const step = m.altKey ? 0.01 : m.shiftKey ? f.bar : 1;
      const d = Math.round(db / step) * step;
      editShot(fv, (sh) => {
        if (part === "start") sh.start = clampNum(a0 + d, 0, z0 - step);
        else if (part === "end") sh.end = Math.max(a0 + step, z0 + d);
        else {
          sh.start = Math.max(0, a0 + d);
          sh.end = sh.start + (z0 - a0);
        }
      });
      const sel = selShot(fv);
      if (sel.length > 0) hint(`Shot: bar ${barBeat(sel[0].start, f.bar)} to ${barBeat(sel[0].end, f.bar)} — Shift: by bars, Alt: free`);
    },
    (u) => {
      endEdit(fv);
      if (!moved) inp.seek(a0);
    }
  );
}

// ------------------------------------------------------------------ view

/** The Film view in the score's place. */
/** function filmView(b: Builder, fv: FilmView, inp: FilmInput) => Undefined */
export function filmView(b, fv, inp) {
  const a = state.project.animation;
  const f = filmFor(fv, inp);
  const beat = inp.showing ? (state.playing ? livePosition() : state.position) : 0;
  const cam = presented(fv, f, beat);
  const ti = sceneAt(f.scenes, beat);
  const scene = ti >= 0 ? f.scenes[ti] : undefined;

  // Bitmaps: what the camera needs now, then soon.
  /** const need: Number[][] */
  const ratio = (fv.off.includes("sharp") ? 1 : Math.min(2, pixelRatio())) * (fv.off.includes("detail") ? 0.5 : 1);
  const need = wants(f, cam, fv.width, fv.height, 0, ratio);
  for (const ahead of [f.bar * 0.5, f.bar]) for (const nd of wants(f, cameraAt(f, beat + ahead), fv.width, fv.height, 1, ratio)) need.push(nd);
  schedule(fv, f, inp, need);

  b.open("div", "film", `film surface-${a.surface}${fv.cinema ? " cinema" : ""}${fv.side && !fv.cinema ? " with-side" : ""}`);
  b.open("div", "main", "film-main");
  if (!fv.cinema) ribbon(b, fv, f, inp);
  stage(b, fv, f, inp, cam, beat, scene);
  if (!fv.cinema) timeline(b, fv, f, inp, beat, ti);
  b.close();
  if (fv.side && !fv.cinema) side(b, fv, f, inp, ti);
  b.close();

  tellAgent(fv, f, scene);
  // While the music plays or the camera glides, the picture draws itself every
  // frame, at the live playhead; the page around it (the timeline, the words
  // over the picture, the bitmaps wanted next) is rebuilt ten times a second.
  // Rebuilding all of it every frame is what slows the picture down.
  // The loop reads the film as last shown, so it stops (and the page's frame
  // is drawn) as soon as the film is no longer showing or another replaces it.
  fv.shown = [f];
  fv.shownInp = [inp];
  if (inp.showing && fv.export < 0 && (state.playing || fv.from.length > 0)) {
    filmLive(`canvas[data-film="${fv.id}"]`, () => liveFrame(fv));
    if (!fv.ticking) {
      fv.ticking = true;
      setTimeout(() => {
        fv.ticking = false;
        invalidate();
      }, 100);
    }
  }
  if (fv.export >= 0) invalidate();
}

/** The picture now, while it moves by itself (the music playing, the camera gliding) in the film last shown; none once it rests. */
/** function liveFrame(fv: FilmView) => GlFrame | Undefined */
function liveFrame(fv) {
  if (fv.shown.length === 0 || fv.shownInp.length === 0) return undefined;
  const f = fv.shown[0];
  const inp = fv.shownInp[0];
  if (!inp.showing || fv.export >= 0 || !(state.playing || fv.from.length > 0)) return undefined;
  const beat = state.playing ? livePosition() : state.position;
  const cam = presented(fv, f, beat);
  const ti = sceneAt(f.scenes, beat);
  return glFrame(fv, f, inp, cam, beat, ti >= 0 ? f.scenes[ti] : undefined, state.playing || state.position > 0, fv.off);
}

/** A frame of the film for the renderer: the camera, the desk, the pages and their bands, the notes lit, the light. */
/** function glFrame(fv: FilmView, f: Film, inp: FilmInput, cam: Cam, beat: Number, scene: Scene | Undefined, lit: Boolean, off: String[]) => GlFrame */
function glFrame(fv, f, inp, cam, beat, scene, lit, off) {
  const d = f.desk;
  const m = Math.max(d.pw, d.ph) * 3;
  const surf = surface(state.project.animation.surface);
  const deskBm = fv.desk.length > 0 ? fv.desk[0] : undefined;
  /** const sheets: GlSheet[] */
  const sheets = [];
  /** function corners(p: Int, x: Number, y: Number, w: Number, h: Number) => Number[] */
  function corners(p, x, y, w, h) {
    const a = onDesk(d, p, x, y);
    const b = onDesk(d, p, x + w, y);
    const c = onDesk(d, p, x + w, y + h);
    const e = onDesk(d, p, x, y + h);
    return [a[0], a[1], b[0], b[1], c[0], c[1], e[0], e[1]];
  }
  /** function url(u: String) => String */
  function url(u) {
    return u === "-" ? "" : u;
  }
  for (let i = 0; i < f.lay.pages.length; i++) {
    const key = fv.pageKeys.length > i + 1 ? fv.pageKeys[i + 1] : "";
    const bm = fv.pages.find((x) => x.key === key);
    sheets.push({
      quad: corners(i, 0, 0, d.pw, d.ph),
      color: bm ? url(bm.url) : "",
      box: [0, 0, d.pw, d.ph],
      scale: PAGE_SCALE,
      rot: d.pages[i].rot,
      page: true,
    });
  }
  // The sharper tiles over the pages, the finest last.
  const tiles = fv.tiles.filter(
    (t) => t.url !== "" && t.url !== "-" && t.page < f.lay.pages.length && fv.pageKeys.length > t.page + 1 && t.key.startsWith(`${fv.pageKeys[t.page + 1]}#`)
  );
  tiles.sort((x, y) => x.scale - y.scale);
  for (const t of tiles) {
    const b = t.box;
    sheets.push({ quad: corners(t.page, b[0], b[1], b[2], b[3]), color: t.url, box: t.box, scale: t.scale, rot: d.pages[t.page].rot, page: false });
  }
  /** const glow: Number[] */
  const glow = [];
  const fx = {
    vignette: off.includes("vignette") ? 0 : cam.fx.vignette,
    spotlight: off.includes("spotlight") ? 0 : cam.fx.spotlight,
    glow: off.includes("glow") ? 0 : cam.fx.glow,
  };
  if (lit && fx.glow > 0.01) {
    // The notes playing (the brightest first, as many as the renderer lights).
    const playing = sparks(f, beat, scene ? scene.staves : []);
    playing.sort((x, y) => y.a - x.a);
    for (const s of playing.slice(0, 48)) {
      const c = onDesk(d, s.page, s.x, s.y);
      for (const x of [c[0], c[1], s.r, Math.min(1, s.a * fx.glow)]) glow.push(x);
    }
  }
  // The lamp: up and to the left of what the camera looks at, high above the desk.
  const lx = -0.55 * cam.span;
  const ly = -0.75 * cam.span;
  return {
    width: fv.width,
    height: fv.height,
    cam: [cam.x, cam.y, cam.span, cam.tilt, cam.turn],
    desk: [d.x0 - m, d.y0 - m, d.x1 + m, d.y1 + m],
    deskColor: surf.color,
    deskTex: deskBm && deskBm.url !== "-" ? deskBm.url : "",
    deskTile: d.pw * 0.9,
    paper: PAPER.color,
    // The paper's texture, laid over the plain paper of the bitmaps: its tiles, each this many points across.
    paperTex: off.includes("paper") ? [] : [PAPER.tooth.url, PAPER.mottle.url, PAPER.grain.url],
    paperSize: [PAPER.tooth.size, PAPER.mottle.size, PAPER.grain.size].map((x) => (x * f.lay.sp) / 7),
    pageSize: [d.pw, d.ph, f.lay.sp],
    sheets: sheets,
    sparks: glow,
    spot: [cam.rx, cam.ry, Math.max(cam.rw, d.pw * 0.55) * 0.62, cam.rh * 0.72, fx.spotlight],
    light: [cam.x + lx, cam.y + ly, 1.15 * cam.span],
    fx: [fx.vignette, fx.glow],
    seed: Math.floor(beat * 97) % 1000,
    finish: !off.includes("finish"),
    ratio: off.includes("sharp") ? 1 : 2,
  };
}

/** Keep the agent's context up to date with the scene at the playhead (and the film being shown). */
/** function tellAgent(fv: FilmView, f: Film, scene: Scene | Undefined) => Undefined */
function tellAgent(fv, f, scene) {
  const fm = state.film;
  const mode = state.project.animation.mode;
  const start = scene ? scene.start : 0;
  const src = scene ? scene.src : -1;
  if (fm.on && fm.mode === mode && fm.shot === src && fm.selected === fv.shot && Math.abs(fm.start - start) < 1e-9) return undefined;
  state.film = {
    on: true,
    mode: mode,
    shot: src,
    selected: fv.shot,
    start: start,
    end: scene ? scene.end : 0,
    frame: scene ? scene.frame : "",
    focus: scene && scene.staves.length > 0 ? channelsOf(f.sc, scene.staves) : [],
    why: scene ? scene.why : "",
  };
  reportContext();
}

/** The picture (drawn by the renderer on its canvas) and what is written over it. */
/** function stage(b: Builder, fv: FilmView, f: Film, inp: FilmInput, cam: Cam, beat: Number, scene: Scene | Undefined) => Undefined */
function stage(b, fv, f, inp, cam, beat, scene) {
  b.open("div", "stage", "film-stage");
  b.on("resize", (e) => {
    if (Math.abs(fv.width - e.targetWidth) > 0.5 || Math.abs(fv.height - e.targetHeight) > 0.5) {
      fv.width = e.targetWidth;
      fv.height = e.targetHeight;
      invalidate();
    }
  });
  b.on("pointerdown", (e) => onStageDown(e, fv, f, beat));
  b.on("wheel", (e) => onStageWheel(e, fv));
  b.on("contextmenu", (e) => e.preventDefault());
  b.on("dblclick", (e) => {
    fv.look = { dx: 0, dy: 0, zoom: 1, turn: 0, tilt: 0 };
    invalidate();
  });
  b.leaf("canvas", "gl", "film-canvas", "");
  b.attr("data-film", fv.id);
  filmDraw(`canvas[data-film="${fv.id}"]`, glFrame(fv, f, inp, cam, beat, scene, inp.showing && (state.playing || state.position > 0), fv.off));

  // What is filmed.
  if (scene) {
    b.open("div", "hud", "film-hud");
    b.leaf("span", "why", "film-why", scene.why !== "" ? scene.why : scene.src >= 0 ? `Shot ${scene.src + 1}` : "");
    b.leaf("span", "what", "film-what", `${scene.frame}${scene.src >= 0 ? " · shot" : " · auto"}`);
    b.close();
  }
  const ready = fv.pages.filter((pg) => done(pg)).length;
  if (fv.export >= 0) b.leaf("div", "loading", "film-loading", `Filming the video… ${Math.round(fv.export * 100)}%`);
  else if (ready < f.lay.pages.length) b.leaf("div", "loading", "film-loading", `Laying out the pages… ${ready}/${f.lay.pages.length}`);
  if (f.sc.empty) b.leaf("div", "empty", "film-empty", "Nothing to film yet: write some notes first.");
  if (fv.cinema) {
    iconButton(b, "exit", "small film-exit", "close", "Leave the cinema (back to the editor)", () => {
      fv.cinema = false;
      invalidate();
    });
  }
  b.close();
}

// ------------------------------------------------------------------ export

/** The frame at the playhead as a picture (1920×1080 PNG), drawn as on screen. */
/** function exportStill(fv: FilmView, f: Film, inp: FilmInput) => Undefined */
function exportStill(fv, f, inp) {
  if (fv.export >= 0) return undefined;
  fv.export = 0;
  invalidate();
  toast("Drawing the frame", "At 1920×1080.", "info");
  filmStill(fv, f, inp)
    .then((url) => {
      fv.export = -1;
      const beat = inp.showing ? state.position : 0;
      const name = `${inp.info.title.replace(/[^A-Za-z0-9 _-]+/g, "").trim() || "film"} - bar ${Math.floor(beat / f.bar) + 1}.png`;
      download(url, name);
      toast("Picture saved", name, "info");
      invalidate();
      return true;
    })
    .catch((e) => {
      fv.export = -1;
      toast("The picture could not be made", String(e), "error");
      invalidate();
      return false;
    });
}

/**
 * A view at the export's size, drawing everything whatever the screen leaves
 * out. It shares `fv`'s bitmaps (and what it makes stays for `fv`, see
 * exported) when they are drawn alike; with the screen's ink plain, the pages
 * differ, and it makes its own.
 */
/** function exportView(fv: FilmView) => FilmView */
function exportView(fv) {
  const vf = newFilmView(`${fv.id}-export`);
  vf.width = VIDEO_W;
  vf.height = VIDEO_H;
  vf.off = [];
  vf.desk = fv.desk;
  if (sharesPages(fv)) {
    vf.sig = fv.sig;
    vf.pageKeys = fv.pageKeys;
    vf.pages = fv.pages;
    vf.tiles = fv.tiles;
  }
  return vf;
}

/** Whether an export can use the bitmaps of the pages on screen (they are drawn as the export draws them). */
/** function sharesPages(fv: FilmView) => Boolean */
function sharesPages(fv) {
  return !fv.off.includes("ink");
}

/** An export is done with its view: what it made stays for `fv` when shared (`shared`: as exportView found it), or is let go. */
/** function exported(fv: FilmView, vf: FilmView, shared: Boolean) => Undefined */
function exported(fv, vf, shared) {
  fv.desk = vf.desk;
  if (shared) {
    fv.pages = vf.pages;
    fv.tiles = vf.tiles;
    return undefined;
  }
  for (const b of vf.pages) drop(b.url);
  for (const b of vf.tiles) drop(b.url);
  vf.pages = [];
  vf.tiles = [];
  return undefined;
}

/** The frame at the playhead, at the export's size, with every bitmap it needs. */
/** function filmStill(fv: FilmView, f: Film, inp: FilmInput) => Promise<String> */
async function filmStill(fv, f, inp) {
  const ef = replan(f, state.project.animation, VIDEO_W / VIDEO_H);
  const shared = sharesPages(fv);
  const vf = exportView(fv);
  const beat = inp.showing ? state.position : 0;
  const cam = cameraAt(ef, beat);
  await ensure(vf, ef, inp, wants(ef, cam, VIDEO_W, VIDEO_H, 0, 1)).catch((e) => {
    exported(fv, vf, shared);
    throw e;
  });
  const si = sceneAt(ef.scenes, beat);
  const frame = glFrame(vf, ef, inp, cam, beat, si >= 0 ? ef.scenes[si] : undefined, beat > 0, []);
  const url = await renderStill(frame).catch((e) => {
    exported(fv, vf, shared);
    throw e;
  });
  exported(fv, vf, shared);
  return url;
}

/** Film the song frame by frame into an MP4 (1920×1080, 30 frames a second) with its mixdown, and download it; `clip`: only fifteen seconds from the playhead. */
/** function exportVideo(fv: FilmView, f: Film, inp: FilmInput, clip: Boolean) => Undefined */
function exportVideo(fv, f, inp, clip) {
  if (fv.export >= 0) return undefined;
  if (f.sc.empty) {
    toast("Nothing to film", "Write some notes first.", "warn");
    return undefined;
  }
  fv.export = 0;
  invalidate();
  toast("Filming the song", "First the mixdown, then the pictures, frame by frame (it plays out faster or slower than the song).", "info");
  filmVideo(fv, f, inp, clip)
    .then((r) => {
      fv.export = -1;
      const name = `${inp.info.title.replace(/[^A-Za-z0-9 _-]+/g, "").trim() || "film"}${clip ? " (clip)" : ""}.mp4`;
      download(r.url, name);
      toast("Film exported", `${name} — ${VIDEO_W}×${VIDEO_H}, ${r.codecs}.`, "info");
      invalidate();
      return true;
    })
    .catch((e) => {
      fv.export = -1;
      toast("The film could not be made", String(e), "error");
      invalidate();
      return false;
    });
}

/** The film as a video: the mixdown's sound, and a frame for each 30th of a second of it (`clip`: fifteen seconds from the playhead). */
/** function filmVideo(fv: FilmView, f: Film, inp: FilmInput, clip: Boolean) => Promise<Encoded> */
async function filmVideo(fv, f, inp, clip) {
  const p = state.project;
  const bpm = p.transport.bpm;
  const spans = performance(inp.showing && state.mode === "song" ? p.repeats : [], f.end);
  let beats = 0;
  for (const s of spans) beats = beats + (s[1] - s[0]);
  // Where the clip starts, in played beats (the first time the playhead's bar plays).
  let start = 0;
  if (clip) {
    let acc = 0;
    for (const s of spans) {
      if (state.position >= s[0] && state.position < s[1]) {
        start = acc + state.position - s[0];
        break;
      }
      acc = acc + (s[1] - s[0]);
    }
  }
  /** let sound: Decoded */
  let sound = { sampleRate: 48000, channels: [], duration: 0 };
  try {
    const r = await sendJson("/api/render", "POST", { bits: 16 });
    sound = await decodeAudioUrl(String(r.url));
  } catch (e) {
    toast("No sound", `The mixdown could not be rendered (${String(e)}): the film is silent.`, "warn");
  }
  const offset = (start * 60) / bpm;
  const seconds = clip ? 15 : Math.max((beats * 60) / bpm + 1.5, sound.duration);
  const frames = Math.ceil(seconds * VIDEO_FPS);
  const ef = replan(f, p.animation, VIDEO_W / VIDEO_H);
  // Share the bitmaps the view has made (and the ones made now stay for it).
  const shared = sharesPages(fv);
  const vf = exportView(fv);
  /** function frameAt(i: Int) => Promise<GlFrame> */
  async function frameAt(i) {
    const beat = writtenAt(spans, start + ((i / VIDEO_FPS) * bpm) / 60);
    const cam = cameraAt(ef, beat);
    await ensure(vf, ef, inp, wants(ef, cam, VIDEO_W, VIDEO_H, 0, 1));
    const si = sceneAt(ef.scenes, beat);
    fv.export = i / frames;
    if (i % 15 === 0) invalidate();
    return glFrame(vf, ef, inp, cam, beat, si >= 0 ? ef.scenes[si] : undefined, beat > 0, []);
  }
  const keep = () => exported(fv, vf, shared);
  const out = await encodeFilm(VIDEO_W, VIDEO_H, VIDEO_FPS, frames, frameAt, sound, offset, (x) => {
    fv.export = x;
  }).catch((e) => {
    keep();
    throw e;
  });
  keep();
  return out;
}

// ------------------------------------------------------------------ ribbon

/** The film's ribbon: back to the paper, play, auto or manual, energy, the desk, shots, the panel, the cinema. */
/** function ribbon(b: Builder, fv: FilmView, f: Film, inp: FilmInput) => Undefined */
function ribbon(b, fv, f, inp) {
  const a = state.project.animation;
  b.open("div", "ribbon", "score-ribbon film-ribbon");
  b.open("div", "g1", "score-group");
  iconButton(b, "back", "small", "score", "Back to the paper (the score as a page to read and edit)", () => inp.back());
  iconButton(b, "play", state.playing ? "small on" : "small", state.playing ? "pause" : "play", state.playing ? "Pause (Space)" : "Play the film (Space)", () =>
    togglePlay()
  );
  b.leaf("span", "t", "film-title", "Film");
  b.close();
  b.open("div", "mode", "film-modes");
  for (const m of ["auto", "manual"]) {
    b.leaf("button", m, a.mode === m ? "film-mode on" : "film-mode", m === "auto" ? "Auto" : "Manual");
    b.attr(
      "title",
      m === "auto"
        ? "Auto: the camera directs itself — mostly the full score, following a part for a while as it comes in or takes the lead"
        : "Manual: the camera films your shots (from the timeline, the panel or the agent), and directs itself between them"
    );
    b.on("click", (e) => {
      if (a.mode !== m) editFilm((x) => (x.mode = m));
    });
  }
  b.close();
  b.leaf("span", "sp", "score-spacer", "");
  b.open("div", "g2", "score-group");
  b.open("label", "energy", "score-ink-knob");
  b.attr("title", "Energy: how much the camera moves on its own — calm, long shots, or restless and close");
  b.leaf("span", "l", "label", "Energy");
  b.leaf("input", "in", "score-slider", "");
  b.attr("type", "range");
  b.attr("min", "0");
  b.attr("max", "1");
  b.attr("step", "0.05");
  b.prop("value", fmt(a.energy, 2));
  b.on("input", (e) => {
    if (!fv.gesture) {
      begin();
      fv.gesture = true;
    }
    anim().energy = Number(e.value);
    changed(true);
  });
  b.on("change", (e) => endEdit(fv));
  b.close();
  b.leaf("span", "dl", "label", "Desk");
  select(
    b,
    "surface",
    "",
    a.surface,
    SURFACES,
    SURFACES.map((x) => x.slice(0, 1).toUpperCase() + x.slice(1)),
    "The desk the pages lie on",
    (val) => editFilm((x) => (x.surface = val))
  );
  iconButton(b, "add", "small", "plus", "Add a shot here (two bars, framed as the director would)", () => shotAt(fv, f, state.position));
  b.open("button", "mp4", fv.export >= 0 ? "btn small icon on" : "btn small icon");
  b.attr("title", "Export the film as an MP4 video (1920×1080, with the mixdown) — Shift: a 15-second clip from the playhead");
  b.attr("aria-label", "Export the film as an MP4 video");
  b.on("pointerenter", (e) => hint("MP4: the whole film as a video with its sound · Shift-click: fifteen seconds from the playhead"));
  b.on("click", (e) => exportVideo(fv, f, inp, e.shiftKey));
  glyph(b, "export");
  b.close();
  iconButton(b, "still", "small", "camera", "Save this frame as a picture (1920×1080 PNG)", () => exportStill(fv, f, inp));
  iconButton(b, "cinema", "small", "fullscreen", "Cinema: just the picture, full screen", () => {
    fv.cinema = true;
    invalidate();
    toggleFullscreen(".film.cinema");
  });
  iconButton(b, "side", fv.side ? "small on" : "small", "sidebar", "Shots and effects", () => {
    fv.side = !fv.side;
    invalidate();
  });
  b.close();
  b.close();
}

// ------------------------------------------------------------------ timeline

/** The scenes along the song: the director's and the shots, the playhead; click to play from there. */
/** function timeline(b: Builder, fv: FilmView, f: Film, inp: FilmInput, beat: Number, ti: Int) => Undefined */
function timeline(b, fv, f, inp, beat, ti) {
  const total = Math.max(1, f.end);
  b.open("div", "timeline", "film-timeline");
  b.on("pointerdown", (e) => {
    const x = e.clientX - e.targetLeft;
    fv.shot = -1;
    fv.auto = NaN;
    inp.seek(clampNum((x / Math.max(1, e.targetWidth)) * total, 0, total));
    invalidate();
  });
  b.on("dblclick", (e) => {
    const x = e.clientX - e.targetLeft;
    shotAt(fv, f, clampNum((x / Math.max(1, e.targetWidth)) * total, 0, total));
  });
  // Bar lines every few bars.
  const every = Math.max(1, Math.ceil(total / f.bar / 24));
  for (let bar = 0; bar * f.bar < total; bar = bar + every) {
    b.leaf("span", `b${bar}`, "film-bar", `${bar + 1}`);
    b.style("left", `${fmt(((bar * f.bar) / total) * 100, 3)}%`);
  }
  for (let i = 0; i < f.scenes.length; i++) {
    const k = f.scenes[i];
    const sel = (k.src >= 0 && k.src === fv.shot) || (k.src < 0 && Math.abs(k.start - fv.auto) < 1e-6);
    b.open("div", `k${i}-${k.src}`, `film-scene${k.src >= 0 ? " shot" : " auto"}${sel ? " sel" : ""}${i === ti ? " now" : ""}`);
    b.style("left", `${fmt((k.start / total) * 100, 3)}%`);
    b.style("width", `${fmt(((k.end - k.start) / total) * 100, 3)}%`);
    b.attr(
      "title",
      `${k.why !== "" ? k.why : k.src >= 0 ? `Shot ${k.src + 1}` : "auto"} — ${k.frame}, bars ${barBeat(k.start, f.bar)}–${barBeat(k.end, f.bar)}${k.src < 0 ? " · double-click to make it a shot" : " · drag to move, drag its edges to stretch"}`
    );
    b.on("pointerdown", (e) => onStripDown(e, fv, f, inp, i, "body"));
    b.on("dblclick", (e) => {
      e.stopPropagation();
      if (k.src < 0) addShot(fv, shotOf(f, i));
    });
    b.leaf("span", "l", "film-scene-label", k.why !== "" ? k.why : k.src >= 0 ? `Shot ${k.src + 1}` : k.frame);
    if (k.src >= 0) {
      for (const part of ["start", "end"]) {
        b.leaf("span", part, `film-edge ${part}`, "");
        b.on("pointerdown", (e) => onStripDown(e, fv, f, inp, i, part));
      }
    }
    b.close();
  }
  if (inp.showing) {
    b.leaf("div", "ph", "film-ph", "");
    b.style("left", `${fmt((clampNum(beat, 0, total) / total) * 100, 3)}%`);
  }
  b.close();
}

// ------------------------------------------------------------------ panel

/** A labeled slider; `onSet` while it moves (one undo step), double-click for `dflt`. */
/** function slider(b: Builder, fv: FilmView, key: String, label: String, tip: String, value: Number, min: Number, max: Number, step: Number, dflt: Number, shown: String, onSet: (Number) => Undefined) => Undefined */
function slider(b, fv, key, label, tip, value, min, max, step, dflt, shown, onSet) {
  b.open("label", key, "film-knob");
  b.attr("title", `${tip} (double-click: ${fmt(dflt, 2)})`);
  b.leaf("span", "l", "film-knob-l", label);
  b.leaf("input", "in", "score-slider", "");
  b.attr("type", "range");
  b.attr("min", String(min));
  b.attr("max", String(max));
  b.attr("step", String(step));
  b.prop("value", String(value));
  b.on("input", (e) => {
    if (!fv.gesture) {
      begin();
      fv.gesture = true;
    }
    anim();
    onSet(Number(e.value));
    changed(true);
  });
  b.on("change", (e) => endEdit(fv));
  b.on("dblclick", (e) => {
    commit(() => {
      anim();
      onSet(dflt);
    });
  });
  b.leaf("span", "v", "film-knob-v", shown);
  b.close();
}

/** The panel: the selected shot, or the film's settings. */
/** function side(b: Builder, fv: FilmView, f: Film, inp: FilmInput, ti: Int) => Undefined */
function side(b, fv, f, inp, ti) {
  const a = state.project.animation;
  b.open("aside", "side", "score-side film-side");
  const sel = selShot(fv);
  if (sel.length > 0) shotPanel(b, fv, f, inp, sel[0]);
  else {
    const ai = Number.isFinite(fv.auto) ? f.scenes.findIndex((k) => k.src < 0 && Math.abs(k.start - fv.auto) < 1e-6) : -1;
    if (ai >= 0) autoPanel(b, fv, f, ai);
    filmPanel(b, fv, f);
  }
  b.close();
}

/** A director's scene, selected: what it frames, and a button to make it a shot. */
/** function autoPanel(b: Builder, fv: FilmView, f: Film, i: Int) => Undefined */
function autoPanel(b, fv, f, i) {
  const k = f.scenes[i];
  b.leaf("div", "ah", "score-side-h", "Director's scene");
  b.open("div", "auto", "film-card");
  b.leaf("div", "why", "film-card-t", k.why !== "" ? k.why : k.frame);
  const names = k.staves.length === 0 ? "every part" : channelsOf(f.sc, k.staves).join(", ");
  b.leaf("div", "what", "film-card-s", `${k.frame} on ${names} · bars ${barBeat(k.start, f.bar)}–${barBeat(k.end, f.bar)} · ${k.transition}`);
  b.leaf("button", "make", "btn small", "Make it a shot");
  b.attr("title", "Write this scene into the project as a shot (manual mode), to change it");
  b.on("click", (e) => addShot(fv, shotOf(f, i)));
  b.close();
}

/** The film's settings: mode, shots, effects. */
/** function filmPanel(b: Builder, fv: FilmView, f: Film) => Undefined */
function filmPanel(b, fv, f) {
  const a = state.project.animation;
  b.leaf("div", "h", "score-side-h", "The film");
  b.open("div", "how", "film-card");
  b.leaf(
    "div",
    "t",
    "film-card-s",
    a.mode === "auto"
      ? "Auto: the camera directs itself. It mostly shows the full score, and follows a part for a short while as it comes in, takes the lead or plays alone. More energy: more often, closer, and more movement."
      : `Manual: ${a.shots.length === 0 ? "no shots yet — the camera directs itself until you add some" : `${a.shots.length} shot${a.shots.length === 1 ? "" : "s"}, and the director between them`}. Double-click the timeline to add a shot; select one to frame it.`
  );
  b.open("div", "acts", "film-acts");
  b.leaf("button", "bake", "btn small", "Write the director's shots");
  b.attr("title", "Start a manual film from the director's: every scene becomes a shot in the project, to change as you like (or to hand to the agent)");
  b.on("click", (e) => {
    const shots = bake(
      f.sc,
      f.scenes.filter((k) => k.src < 0)
    );
    commit(() => {
      const x = anim();
      x.mode = "manual";
      x.shots = shots;
    });
    fv.shot = -1;
  });
  if (a.shots.length > 0) {
    b.leaf("button", "clear", "btn small ghost", "Clear the shots");
    b.attr("title", "Remove every shot (the director films it all again)");
    b.on("click", (e) => {
      commit(() => {
        anim().shots = [];
      });
      fv.shot = -1;
    });
  }
  b.close();
  b.close();
  b.leaf("div", "eh", "score-side-h", "Effects");
  effectSliders(b, fv, a.effects, (type, x) => setEffect(anim().effects, type, x));
  drawingPanel(b, fv);
  b.leaf("div", "kh", "score-side-h", "Keys");
  b.leaf(
    "div",
    "keys",
    "film-card-s film-tip",
    "Drag the picture to look about (Shift: turn and lean), wheel to zoom, double-click to go back to the film's camera. With a shot selected (Manual, paused), they frame the shot."
  );
}

/** Sliders for the effects of a list. */
/** function effectSliders(b: Builder, fv: FilmView, list: FilmEffect[], onSet: (String, Number) => Undefined) => Undefined */
function effectSliders(b, fv, list, onSet) {
  const names = ["Vignette", "Spotlight", "Glow"];
  const tips = [
    "Vignette: the picture darkens toward its edges",
    "Spotlight: a pool of light on the framed staves, the rest of the desk dimmed",
    "Glow: the notes playing light up, their ink glowing warm",
  ];
  const fx = [0.5, 0, 1];
  for (let i = 0; i < EFFECTS.length; i++) {
    const e = list.find((x) => x.type === EFFECTS[i]);
    const v = e ? e.amount : fx[i];
    // Left out on screen: dimmed (the film keeps it).
    b.open("div", EFFECTS[i], fv.off.includes(EFFECTS[i]) ? "film-fx off" : "film-fx");
    slider(b, fv, EFFECTS[i], names[i], tips[i], v, 0, 1, 0.05, fx[i], e ? fmt(v, 2) : `${fmt(v, 2)}`, (x) => onSet(EFFECTS[i], x));
    b.close();
  }
}

/** What the screen draws: Quality (everything), Performance (nothing extra) or Custom, and each thing on its own. */
/** function drawingPanel(b: Builder, fv: FilmView) => Undefined */
function drawingPanel(b, fv) {
  /** function set(off: String[]) => Undefined */
  function set(off) {
    fv.off = off;
    savePref(DRAWING_PREF, off.length > 0 ? off.join(",") : "none");
    invalidate();
  }
  const preset = fv.off.length === 0 ? "quality" : fv.off.length === DRAWING.length ? "performance" : "custom";
  b.leaf("div", "dh", "score-side-h", "On screen");
  b.open("div", "dm", "film-modes film-presets");
  for (const p of ["quality", "performance", "custom"]) {
    b.leaf("button", p, preset === p ? "film-mode on" : "film-mode", p === "quality" ? "Quality" : p === "performance" ? "Performance" : "Custom");
    b.attr(
      "title",
      p === "quality"
        ? "Draw everything on screen"
        : p === "performance"
          ? "Leave out every effect and draw fewer pixels: smoother on slow machines (exports keep everything)"
          : "Some things left out: pick them below"
    );
    if (p === "quality") b.on("click", (e) => set([]));
    else if (p === "performance") b.on("click", (e) => set(DRAWING.map((x) => x[0])));
  }
  b.close();
  b.open("div", "dc", "film-chips film-drawing");
  for (const x of DRAWING) {
    const on = !fv.off.includes(x[0]);
    b.leaf("button", x[0], on ? "film-chip on" : "film-chip", `${on ? "✓ " : ""}${x[1]}`);
    b.attr("title", `${x[2]} — ${on ? "on; click to leave it out" : "left out; click to draw it"}`);
    b.on("click", (e) => set(on ? fv.off.concat([x[0]]) : fv.off.filter((k) => k !== x[0])));
  }
  b.close();
}

/** The selected shot: when, what, how it is framed, how it comes in, its effects. */
/** function shotPanel(b: Builder, fv: FilmView, f: Film, inp: FilmInput, s: Shot) => Undefined */
function shotPanel(b, fv, f, inp, s) {
  const idx = fv.shot;
  b.open("div", "sh", "score-side-h film-side-h");
  b.leaf("span", "t", "", `Shot ${idx + 1}`);
  iconButton(b, "x", "small ghost", "close", "Back to the film's settings", () => {
    fv.shot = -1;
    invalidate();
  });
  b.close();
  textInput(b, "label", "film-label", s.label, "Label (shown on the timeline)…", (val) => {
    if (val !== s.label) commit(() => (s.label = val));
  });
  // When.
  b.open("div", "when", "film-when");
  /** function nudge(key: String, text: String, tip: String, fn: () => Undefined) => Undefined */
  function nudge(key, text, tip, fn) {
    b.leaf("button", key, "btn small ghost film-nudge", text);
    b.attr("title", tip);
    b.on("click", (e) => commit(fn));
  }
  b.leaf("span", "a", "film-when-t", `bar ${barBeat(s.start, f.bar)}`);
  nudge("a-", "−", "Start a beat earlier", () => (s.start = Math.max(0, s.start - 1)));
  nudge("a+", "+", "Start a beat later", () => (s.start = Math.min(s.end - 0.25, s.start + 1)));
  b.leaf("span", "z", "film-when-t", `to ${barBeat(s.end, f.bar)}`);
  nudge("z-", "−", "End a beat earlier", () => (s.end = Math.max(s.start + 0.25, s.end - 1)));
  nudge("z+", "+", "End a beat later", () => (s.end = s.end + 1));
  b.close();

  // What: channels, or a role.
  b.leaf("div", "fh", "score-side-h", "Frames");
  /** const used: String[] */
  const used = [];
  for (const st of f.sc.staves) for (const c of st.channels) if (!used.includes(c)) used.push(c);
  b.open("div", "focus", "film-chips");
  for (const id of used) {
    const ch = state.project.channels.find((c) => c.id === id);
    const on = s.focus.includes(id);
    b.open("button", id, on ? "film-chip on" : "film-chip");
    b.attr("title", on ? `Leave ${ch ? ch.name : id} out of the frame` : `Frame ${ch ? ch.name : id}`);
    b.on("click", (e) =>
      commit(() => {
        s.focus = on ? s.focus.filter((x) => x !== id) : s.focus.concat([id]);
        if (s.focus.length > 0) s.role = "";
      })
    );
    b.leaf("span", "dot", "score-dot", "");
    b.style("--c", ch ? ch.color : "#888");
    b.leaf("span", "n", "", ch ? ch.name : id);
    b.close();
  }
  b.close();
  b.open("div", "rf", "film-row");
  b.leaf("span", "rl", "label", "Part");
  select(
    b,
    "role",
    "",
    s.focus.length > 0 ? "channels" : s.role === "" ? "all" : s.role,
    s.focus.length > 0 ? ["channels"].concat(ROLES) : ROLES,
    s.focus.length > 0 ? ["Channels above", "Lead", "Rhythm", "Background", "Everyone"] : ["Lead", "Rhythm", "Background", "Everyone"],
    "The part of the band to frame (found by the camera), when no channel is picked",
    (val) =>
      commit(() => {
        if (val === "channels") return undefined;
        s.focus = [];
        s.role = val === "all" ? "" : val;
      })
  );
  b.leaf("span", "fl", "label", "Frame");
  select(
    b,
    "frame",
    "",
    s.frame === "" ? "close" : s.frame,
    FRAMES,
    ["Desk", "Page", "System", "Medium (2 bars)", "Close (1 bar)", "Detail (a beat)"],
    "How much the picture holds",
    (val) => commit(() => (s.frame = val))
  );
  b.close();
  b.open("div", "at", "film-row");
  const fixed = Number.isFinite(s.at);
  b.leaf("button", "follow", fixed ? "btn small ghost" : "btn small ghost on-text", fixed ? `Looks at bar ${barBeat(s.at, f.bar)}` : "✓ Follows the music");
  b.attr("title", fixed ? "Follow the playhead instead" : "Look at the playhead's beat for the whole shot instead of following the music");
  b.on("click", (e) => commit(() => (s.at = fixed ? NaN : Math.max(0, Math.round(state.position * 4) / 4))));
  b.close();

  // The camera.
  const frame = s.frame === "" ? "close" : s.frame;
  const zoom = Number.isFinite(s.zoom) ? s.zoom : 1;
  const tilt = Number.isFinite(s.tilt) ? s.tilt : defaultTilt(frame);
  const turn = Number.isFinite(s.turn) ? s.turn : 0;
  b.leaf("div", "ch", "score-side-h", "Camera");
  slider(
    b,
    fv,
    "zoom",
    "Zoom",
    "Closer or farther than the frame",
    Math.log2(zoom),
    -2,
    3,
    0.05,
    0,
    `${fmt(zoom, 2)}×`,
    (x) => (s.zoom = Math.round(Math.pow(2, x) * 1000) / 1000)
  );
  slider(
    b,
    fv,
    "tilt",
    "Lean",
    "Degrees the camera leans from looking straight down",
    tilt,
    0,
    75,
    1,
    defaultTilt(frame),
    `${fmt(tilt, 0)}°`,
    (x) => (s.tilt = x)
  );
  slider(b, fv, "turn", "Turn", "Degrees the camera turns: the music runs diagonally", turn, -90, 90, 1, 0, `${fmt(turn, 0)}°`, (x) => (s.turn = x));
  const drifts = Number.isFinite(s.to.zoom) || Number.isFinite(s.to.turn) || Number.isFinite(s.to.tilt);
  b.open("div", "drift", "film-row");
  b.leaf("button", "dr", drifts ? "btn small ghost on-text" : "btn small ghost", drifts ? "✓ Drifts" : "Drift");
  b.attr("title", drifts ? "Stay put through the shot" : "Move slowly through the shot: push in and turn a little by its end");
  b.on("click", (e) =>
    commit(() => {
      s.to = drifts ? noMove() : { zoom: Math.round(zoom * 1.25 * 100) / 100, tilt: NaN, turn: turn + (turn >= 0 ? 8 : -8), offset: [] };
    })
  );
  b.close();
  if (drifts) {
    const z1 = Number.isFinite(s.to.zoom) ? s.to.zoom : zoom;
    const t1 = Number.isFinite(s.to.turn) ? s.to.turn : turn;
    const l1 = Number.isFinite(s.to.tilt) ? s.to.tilt : tilt;
    slider(
      b,
      fv,
      "zoom1",
      "→ Zoom",
      "Zoom at the end of the shot",
      Math.log2(z1),
      -2,
      3,
      0.05,
      Math.log2(zoom),
      `${fmt(z1, 2)}×`,
      (x) => (s.to.zoom = Math.round(Math.pow(2, x) * 1000) / 1000)
    );
    slider(b, fv, "tilt1", "→ Lean", "Lean at the end of the shot", l1, 0, 75, 1, tilt, `${fmt(l1, 0)}°`, (x) => (s.to.tilt = x));
    slider(b, fv, "turn1", "→ Turn", "Turn at the end of the shot", t1, -90, 90, 1, turn, `${fmt(t1, 0)}°`, (x) => (s.to.turn = x));
  }

  // Coming in.
  b.leaf("div", "th", "score-side-h", "Coming in");
  b.open("div", "tr", "film-row");
  select(
    b,
    "transition",
    "",
    s.transition === "" ? "glide" : s.transition,
    TRANSITIONS,
    ["Glide", "Cut", "Swoop", "Whip"],
    "How the camera comes into the shot: a smooth move, a cut, a rise away from the desk and down again, a fast blurred whip",
    (val) => commit(() => (s.transition = val === "glide" ? "" : val))
  );
  select(b, "ease", "", s.ease === "" ? "smooth" : s.ease, EASES, ["Smooth", "Linear", "Ease in", "Ease out", "Snap"], "The curve of the move", (val) =>
    commit(() => (s.ease = val === "smooth" ? "" : val))
  );
  b.close();
  if (s.transition !== "cut") {
    const k = f.scenes.find((t) => t.src === idx);
    const g = Number.isFinite(s.glide) ? s.glide : k ? k.glide : f.bar;
    slider(b, fv, "glide", "Beats", "Beats the move into the shot scenes", g, 0, 16, 0.25, k ? k.glide : f.bar, fmt(g, 2), (x) => (s.glide = x));
  }

  b.leaf("div", "eh", "score-side-h", "Effects");
  effectSliders(b, fv, s.effects.length > 0 ? s.effects : state.project.animation.effects, (type, x) => setEffect(s.effects, type, x));

  b.open("div", "acts", "film-acts");
  b.leaf("button", "play", "btn small", "Play it");
  b.attr("title", "Play from the shot's start");
  b.on("click", (e) => {
    inp.seek(Math.max(0, s.start - 0.5));
    if (!state.playing) togglePlay();
  });
  b.leaf("button", "dup", "btn small ghost", "Duplicate");
  b.attr("title", "Another shot like it, right after it");
  b.on("click", (e) => {
    const c = decodeShot(JSON.parse(JSON.stringify(s)));
    c.to = { zoom: s.to.zoom, tilt: s.to.tilt, turn: s.to.turn, offset: s.to.offset.slice() };
    c.zoom = s.zoom;
    c.tilt = s.tilt;
    c.turn = s.turn;
    c.at = s.at;
    c.glide = s.glide;
    const len = s.end - s.start;
    c.start = s.end;
    c.end = s.end + len;
    addShot(fv, c);
  });
  b.leaf("button", "del", "btn small ghost", "Delete");
  b.attr("title", "Remove this shot (the director films its time)");
  b.on("click", (e) => {
    commit(() => {
      anim().shots = state.project.animation.shots.filter((x) => x !== s);
    });
    fv.shot = -1;
  });
  b.close();
}
