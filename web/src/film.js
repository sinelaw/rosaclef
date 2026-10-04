// The film of the song: the score's pages lie on a desk, and a camera in 3D
// space above them follows the music — zooming in on a part, pulling back to
// the whole band, leaning and turning so the music runs diagonally across the
// picture (ui/film.js draws it).
//
// Everything here is plain data and pure functions of the song's time, so a
// film plays the same every time, can be scrubbed, and is easy to reason
// about for an agent writing shots in project.json:
//
//  - The desk (`layDesk`): the score engraved for printed pages (pdf.js), the
//    pages laid in rows, each a little askew. Desk units are points.
//  - Roles (`findRoles`): which parts carry the melody (lead), the beat (drums
//    and bass: rhythm) or the harmony (chords and pads: background).
//  - Scenes (`plan`): the shots as filmed, covering the whole song. In auto
//    mode the director cuts the song into phrases and frames whatever carries
//    each one — a part playing alone, a part coming in, the lead, the groove,
//    the whole band. In manual mode the project's shots are filmed, and the
//    director fills the time between them.
//  - The camera (`cameraAt`): for a song beat, where the camera looks (a point
//    on the desk), how much it sees (`span`, desk units across the picture),
//    how it leans (`tilt`) and turns (`turn`), and the effects' amounts. A
//    shot follows the playhead along its system and drifts toward its `to`
//    values; the move into it glides, cuts, swoops or whips.

import { TPQ } from "./notation.js";
import { timeX } from "./engrave.js";
import { pdfLayout } from "./pdf.js";

/** The effects' amounts (0..1). */
/** type Fx = { vignette: Number, spotlight: Number, focus: Number, glow: Number } */
/** A page lying on the desk: its center and its turn (degrees), in desk units. */
/** type DeskPage = { x: Number, y: Number, rot: Number } */
/** The pages on the desk (each `pw` by `ph`) and the bounds of what they cover. */
/** type Desk = { pages: DeskPage[], pw: Number, ph: Number, x0: Number, y0: Number, x1: Number, y1: Number } */
/** Where an engraved system lies: its page, and its top on the page (points). */
/** type SysAt = { page: Int, top: Number } */
/**
 * A shot as filmed (song beats): the staves it frames (empty: all), its frame,
 * the camera at its start and end (zoom, tilt, turn, offset), how it comes in,
 * its effects; `src` is its index in `animation.shots` (-1: the director's own),
 * `why` what the director saw.
 */
/** type Scene = { start: Number, end: Number, staves: Int[], frame: String, at: Number, zoom: Number, tilt: Number, turn: Number, ox: Number, oy: Number, zoom1: Number, tilt1: Number, turn1: Number, ox1: Number, oy1: Number, transition: String, glide: Number, ease: String, fx: Fx, src: Int, why: String } */
/**
 * The camera: the desk point it looks at, the desk units across the picture,
 * its lean and turn (degrees), motion blur (0..1), the framed region (center
 * and size, desk units) and the effects.
 */
/** type Cam = { x: Number, y: Number, span: Number, tilt: Number, turn: Number, blur: Number, rx: Number, ry: Number, rw: Number, rh: Number, fx: Fx } */
/** Each staff's role, and the staff of the main melody (-1: none). */
/** type Roles = { roles: String[], lead: Int } */
/** A song filmed: the printed layout, the desk, the score, the roles, the scenes, where each system lies, the picture's aspect (width / height) and the song's end (beats). */
/** type Film = { lay: PdfLayout, desk: Desk, sc: Score, roles: Roles, scenes: Scene[], sys: SysAt[], aspect: Number, end: Number, bar: Number, onsets: Number[][] } */
/** A note lit as it plays: its page, its notehead (page points) and how bright (0..1). */
/** type Spark = { page: Int, x: Number, y: Number, a: Number } */

/** Frames, widest first, and how many bars the following ones hold. */
export const FRAMES = ["desk", "page", "system", "medium", "close", "detail"];
export const ROLES = ["lead", "rhythm", "background", "all"];
export const TRANSITIONS = ["glide", "cut", "swoop", "whip"];
export const EASES = ["smooth", "linear", "in", "out", "snap"];
export const EFFECTS = ["vignette", "spotlight", "focus", "glow"];
export const SURFACES = ["walnut", "oak", "slate", "felt", "marble"];

/** The effects' amounts when the film does not set them. */
/** function defaultFx() => Fx */
export function defaultFx() {
  return { vignette: 0.5, spotlight: 0, focus: 0, glow: 0 };
}

/** The camera's lean for a frame when the shot does not set it: the closer, the more. */
/** function defaultTilt(frame: String) => Number */
export function defaultTilt(frame) {
  if (frame === "desk") return 0;
  if (frame === "page") return 8;
  if (frame === "system") return 14;
  if (frame === "medium") return 20;
  if (frame === "detail") return 58;
  return 42;
}

/** Bars a following frame holds (0: not a following frame): two bars, one, or a beat (in 4/4) — close enough to see the ink. */
/** function frameBars(frame: String) => Number */
function frameBars(frame) {
  if (frame === "medium") return 2;
  if (frame === "close") return 1;
  if (frame === "detail") return 0.25;
  return 0;
}

// ------------------------------------------------------------------ helpers

/** A repeatable pseudo-random number in [0, 1) for `i`. */
/** function hash(i: Number) => Number */
export function hash(i) {
  const s = Math.sin(i * 12.9898 + 78.233) * 43758.5453;
  return s - Math.floor(s);
}

/** function clamp(x: Number, lo: Number, hi: Number) => Number */
function clamp(x, lo, hi) {
  return Math.max(lo, Math.min(hi, x));
}

/** function lerp(a: Number, b: Number, u: Number) => Number */
function lerp(a, b, u) {
  return a + (b - a) * u;
}

/** Between two zooms or spans, evenly on a log scale (as the eye sees a zoom). */
/** function lerpLog(a: Number, b: Number, u: Number) => Number */
function lerpLog(a, b, u) {
  return Math.exp(lerp(Math.log(Math.max(1e-6, a)), Math.log(Math.max(1e-6, b)), u));
}

/** Between two angles (degrees), the short way round. */
/** function lerpAngle(a: Number, b: Number, u: Number) => Number */
function lerpAngle(a, b, u) {
  let d = ((((b - a) % 360) + 540) % 360) - 180;
  return a + d * u;
}

/** A move's curve: 0..1 → 0..1. */
/** function ease(kind: String, u: Number) => Number */
export function ease(kind, u) {
  const x = clamp(u, 0, 1);
  if (kind === "linear") return x;
  if (kind === "in") return x * x * x;
  if (kind === "out") return 1 - (1 - x) * (1 - x) * (1 - x);
  if (kind === "snap") return x >= 1 ? 1 : 1 - Math.pow(2, -10 * x) * (1 - x);
  // smooth: smootherstep, no jolt at either end.
  return x * x * x * (x * (x * 6 - 15) + 10);
}

/** Effects: the defaults, then each list in turn (by type). */
/** function fxOf(lists: FilmEffect[][]) => Fx */
export function fxOf(lists) {
  const fx = defaultFx();
  for (const list of lists) {
    for (const e of list) {
      const a = clamp(e.amount, 0, 1);
      if (e.type === "vignette") fx.vignette = a;
      else if (e.type === "spotlight") fx.spotlight = a;
      else if (e.type === "focus") fx.focus = a;
      else if (e.type === "glow") fx.glow = a;
    }
  }
  return fx;
}

/** function mixFx(a: Fx, b: Fx, u: Number) => Fx */
function mixFx(a, b, u) {
  return {
    vignette: lerp(a.vignette, b.vignette, u),
    spotlight: lerp(a.spotlight, b.spotlight, u),
    focus: lerp(a.focus, b.focus, u),
    glow: lerp(a.glow, b.glow, u),
  };
}

// ------------------------------------------------------------------ desk

/** Lay `n` pages of `pw` by `ph` on the desk: in rows (a row for up to four), each a little askew. */
/** function layDesk(n: Int, pw: Number, ph: Number) => Desk */
export function layDesk(n, pw, ph) {
  const count = Math.max(1, n);
  const cols = count <= 4 ? count : Math.ceil(Math.sqrt(count * 2));
  const gx = pw * 0.14;
  const gy = ph * 0.1;
  /** const pages: DeskPage[] */
  const pages = [];
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  for (let i = 0; i < count; i++) {
    const r = Math.floor(i / cols);
    const c = i % cols;
    const x = c * (pw + gx) + pw / 2 + (hash(i * 5 + 1) - 0.5) * gx * 0.5;
    const y = r * (ph + gy) + ph / 2 + (hash(i * 5 + 2) - 0.5) * gy * 0.5;
    const rot = (hash(i * 5 + 3) - 0.5) * (count === 1 ? 2 : 3.4);
    pages.push({ x: x, y: y, rot: rot });
    x0 = Math.min(x0, x - pw / 2);
    y0 = Math.min(y0, y - ph / 2);
    x1 = Math.max(x1, x + pw / 2);
    y1 = Math.max(y1, y + ph / 2);
  }
  return { pages: pages, pw: pw, ph: ph, x0: x0, y0: y0, x1: x1, y1: y1 };
}

/** A point of page `p` (points from its top left) on the desk. */
/** function onDesk(desk: Desk, p: Int, x: Number, y: Number) => Number[] */
export function onDesk(desk, p, x, y) {
  const pg = desk.pages[Math.max(0, Math.min(desk.pages.length - 1, p))];
  const a = (pg.rot * Math.PI) / 180;
  const dx = x - desk.pw / 2;
  const dy = y - desk.ph / 2;
  return [pg.x + dx * Math.cos(a) - dy * Math.sin(a), pg.y + dx * Math.sin(a) + dy * Math.cos(a)];
}

/** Where each system of a printed layout lies. */
/** function systemsAt(lay: PdfLayout) => SysAt[] */
export function systemsAt(lay) {
  /** const out: SysAt[] */
  const out = lay.page.systems.map((s) => ({ page: 0, top: 0 }));
  for (let p = 0; p < lay.pages.length; p++) for (const on of lay.pages[p]) out[on.sys] = { page: p, top: on.top };
  return out;
}

/** The system playing at a tick (the first before the music, the last after it). */
/** function systemAt(page: Page, tick: Number) => Int */
export function systemAt(page, tick) {
  const ss = page.systems;
  if (ss.length === 0) return -1;
  for (let i = 0; i < ss.length; i++) if (tick < ss[i].end) return i;
  return ss.length - 1;
}

/** Ticks in the bar at `tick`. */
/** function barTicks(sc: Score, tick: Number) => Number */
function barTicks(sc, tick) {
  let len = 4 * TPQ;
  for (const m of sc.measures) {
    if (m.start > tick) break;
    len = m.length / Math.max(1, m.count);
  }
  return len;
}

// ------------------------------------------------------------------ roles

/** The staves of each part (a grand staff is one part on two staves): the first staff's index for each. */
/** function groupsOf(sc: Score) => Int[] */
function groupsOf(sc) {
  /** const out: Int[] */
  const out = [];
  for (const st of sc.staves) if (!out.includes(st.group)) out.push(st.group);
  return out;
}

/** The staves of the parts `groups`. */
/** function stavesOf(sc: Score, groups: Int[]) => Int[] */
function stavesOf(sc, groups) {
  /** const out: Int[] */
  const out = [];
  for (let i = 0; i < sc.staves.length; i++) if (groups.includes(sc.staves[i].group)) out.push(i);
  return out;
}

/** The part a note is written in (-1: not shown). */
/** function groupOfNote(sc: Score, n: SrcNote) => Int */
function groupOfNote(sc, n) {
  for (const st of sc.staves) if (st.channels.includes(n.channel)) return st.group;
  return -1;
}

/**
 * Who plays what. Drums, and low single lines (a bass), keep the rhythm;
 * chords and long held notes are the background; the other lines are leads,
 * and the main melody is the busiest, highest of them.
 */
/** function findRoles(sc: Score) => Roles */
export function findRoles(sc) {
  /** const roles: String[] */
  const roles = sc.staves.map((st) => "background");
  const groups = groupsOf(sc);
  let lead = -1;
  let best = -1;
  /** const pads: Int[] */
  const pads = [];
  /** const padScore: Number[] */
  const padScore = [];
  for (const g of groups) {
    const st = sc.staves[g];
    /** const pitches: Int[] */
    const pitches = [];
    /** const onsets: Int[] */
    const onsets = [];
    let dur = 0;
    let first = Infinity;
    let last = -Infinity;
    for (const n of sc.notes) {
      if (groupOfNote(sc, n) !== g) continue;
      pitches.push(n.pitch);
      const on = Math.round(n.start * 4);
      if (!onsets.includes(on)) onsets.push(on);
      dur = dur + (n.end - n.start);
      first = Math.min(first, n.start);
      last = Math.max(last, n.end);
    }
    let role = "background";
    if (pitches.length === 0) role = "background";
    else if (st.drum) role = "rhythm";
    else {
      pitches.sort((a, b) => a - b);
      const med = pitches[Math.floor(pitches.length / 2)];
      const chordy = pitches.length / Math.max(1, onsets.length);
      const mean = dur / pitches.length;
      const busy = onsets.length / Math.max(1, last - first);
      if (med < 50 && chordy < 1.6) role = "rhythm";
      else if (chordy >= 2.2 || (mean >= 1.5 && busy < 1.2)) {
        role = "background";
        pads.push(g);
        padScore.push(busy * (1 + (med - 60) / 36));
      } else {
        role = "lead";
        const score = busy * (1 + (med - 60) / 36) * (2.4 - Math.min(chordy, 2.2));
        if (score > best) {
          best = score;
          lead = g;
        }
      }
    }
    for (const i of stavesOf(sc, [g])) roles[i] = role;
  }
  // No single line: the melody is on top of the chords.
  if (lead < 0 && pads.length > 0) {
    let k = 0;
    for (let i = 1; i < pads.length; i++) if (padScore[i] > padScore[k]) k = i;
    lead = pads[k];
    for (const i of stavesOf(sc, [lead])) roles[i] = "lead";
  }
  return { roles: roles, lead: lead };
}

/** The staves a shot frames: its channels, or a role's staves (empty: every staff). */
/** function stavesFor(sc: Score, roles: Roles, focus: String[], role: String) => Int[] */
export function stavesFor(sc, roles, focus, role) {
  /** const out: Int[] */
  const out = [];
  if (focus.length > 0) {
    for (let i = 0; i < sc.staves.length; i++) if (sc.staves[i].channels.some((c) => focus.includes(c))) out.push(i);
    return out;
  }
  if (role === "lead" && roles.lead >= 0) return stavesOf(sc, [roles.lead]);
  if (role === "rhythm" || role === "background" || role === "lead") {
    for (let i = 0; i < sc.staves.length; i++) if (roles.roles[i] === role) out.push(i);
  }
  return out;
}

/** The channels written on some staves (all of them for none). */
/** function channelsOf(sc: Score, staves: Int[]) => String[] */
export function channelsOf(sc, staves) {
  /** const out: String[] */
  const out = [];
  for (let i = 0; i < sc.staves.length; i++) {
    if (staves.length > 0 && !staves.includes(i)) continue;
    for (const c of sc.staves[i].channels) if (!out.includes(c)) out.push(c);
  }
  return out;
}

/** The name of some staves' parts: "Lead", "Bass and Drums". */
/** function partNames(sc: Score, staves: Int[]) => String */
function partNames(sc, staves) {
  /** const names: String[] */
  const names = [];
  for (const i of staves) if (!names.includes(sc.staves[i].name)) names.push(sc.staves[i].name);
  if (names.length <= 1) return names.length === 1 ? names[0] : "everyone";
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

// ------------------------------------------------------------------ scenes

/** A shot of the project as a scene (`bar`: beats in a bar, for the default glide). */
/** function shotScene(sc: Score, roles: Roles, film: Animation, s: Shot, src: Int, bar: Number) => Scene */
export function shotScene(sc, roles, film, s, src, bar) {
  const frame = FRAMES.includes(s.frame) ? s.frame : "close";
  const zoom = Number.isFinite(s.zoom) ? clamp(s.zoom, 0.1, 10) : 1;
  const tilt = Number.isFinite(s.tilt) ? clamp(s.tilt, 0, 75) : defaultTilt(frame);
  const turn = Number.isFinite(s.turn) ? s.turn : 0;
  const ox = s.offset.length === 2 ? s.offset[0] : 0;
  const oy = s.offset.length === 2 ? s.offset[1] : 0;
  const transition = TRANSITIONS.includes(s.transition) ? s.transition : "glide";
  const len = Math.max(0, s.end - s.start);
  const glide =
    transition === "cut" ? 0 : Number.isFinite(s.glide) ? Math.max(0, s.glide) : transition === "whip" ? Math.min(0.75, len * 0.5) : Math.min(bar, len * 0.5);
  return {
    start: s.start,
    end: s.end,
    staves: stavesFor(sc, roles, s.focus, s.role),
    frame: frame,
    at: Number.isFinite(s.at) ? s.at : NaN,
    zoom: zoom,
    tilt: tilt,
    turn: turn,
    ox: ox,
    oy: oy,
    zoom1: Number.isFinite(s.to.zoom) ? clamp(s.to.zoom, 0.1, 10) : zoom,
    tilt1: Number.isFinite(s.to.tilt) ? clamp(s.to.tilt, 0, 75) : tilt,
    turn1: Number.isFinite(s.to.turn) ? s.to.turn : turn,
    ox1: s.to.offset.length === 2 ? s.to.offset[0] : ox,
    oy1: s.to.offset.length === 2 ? s.to.offset[1] : oy,
    transition: transition,
    glide: glide,
    ease: EASES.includes(s.ease) ? s.ease : "smooth",
    fx: fxOf([film.effects, s.effects]),
    src: src,
    why: s.label,
  };
}

/** A scene cut to [start, end) (it keeps its camera; coming back after a shot, it glides in). */
/** function clip(k: Scene, start: Number, end: Number, bar: Number) => Scene */
function clip(k, start, end, bar) {
  const back = start > k.start + 1e-9;
  return {
    start: start,
    end: end,
    staves: k.staves,
    frame: k.frame,
    at: k.at,
    zoom: k.zoom,
    tilt: k.tilt,
    turn: k.turn,
    ox: k.ox,
    oy: k.oy,
    zoom1: k.zoom1,
    tilt1: k.tilt1,
    turn1: k.turn1,
    ox1: k.ox1,
    oy1: k.oy1,
    transition: back ? "glide" : k.transition,
    glide: back ? Math.min(bar, (end - start) * 0.5) : Math.min(k.glide, end - start),
    ease: k.ease,
    fx: k.fx,
    src: k.src,
    why: k.why,
  };
}

/** Put a scene over a list of scenes: where they overlap, it is filmed. */
/** function paint(list: Scene[], k: Scene, bar: Number) => Scene[] */
function paint(list, k, bar) {
  /** const out: Scene[] */
  const out = [];
  for (const t of list) {
    if (t.end <= k.start + 1e-9 || t.start >= k.end - 1e-9) {
      out.push(t);
      continue;
    }
    if (t.start < k.start - 1e-9) out.push(clip(t, t.start, k.start, bar));
    if (t.end > k.end + 1e-9) out.push(clip(t, k.end, t.end, bar));
  }
  out.push(k);
  out.sort((a, b) => a.start - b.start);
  return out;
}

/** The bars of a score: their starts in ticks, and the end. */
/** function barStarts(sc: Score) => Number[] */
function barStarts(sc) {
  /** const out: Number[] */
  const out = [];
  for (const m of sc.measures) {
    const len = m.length / Math.max(1, m.count);
    for (let k = 0; k < m.count; k++) out.push(m.start + k * len);
  }
  out.push(sc.end);
  return out;
}

/** The director: the song cut into phrases, each framed on what carries it. */
/** function autoScenes(sc: Score, roles: Roles, film: Animation, pages: Int, bar: Number) => Scene[] */
export function autoScenes(sc, roles, film, pages, bar) {
  /** const out: Scene[] */
  const out = [];
  const bs = barStarts(sc);
  const nb = bs.length - 1;
  if (sc.empty || nb <= 0) return out;
  const energy = clamp(film.energy, 0, 1);
  const groups = groupsOf(sc);
  // Which parts play in each bar.
  /** const act: Int[][] */
  const act = [];
  for (let b = 0; b < nb; b++) act.push([]);
  for (const n of sc.notes) {
    const g = groupOfNote(sc, n);
    if (g < 0) continue;
    const t0 = n.start * TPQ;
    const t1 = n.end * TPQ;
    for (let b = 0; b < nb; b++) {
      if (bs[b + 1] <= t0) continue;
      if (bs[b] >= t1) break;
      if (!act[b].includes(g)) act[b].push(g);
    }
  }
  // Section starts the producer marked: colored passages and repeats.
  /** const marks: Number[] */
  const marks = [];
  for (const m of sc.marks) marks.push(m.start * TPQ);
  for (const r of sc.repeats) {
    marks.push(r.start * TPQ);
    marks.push(r.end * TPQ);
  }
  const marked = (b) => marks.some((t) => Math.abs(t - bs[b]) < 1);
  /** function same(a: Int[], b: Int[]) => Boolean */
  function same(a, b) {
    return a.length === b.length && a.every((x) => b.includes(x));
  }
  // Phrases: a few bars each, cut early where a part comes in or where one is left alone.
  const P = energy < 0.34 ? 8 : energy < 0.67 ? 4 : 2;
  const minLen = energy < 0.5 ? 2 : 1;
  /** const phrases: Int[][] */
  const phrases = [];
  let a = 0;
  for (let b = 1; b < nb; b++) {
    const len = b - a;
    const enters = act[b].some((g) => !act[b - 1].includes(g) && (b < 2 || !act[b - 2].includes(g)));
    const alone = (act[b].length === 1) !== (act[b - 1].length === 1) || (act[b].length === 1 && !same(act[b], act[b - 1]));
    if (len >= P || (len >= minLen && (enters || alone)) || (len >= 1 && marked(b))) {
      phrases.push([a, b]);
      a = b;
    }
  }
  phrases.push([a, nb]);

  let full = 0;
  /** const prevActive: Int[] */
  let prevActive = [];
  for (let i = 0; i < phrases.length; i++) {
    const b0 = phrases[i][0];
    const b1 = phrases[i][1];
    /** const A: Int[] */
    const A = [];
    for (let b = b0; b < b1; b++) for (const g of act[b]) if (!A.includes(g)) A.push(g);
    A.sort((x, y) => x - y);
    const E = A.filter((g) => !prevActive.includes(g));
    const roleOf = (g) => roles.roles[g];
    let frame = "close";
    /** let focus: Int[] */
    let focus = [];
    let why = "";
    if (A.length === 0) {
      frame = "system";
      why = "a rest";
    } else if (A.length === 1) {
      focus = [A[0]];
      frame = "detail";
      why = `${partNames(sc, stavesOf(sc, A))} alone`;
    } else if (E.length > 0 && E.length < A.length && i > 0) {
      // A part coming in, up close: the lead if it is one of them, else the first.
      const one = roles.lead >= 0 && E.includes(roles.lead) ? roles.lead : E[0];
      focus = [one];
      frame = "detail";
      why = `${partNames(sc, stavesOf(sc, [one]))} comes in`;
    } else if (A.every((g) => roleOf(g) === "rhythm")) {
      focus = A;
      frame = "medium";
      why = "the rhythm section";
    } else if (A.every((g) => roleOf(g) === "background")) {
      focus = A;
      frame = "system";
      why = "the background";
    } else {
      const rhythm = A.filter((g) => roleOf(g) === "rhythm");
      const hasLead = roles.lead >= 0 && A.includes(roles.lead);
      const c = full % 4;
      full = full + 1;
      if (hasLead && (c === 0 || c === 2)) {
        focus = [roles.lead];
        frame = c === 0 ? "detail" : "close";
        why = "the lead";
      } else if (c === 3 && rhythm.length > 0) {
        focus = [rhythm[0]];
        frame = "detail";
        why = "the groove";
      } else {
        frame = energy < 0.34 ? "page" : "system";
        why = "the whole band";
      }
    }
    prevActive = A;
    const staves = stavesOf(sc, focus);
    const wide = frame === "desk" || frame === "page" || frame === "system";
    const r1 = hash(i * 3 + 1);
    const r2 = hash(i * 3 + 2);
    const sign = i % 2 === 0 ? 1 : -1;
    // Close up, the camera comes down low over the page (the drops of ink stand against it).
    const tilt = frame === "desk" ? 2 + 10 * energy * r1 : wide ? 5 + 20 * energy * r1 : frame === "detail" ? 50 + 16 * r1 : 34 + 16 * r1;
    const turn = sign * (frame === "desk" ? 3 * energy * r2 : wide ? 2 + 12 * energy * r2 : 4 + 30 * energy * r2);
    const start = bs[b0] / TPQ;
    const end = bs[b1] / TPQ;
    const len = end - start;
    let transition = "glide";
    let glide = Math.min(bar, len * 0.4);
    if (energy > 0.7 && r1 > 0.78) {
      transition = "cut";
      glide = 0;
    } else if (energy > 0.55 && r2 > 0.82) {
      transition = "whip";
      glide = Math.min(0.75, len * 0.3);
    }
    const fx = fxOf([film.effects]);
    out.push({
      start: start,
      end: end,
      staves: staves,
      frame: frame,
      at: NaN,
      zoom: 1,
      tilt: tilt,
      turn: turn,
      ox: 0,
      oy: 0,
      zoom1: wide ? 1.06 : 1.08 + 0.2 * energy * r2,
      tilt1: tilt,
      turn1: turn + sign * (2 + 8 * energy),
      ox1: 0,
      oy1: 0,
      transition: transition,
      glide: glide,
      ease: "smooth",
      fx: fx,
      src: -1,
      why: why,
    });
  }
  const fx = fxOf([film.effects]);
  // The opening: the camera starts high over the desk and swoops down to the first shot.
  const first = (bs[1] - bs[0]) / TPQ;
  if (nb >= 3) {
    const f0 = out[0];
    const open = Math.min(energy < 0.34 ? 2 * first : first, (f0.end - f0.start) * 0.5);
    out.unshift(wideScene(f0.start, f0.start + open, pages > 1 ? "desk" : "page", 1.12, "the opening", fx));
    f0.start = f0.start + open;
    f0.transition = "swoop";
    f0.glide = Math.min(2 * first, (f0.end - f0.start) * 0.7);
  }
  // The close: over the last bar the camera rises to see the page.
  const lastBar = (bs[nb] - bs[nb - 1]) / TPQ;
  const fin = out[out.length - 1];
  if (nb >= 6 && fin.end - fin.start >= 2 * lastBar - 1e-9) {
    fin.end = fin.end - lastBar;
    const close = wideScene(fin.end, fin.end + lastBar, "page", 0.85, "the close", fx);
    close.glide = lastBar * 0.9;
    out.push(close);
  }
  return out;
}

/** A wide, calm scene of the director's (the opening, the close), drifting to `zoom1`. */
/** function wideScene(start: Number, end: Number, frame: String, zoom1: Number, why: String, fx: Fx) => Scene */
function wideScene(start, end, frame, zoom1, why, fx) {
  return {
    start: start,
    end: end,
    staves: [],
    frame: frame,
    at: NaN,
    zoom: 1,
    tilt: 4,
    turn: 0,
    ox: 0,
    oy: 0,
    zoom1: zoom1,
    tilt1: 10,
    turn1: 3,
    ox1: 0,
    oy1: 0,
    transition: "glide",
    glide: 0,
    ease: "smooth",
    fx: fx,
    src: -1,
    why: why,
  };
}

/** The scenes of a film: the director's, with the project's shots over them in manual mode. */
/** function plan(film: Animation, sc: Score, roles: Roles, pages: Int, bar: Number) => Scene[] */
export function plan(film, sc, roles, pages, bar) {
  let list = autoScenes(sc, roles, film, pages, bar);
  if (film.mode !== "manual") return list;
  for (let i = 0; i < film.shots.length; i++) {
    const s = film.shots[i];
    if (!(s.end > s.start)) continue;
    list = paint(list, shotScene(sc, roles, film, s, i, bar), bar);
  }
  return list;
}

/** The scene filmed at a beat (-1: none). */
/** function sceneAt(scenes: Scene[], beat: Number) => Int */
export function sceneAt(scenes, beat) {
  if (scenes.length === 0) return -1;
  let k = 0;
  for (let i = 0; i < scenes.length; i++) if (scenes[i].start <= beat + 1e-9) k = i;
  return k;
}

/** The scenes as shots to write into the project (to start a manual film from the director's). */
/** function bake(sc: Score, scenes: Scene[]) => Shot[] */
export function bake(sc, scenes) {
  /** const out: Shot[] */
  const out = [];
  const r = (x) => Math.round(x * 100) / 100;
  for (const k of scenes) {
    const moved = Math.abs(k.zoom1 - k.zoom) > 1e-3 || Math.abs(k.turn1 - k.turn) > 1e-3 || Math.abs(k.tilt1 - k.tilt) > 1e-3;
    /** const fx: FilmEffect[] */
    const fx = [];
    if (k.fx.spotlight > 0) fx.push({ type: "spotlight", amount: r(k.fx.spotlight) });
    out.push({
      start: r(k.start),
      end: r(k.end),
      label: k.why,
      focus: k.staves.length > 0 ? channelsOf(sc, k.staves) : [],
      role: "",
      frame: k.frame,
      zoom: Math.abs(k.zoom - 1) > 1e-3 ? r(k.zoom) : NaN,
      tilt: r(k.tilt),
      turn: r(k.turn),
      offset: k.ox !== 0 || k.oy !== 0 ? [r(k.ox), r(k.oy)] : [],
      at: k.at,
      to: moved
        ? {
            zoom: Math.abs(k.zoom1 - k.zoom) > 1e-3 ? r(k.zoom1) : NaN,
            tilt: Math.abs(k.tilt1 - k.tilt) > 1e-3 ? r(k.tilt1) : NaN,
            turn: Math.abs(k.turn1 - k.turn) > 1e-3 ? r(k.turn1) : NaN,
            offset: [],
          }
        : { zoom: NaN, tilt: NaN, turn: NaN, offset: [] },
      transition: k.transition === "glide" ? "" : k.transition,
      glide: k.transition === "cut" ? NaN : r(k.glide),
      ease: k.ease === "smooth" ? "" : k.ease,
      effects: fx,
    });
  }
  return out;
}

// ------------------------------------------------------------------ the film

/** A song to film: its score printed on `paper` ("a4" or "letter"), seen at an aspect (picture width / height). */
/** function makeFilm(film: Animation, sc: Score, paper: String, hideEmpty: Boolean, aspect: Number, bar: Number) => Film */
export function makeFilm(film, sc, paper, hideEmpty, aspect, bar) {
  const lay = pdfLayout(sc, paper, hideEmpty);
  const desk = layDesk(lay.pages.length, lay.w, lay.h);
  const roles = findRoles(sc);
  return {
    lay: lay,
    desk: desk,
    sc: sc,
    roles: roles,
    scenes: plan(film, sc, roles, lay.pages.length, bar),
    sys: systemsAt(lay),
    aspect: Math.max(0.2, aspect),
    end: sc.end / TPQ,
    bar: bar,
    onsets: onsetsOf(sc),
  };
}

/** The same film with its scenes planned again (the shots or the picture's aspect changed; the pages did not). */
/** function replan(f: Film, film: Animation, aspect: Number) => Film */
export function replan(f, film, aspect) {
  return {
    lay: f.lay,
    desk: f.desk,
    sc: f.sc,
    roles: f.roles,
    scenes: plan(film, f.sc, f.roles, f.lay.pages.length, f.bar),
    sys: f.sys,
    aspect: Math.max(0.2, aspect),
    end: f.end,
    bar: f.bar,
    onsets: f.onsets,
  };
}

/** Where notes start on each staff (ticks, sorted). */
/** function onsetsOf(sc: Score) => Number[][] */
function onsetsOf(sc) {
  /** const out: Number[][] */
  const out = sc.staves.map((st) => []);
  for (const n of sc.notes) {
    const t = Math.round(n.start * TPQ);
    for (let i = 0; i < sc.staves.length; i++) if (sc.staves[i].channels.includes(n.channel)) out[i].push(t);
  }
  for (const o of out) o.sort((a, b) => a - b);
  return out;
}

/**
 * Ticks a detail frame holds at a tick: a beat, or the gap from the note
 * sounding to the next one on the framed staves when it is longer (up to a
 * bar), averaged around the tick so the frame breathes instead of jumping.
 */
/** function detailTicks(f: Film, staves: Int[], tick: Number) => Number */
function detailTicks(f, staves, tick) {
  const bar = barTicks(f.sc, tick);
  const beat = bar / 4;
  const rows = staves.length > 0 ? staves : f.onsets.map((o, i) => i);
  let sum = 0;
  for (const d of [-1, -0.75, -0.5, -0.25, 0, 0.25, 0.5, 0.75, 1]) {
    const t = tick + d * beat;
    let prev = -Infinity;
    let next = Infinity;
    for (const i of rows) {
      const o = i < f.onsets.length ? f.onsets[i] : [];
      for (const x of o) {
        if (x <= t) prev = Math.max(prev, x);
        else {
          next = Math.min(next, x);
          break;
        }
      }
    }
    const gap = Number.isFinite(prev) && Number.isFinite(next) ? (next - prev) * 1.25 : beat;
    sum = sum + clamp(gap, beat, bar);
  }
  return sum / 9;
}

/** The region a scene frames at a tick of system `si`: center and size on the desk ([x, y, w, h]). */
/** function sysRegion(f: Film, k: Scene, si: Int, tick: Number) => Number[] */
function sysRegion(f, k, si, tick) {
  const lay = f.lay;
  const sp = lay.sp;
  const s = lay.page.systems[si];
  const at = f.sys[si];
  if (k.frame === "page") {
    const c = onDesk(f.desk, at.page, f.desk.pw / 2, f.desk.ph / 2);
    return [c[0], c[1], f.desk.pw, f.desk.ph];
  }
  // The framed staves' extent (all of the system's when none of them is in it).
  let y0 = Infinity;
  let y1 = -Infinity;
  for (const row of s.rows) {
    if (k.staves.length > 0 && !k.staves.includes(row.staff)) continue;
    y0 = Math.min(y0, row.y - 2.4);
    y1 = Math.max(y1, row.y + 6.4);
  }
  if (y0 === Infinity) {
    y0 = -2;
    y1 = s.height + 2;
  }
  let xa = -0.5;
  let xb = s.x1 + 0.8;
  const bars = frameBars(k.frame);
  if (bars > 0) {
    const sysW = s.x1 - s.x0;
    const held = k.frame === "detail" ? detailTicks(f, k.staves, tick) : bars * barTicks(f.sc, tick);
    const w = Math.min(sysW + 1, (sysW * held) / Math.max(1, s.end - s.start));
    // The playhead, smoothed over about what the frame holds, a little left of the middle.
    const b = barTicks(f.sc, tick) * Math.max(0.5, bars);
    let x = 0;
    for (const d of [-0.5, -0.25, 0, 0.25, 0.5]) x = x + timeX(s.times, clamp(tick + d * b, s.start, s.end));
    const c = clamp(x / 5 + w * 0.1, s.x0 + w / 2 - 1, s.x1 - w / 2 + 0.6);
    xa = c - w / 2;
    xb = c + w / 2;
    if (w >= sysW) {
      xa = s.x0 - 1;
      xb = s.x1 + 0.6;
    }
  }
  const c = onDesk(f.desk, at.page, lay.left + ((xa + xb) / 2) * sp, at.top + ((y0 + y1) / 2) * sp);
  return [c[0], c[1], (xb - xa) * sp, (y1 - y0) * sp];
}

/** The region a scene frames at a beat ([x, y, w, h] on the desk), gliding on to the next system as one ends. */
/** function region(f: Film, k: Scene, beat: Number) => Number[] */
export function region(f, k, beat) {
  const d = f.desk;
  const systems = f.lay.page.systems;
  if (k.frame === "desk" || systems.length === 0) return [(d.x0 + d.x1) / 2, (d.y0 + d.y1) / 2, d.x1 - d.x0, d.y1 - d.y0];
  const follow = !Number.isFinite(k.at);
  const tick = (follow ? beat : k.at) * TPQ;
  const si = systemAt(f.lay.page, tick);
  const r = sysRegion(f, k, si, tick);
  if (!follow || si + 1 >= systems.length) return r;
  // The last bar or so of a system: move on to the start of the next one.
  const s = systems[si];
  const blend = Math.min(barTicks(f.sc, tick) * 0.75, (s.end - s.start) * 0.5);
  if (tick <= s.end - blend) return r;
  const next = sysRegion(f, k, si + 1, systems[si + 1].start);
  const u = ease("smooth", (tick - (s.end - blend)) / blend);
  // Far to go for so close a frame: pull back on the way, and come down again.
  const dist = Math.hypot(next[0] - r[0], next[1] - r[1]) / Math.max(1, Math.min(r[2], next[2]));
  const rise = 1 + clamp(dist * 0.6 - 0.6, 0, 8) * Math.sin(Math.PI * u);
  return [lerp(r[0], next[0], u), lerp(r[1], next[1], u), lerpLog(r[2], next[2], u) * rise, lerpLog(r[3], next[3], u) * rise];
}

/** The camera for a scene, framing a region, `u` of the way through the shot (eased). */
/** function camFor(f: Film, k: Scene, r: Number[], u: Number) => Cam */
function camFor(f, k, r, u) {
  const zoom = lerpLog(k.zoom, k.zoom1, u);
  const tilt = lerp(k.tilt, k.tilt1, u);
  const turn = lerpAngle(k.turn, k.turn1, u);
  const ox = lerp(k.ox, k.ox1, u);
  const oy = lerp(k.oy, k.oy1, u);
  const th = (turn * Math.PI) / 180;
  const c = Math.abs(Math.cos(th));
  const s = Math.abs(Math.sin(th));
  // The region turned with the camera; a leaning camera sees it shorter.
  const rw = r[2] * c + r[3] * s;
  const rh = (r[2] * s + r[3] * c) * (0.55 + 0.45 * Math.cos((tilt * Math.PI) / 180));
  const span = Math.max(rw * 1.12, rh * 1.2 * f.aspect) / zoom;
  // The offset is in the picture's axes: turned back onto the desk.
  const dx = ox * span;
  const dy = (oy * span) / f.aspect;
  return {
    x: r[0] + dx * Math.cos(th) - dy * Math.sin(th),
    y: r[1] + dx * Math.sin(th) + dy * Math.cos(th),
    span: span,
    tilt: tilt,
    turn: turn,
    blur: 0,
    rx: r[0],
    ry: r[1],
    rw: r[2],
    rh: r[3],
    fx: k.fx,
  };
}

/** Between two cameras. */
/** function mixCam(a: Cam, b: Cam, u: Number) => Cam */
export function mixCam(a, b, u) {
  return {
    x: lerp(a.x, b.x, u),
    y: lerp(a.y, b.y, u),
    span: lerpLog(a.span, b.span, u),
    tilt: lerp(a.tilt, b.tilt, u),
    turn: lerpAngle(a.turn, b.turn, u),
    blur: lerp(a.blur, b.blur, u),
    rx: lerp(a.rx, b.rx, u),
    ry: lerp(a.ry, b.ry, u),
    rw: lerpLog(a.rw, b.rw, u),
    rh: lerpLog(a.rh, b.rh, u),
    fx: mixFx(a.fx, b.fx, u),
  };
}

/** The camera of scene `i` at a beat (no transition). */
/** function sceneCam(f: Film, i: Int, beat: Number) => Cam */
function sceneCam(f, i, beat) {
  const k = f.scenes[i];
  const len = Math.max(1e-6, k.end - k.start);
  return camFor(f, k, region(f, k, beat), ease("smooth", (beat - k.start) / len));
}

/** The camera at a song beat. */
/** function cameraAt(f: Film, beat: Number) => Cam */
export function cameraAt(f, beat) {
  const i = sceneAt(f.scenes, beat);
  if (i < 0) {
    const d = f.desk;
    const r = [(d.x0 + d.x1) / 2, (d.y0 + d.y1) / 2, d.x1 - d.x0, d.y1 - d.y0];
    const span = Math.max(r[2] * 1.12, r[3] * 1.2 * f.aspect);
    return { x: r[0], y: r[1], span: span, tilt: 0, turn: 0, blur: 0, rx: r[0], ry: r[1], rw: r[2], rh: r[3], fx: defaultFx() };
  }
  const k = f.scenes[i];
  const cur = sceneCam(f, i, beat);
  if (i === 0 || k.glide <= 0 || k.transition === "cut" || beat >= k.start + k.glide) return cur;
  // Moving in from the shot before (which goes on following the music meanwhile).
  const prev = sceneCam(f, i - 1, beat);
  const v = ease(k.ease, (beat - k.start) / k.glide);
  const cam = mixCam(prev, cur, v);
  const bump = Math.sin(Math.PI * v);
  // Far apart, the camera rises away from the desk on its way (a swoop always does).
  const d = Math.hypot(prev.x - cur.x, prev.y - cur.y) / Math.max(1, Math.min(prev.span, cur.span));
  const rise = k.transition === "swoop" ? Math.max(0.6, 0.35 * d) : k.transition === "whip" ? 0 : clamp(0.3 * (d - 1.2), 0, 1);
  cam.span = cam.span * (1 + rise * bump);
  cam.tilt = cam.tilt * (1 - Math.min(0.5, 0.3 * rise) * bump);
  if (k.transition === "whip") cam.blur = bump;
  return cam;
}

/** The notes lit at a beat: those sounding (and fading just after) in the systems around the playhead. */
/** function sparks(f: Film, beat: Number, staves: Int[]) => Spark[] */
export function sparks(f, beat, staves) {
  /** const out: Spark[] */
  const out = [];
  const systems = f.lay.page.systems;
  const si = systemAt(f.lay.page, beat * TPQ);
  if (si < 0) return out;
  const sp = f.lay.sp;
  const tail = 0.2;
  for (let j = Math.max(0, si - 1); j <= si; j++) {
    const s = systems[j];
    const at = f.sys[j];
    for (const h of s.heads) {
      const n = f.sc.notes[h.src];
      if (n.start > beat || beat >= n.end + tail) continue;
      const attack = Math.exp(-(beat - n.start) * 2.5);
      let a = beat < n.end ? 0.45 + 0.55 * attack : 0.45 * (1 - (beat - n.end) / tail);
      a = a * (0.6 + 0.4 * n.velocity);
      if (staves.length > 0 && !staves.includes(h.staff)) a = a * 0.45;
      out.push({ page: at.page, x: f.lay.left + (h.x + h.w / 2) * sp, y: at.top + h.y * sp, a: a });
    }
  }
  return out;
}

// ------------------------------------------------------------------ playing order

/**
 * The song as it plays: spans of written beats [start, end) one after the
 * other, its repeats taken (as crates/core/src/form.rs plays them), up to `end`.
 */
/** function performance(repeats: Repeat[], end: Number) => Number[][] */
export function performance(repeats, end) {
  /** const out: Number[][] */
  const out = [];
  /** function push(a: Number, b: Number) => Undefined */
  function push(a, b) {
    if (b <= a + 1e-9) return undefined;
    const last = out.length > 0 ? out[out.length - 1] : [];
    if (last.length === 2 && Math.abs(last[1] - a) < 1e-9) last[1] = b;
    else out.push([a, b]);
  }
  const reps = repeats.filter((r) => Number.isFinite(r.start) && Number.isFinite(r.end) && r.start >= 0 && r.end > r.start + 1e-9);
  reps.sort((a, b) => a.start - b.start);
  let cursor = 0;
  for (const r of reps) {
    if (r.end <= cursor + 1e-9) continue;
    push(cursor, Math.max(r.start, cursor));
    const start = Math.max(r.start, cursor);
    const times = Math.max(1, Math.min(99, r.times));
    const endings = r.endings.filter((e) => e.end > e.start + 1e-9);
    endings.sort((a, b) => a.start - b.start);
    let after = r.end;
    for (let pass = 1; pass <= times; pass++) {
      let t = start;
      for (const e of endings) {
        if (e.passes.includes(pass) || e.end <= t + 1e-9 || e.start >= r.end - 1e-9) continue;
        push(t, Math.max(e.start, t));
        t = Math.max(t, e.end);
      }
      push(t, r.end);
      if (pass === times) for (const e of endings) if (Math.abs(e.start - after) < 1e-9 && !e.passes.includes(pass)) after = e.end;
    }
    cursor = after;
  }
  push(cursor, Math.max(cursor, end));
  return out;
}

/** The written beat playing `beats` into the performance (past the end: the end, and on). */
/** function writtenAt(spans: Number[][], beats: Number) => Number */
export function writtenAt(spans, beats) {
  let t = beats;
  for (const s of spans) {
    const len = s[1] - s[0];
    if (t < len) return s[0] + t;
    t = t - len;
  }
  return spans.length > 0 ? spans[spans.length - 1][1] + t : beats;
}
