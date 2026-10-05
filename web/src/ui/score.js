// The Score view: the song, one playlist track or one pattern as sheet music
// (notation.js writes it down, engrave.js lays it out), to read, play along
// with and edit.
//
// It shows in two places: next to the playlist (the "Score" tab of the top
// pane, the whole song by default) and in the dock (F10, the pattern in the
// piano roll by default). Each place keeps its own view; both read and write
// the project, so the piano roll, the playlist and the agent see every change.
//
// The page is engraved once per change of the project, the scope or the
// width (`cached`), and only the systems in view become nodes: each system
// is one SVG with a filled path and a glyph run per color.
//
// Editing:
//  - Select (Shift+E): click a note to select it (Shift adds), drag it up or
//    down the staff (by step, in the key) or along the bar (by the grid);
//    double-click opens it in the piano roll; right-click deletes. Dragging on
//    the paper selects a passage to color; a click there moves the playhead.
//  - Write (Shift+P): click on a staff to add a note of the chosen value.
//  - Delete, ↑/↓ (Shift: octave) act on the selection, as in the piano roll.
//  - A staff's name (on the page or in the sidebar) opens its part's menu: the
//    channel in the rack, its sound, its notes in the piano roll, its mixer
//    insert, or the whole part given to another channel (`moveRole` in
//    model.js). A chosen passage can be given to another channel too.
//  - Under the pointer, what a click would act on is lit: a name glows and is
//    underlined, a note lights up, and a line shows where the playhead would go.

import { drag, fmt, loadPref, savePref, pressOrTap, downloadPdf, downloadImagePdf, textWidth, paperSize, pixelRatio } from "#platform";
import { state, commit, begin, changed, invalidate, hint, setFocus, reportContext, currentPattern, selectChannel } from "../store.js";
import { PALETTE, semitonesText, moveRole, cloneProject } from "../model.js";
import {
  buildScore,
  TPQ,
  KEYS,
  keyLabel,
  keyAlter,
  spell,
  spelledName,
  stepPitch,
  drumAt,
  kindDrum,
  channelKind,
  bottomStep,
  passesText,
  withoutColors,
} from "../notation.js";
import { engrave, timeX, xTick, GLOSS, SHEEN } from "../engrave.js";
import { inkFilter, glintOpacity, PAPER, LACQUER, GLOSS_DEFAULT, GLOSS_MAX, SHINE_DEFAULT } from "../ink.js";
import { scorePdf, pdfLayout, pageSvg, pdfInfo } from "../pdf.js";
import { preview, seek } from "../audio.js";
import { select, iconButton, glyph, textInput } from "./widgets.js";
import { followButton } from "./playlist.js";
import { revealDock, openDock, showInsert } from "./panes.js";
import { instrumentLabel } from "./instruments.js";
import { browseInstrument } from "./browser.js";
import { toast } from "./toast.js";
import { filmView, newFilmView } from "./film.js";
import { trackIx, trackIndex, noteIx, noteIndex, insertIndex } from "#brands";

// ------------------------------------------------------------------ state

/** A passage being chosen with the pointer (ticks, and the staves it spans). */
/** type Range = { on: Boolean, t0: Number, t1: Number, s0: Int, s1: Int } */
/** Where Write would put a note: a system, a staff row, a step and a tick (-1: nowhere). */
/** type Ghost = { sys: Int, row: Int, step: Int, tick: Number, x: Number } */
/** What the pointer is over, and so what a click there would do: a staff's "name" (label `idx`), a
 * "note" (head `idx`), the "paper" (the playhead would move to `x`), the "desk" beside the page; "" nothing. */
/** type Hover = { kind: String, sys: Int, idx: Int, x: Number } */
/** The menu of a part, opened from its name (on the page or in the sidebar): the channels of its staff,
 * the one it acts on, and where it opens (client pixels). */
/** type PartMenu = { on: Boolean, channels: String[], channel: String, x: Number, y: Number } */
/** The engraved score of the last flush, and what it was built from. */
/** `score` keeps its colored passages (the sidebar lists them); `ink` is what is engraved and printed (without them when colors are hidden). */
/** type Cached = { key: String, score: Score, ink: Score, page: Page } */
/** The notation the film is made of, and what it was built from. */
/** type FilmScore = { key: String, score: Score } */
/** `size`: a staff space in pixels at 100% (the music's size, laid out to fit); `zoom`: magnification of that page. */
/** type ScoreView = { id: String, scope: String, size: Number, zoom: Number, width: Number, height: Number, scrollTop: Number, scrollLeft: Number, panning: Boolean, pdfMenu: Boolean, tool: String, value: Int, dot: Boolean, grid: Int, night: Boolean, colors: Boolean, ink: String, gloss: Number, shine: Number, side: Boolean, condense: Boolean, range: Range, ghost: Ghost, hover: Hover, menu: PartMenu, cache: Cached[], film: Boolean, fv: FilmView, filmCache: FilmScore[] } */

/** function newView(id: String, scope: String) => ScoreView */
function newView(id, scope) {
  return {
    id: id,
    scope: scope,
    size: 7,
    zoom: 1,
    width: 900,
    height: 400,
    scrollTop: 0,
    scrollLeft: 0,
    panning: false,
    pdfMenu: false,
    tool: "select",
    value: 48,
    dot: false,
    grid: 12,
    night: false,
    colors: true,
    ink: "wet",
    gloss: GLOSS_DEFAULT,
    shine: SHINE_DEFAULT,
    side: true,
    condense: true,
    range: { on: false, t0: 0, t1: 0, s0: 0, s1: 0 },
    ghost: { sys: -1, row: -1, step: 0, tick: -1, x: 0 },
    hover: { kind: "", sys: -1, idx: -1, x: 0 },
    menu: { on: false, channels: [], channel: "", x: 0, y: 0 },
    cache: [],
    film: false,
    fv: newFilmView(id),
    filmCache: [],
  };
}

/** The score beside the playlist, and the one in the dock. */
export const topScore = newView("top", "song");
export const dockScore = newView("dock", "current");

const PREF = "rosaclef.score.";

/** The music's size (a staff space in pixels at 100%), and the zoom's steps. */
const SIZE_MIN = 4;
const SIZE_MAX = 16;
const ZOOMS = [0.5, 0.67, 0.75, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3];

/** Read the persisted view settings (size, zoom, paper, sidebar). */
export function loadScorePrefs() {
  for (const v of [topScore, dockScore]) {
    // The size was once kept as "zoom".
    const sz = loadPref(`${PREF}${v.id}.size`);
    const size = Number(sz !== "" ? sz : loadPref(`${PREF}${v.id}.zoom`));
    if (size >= SIZE_MIN && size <= SIZE_MAX) v.size = size;
    const z = Number(loadPref(`${PREF}${v.id}.magnify`));
    if (z >= ZOOMS[0] && z <= ZOOMS[ZOOMS.length - 1]) v.zoom = z;
    v.night = loadPref(`${PREF}${v.id}.night`) === "1";
    v.colors = loadPref(`${PREF}${v.id}.colors`) !== "0";
    if (loadPref(`${PREF}${v.id}.ink`) === "dry") v.ink = "dry";
    const gl = loadPref(`${PREF}${v.id}.gloss`);
    if (gl !== "" && Number(gl) >= 0 && Number(gl) <= GLOSS_MAX) v.gloss = Number(gl);
    const sh = loadPref(`${PREF}${v.id}.shine`);
    if (sh !== "" && Number(sh) >= 0 && Number(sh) <= 1) v.shine = Number(sh);
    if (loadPref(`${PREF}${v.id}.side`) === "0") v.side = false;
    v.film = loadPref(`${PREF}${v.id}.film`) === "1";
    const sc = loadPref(`${PREF}${v.id}.scope`);
    if (sc !== "") v.scope = sc;
  }
}

/** function savePrefs(v: ScoreView) => Undefined */
function savePrefs(v) {
  savePref(`${PREF}${v.id}.size`, String(v.size));
  savePref(`${PREF}${v.id}.magnify`, fmt(v.zoom, 2));
  savePref(`${PREF}${v.id}.night`, v.night ? "1" : "0");
  savePref(`${PREF}${v.id}.colors`, v.colors ? "1" : "0");
  savePref(`${PREF}${v.id}.ink`, v.ink);
  savePref(`${PREF}${v.id}.gloss`, fmt(v.gloss, 2));
  savePref(`${PREF}${v.id}.shine`, fmt(v.shine, 2));
  savePref(`${PREF}${v.id}.side`, v.side ? "1" : "0");
  savePref(`${PREF}${v.id}.film`, v.film ? "1" : "0");
  savePref(`${PREF}${v.id}.scope`, v.scope);
}

/** The view the keyboard acts on: the one in the dock when it shows. */
/** function activeView() => ScoreView */
function activeView() {
  return state.dock === "score" ? dockScore : topScore;
}

// ------------------------------------------------------------------ scope

/** function scopeOf(v: ScoreView) => Scope */
function scopeOf(v) {
  if (v.scope === "current") return { kind: "pattern", track: 0, pattern: state.pattern };
  if (v.scope.startsWith("track:")) return { kind: "track", track: Math.round(Number(v.scope.slice(6))), pattern: "" };
  if (v.scope.startsWith("pattern:")) return { kind: "pattern", track: 0, pattern: v.scope.slice(8) };
  return { kind: "song", track: 0, pattern: "" };
}

/** A heading for the scope. */
/** function scopeTitle(sc: Scope) => String */
function scopeTitle(sc) {
  const p = state.project;
  if (sc.kind === "pattern") {
    const pat = p.patterns.find((x) => x.id === sc.pattern);
    return pat ? pat.name : "No pattern";
  }
  if (sc.kind === "track") {
    const tr = sc.track < p.playlist.tracks.length ? p.playlist.tracks[sc.track] : undefined;
    return tr ? tr.name : `Track ${sc.track + 1}`;
  }
  return p.meta.title;
}

// ------------------------------------------------------------------ geometry

/** Page geometry in pixels for the current width, size and zoom. The page is laid out
 * to fit the view at 100% (its width in staff spaces depends on the size alone); the
 * zoom magnifies it, wider than the view if need be. */
/** type PageGeo = { sp: Number, left: Number, top: Number, outer: Number, padX: Number, head: Number, paperW: Number, widthSp: Number } */

/** function pageGeo(v: ScoreView, sc: Scope) => PageGeo */
function pageGeo(v, sc) {
  const z = v.zoom;
  const sp = v.size * z;
  const outer = v.width < 640 ? 8 : 22;
  const fitW = Math.max(240, Math.min(v.width - outer * 2, 230 * v.size));
  const fitPad = Math.max(10, Math.min(5 * v.size, fitW * 0.06));
  const paperW = fitW * z;
  const padX = fitPad * z;
  const head = sc.kind === "song" ? 15 * sp : 10 * sp;
  return {
    sp: sp,
    left: Math.max(outer, (v.width - paperW) / 2),
    top: outer,
    outer: outer,
    padX: padX,
    head: head,
    paperW: paperW,
    widthSp: (fitW - fitPad * 2) / v.size,
  };
}

/** The engraved page for this view (rebuilt only when the project, scope, grid or width changed). */
/** function cached(v: ScoreView, sc: Scope, geo: PageGeo) => Cached */
function cached(v, sc, geo) {
  const key = `${state.edits}|${sc.kind}|${sc.track}|${sc.pattern}|${v.grid}|${Math.round(geo.widthSp * 4)}|${v.condense}|${v.colors}`;
  const hit = v.cache.find((c) => c.key === key);
  if (hit) return hit;
  const score = buildScore(state.project, sc, v.grid);
  const ink = v.colors ? score : withoutColors(score);
  const page = engrave(ink, { width: geo.widthSp, hideEmpty: v.condense && sc.kind !== "pattern" });
  const c = { key: key, score: score, ink: ink, page: page };
  v.cache.length = 0;
  v.cache.push(c);
  return c;
}

/** y (px, in the scroller's content) of a system's top. */
/** function sysY(geo: PageGeo, s: Sys) => Number */
function sysY(geo, s) {
  return geo.top + geo.padX * 0.6 + geo.head + s.top * geo.sp;
}

/** function paperH(geo: PageGeo, page: Page) => Number */
function paperH(geo, page) {
  return geo.padX * 0.6 + geo.head + page.height * geo.sp + geo.padX;
}

/** The system under a page y (px), or -1. */
/** function sysAt(geo: PageGeo, page: Page, y: Number) => Int */
function sysAt(geo, page, y) {
  for (let i = 0; i < page.systems.length; i++) {
    const s = page.systems[i];
    const top = sysY(geo, s);
    if (y >= top - 1.2 * geo.sp && y < top + (s.height + 1.2) * geo.sp) return i;
  }
  return -1;
}

/** The staff row nearest a system y (staff spaces), or -1. */
/** function rowAt(s: Sys, y: Number) => Int */
function rowAt(s, y) {
  let best = -1;
  let bestD = 99;
  for (let r = 0; r < s.rows.length; r++) {
    const d = Math.abs(y - (s.rows[r].y + 2));
    if (d < bestD) {
      bestD = d;
      best = r;
    }
  }
  return bestD < 7 ? best : -1;
}

/** The notehead under a point of a system (staff spaces), or -1. */
/** function headAt(s: Sys, x: Number, y: Number) => Int */
function headAt(s, x, y) {
  let best = -1;
  let bestD = 99;
  for (let i = 0; i < s.heads.length; i++) {
    const h = s.heads[i];
    const dx = Math.max(0, Math.abs(x - (h.x + h.w / 2)) - h.w / 2);
    const dy = Math.max(0, Math.abs(y - h.y) - 0.45);
    const d = dx + dy;
    if (d < 0.35 && d < bestD) {
      bestD = d;
      best = i;
    }
  }
  return best;
}

/** The width of a staff name (staff spaces), as the sidebar's serif sets it. */
/** function nameWidth(l: Label) => Number */
function nameWidth(l) {
  return textWidth("Times-Italic", l.text) * (l.cls.includes("short") ? 1.3 : 1.55);
}

/** The staff name under a point of a system (staff spaces): its label's index, or -1. */
/** function nameAt(s: Sys, x: Number, y: Number) => Int */
function nameAt(s, x, y) {
  for (let i = 0; i < s.labels.length; i++) {
    const l = s.labels[i];
    if (l.staff < 0) continue;
    const w = nameWidth(l);
    if (x >= l.x - w - 0.5 && x <= l.x + 0.4 && y >= l.y - 1.6 && y <= l.y + 0.6) return i;
  }
  return -1;
}

/** Point the hover at something else (redrawn only when it changes). */
/** function setHover(v: ScoreView, kind: String, sys: Int, idx: Int, x: Number) => Undefined */
function setHover(v, kind, sys, idx, x) {
  const h = v.hover;
  if (h.kind === kind && h.sys === sys && h.idx === idx && h.x === x) return undefined;
  v.hover = { kind: kind, sys: sys, idx: idx, x: x };
  invalidate();
}

// ------------------------------------------------------------------ colors

/** function hexRgb(hex: String) => Int[] */
function hexRgb(hex) {
  const h = hex.startsWith("#") ? hex.slice(1) : hex;
  const n = parseInt(h.length === 3 ? `${h[0]}${h[0]}${h[1]}${h[1]}${h[2]}${h[2]}` : h, 16);
  if (!(n >= 0)) return [128, 128, 128];
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** A mark color as ink: deepened on paper (to read like colored ink), brightened at night. */
/** function inkColor(hex: String, night: Boolean) => String */
function inkColor(hex, night) {
  const c = hexRgb(hex);
  /** const out: Int[] */
  const out = [];
  for (const v of c) out.push(night ? Math.round(v + (255 - v) * 0.25) : Math.round(v * 0.68));
  return `rgb(${out[0]},${out[1]},${out[2]})`;
}

/** function tint(hex: String, a: Number) => String */
function tint(hex, a) {
  const c = hexRgb(hex);
  return `rgba(${c[0]},${c[1]},${c[2]},${fmt(a, 3)})`;
}

// ------------------------------------------------------------------ editing

/** The note a score head stands for, if it still exists. */
/** function srcNote(sc: Score, src: Int) => Note? */
function srcNote(sc, src) {
  if (src < 0 || src >= sc.notes.length) return undefined;
  const n = sc.notes[src];
  const pat = state.project.patterns.find((x) => x.id === n.pattern);
  return pat && n.index < pat.notes.length ? pat.notes[n.index] : undefined;
}

/** function isSelected(sc: Score, src: Int) => Boolean */
function isSelected(sc, src) {
  const n = sc.notes[src];
  return n.pattern === state.pattern && state.selection.some((s) => noteIndex(s) === n.index);
}

/** Select a score note: it becomes the piano roll's pattern, channel and selection. */
/** function selectNote(sc: Score, src: Int, add: Boolean) => Undefined */
function selectNote(sc, src, add) {
  const n = sc.notes[src];
  const keep = add && state.pattern === n.pattern ? state.selection.filter((s) => noteIndex(s) !== n.index) : [];
  if (state.pattern !== n.pattern) state.pattern = n.pattern;
  state.channel = n.channel;
  keep.push(noteIx(n.index));
  state.selection = keep;
  reportContext();
  invalidate();
}

/** A pitch for a staff step: the key's alteration, or a drum on a drum staff. */
/** function pitchAt(sc: Score, row: Row, step: Int) => Int */
function pitchAt(sc, row, step) {
  if (row.drum) return drumAt(step);
  return stepPitch(step, keyAlter(((step % 7) + 7) % 7, sc.fifths));
}

/** The channel a new note on a staff goes to (a drum staff: the drum written there). */
/** function channelFor(sc: Score, row: Row, step: Int) => String */
function channelFor(sc, row, step) {
  const st = sc.staves[row.staff];
  if (st.drum && st.channels.length > 1) {
    for (const id of st.channels) if (kindDrum(channelKind(state.project, id)).step === step) return id;
  }
  return st.channel;
}

/** The pattern and pattern beat a score beat writes to (song: the clip playing there). */
/** type Target = { pattern: Pattern, beat: Number } */

/** function targetAt(sc: Scope, beat: Number, channel: String) => Target? */
function targetAt(sc, beat, channel) {
  const p = state.project;
  if (sc.kind === "pattern") {
    const pat = p.patterns.find((x) => x.id === sc.pattern);
    if (!pat || beat >= pat.length) return undefined;
    return { pattern: pat, beat: beat };
  }
  /** const found: Target[] */
  const found = [];
  const hidden = p.score.hiddenTracks.map(trackIndex);
  for (const c of p.playlist.clips) {
    if (c.pattern === "" || beat < c.start || beat >= c.start + c.length) continue;
    const tr = trackIndex(c.track);
    if (sc.kind === "track" ? tr !== sc.track : hidden.includes(tr)) continue;
    const pat = p.patterns.find((x) => x.id === c.pattern);
    if (!pat || pat.length <= 0) continue;
    const t = (((beat - c.start + c.offset) % pat.length) + pat.length) % pat.length;
    const t2 = { pattern: pat, beat: t };
    if (pat.notes.some((n) => n.channel === channel)) return t2;
    found.push(t2);
  }
  return found.length > 0 ? found[0] : undefined;
}

/** Write a note where the ghost is. */
/** function writeNote(v: ScoreView, sc: Score, s: Sys) => Undefined */
function writeNote(v, sc, s) {
  const g = v.ghost;
  if (g.row < 0 || g.tick < 0) return undefined;
  const row = s.rows[g.row];
  const pitch = pitchAt(sc, row, g.step);
  const channel = channelFor(sc, row, g.step);
  const scope = scopeOf(v);
  const beat = g.tick / TPQ;
  const t = targetAt(scope, beat, channel);
  if (t === undefined) {
    toast(
      scope.kind === "pattern" ? "Past the end of the pattern" : "No pattern plays here",
      scope.kind === "pattern" ? "Lengthen the pattern to write there." : "Place a clip in the playlist first.",
      "warn"
    );
    return undefined;
  }
  const len = (v.dot ? v.value * 1.5 : v.value) / TPQ;
  const pat = t.pattern;
  const at = t.beat;
  commit(() => {
    pat.notes.push({
      channel: channel,
      pitch: pitch,
      start: Math.round(at * 1e6) / 1e6,
      length: Math.min(len, Math.max(0.0625, pat.length - at)),
      velocity: 0.8,
    });
  });
  state.pattern = pat.id;
  state.channel = channel;
  state.selection = [noteIx(pat.notes.length - 1)];
  reportContext();
  preview(channel, pitch, 0.8);
}

/** Press on a notehead: select it; dragging moves it by staff steps and along the grid. */
/** function grabNote(e: Ev, v: ScoreView, sc: Score, s: Sys, hi: Int, y0: Number) => Undefined */
function grabNote(e, v, sc, s, hi, y0) {
  const h = s.heads[hi];
  const n0 = sc.notes[h.src];
  if (e.button === 2) {
    const pat = state.project.patterns.find((x) => x.id === n0.pattern);
    if (pat && n0.index < pat.notes.length) {
      commit(() => {
        pat.notes.splice(n0.index, 1);
      });
      state.selection = [];
      reportContext();
    }
    return undefined;
  }
  if (!isSelected(sc, h.src) || e.shiftKey) selectNote(sc, h.src, e.shiftKey);
  const pat = state.project.patterns.find((x) => x.id === n0.pattern);
  if (!pat) return undefined;
  const first = pat.notes[n0.index];
  if (first) preview(first.channel, first.pitch, first.velocity);
  const row = s.rows.find((r) => r.staff === h.staff);
  const drum = row ? row.drum : false;
  const picked = state.selection.map(noteIndex).filter((i) => i < pat.notes.length);
  const orig = picked.map((i) => ({ i: i, start: pat.notes[i].start, pitch: pat.notes[i].pitch }));
  const x0 = e.clientX;
  const tick0 = xTick(s.times, h.x + h.w / 2);
  let moved = false;
  let lastPitch = first ? first.pitch : 0;
  drag(
    e,
    (m) => {
      const dx = m.clientX - x0;
      const dy = m.clientY - y0;
      if (!moved && Math.abs(dx) < 4 && Math.abs(dy) < 4) return undefined;
      if (!moved) {
        moved = true;
        begin();
      }
      const sp = v.size * v.zoom;
      const steps = drum ? 0 : -Math.round((dy / sp) * 2);
      const tick = xTick(s.times, h.x + h.w / 2 + dx / sp);
      const grid = v.grid / TPQ;
      const db = Math.round((tick - tick0) / v.grid) * grid;
      for (const o of orig) {
        const nn = pat.notes[o.i];
        if (steps !== 0) {
          const target = spell(Math.round(o.pitch), sc.fifths).step + steps;
          nn.pitch = Math.max(0, Math.min(127, stepPitch(target, keyAlter(((target % 7) + 7) % 7, sc.fifths))));
        } else nn.pitch = o.pitch;
        nn.start = Math.max(0, Math.min(pat.length - 0.0625, o.start + db));
      }
      const p = pat.notes[n0.index].pitch;
      if (p !== lastPitch) {
        lastPitch = p;
        preview(pat.notes[n0.index].channel, p, 0.8);
      }
      hint(`${spelledName(Math.round(p), sc.fifths)} — drag up or down by step, along the bar by the grid`);
      changed(true);
    },
    (u) => undefined
  );
}

// ------------------------------------------------------------------ marks

/** Color the chosen passage. */
/** function colorRange(v: ScoreView, sc: Score, color: String) => Undefined */
function colorRange(v, sc, color) {
  const r = v.range;
  if (!r.on) return undefined;
  const scope = scopeOf(v);
  /** const ids: String[] */
  const ids = [];
  const all = r.s0 <= 0 && r.s1 >= sc.staves.length - 1;
  if (!all) {
    for (let i = r.s0; i <= r.s1; i++) for (const id of sc.staves[i].channels) if (!ids.includes(id)) ids.push(id);
  }
  const t0 = Math.min(r.t0, r.t1) / TPQ;
  const t1 = Math.max(r.t0, r.t1) / TPQ;
  commit(() => {
    state.project.score.marks.push({
      start: Math.round(t0 * 1e4) / 1e4,
      end: Math.round(t1 * 1e4) / 1e4,
      color: color,
      label: "",
      pattern: scope.kind === "pattern" ? scope.pattern : "",
      channels: ids,
    });
  });
  v.range.on = false;
  // A passage just colored is meant to be seen.
  if (!v.colors) {
    v.colors = true;
    savePrefs(v);
  }
}

/** Remove the colors of marks that overlap the chosen passage. */
/** function clearRange(v: ScoreView, sc: Score) => Undefined */
function clearRange(v, sc) {
  const r = v.range;
  const t0 = Math.min(r.t0, r.t1) / TPQ;
  const t1 = Math.max(r.t0, r.t1) / TPQ;
  /** const gone: Int[] */
  const gone = [];
  for (const m of sc.marks) if (m.start < t1 && m.end > t0 && !gone.includes(m.mark)) gone.push(m.mark);
  if (gone.length === 0) return undefined;
  commit(() => {
    /** const keep: ScoreMark[] */
    const keep = [];
    const marks = state.project.score.marks;
    for (let i = 0; i < marks.length; i++) if (!gone.includes(i)) keep.push(marks[i]);
    state.project.score.marks = keep;
  });
  v.range.on = false;
}

// ------------------------------------------------------------------ repeats

/** The bars the chosen passage touches, in beats: [start, end). */
/** function rangeBars(sc: Score, r: Range) => Number[] */
function rangeBars(sc, r) {
  const t0 = Math.min(r.t0, r.t1);
  const t1 = Math.max(r.t0, r.t1);
  let a = t0;
  let z = t1;
  for (const m of sc.measures) {
    const bar = m.length / m.count;
    for (let k = 0; k < m.count; k++) {
      const s = m.start + k * bar;
      if (s <= t0 && t0 < s + bar) a = s;
      if (s < t1 && t1 <= s + bar) z = s + bar;
    }
  }
  return [Math.round((a / TPQ) * 1e4) / 1e4, Math.round((z / TPQ) * 1e4) / 1e4];
}

/** Where a repeat stops, with the endings that follow it. */
/** function repeatEnd(r: Repeat) => Number */
function repeatEnd(r) {
  let end = r.end;
  for (const e of r.endings) if (e.start >= r.end - 1e-9) end = Math.max(end, e.end);
  return end;
}

/** Repeat the chosen bars (twice), replacing the repeats they overlap. */
/** function repeatRange(v: ScoreView, sc: Score) => Undefined */
function repeatRange(v, sc) {
  const ab = rangeBars(sc, v.range);
  const a = ab[0];
  const z = ab[1];
  commit(() => {
    const keep = state.project.repeats.filter((r) => r.start >= z || repeatEnd(r) <= a);
    keep.push({ start: a, end: z, times: 2, endings: [] });
    keep.sort((x, y) => x.start - y.start);
    state.project.repeats = keep;
  });
  v.range.on = false;
}

/** Make the chosen bars an ending of the repeat they close (or follow):
 * inside it, the passes before the last; right after it, the last pass. */
/** function endingRange(v: ScoreView, sc: Score) => Undefined */
function endingRange(v, sc) {
  const ab = rangeBars(sc, v.range);
  const a = ab[0];
  const z = ab[1];
  const r = state.project.repeats.find((x) => a > x.start + 1e-9 && a <= repeatEnd(x) + 1e-9);
  if (r === undefined) {
    toast("No repeat here", "An ending goes at the end of a repeat, or right after it: repeat some bars first.", "warn");
    return undefined;
  }
  const others = r.endings.filter((e) => e.end <= a + 1e-9 || e.start >= z - 1e-9);
  /** const free: Int[] */
  const free = [];
  for (let k = 1; k <= r.times; k++) if (!others.some((e) => e.passes.includes(k))) free.push(k);
  const inside = a < r.end - 1e-9;
  let passes = inside ? free.filter((k) => k < r.times) : free.includes(r.times) ? [r.times] : free;
  if (passes.length === 0) passes = free.slice(0, 1);
  if (passes.length === 0) {
    toast("No pass left", "Every pass of this repeat already has its ending.", "warn");
    return undefined;
  }
  const at = state.project.repeats.indexOf(r);
  commit(() => {
    const rr = state.project.repeats[at];
    rr.endings = others.concat([{ start: a, end: inside ? Math.min(z, rr.end) : z, passes: passes }]);
    rr.endings.sort((x, y) => x.start - y.start);
  });
  v.range.on = false;
}

// ------------------------------------------------------------------ parts

/** function channelById(id: String) => Channel? */
function channelById(id) {
  return state.project.channels.find((c) => c.id === id);
}

/** Open the menu of a part (the channels of one staff) at a point of the window; its channel is selected. */
/** function openPartMenu(v: ScoreView, channels: String[], x: Number, y: Number) => Undefined */
function openPartMenu(v, channels, x, y) {
  if (channels.length === 0) return undefined;
  const pick = channels.includes(state.channel) ? state.channel : channels[0];
  v.menu = { on: true, channels: channels, channel: pick, x: x, y: y };
  v.range.on = false;
  v.hover = { kind: "", sys: -1, idx: -1, x: 0 };
  selectChannel(pick);
}

/** function closePartMenu(v: ScoreView) => Undefined */
function closePartMenu(v) {
  if (!v.menu.on) return undefined;
  v.menu.on = false;
  invalidate();
}

/** Where a move applies, in words. */
/** function scopeWords(sc: Scope) => String */
function scopeWords(sc) {
  if (sc.kind === "pattern") return `in the pattern ${scopeTitle(sc)}`;
  if (sc.kind === "track") return `on the track ${scopeTitle(sc)}`;
  return "throughout the song";
}

/** Give the notes of channels `parts` starting in beats [t0, t1) of what the view shows to channel
 * `to` (moveRole in model.js: clips are split and patterns copied so nothing else changes). One undo step. */
/** `where` says where, in words ("throughout the song", "in bars 5–8"). */
/** function movePart(v: ScoreView, parts: String[], to: String, t0: Number, t1: Number, where: String) => Undefined */
function movePart(v, parts, to, t0, t1, where) {
  const scope = scopeOf(v);
  const dest = channelById(to);
  if (!dest) return undefined;
  const role = { from: parts, to: to, t0: t0, t1: t1 };
  const names = parts.map((id) => {
    const c = channelById(id);
    return c ? c.name : id;
  });
  const what = names.length === 1 ? names[0] : `${names.length} parts`;
  // A dry run first: nothing to move makes no undo step.
  if (moveRole(cloneProject(state.project), scope, role).notes === 0) {
    toast("Nothing to move", `${what} has no notes ${where}.`, "warn");
    return undefined;
  }
  let res = { notes: 0, splits: 0, copies: 0 };
  commit(() => {
    res = moveRole(state.project, scope, role);
    // The part stays in view on its new staff.
    state.project.score.hidden = state.project.score.hidden.filter((x) => x !== to);
  });
  selectChannel(to);
  /** const how: String[] */
  const how = [];
  if (res.copies > 0)
    how.push(res.copies === 1 ? `a pattern copied: where else it plays keeps ${what}` : `${res.copies} patterns copied: where else they play keeps ${what}`);
  if (res.splits > 0) how.push(`${res.splits === 1 ? "a clip" : `${res.splits} clips`} split at the chosen bars`);
  toast(
    `${what} → ${dest.name}`,
    `${res.notes} note${res.notes === 1 ? "" : "s"} moved ${where}${how.length > 0 ? ` (${how.join("; ")})` : ""}. Ctrl+Z undoes.`,
    "info"
  );
}

/** The pattern the piano roll opens for a part: the scope's, or where the part first plays. */
/** function partPattern(v: ScoreView, sc: Score, id: String) => String */
function partPattern(v, sc, id) {
  const scope = scopeOf(v);
  if (scope.kind === "pattern") return scope.pattern;
  const n = sc.notes.find((x) => x.channel === id);
  return n ? n.pattern : state.pattern;
}

/** function menuItem(b: Builder, v: ScoreView, key: String, icon: String, label: String, tip: String, onClick: () => Undefined) => Undefined */
function menuItem(b, v, key, icon, label, tip, onClick) {
  b.open("button", key, "auto-menu-item");
  b.attr("title", tip);
  b.on("pointerenter", (e) => hint(tip));
  b.on("click", (e) => {
    v.menu.on = false;
    onClick();
    invalidate();
  });
  glyph(b, icon);
  b.leaf("span", "l", "", label);
  b.close();
}

/** The part's menu: reach its instrument (the rack, the piano roll, the mixer), or give the part to another channel. */
/** function partMenu(b: Builder, v: ScoreView, c: Cached) => Undefined */
function partMenu(b, v, c) {
  const m = v.menu;
  const ch = channelById(m.channel);
  if (!m.on || !ch) return undefined;
  const p = state.project;
  b.leaf("div", "menu-back", "auto-backdrop", "");
  b.on("pointerdown", (e) => {
    e.preventDefault();
    closePartMenu(v);
  });
  b.on("contextmenu", (e) => {
    e.preventDefault();
    closePartMenu(v);
  });
  b.open("div", "menu", "auto-menu score-partmenu");
  b.style("left", `min(${m.x}px, calc(100vw - 290px))`);
  b.style("top", `min(${m.y}px, calc(100vh - 420px))`);
  b.on("contextmenu", (e) => {
    e.preventDefault();
  });
  b.open("div", "t", "auto-menu-title score-partmenu-title");
  b.leaf("i", "sw", "swatch", "");
  b.style("--c", ch.color);
  b.leaf("span", "n", "", ch.name);
  b.close();
  b.leaf("div", "s", "auto-menu-sub", `${instrumentLabel(ch.instrument)} · ${insertIndex(ch.mixer) === 0 ? "Master" : `Insert ${insertIndex(ch.mixer)}`}`);
  // A drum staff holds several channels: choose the one to act on.
  if (m.channels.length > 1) {
    b.open("div", "chs", "score-partmenu-chs");
    for (const id of m.channels) {
      const cc = channelById(id);
      if (!cc) continue;
      b.open("button", id, id === m.channel ? "score-partmenu-ch on" : "score-partmenu-ch");
      b.attr("title", `Act on ${cc.name}`);
      b.on("click", (e) => {
        m.channel = id;
        selectChannel(id);
      });
      b.leaf("i", "sw", "swatch", "");
      b.style("--c", cc.color);
      b.leaf("span", "n", "", cc.name);
      b.close();
    }
    b.close();
  }
  const id = ch.id;
  menuItem(b, v, "rack", "rack", "Open in the channel rack", `Select ${ch.name} in the channel rack: its instrument, sound and settings`, () => {
    selectChannel(id);
    openDock("rack");
  });
  menuItem(b, v, "sound", "swap", "Change its sound…", `Show sounds like ${ch.name}'s in the browser: ⇄ on any of them swaps it in, the notes stay`, () => {
    selectChannel(id);
    openDock("rack");
    browseInstrument(ch.instrument.type);
    hint(`In the browser, ⇄ on any instrument swaps it into ${ch.name} — or drag it onto the channel's row`);
  });
  menuItem(b, v, "roll", "piano", "Edit its notes in the piano roll", `Open ${ch.name}'s notes in the piano roll (F7)`, () => {
    const pat = partPattern(v, c.score, id);
    if (pat !== "" && state.pattern !== pat) {
      state.pattern = pat;
      state.selection = [];
    }
    selectChannel(id);
    revealDock("piano");
  });
  menuItem(
    b,
    v,
    "mix",
    "mixer",
    insertIndex(ch.mixer) === 0 ? "Show in the mixer (Master)" : `Show in the mixer (Insert ${insertIndex(ch.mixer)})`,
    "The mixer insert this channel plays through: its level, pan and effects (F9)",
    () => {
      showInsert(ch.mixer);
    }
  );
  menuItem(
    b,
    v,
    "mute",
    ch.mute ? "speaker" : "mute",
    ch.mute ? "Unmute" : "Mute",
    ch.mute ? `Let ${ch.name} play again` : `Silence ${ch.name} (the score still shows it)`,
    () => {
      commit(() => {
        ch.mute = !ch.mute;
      });
    }
  );
  menuItem(b, v, "hide", "close", "Hide in the score", `Leave ${ch.name} out of the score (the sidebar's eye brings it back)`, () => {
    commit(() => {
      if (!p.score.hidden.includes(id)) p.score.hidden.push(id);
    });
  });
  // Give the part to another instrument.
  const others = p.channels.filter((x) => x.id !== id);
  if (others.length > 0) {
    b.leaf("div", "mh", "score-partmenu-h", `Move ${ch.name}'s part to… ${scopeWords(scopeOf(v))}`);
    b.open("div", "to", "score-partmenu-to");
    for (const o of others) {
      b.open("button", o.id, "auto-menu-item score-partmenu-dest");
      const tip = `Give every note of ${ch.name} ${scopeWords(scopeOf(v))} to ${o.name} (${instrumentLabel(o.instrument)}) — Ctrl+Z undoes`;
      b.attr("title", tip);
      b.on("pointerenter", (e) => hint(tip));
      b.on("click", (e) => {
        v.menu.on = false;
        movePart(v, [id], o.id, 0, Infinity, scopeWords(scopeOf(v)));
      });
      b.leaf("i", "sw", "swatch", "");
      b.style("--c", o.color);
      b.leaf("span", "l", "", o.name);
      b.leaf("span", "i", "score-partmenu-ins", instrumentLabel(o.instrument));
      b.close();
    }
    b.close();
  }
  b.close();
}

// ------------------------------------------------------------------ pointer

/** function onPaperDown(e: Ev, v: ScoreView, c: Cached, geo: PageGeo) => Undefined */
function onPaperDown(e, v, c, geo) {
  const px = e.clientX - e.targetLeft + e.scrollLeft;
  const py = e.clientY - e.targetTop + e.scrollTop;
  const page = c.page;
  const si = sysAt(geo, page, py);
  if (si < 0) {
    v.range.on = false;
    invalidate();
    return undefined;
  }
  const s = page.systems[si];
  const x = (px - geo.left - geo.padX) / geo.sp;
  const y = (py - sysY(geo, s)) / geo.sp;
  // A staff's name: the menu of its part (its instrument, or move it to another).
  const ni = nameAt(s, x, y);
  if (ni >= 0) {
    e.preventDefault();
    v.range.on = false;
    openPartMenu(v, c.score.staves[s.labels[ni].staff].channels, e.clientX + 4, e.clientY + 8);
    return undefined;
  }
  const hi = headAt(s, x, y);
  if (v.tool === "select" || e.button === 2 || hi >= 0) {
    if (hi >= 0) {
      e.preventDefault();
      v.range.on = false;
      grabNote(e, v, c.score, s, hi, e.clientY);
      return undefined;
    }
    if (e.button === 2) return undefined;
  }
  if (v.tool === "write") {
    e.preventDefault();
    writeNote(v, c.score, s);
    return undefined;
  }
  // Select on empty paper: a click moves the playhead, a drag chooses a passage.
  e.preventDefault();
  const r0 = Math.max(0, rowAt(s, y));
  const st0 = s.rows.length > 0 ? s.rows[r0].staff : 0;
  const t0 = snapTick(xTick(s.times, x), v.grid, false);
  const sx = e.clientX;
  const sy = e.clientY;
  let dragging = false;
  drag(
    e,
    (m) => {
      if (!dragging && Math.abs(m.clientX - sx) + Math.abs(m.clientY - sy) < 6) return undefined;
      dragging = true;
      const mx = (m.clientX - sx + px - geo.left - geo.padX) / geo.sp;
      const my = m.clientY - sy + py;
      const sj = Math.max(0, sysAt(geo, page, my));
      const s2 = page.systems[sj];
      const t1 = snapTick(xTick(s2.times, mx), v.grid, true);
      const r1 = rowAt(s2, (my - sysY(geo, s2)) / geo.sp);
      const st1 = r1 >= 0 ? s2.rows[r1].staff : st0;
      v.range = { on: true, t0: t0, t1: Math.max(t1, t0 + v.grid), s0: Math.min(st0, st1), s1: Math.max(st0, st1) };
      invalidate();
    },
    (u) => {
      if (!dragging) {
        v.range.on = false;
        state.selection = [];
        seekScore(v, seekTick(v, s, x) / TPQ);
        reportContext();
        invalidate();
      }
    }
  );
}

/** Where a click on the paper moves the playhead: the nearest grid line. */
/** function seekTick(v: ScoreView, s: Sys, x: Number) => Number */
function seekTick(v, s, x) {
  return Math.max(s.start, Math.round(xTick(s.times, x) / v.grid) * v.grid);
}

/** Whether a click on the paper moves the playhead (in pattern mode, only the pattern playing). */
/** function canSeek(v: ScoreView) => Boolean */
function canSeek(v) {
  const sc = scopeOf(v);
  return sc.kind === "pattern" ? state.mode === "pattern" && state.pattern === sc.pattern : state.mode === "song";
}

/** function snapTick(t: Number, grid: Int, up: Boolean) => Number */
function snapTick(t, grid, up) {
  return (up ? Math.ceil(t / grid - 1e-6) : Math.floor(t / grid + 1e-6)) * grid;
}

/** Move the playhead to a score beat (song time, or the pattern's own in pattern mode). */
/** function seekScore(v: ScoreView, beat: Number) => Undefined */
function seekScore(v, beat) {
  const sc = scopeOf(v);
  if (sc.kind === "pattern") {
    if (state.mode === "pattern" && state.pattern === sc.pattern) seek(Math.max(0, beat));
  } else if (state.mode === "song") seek(Math.max(0, beat));
}

/** function onPaperMove(e: Ev, v: ScoreView, c: Cached, geo: PageGeo) => Undefined */
function onPaperMove(e, v, c, geo) {
  const px = e.clientX - e.targetLeft + e.scrollLeft;
  const py = e.clientY - e.targetTop + e.scrollTop;
  const page = c.page;
  const si = sysAt(geo, page, py);
  const g = v.ghost;
  // While a button is held (a drag), what is under the pointer is not what a click would do.
  if (e.buttons !== 0 || v.tool === "pan") {
    setHover(v, "", -1, -1, 0);
    return undefined;
  }
  if (px < geo.left || px > geo.left + geo.paperW) {
    setHover(v, "desk", -1, -1, 0);
    hint("Drag to move the page about");
  }
  if (si < 0) {
    if (g.sys >= 0) {
      v.ghost = { sys: -1, row: -1, step: 0, tick: -1, x: 0 };
      invalidate();
    }
    if (px >= geo.left && px <= geo.left + geo.paperW) setHover(v, "", -1, -1, 0);
    return undefined;
  }
  const s = page.systems[si];
  const x = (px - geo.left - geo.padX) / geo.sp;
  const y = (py - sysY(geo, s)) / geo.sp;
  const ni = nameAt(s, x, y);
  if (ni >= 0) {
    setHover(v, "name", si, ni, 0);
    const st = c.score.staves[s.labels[ni].staff];
    const ch = channelById(st.channel);
    hint(
      `${ch ? ch.name : st.name}${ch ? ` (${instrumentLabel(ch.instrument)})` : ""} — click: its instrument in the channel rack, the piano roll or the mixer, or move the part to another instrument`
    );
    if (g.sys >= 0) {
      v.ghost = { sys: -1, row: -1, step: 0, tick: -1, x: 0 };
      invalidate();
    }
    return undefined;
  }
  if (px < geo.left || px > geo.left + geo.paperW) {
    if (g.sys >= 0) {
      v.ghost = { sys: -1, row: -1, step: 0, tick: -1, x: 0 };
      invalidate();
    }
    return undefined;
  }
  const hi = headAt(s, x, y);
  if (v.tool === "select") {
    if (hi >= 0) {
      setHover(v, "note", si, hi, 0);
      const n = c.score.notes[s.heads[hi].src];
      const ch = channelById(n.channel);
      hint(
        `${spelledName(n.pitch, c.score.fifths)} · ${ch ? ch.name : n.channel} — click to select, drag to move, double-click for the piano roll, right-click to delete`
      );
    } else if (x >= s.x0 - 0.5 && x <= s.x1 && canSeek(v)) {
      setHover(v, "paper", si, -1, seekTick(v, s, x));
      hint(
        `Click to move the playhead to bar ${barOf(c.score, seekTick(v, s, x))} · drag across the music to choose a passage (color it, repeat it, move it to another instrument)`
      );
    } else {
      setHover(v, "", -1, -1, 0);
      hint("Drag across the music to choose a passage (color it, repeat it, move it to another instrument) · Write (Shift+P) adds notes");
    }
    return undefined;
  }
  // Write: a note under the pointer is still grabbed (and lit); elsewhere the ghost shows the note to write.
  setHover(v, hi >= 0 ? "note" : "", hi >= 0 ? si : -1, hi, 0);
  const r = rowAt(s, y);
  if (r < 0 || x < s.x0 - 0.5 || x > s.x1) {
    if (g.sys >= 0) {
      v.ghost = { sys: -1, row: -1, step: 0, tick: -1, x: 0 };
      invalidate();
    }
    return undefined;
  }
  const row = s.rows[r];
  const top = bottomStep(row.clef) + 8;
  const step = Math.round(top - (y - row.y) * 2);
  const tick = snapTick(xTick(s.times, x - 0.4), v.grid, false);
  const tx = timeX(s.times, tick) - 0.59;
  if (g.sys !== si || g.row !== r || g.step !== step || g.tick !== tick) {
    v.ghost = { sys: si, row: r, step: step, tick: tick, x: tx };
    const pitch = pitchAt(c.score, row, step);
    hint(`${row.drum ? "Drum" : spelledName(pitch, c.score.fifths)} at bar ${barOf(c.score, tick)} — click to write it`);
    invalidate();
  }
}

/** function barOf(sc: Score, tick: Number) => Int */
function barOf(sc, tick) {
  for (let i = sc.measures.length - 1; i >= 0; i--) {
    const m = sc.measures[i];
    if (m.start <= tick) return m.number + Math.min(m.count - 1, Math.floor((tick - m.start) / (m.length / m.count)));
  }
  return 1;
}

// ------------------------------------------------------------------ render

/** function paperView(b: Builder, v: ScoreView, c: Cached, geo: PageGeo, sc: Scope) => Undefined */
function paperView(b, v, c, geo, sc) {
  const page = c.page;
  const score = c.score;
  const total = geo.top * 2 + paperH(geo, page);
  // The scroller, under a lamp that stays put while the paper slides beneath it.
  b.open("div", "view", "score-view");
  const over = v.hover.kind === "" ? "" : ` over-${v.hover.kind}`;
  b.open("div", "scroll", `score-scroll${v.tool === "write" ? " writing" : v.tool === "pan" ? " hand" : ""}${v.panning ? " grabbing" : ""}${over}`);
  // The width the page is laid out for: it matches the element once the layout has caught up with a resize.
  b.attr("data-width", String(Math.round(v.width)));
  b.on("scroll", (e) => {
    v.scrollTop = e.scrollTop;
    v.scrollLeft = e.scrollLeft;
    v.menu.on = false;
    invalidate();
  });
  b.on("resize", (e) => {
    if (Math.abs(v.width - e.targetWidth) > 0.5 || Math.abs(v.height - e.targetHeight) > 0.5) {
      v.width = e.targetWidth;
      v.height = e.targetHeight;
      invalidate();
    }
  });
  b.on("pointerdown", (e) => {
    if (v.pdfMenu) {
      v.pdfMenu = false;
      invalidate();
    }
    // The hand, the middle button, or a press on the desk beside the page: move the page about.
    const px = e.clientX - e.targetLeft + e.scrollLeft;
    const desk = px < geo.left || px > geo.left + geo.paperW;
    if (e.pointerType !== "touch" && (v.tool === "pan" || e.button === 1 || (desk && e.button === 0))) {
      e.preventDefault();
      pan(e, v, geo, total);
      return undefined;
    }
    pressOrTap(e, (d) => onPaperDown(d, v, c, geo));
  });
  b.on("pointermove", (e) => onPaperMove(e, v, c, geo));
  b.on("pointerleave", (e) => {
    if (v.ghost.sys >= 0) {
      v.ghost = { sys: -1, row: -1, step: 0, tick: -1, x: 0 };
      invalidate();
    }
    setHover(v, "", -1, -1, 0);
  });
  b.on("dblclick", (e) => {
    const px = e.clientX - e.targetLeft + e.scrollLeft;
    const py = e.clientY - e.targetTop + e.scrollTop;
    const si = sysAt(geo, page, py);
    if (si < 0) return undefined;
    const s = page.systems[si];
    const hi = headAt(s, (px - geo.left - geo.padX) / geo.sp, (py - sysY(geo, s)) / geo.sp);
    if (hi >= 0 && v.tool === "select") {
      selectNote(score, s.heads[hi].src, false);
      revealDock("piano");
    }
  });
  b.on("contextmenu", (e) => {
    e.preventDefault();
  });
  b.on("wheel", (e) => {
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      // Magnify about the pointer.
      zoomAt(v, v.zoom * (e.deltaY < 0 ? 1.1 : 1 / 1.1), e.clientX - e.targetLeft, e.clientY - e.targetTop);
    }
  });
  b.prop("scrollTop", String(v.scrollTop));
  b.prop("scrollLeft", String(v.scrollLeft));

  b.open("div", "content", "score-content");
  b.style("height", `${total}px`);
  b.style("width", `${contentW(v, geo)}px`);

  b.open("div", "paper", "score-paper");
  b.style("left", `${geo.left}px`);
  b.style("top", `${geo.top}px`);
  b.style("width", `${geo.paperW}px`);
  b.style("height", `${paperH(geo, page)}px`);
  b.style("--sp", `${geo.sp}px`);
  titleBlock(b, v, score, geo, sc);
  if (score.empty) {
    b.open("div", "empty", "score-empty");
    b.leaf("div", "t", "", sc.kind === "pattern" ? "This pattern has no notes yet." : "Nothing to write down here yet.");
    b.leaf(
      "div",
      "s",
      "",
      v.tool === "write"
        ? ""
        : state.project.score.hidden.length > 0
          ? "Some parts are hidden — show them in the sidebar."
          : "Draw notes in the piano roll, or switch to Write."
    );
    b.close();
  }
  b.close();

  inkDefs(b, v);

  // Systems in view (and a screen around).
  const lo = v.scrollTop - v.height;
  const hiY = v.scrollTop + v.height * 2;
  const sel = state.selection.map(noteIndex);
  for (let i = 0; i < page.systems.length; i++) {
    const s = page.systems[i];
    const y = sysY(geo, s);
    if (y + s.height * geo.sp < lo || y > hiY) continue;
    systemView(b, v, c, geo, i, y, sel);
  }

  playheadView(b, v, c, geo, sc);
  b.close();
  b.close();
  b.leaf("div", "lamp", "score-lamp", "");
  b.close();
}

/** The choice of PDF, under the ribbon's export button. */
/** function pdfMenu(b: Builder, v: ScoreView) => Undefined */
function pdfMenu(b, v) {
  b.open("div", "pdfmenu", "score-pdfmenu");
  /** function item(key: String, title: String, note: String, asShown: Boolean) => Undefined */
  function item(key, title, note, asShown) {
    b.open("button", key, "score-pdfitem");
    b.on("click", (e) => {
      v.pdfMenu = false;
      invalidate();
      exportPdf(v, asShown);
    });
    b.leaf("span", "t", "score-pdfitem-t", title);
    b.leaf("span", "n", "score-pdfitem-n", note);
    b.close();
  }
  item("vector", "Vector PDF", "Crisp at any size, small, ready to print", false);
  item("shown", "As on screen", `The paper's texture and the ${v.ink} ink's effects — an image a page`, true);
  b.close();
}

/** The line under the title: what is shown, and the key. */
/** function subtitle(score: Score, sc: Scope) => String */
function subtitle(score, sc) {
  const p = state.project;
  /** const sub: String[] */
  const sub = [];
  if (sc.kind === "pattern") sub.push(`pattern · ${p.patterns.find((x) => x.id === sc.pattern) ? fmt(score.end / TPQ, 0) : "0"} beats`);
  if (sc.kind === "track") sub.push("one track of the song");
  if (!score.empty) sub.push(keyLabel(score.keyName));
  // The master transpose shifts what plays, not what is written.
  const t = p.transport.transpose;
  if (t !== 0) sub.push(`sounds ${semitonesText(t)} semitone${Math.abs(t) === 1 ? "" : "s"}`);
  return sub.join(" · ");
}

/** A file name in plain ASCII (browsers drop names they cannot pass on): accents dropped, ligatures and symbols spelled. */
/** function fileName(title: String) => String */
export function fileName(title) {
  const plain = title
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .replaceAll("œ", "oe")
    .replaceAll("Œ", "OE")
    .replaceAll("æ", "ae")
    .replaceAll("Æ", "AE")
    .replaceAll("ß", "ss")
    .replaceAll("♭", "b")
    .replaceAll("♯", "#")
    .replace(/[·–—]/g, "-")
    .replace(/[‘’]/g, "'")
    .replace(/[^\x20-\x7e]/g, "")
    .replace(/[\\/:*?"<>|]+/g, "-")
    .replace(/\s+/g, " ")
    .trim();
  return plain === "" ? "score" : plain;
}

/** Download what the view shows as a PDF (A4, or US Letter where that is the paper). */
/** Pixels a point for pages drawn as on screen: 200 dots an inch. */
const PRINT_SCALE = 200 / 72;

/** Download the score as a PDF: vector, or (`asShown`) drawn as on screen, the paper's
 * textures and the ink's effects in an image a page. */
/** function exportPdf(v: ScoreView, asShown: Boolean) => Undefined */
function exportPdf(v, asShown) {
  const sc = scopeOf(v);
  const c = cached(v, sc, pageGeo(v, sc));
  if (c.score.empty) {
    toast("Nothing to print", "This score has no notes to write down.", "warn");
    return undefined;
  }
  const p = state.project;
  const title = scopeTitle(sc);
  const info = { title: title, subtitle: subtitle(c.score, sc), author: sc.kind === "song" ? p.meta.author : "", bpm: p.transport.bpm };
  const hide = v.condense && sc.kind !== "pattern";
  const name = `${fileName(title)}.pdf`;
  const size = paperSize() === "letter" ? "US Letter" : "A4";
  if (asShown) {
    const lay = pdfLayout(c.ink, paperSize(), hide);
    const look = { wet: v.ink === "wet", gloss: v.gloss, shine: v.shine, filters: true };
    /** const svgs: String[] */
    const svgs = [];
    for (let i = 0; i < lay.pages.length; i++) svgs.push(pageSvg(lay, i, info, look, PRINT_SCALE));
    toast("Drawing the pages", `${svgs.length} page${svgs.length === 1 ? "" : "s"} on paper, in ${v.ink} ink…`, "info");
    downloadImagePdf(name, pdfInfo(title), svgs, lay.w, lay.h, PRINT_SCALE)
      .then((ok) => {
        toast("Score exported", `${name} — as on screen, ${size}.`, "info");
        return true;
      })
      .catch((e) => {
        toast("The PDF could not be written", String(e), "error");
        return false;
      });
    return undefined;
  }
  const objs = scorePdf(c.ink, info, paperSize(), hide, textWidth);
  downloadPdf(name, objs)
    .then((ok) => {
      toast("Score exported", `${name} — vector, ${size}.`, "info");
      return true;
    })
    .catch((e) => {
      toast("The PDF could not be written", String(e), "error");
      return false;
    });
}

/** function titleBlock(b: Builder, v: ScoreView, score: Score, geo: PageGeo, sc: Scope) => Undefined */
function titleBlock(b, v, score, geo, sc) {
  const p = state.project;
  b.open("div", "head", sc.kind === "song" ? "score-head song" : "score-head");
  b.style("padding", `${geo.padX * 0.6}px ${geo.padX}px 0`);
  b.style("height", `${geo.head}px`);
  b.leaf("div", "title", "score-title", scopeTitle(sc));
  b.leaf("div", "sub", "score-sub", subtitle(score, sc));
  b.open("div", "row", "score-row");
  b.style("padding", `0 ${geo.padX}px`);
  b.open("span", "tempo", "score-tempo");
  b.leaf("span", "n", "smufl", "\u{eca5}");
  b.leaf("span", "v", "", ` = ${fmt(p.transport.bpm, 0)}`);
  b.close();
  b.leaf("span", "author", "score-author", sc.kind === "song" ? p.meta.author : "");
  b.close();
  b.close();
}

/** Build an SVG element and its children (described in ink.js) into the view. */
/** function build(b: Builder, e: El) => Undefined */
function build(b, e) {
  if (e.kids.length === 0) b.leaf(e.tag, e.key, "", "");
  else b.open(e.tag, e.key, "");
  for (let i = 0; i + 1 < e.attrs.length; i = i + 2) b.attr(e.attrs[i], e.attrs[i + 1]);
  if (e.kids.length === 0) return undefined;
  for (const k of e.kids) build(b, k);
  b.close();
}

/** The ink filter the engraving is drawn through (see inkFilter in ink.js). */
/** function inkDefs(b: Builder, v: ScoreView) => Undefined */
function inkDefs(b, v) {
  b.open("svg", "defs", "score-defs");
  b.attr("width", "0");
  b.attr("height", "0");
  b.attr("aria-hidden", "true");
  b.open("defs", "d", "");
  build(b, inkFilter(`score-ink-${v.id}`, v.ink === "wet", v.night, v.gloss, v.shine, v.size * v.zoom * pixelRatio()));
  b.close();
  b.close();
}

/** function systemView(b: Builder, v: ScoreView, c: Cached, geo: PageGeo, i: Int, y: Number, sel: Int[]) => Undefined */
function systemView(b, v, c, geo, i, y, sel) {
  const s = c.page.systems[i];
  const sp = geo.sp;
  const w = geo.paperW - geo.padX * 2;
  const pad = 1.6;
  b.open("svg", `s${i}`, "score-sys");
  b.attr("viewBox", `0 ${fmt(-pad, 2)} ${fmt(w / sp, 3)} ${fmt(s.height + pad * 2, 3)}`);
  b.attr("width", fmt(w, 1));
  b.attr("height", fmt((s.height + pad * 2) * sp, 1));
  b.style("left", `${geo.left + geo.padX}px`);
  b.style("top", `${y - pad * sp}px`);
  // Colored passages behind the music.
  for (let k = 0; k < s.bands.length; k++) {
    const bd = s.bands[k];
    b.leaf("rect", `band${k}`, "score-band", "");
    b.attr("x", fmt(bd.x, 2));
    b.attr("y", fmt(bd.y, 2));
    b.attr("width", fmt(bd.w, 2));
    b.attr("height", fmt(bd.h, 2));
    b.attr("rx", "0.8");
    b.attr("fill", tint(bd.color, v.night ? 0.2 : 0.14));
    b.attr("stroke", tint(bd.color, v.night ? 0.55 : 0.45));
  }
  // The passage being chosen.
  const r = v.range;
  if (r.on) {
    const t0 = Math.min(r.t0, r.t1);
    const t1 = Math.max(r.t0, r.t1);
    if (t1 > s.start && t0 < s.end) {
      let ya = 1e9;
      let yb = -1e9;
      for (const row of s.rows) {
        if (row.staff < r.s0 || row.staff > r.s1) continue;
        ya = Math.min(ya, row.y - 1.4);
        yb = Math.max(yb, row.y + 5.4);
      }
      if (ya < yb) {
        const x0 = t0 <= s.start ? s.x0 - 0.8 : timeX(s.times, t0) - 0.9;
        const x1 = t1 >= s.end ? s.x1 + 0.4 : timeX(s.times, t1) - 0.9;
        b.leaf("rect", "range", "score-range", "");
        b.attr("x", fmt(x0, 2));
        b.attr("y", fmt(ya, 2));
        b.attr("width", fmt(Math.max(0.4, x1 - x0), 2));
        b.attr("height", fmt(yb - ya, 2));
        b.attr("rx", "0.8");
      }
    }
  }
  // The engraving itself, through the ink filter.
  // Staff lines are hairlines: drawn plainly, so they stay smooth and even.
  for (let k = 0; k < s.inks.length; k++) {
    const ink = s.inks[k];
    if (ink.color !== "staff" || ink.d === "") continue;
    b.leaf("path", `p${k}`, "staff", "");
    b.attr("d", ink.d);
  }
  b.open("g", "ink", "inked");
  b.attr("filter", `url(#score-ink-${v.id})`);
  for (let k = 0; k < s.inks.length; k++) {
    const ink = s.inks[k];
    if (ink.color === GLOSS || ink.color === SHEEN || ink.color === "staff") continue;
    const named = ink.color === "" || ink.color === "staff";
    const cls = ink.color === "staff" ? "staff" : "ink";
    if (ink.d !== "") {
      b.leaf("path", `p${k}`, named ? cls : "", "");
      b.attr("d", ink.d);
      if (!named) b.attr("fill", inkColor(ink.color, v.night));
    }
    if (ink.text !== "") {
      b.leaf("text", `t${k}`, named ? `glyphs ${cls}` : "glyphs", ink.text);
      b.attr("x", ink.xs);
      b.attr("y", ink.ys);
      if (!named) b.attr("fill", inkColor(ink.color, v.night));
    }
  }
  for (let k = 0; k < s.braces.length; k++) {
    const br = s.braces[k];
    b.leaf("text", `br${k}`, "glyphs ink", "\u{e000}");
    b.attr("transform", `translate(${fmt(br.x, 2)} ${fmt(br.y, 2)}) scale(${fmt(br.s, 3)})`);
  }
  for (let k = 0; k < s.labels.length; k++) {
    const l = s.labels[k];
    b.leaf("text", `l${k}`, l.cls, l.text);
    b.attr("x", fmt(l.x, 2));
    b.attr("y", fmt(l.y, 2));
    b.attr("text-anchor", l.anchor);
  }
  b.close();
  for (let k = 0; k < s.bands.length; k++) {
    const bd = s.bands[k];
    if (bd.label === "" || !bd.first) continue;
    b.leaf("text", `bl${k}`, "score-tag", bd.label);
    b.attr("x", fmt(bd.x + 0.5, 2));
    b.attr("y", fmt(bd.y - 0.45, 2));
    b.attr("fill", inkColor(bd.color, v.night));
  }
  // Selected notes, drawn again on top in the selection color.
  /** const xs: String[] */
  const xs = [];
  /** const ys: String[] */
  const ys = [];
  let chars = "";
  if (sel.length > 0) {
    for (const h of s.heads) {
      const n = c.score.notes[h.src];
      if (n.pattern !== state.pattern || !sel.includes(n.index)) continue;
      chars = chars + h.glyph;
      xs.push(fmt(h.x, 2));
      ys.push(fmt(h.y, 2));
    }
  }
  if (chars !== "") {
    b.leaf("text", "sel", "glyphs sel", chars);
    b.attr("x", xs.join(" "));
    b.attr("y", ys.join(" "));
  }
  hoverView(b, v, s, i);
  // The gloss of the wet ink, over the music (and the selection): the soft sheen, then the glints.
  if (v.ink === "wet") {
    for (const run of [SHEEN, GLOSS]) {
      const ink = s.inks.find((x) => x.color === run);
      if (!ink) continue;
      b.leaf("path", run, run, "");
      b.attr("d", ink.d);
      b.style("opacity", fmt(glintOpacity(run, v.gloss, v.shine), 3));
    }
  }
  // Where Write would put a note.
  const g = v.ghost;
  if (v.tool === "write" && g.sys === i && g.row >= 0 && g.row < s.rows.length) {
    const row = s.rows[g.row];
    const gy = row.y + (bottomStep(row.clef) + 8 - g.step) / 2;
    b.leaf("text", "ghost", "glyphs ghost", v.value >= 192 ? "\u{e0a2}" : v.value >= 96 ? "\u{e0a3}" : "\u{e0a4}");
    b.attr("x", fmt(g.x, 2));
    b.attr("y", fmt(gy, 2));
    // Ledger lines it would need.
    let k = 0;
    for (let ly = -1; ly >= gy - row.y - 0.01; ly = ly - 1) {
      b.leaf("rect", `gl${k}`, "ghost-ledger", "");
      b.attr("x", fmt(g.x - 0.4, 2));
      b.attr("y", fmt(row.y + ly - 0.08, 2));
      b.attr("width", "1.98");
      b.attr("height", "0.16");
      k = k + 1;
    }
    for (let ly = 5; ly <= gy - row.y + 0.01; ly = ly + 1) {
      b.leaf("rect", `gl${k}`, "ghost-ledger", "");
      b.attr("x", fmt(g.x - 0.4, 2));
      b.attr("y", fmt(row.y + ly - 0.08, 2));
      b.attr("width", "1.98");
      b.attr("height", "0.16");
      k = k + 1;
    }
  }
  b.close();
}

/** What a click would act on, lit under the pointer: a staff's name (underlined, glowing), a note
 * (lit to be grabbed), or the line on the paper the playhead would move to. */
/** function hoverView(b: Builder, v: ScoreView, s: Sys, i: Int) => Undefined */
function hoverView(b, v, s, i) {
  const h = v.hover;
  if (h.sys !== i || v.menu.on) return undefined;
  if (h.kind === "name" && h.idx < s.labels.length) {
    const l = s.labels[h.idx];
    const w = nameWidth(l);
    b.leaf("rect", "hname-line", "name-line", "");
    b.attr("x", fmt(l.x - w, 2));
    b.attr("y", fmt(l.y + 0.3, 2));
    b.attr("width", fmt(w, 2));
    b.attr("height", "0.13");
    b.attr("rx", "0.06");
    b.leaf("text", "hname", `${l.cls} name-hot`, l.text);
    b.attr("x", fmt(l.x, 2));
    b.attr("y", fmt(l.y, 2));
    b.attr("text-anchor", l.anchor);
  }
  if (h.kind === "note" && h.idx < s.heads.length) {
    const hd = s.heads[h.idx];
    b.leaf("text", "hnote", "glyphs hov", hd.glyph);
    b.attr("x", fmt(hd.x, 2));
    b.attr("y", fmt(hd.y, 2));
  }
  if (h.kind === "paper" && s.rows.length > 0) {
    const y0 = s.rows[0].y - 2;
    const y1 = s.rows[s.rows.length - 1].y + 6;
    b.leaf("rect", "hseek", "seek-line", "");
    b.attr("x", fmt(timeX(s.times, h.x) + 0.59 - 0.07, 2));
    b.attr("y", fmt(y0, 2));
    b.attr("width", "0.14");
    b.attr("height", fmt(y1 - y0, 2));
    b.attr("rx", "0.07");
  }
}

/** The playhead: a line through the system that is playing. */
/** function playheadView(b: Builder, v: ScoreView, c: Cached, geo: PageGeo, sc: Scope) => Undefined */
function playheadView(b, v, c, geo, sc) {
  const showing = sc.kind === "pattern" ? state.mode === "pattern" && state.pattern === sc.pattern : state.mode === "song";
  if (!showing || (!state.playing && state.position <= 0)) return undefined;
  const tick = state.position * TPQ;
  const page = c.page;
  for (let i = 0; i < page.systems.length; i++) {
    const s = page.systems[i];
    if (tick < s.start || tick >= s.end) continue;
    const x = geo.left + geo.padX + (timeX(s.times, tick) + 0.59) * geo.sp;
    const y = sysY(geo, s);
    b.leaf("div", "ph", state.playing ? "score-ph on" : "score-ph", "");
    b.style("left", `${x}px`);
    b.style("top", `${y + (s.rows.length > 0 ? s.rows[0].y - 2 : 0) * geo.sp}px`);
    const bottom = s.rows.length > 0 ? s.rows[s.rows.length - 1].y + 6 : s.height;
    b.style("height", `${(bottom - (s.rows.length > 0 ? s.rows[0].y - 2 : 0)) * geo.sp}px`);
    // Follow: keep the playing system in view.
    if (state.follow && state.playing && (y < v.scrollTop || y + s.height * geo.sp > v.scrollTop + v.height)) {
      v.scrollTop = Math.max(0, y - Math.min(40, v.height * 0.1));
    }
    // Zoomed in, keep it in view across too.
    if (state.follow && state.playing && (x < v.scrollLeft + 20 || x > v.scrollLeft + v.width - 20)) {
      v.scrollLeft = Math.max(0, x - v.width * 0.25);
    }
    return undefined;
  }
}

// ------------------------------------------------------------------ sidebar

/** function eye(b: Builder, key: String, on: Boolean, tip: String, onClick: () => Undefined) => Undefined */
function eye(b, key, on, tip, onClick) {
  b.open("button", key, on ? "score-eye on" : "score-eye");
  b.attr("title", tip);
  b.attr("aria-label", tip);
  b.attr("aria-pressed", on ? "true" : "false");
  b.on("click", (e) => onClick());
  b.on("pointerenter", (e) => hint(tip));
  b.open("svg", "g", "glyph");
  b.attr("viewBox", "0 0 24 24");
  b.leaf("path", "p", "", "");
  b.attr(
    "d",
    on
      ? "M2.5 12s3.5-6.5 9.5-6.5 9.5 6.5 9.5 6.5-3.5 6.5-9.5 6.5S2.5 12 2.5 12zM12 9.2a2.8 2.8 0 1 0 0 5.6 2.8 2.8 0 1 0 0-5.6z"
      : "M4 4l16 16M9.5 6a10 10 0 0 1 2.5-.5c6 0 9.5 6.5 9.5 6.5a17 17 0 0 1-2.6 3.4M6.3 7.6C3.9 9.4 2.5 12 2.5 12s3.5 6.5 9.5 6.5a9.5 9.5 0 0 0 4-.9"
  );
  b.close();
  b.close();
}

const CLEF_IDS = ["auto", "treble", "treble8vb", "bass", "alto", "grand", "percussion"];
const CLEF_NAMES = ["Auto", "Treble", "Treble 8vb", "Bass", "Alto", "Grand staff", "Drums"];

/** function sideView(b: Builder, v: ScoreView, c: Cached, sc: Scope, geo: PageGeo) => Undefined */
function sideView(b, v, c, sc, geo) {
  const p = state.project;
  const settings = p.score;
  b.open("aside", "side", "score-side");

  // Parts: every channel with notes here.
  b.leaf("div", "h1", "score-side-h", "Parts");
  /** const used: String[] */
  const used = [];
  for (const n of c.score.notes) if (!used.includes(n.channel)) used.push(n.channel);
  b.open("div", "parts", "score-list");
  if (used.length === 0) b.leaf("div", "none", "score-none", "No notes here.");
  for (const ch of p.channels) {
    if (!used.includes(ch.id)) continue;
    const shown = !settings.hidden.includes(ch.id);
    b.open("div", `c-${ch.id}`, shown ? "score-part" : "score-part off");
    eye(b, "eye", shown, shown ? `Hide ${ch.name} in the score` : `Show ${ch.name} in the score`, () => {
      commit(() => {
        if (shown) settings.hidden.push(ch.id);
        else settings.hidden = settings.hidden.filter((x) => x !== ch.id);
      });
    });
    b.leaf("span", "dot", "score-dot", "");
    b.style("--c", ch.color);
    b.leaf("button", "name", "score-part-name hot", ch.name);
    const tip = `${ch.name} (${instrumentLabel(ch.instrument)}) — its instrument in the channel rack, the piano roll or the mixer, or move the part to another instrument`;
    b.attr("title", tip);
    b.on("pointerenter", (e) => hint(tip));
    b.on("click", (e) => openPartMenu(v, [ch.id], e.clientX + 4, e.clientY + 8));
    const set = settings.clefs.find((x) => x.key === ch.id);
    select(b, "clef", "score-clef", set ? set.value : "auto", CLEF_IDS, CLEF_NAMES, `Clef for ${ch.name}`, (val) => {
      commit(() => {
        settings.clefs = settings.clefs.filter((x) => x.key !== ch.id);
        if (val !== "auto") settings.clefs.push({ key: ch.id, value: val });
      });
    });
    b.close();
  }
  b.close();
  if (used.length > 1) {
    b.open("div", "bulk", "score-bulk");
    b.leaf("button", "all", "btn small ghost", "Show all");
    b.on("click", (e) => {
      if (settings.hidden.length > 0)
        commit(() => {
          settings.hidden = [];
        });
    });
    b.leaf("button", "cond", v.condense ? "btn small ghost on-text" : "btn small ghost", v.condense ? "✓ Hide resting staves" : "Hide resting staves");
    b.attr("title", "Leave out the staves that only rest in a system, as orchestral scores do");
    b.on("click", (e) => {
      v.condense = !v.condense;
      invalidate();
    });
    b.close();
  }

  // Tracks (the song): leave a playlist track out.
  if (sc.kind === "song") {
    /** const tracks: Int[] */
    const tracks = [];
    for (const cl of p.playlist.clips) if (cl.pattern !== "" && !tracks.includes(trackIndex(cl.track))) tracks.push(trackIndex(cl.track));
    tracks.sort((a, b) => a - b);
    if (tracks.length > 1) {
      b.leaf("div", "h2", "score-side-h", "Tracks");
      b.open("div", "tracks", "score-list");
      const hidden = settings.hiddenTracks.map(trackIndex);
      for (const t of tracks) {
        const tr = t < p.playlist.tracks.length ? p.playlist.tracks[t] : undefined;
        const name = tr ? tr.name : `Track ${t + 1}`;
        const shown = !hidden.includes(t);
        b.open("div", `t${t}`, shown ? "score-part" : "score-part off");
        eye(b, "eye", shown, shown ? `Leave ${name} out of the score` : `Bring ${name} back into the score`, () => {
          commit(() => {
            if (shown) settings.hiddenTracks.push(trackIx(t));
            else settings.hiddenTracks = settings.hiddenTracks.filter((x) => trackIndex(x) !== t);
          });
        });
        b.leaf("span", "name", "score-part-name", name);
        b.leaf("button", "only", "btn small ghost score-only", "Only");
        b.attr("title", `Show just ${name} (the track's own score)`);
        b.on("click", (e) => setScope(v, `track:${t}`));
        b.close();
      }
      b.close();
    }
  }

  if (sc.kind !== "pattern") repeatsSide(b, v, c, geo);

  // Colored passages.
  b.open("div", "h3", "score-side-h with-eye");
  b.leaf("span", "t", "", "Colors");
  eye(b, "eye", v.colors, v.colors ? "Hide the colors: write everything in plain ink" : "Show the colored passages", () => {
    v.colors = !v.colors;
    savePrefs(v);
    invalidate();
  });
  b.close();
  b.open("div", "marks", "score-list");
  const marks = settings.marks;
  let any = false;
  for (let i = 0; i < marks.length; i++) {
    const m = marks[i];
    const spans = c.score.marks.filter((x) => x.mark === i);
    if (spans.length === 0) continue;
    any = true;
    b.open("div", `m${i}`, "score-mark");
    b.open("button", "sw", "score-swatch");
    b.style("--c", m.color);
    b.attr("title", "Change the color");
    b.on("click", (e) => {
      const k = PALETTE.indexOf(m.color);
      commit(() => {
        m.color = PALETTE[(k + 1) % PALETTE.length];
      });
    });
    b.close();
    textInput(b, "label", "score-mark-label", m.label, "Label…", (val) => {
      if (val !== m.label)
        commit(() => {
          m.label = val;
        });
    });
    const first = spans[0];
    const bar0 = barOf(c.score, first.start * TPQ);
    const bar1 = barOf(c.score, first.end * TPQ - 1);
    b.leaf("button", "go", "score-mark-where", bar0 === bar1 ? `bar ${bar0}` : `bars ${bar0}–${bar1}`);
    b.attr("title", m.pattern !== "" ? `In pattern ${m.pattern}, wherever it plays — click to show` : "Click to show");
    b.on("click", (e) => {
      for (const s of c.page.systems) {
        if (first.start * TPQ >= s.start && first.start * TPQ < s.end) {
          v.scrollTop = Math.max(0, sysY(geo, s) - 30);
          invalidate();
        }
      }
    });
    iconButton(b, "x", "small ghost", "close", "Remove this color", () => {
      commit(() => {
        state.project.score.marks = state.project.score.marks.filter((x) => x !== m);
      });
    });
    b.close();
  }
  if (!any) b.leaf("div", "none", "score-none", "Drag across the music to color a passage.");
  b.close();
  b.close();
}

/** Scroll to the system that holds a beat. */
/** function showBeat(v: ScoreView, c: Cached, geo: PageGeo, beat: Number) => Undefined */
function showBeat(v, c, geo, beat) {
  for (const s of c.page.systems) {
    if (beat * TPQ >= s.start && beat * TPQ < s.end) {
      v.scrollTop = Math.max(0, sysY(geo, s) - 30);
      invalidate();
    }
  }
}

/** The song's repeats: how many times each plays, and which passes take which ending. */
/** function repeatsSide(b: Builder, v: ScoreView, c: Cached, geo: PageGeo) => Undefined */
function repeatsSide(b, v, c, geo) {
  const reps = state.project.repeats;
  b.leaf("div", "hr", "score-side-h", "Repeats");
  b.open("div", "repeats", "score-list");
  if (reps.length === 0) b.leaf("div", "none", "score-none", "Drag across some bars, then Repeat.");
  for (let i = 0; i < reps.length; i++) {
    const r = reps[i];
    const bar0 = barOf(c.score, r.start * TPQ);
    const bar1 = barOf(c.score, r.end * TPQ - 1);
    b.open("div", `r${i}`, "score-mark score-rep");
    b.leaf("span", "sign", "score-rep-sign", "\u{e040}");
    b.leaf("button", "go", "score-mark-where score-rep-where", bar0 === bar1 ? `bar ${bar0}` : `bars ${bar0}–${bar1}`);
    b.attr("title", "Click to show");
    b.on("click", (e) => showBeat(v, c, geo, r.start));
    b.open("span", "times", "score-rep-times");
    iconButton(b, "less", "small ghost", "minus", "Play it one time less", () => {
      if (r.times > 2)
        commit(() => {
          const rr = state.project.repeats[i];
          rr.times = rr.times - 1;
          for (const e of rr.endings) e.passes = e.passes.filter((k) => k <= rr.times);
          rr.endings = rr.endings.filter((e) => e.passes.length > 0);
        });
    });
    b.leaf("span", "n", "score-rep-n", `×${r.times}`);
    b.attr("title", `Plays ${r.times} times`);
    iconButton(b, "more", "small ghost", "plus", "Play it one more time", () => {
      if (r.times < 99)
        commit(() => {
          const rr = state.project.repeats[i];
          // The ending after the repeat stays the last pass's.
          for (const e of rr.endings) if (e.start >= rr.end - 1e-9) e.passes = e.passes.map((k) => (k === rr.times ? k + 1 : k));
          rr.times = rr.times + 1;
        });
    });
    b.close();
    iconButton(b, "x", "small ghost", "close", "Remove this repeat (and its endings)", () => {
      commit(() => {
        state.project.repeats = state.project.repeats.filter((x) => x !== r);
      });
    });
    b.close();
    for (let j = 0; j < r.endings.length; j++) {
      const en = r.endings[j];
      const e0 = barOf(c.score, en.start * TPQ);
      const e1 = barOf(c.score, en.end * TPQ - 1);
      b.open("div", `r${i}e${j}`, "score-mark score-ending");
      b.leaf("span", "text", "score-ending-text", passesText(en.passes));
      b.leaf("button", "go", "score-mark-where", e0 === e1 ? `bar ${e0}` : `bars ${e0}–${e1}`);
      b.attr("title", "Click to show");
      b.on("click", (e) => showBeat(v, c, geo, en.start));
      b.open("span", "passes", "score-passes");
      for (let k = 1; k <= r.times; k++) {
        const on = en.passes.includes(k);
        b.leaf("button", `p${k}`, on ? "score-pass on" : "score-pass", `${k}`);
        b.attr("title", on ? `Pass ${k} plays this ending — click to skip it` : `Play this ending on pass ${k}`);
        b.attr("aria-pressed", on ? "true" : "false");
        b.on("click", (e) => {
          if (on && en.passes.length === 1) return undefined;
          commit(() => {
            const ee = state.project.repeats[i].endings[j];
            if (on) ee.passes = ee.passes.filter((x) => x !== k);
            else {
              ee.passes.push(k);
              ee.passes.sort((x, y) => x - y);
            }
          });
        });
      }
      b.close();
      iconButton(b, "x", "small ghost", "close", "Remove this ending", () => {
        commit(() => {
          const rr = state.project.repeats[i];
          rr.endings = rr.endings.filter((x) => x !== en);
        });
      });
      b.close();
    }
  }
  b.close();
}

/** The bar over a chosen passage: colors to paint it with. */
/** function rangeBar(b: Builder, v: ScoreView, c: Cached) => Undefined */
function rangeBar(b, v, c) {
  const r = v.range;
  if (!r.on) return undefined;
  const bar0 = barOf(c.score, Math.min(r.t0, r.t1));
  const bar1 = barOf(c.score, Math.max(r.t0, r.t1) - 1);
  const staves = r.s1 - r.s0 + 1;
  b.open("div", "rangebar", "score-rangebar");
  b.leaf(
    "span",
    "what",
    "score-range-what",
    `${bar0 === bar1 ? `Bar ${bar0}` : `Bars ${bar0}–${bar1}`} · ${staves === c.score.staves.length ? "all staves" : staves === 1 ? "1 staff" : `${staves} staves`}`
  );
  for (const col of PALETTE.slice(0, 8)) {
    b.open("button", col, "score-swatch big");
    b.style("--c", col);
    b.attr("title", "Color this passage");
    b.on("click", (e) => colorRange(v, c.score, col));
    b.close();
  }
  b.leaf("button", "clear", "btn small ghost", "Clear colors");
  b.attr("title", "Remove the colors that touch this passage");
  b.on("click", (e) => clearRange(v, c.score));
  moveRangeSelect(b, v, c);
  if (scopeOf(v).kind !== "pattern") {
    b.leaf("button", "repeat", "btn small ghost", "Repeat");
    b.attr("title", "Repeat these bars (play them twice; set how many times under Repeats)");
    b.on("click", (e) => repeatRange(v, c.score));
    b.leaf("button", "ending", "btn small ghost", "Ending");
    b.attr("title", "Make these bars an ending: inside a repeat they play on the passes before the last; right after it, on the last");
    b.on("click", (e) => endingRange(v, c.score));
  }
  iconButton(b, "x", "small ghost", "close", "Cancel (Escape)", () => {
    v.range.on = false;
    invalidate();
  });
  b.close();
}

/** The channels with notes in the chosen passage, on the chosen staves. */
/** function rangeChannels(v: ScoreView, sc: Score) => String[] */
function rangeChannels(v, sc) {
  const r = v.range;
  const t0 = Math.min(r.t0, r.t1) / TPQ;
  const t1 = Math.max(r.t0, r.t1) / TPQ;
  /** const staffed: String[] */
  const staffed = [];
  for (let k = r.s0; k <= r.s1 && k < sc.staves.length; k++) for (const id of sc.staves[k].channels) if (!staffed.includes(id)) staffed.push(id);
  /** const out: String[] */
  const out = [];
  for (const n of sc.notes) {
    if (n.start < t0 - 1e-6 || n.start >= t1 - 1e-6 || !staffed.includes(n.channel) || out.includes(n.channel)) continue;
    out.push(n.channel);
  }
  return out;
}

/** Give the chosen passage's part (the chosen staves) to another instrument. */
/** function moveRangeSelect(b: Builder, v: ScoreView, c: Cached) => Undefined */
function moveRangeSelect(b, v, c) {
  const parts = rangeChannels(v, c.score);
  if (parts.length === 0) return undefined;
  /** const ids: String[] */
  const ids = [""];
  /** const names: String[] */
  const names = ["Move to…"];
  for (const ch of state.project.channels) {
    if (parts.length === 1 && parts[0] === ch.id) continue;
    ids.push(ch.id);
    names.push(ch.name);
  }
  if (ids.length === 1) return undefined;
  const who = parts.map((id) => {
    const ch = channelById(id);
    return ch ? ch.name : id;
  });
  select(
    b,
    "move",
    "score-range-move",
    "",
    ids,
    names,
    `Give these bars of ${who.join(", ")} to another instrument (the rest of the song keeps ${parts.length === 1 ? "it" : "them"}; Ctrl+Z undoes)`,
    (val) => {
      if (val === "") return undefined;
      const r = v.range;
      v.range.on = false;
      const bar0 = barOf(c.score, Math.min(r.t0, r.t1));
      const bar1 = barOf(c.score, Math.max(r.t0, r.t1) - 1);
      const bars = bar0 === bar1 ? `in bar ${bar0}` : `in bars ${bar0}–${bar1}`;
      movePart(v, parts, val, Math.min(r.t0, r.t1) / TPQ, Math.max(r.t0, r.t1) / TPQ, scopeOf(v).kind === "pattern" ? `${bars} of the pattern` : bars);
    }
  );
}

// ------------------------------------------------------------------ public

/** function setScope(v: ScoreView, scope: String) => Undefined */
function setScope(v, scope) {
  v.scope = scope;
  v.scrollTop = 0;
  v.range.on = false;
  savePrefs(v);
  invalidate();
}

/** The music's size: larger notes, fewer bars on a line (the page is laid out again). */
/** function setSize(v: ScoreView, size: Number) => Undefined */
function setSize(v, size) {
  const next = Math.max(SIZE_MIN, Math.min(SIZE_MAX, Math.round(size)));
  if (next === v.size) return undefined;
  v.scrollTop = v.scrollTop * (next / v.size);
  v.size = next;
  savePrefs(v);
  invalidate();
}

/** The next zoom step up (dir 1) or down (-1) from the current one. */
/** function zoomStep(v: ScoreView, dir: Int) => Number */
function zoomStep(v, dir) {
  if (dir > 0) return ZOOMS.find((z) => z > v.zoom + 0.001) ?? ZOOMS[ZOOMS.length - 1];
  let down = ZOOMS[0];
  for (const z of ZOOMS) if (z < v.zoom - 0.001) down = z;
  return down;
}

/** Magnify the page, keeping the point (ax, ay) of the view (pixels from its corner) where it is. */
/** function zoomAt(v: ScoreView, z: Number, ax: Number, ay: Number) => Undefined */
function zoomAt(v, z, ax, ay) {
  const next = Math.max(ZOOMS[0], Math.min(ZOOMS[ZOOMS.length - 1], Math.round(z * 100) / 100));
  if (next === v.zoom) return undefined;
  const r = next / v.zoom;
  v.scrollTop = Math.max(0, (v.scrollTop + ay) * r - ay);
  v.scrollLeft = Math.max(0, (v.scrollLeft + ax) * r - ax);
  v.zoom = next;
  savePrefs(v);
  invalidate();
}

/** Width of the scroller's content: the view, or the zoomed page and its margins. */
/** function contentW(v: ScoreView, geo: PageGeo) => Number */
function contentW(v, geo) {
  return Math.max(v.width, geo.left + geo.paperW + geo.outer);
}

/** Drag the page about (the view scrolls the other way). */
/** function pan(e: Ev, v: ScoreView, geo: PageGeo, total: Number) => Undefined */
function pan(e, v, geo, total) {
  const x0 = e.clientX;
  const y0 = e.clientY;
  const l0 = v.scrollLeft;
  const t0 = v.scrollTop;
  v.panning = true;
  invalidate();
  drag(
    e,
    (m) => {
      v.scrollLeft = Math.max(0, Math.min(contentW(v, geo) - v.width, l0 - (m.clientX - x0)));
      v.scrollTop = Math.max(0, Math.min(total - v.height, t0 - (m.clientY - y0)));
      invalidate();
    },
    (u) => {
      v.panning = false;
      invalidate();
    }
  );
}

/** The notation the film is made of (no page is engraved for the view's width). */
/** function filmScore(v: ScoreView, sc: Scope) => Score */
function filmScore(v, sc) {
  const key = `${state.edits}|${sc.kind}|${sc.track}|${sc.pattern}|${v.grid}|${v.colors}`;
  for (const c of v.filmCache) if (c.key === key) return c.score;
  const score = buildScore(state.project, sc, v.grid);
  const ink = v.colors ? score : withoutColors(score);
  v.filmCache.length = 0;
  v.filmCache.push({ key: key, score: ink });
  return ink;
}

/** The film of what the view shows (film.js, ui/film.js). */
/** function filmOf(b: Builder, v: ScoreView, sc: Scope) => Undefined */
function filmOf(b, v, sc) {
  const p = state.project;
  const score = filmScore(v, sc);
  const m = score.measures.length > 0 ? score.measures[0] : undefined;
  filmView(b, v.fv, {
    sc: score,
    hide: v.condense && sc.kind !== "pattern",
    look: { wet: v.ink === "wet", gloss: v.gloss, shine: v.shine, filters: true },
    info: { title: scopeTitle(sc), subtitle: subtitle(score, sc), author: sc.kind === "song" ? p.meta.author : "", bpm: p.transport.bpm },
    bar: m ? m.length / Math.max(1, m.count) / TPQ : p.transport.beatsPerBar,
    showing: sc.kind === "pattern" ? state.mode === "pattern" && state.pattern === sc.pattern : state.mode === "song",
    seek: (beat) => seekScore(v, beat),
    back: () => {
      v.film = false;
      state.film.on = false;
      reportContext();
      savePrefs(v);
      invalidate();
    },
  });
}

/** The score in a pane: `id` "top" (beside the playlist) or "dock". */
/** function scoreView(b: Builder, v: ScoreView) => Undefined */
export function scoreView(b, v) {
  const sc = scopeOf(v);
  if (v.film) {
    b.open("div", `score-${v.id}`, `score score-${v.id} filming`);
    b.on("pointerdown", (e) => setFocus("score"));
    filmOf(b, v, sc);
    b.close();
    return undefined;
  }
  b.open("div", `score-${v.id}`, `score score-${v.id}${v.night ? " night" : ""}${v.side ? " with-side" : ""}`);
  // The paper's textures (ink.js).
  const paper = v.night ? LACQUER : PAPER;
  b.style("--tooth", `url("${paper.tooth.url}")`);
  b.style("--mottle", `url("${paper.mottle.url}")`);
  b.style("--grain", `url("${paper.grain.url}")`);
  b.on("pointerdown", (e) => setFocus("score"));
  const geo = pageGeo(v, sc);
  const c = cached(v, sc, geo);
  if (v.side) sideView(b, v, c, sc, geo);
  b.open("div", "main", "score-main");
  ribbon(b, v);
  paperView(b, v, c, geo, sc);
  rangeBar(b, v, c);
  if (v.pdfMenu) pdfMenu(b, v);
  b.close();
  partMenu(b, v, c);
  b.close();
}

const VALUES = [
  { v: 192, ch: "\u{e1d2}", name: "Whole note" },
  { v: 96, ch: "\u{e1d3}", name: "Half note" },
  { v: 48, ch: "\u{e1d5}", name: "Quarter note" },
  { v: 24, ch: "\u{e1d7}", name: "Eighth note" },
  { v: 12, ch: "\u{e1d9}", name: "Sixteenth note" },
];

/** The pane's toolbar for a score view: follow, and what to show. */
/** function scoreTools(b: Builder, v: ScoreView) => Undefined */
export function scoreTools(b, v) {
  const p = state.project;
  followButton(b);
  /** const ids: String[] */
  const ids = ["song"];
  /** const names: String[] */
  const names = ["Whole song"];
  if (v.id === "dock") {
    ids.push("current");
    names.push("Piano roll's pattern");
  }
  /** const tracks: Int[] */
  const tracks = [];
  for (const cl of p.playlist.clips) if (cl.pattern !== "" && !tracks.includes(trackIndex(cl.track))) tracks.push(trackIndex(cl.track));
  tracks.sort((a, b) => a - b);
  for (const t of tracks) {
    ids.push(`track:${t}`);
    names.push(`Track · ${t < p.playlist.tracks.length ? p.playlist.tracks[t].name : String(t + 1)}`);
  }
  for (const pat of p.patterns) {
    ids.push(`pattern:${pat.id}`);
    names.push(`Pattern · ${pat.name}`);
  }
  if (!ids.includes(v.scope)) {
    ids.push(v.scope);
    names.push("—");
  }
  select(b, "scope", "score-scope", v.scope, ids, names, "What to show: the song, one playlist track or one pattern", (val) => setScope(v, val));
  if (v.id === "dock" && v.scope === "current") iconButton(b, "roll", "small", "piano", "Back to the piano roll (F7)", () => revealDock("piano"));
}

/** A small slider for one of the wet ink's knobs, 0..`max`; saved when let go. */
/** function inkSlider(b: Builder, v: ScoreView, key: String, label: String, tip: String, value: Number, max: Number, dflt: Number, onSet: (Number) => Undefined) => Undefined */
function inkSlider(b, v, key, label, tip, value, max, dflt, onSet) {
  b.open("label", key, "score-ink-knob");
  b.attr("title", tip);
  b.leaf("span", "l", "label", label);
  b.leaf("input", "in", "score-slider", "");
  b.attr("type", "range");
  b.attr("min", "0");
  b.attr("max", fmt(max, 2));
  b.attr("step", "0.05");
  b.prop("value", fmt(value, 2));
  b.on("input", (e) => {
    onSet(Number(e.value));
    invalidate();
  });
  b.on("change", (e) => savePrefs(v));
  b.on("dblclick", (e) => {
    onSet(dflt);
    savePrefs(v);
    invalidate();
  });
  b.close();
}

/** The ribbon over the paper: tools, note values, key, grid, zoom, paper or night, sidebar. */
/** function ribbon(b: Builder, v: ScoreView) => Undefined */
function ribbon(b, v) {
  const p = state.project;
  b.open("div", "ribbon", "score-ribbon");
  b.open("div", "tools", "score-group");
  iconButton(
    b,
    "sel",
    v.tool === "select" ? "small on" : "small",
    "select",
    "Select: click notes, drag them, drag across the music to color it (Shift+E)",
    () => {
      v.tool = "select";
      v.ghost = { sys: -1, row: -1, step: 0, tick: -1, x: 0 };
      invalidate();
    }
  );
  iconButton(b, "write", v.tool === "write" ? "small on" : "small", "draw", "Write: click on a staff to add a note (Shift+P)", () => {
    v.tool = "write";
    v.range.on = false;
    invalidate();
  });
  iconButton(
    b,
    "pan",
    v.tool === "pan" ? "small on" : "small",
    "hand",
    "Hand: drag to move the page about (Shift+H; or drag with the middle button, or beside the page)",
    () => {
      v.tool = "pan";
      v.ghost = { sys: -1, row: -1, step: 0, tick: -1, x: 0 };
      invalidate();
    }
  );
  if (v.tool === "write") {
    b.open("div", "values", "score-values");
    for (const val of VALUES) {
      b.leaf("button", `v${val.v}`, v.value === val.v ? "score-value on" : "score-value", val.ch);
      b.attr("title", val.name);
      b.on("click", (e) => {
        v.value = val.v;
        invalidate();
      });
    }
    b.leaf("button", "dot", v.dot ? "score-value dot on" : "score-value dot", "\u{e1e7}");
    b.attr("title", "Dotted (half as long again)");
    b.on("click", (e) => {
      v.dot = !v.dot;
      invalidate();
    });
    b.close();
  }
  b.close();
  b.leaf("span", "sp", "score-spacer", "");
  b.open("div", "fmt", "score-group");
  b.leaf("span", "kl", "label", "Key");
  /** const keyIds: String[] */
  const keyIds = ["auto"];
  /** const keyNames: String[] */
  const keyNames = ["Auto"];
  for (const k of KEYS) {
    keyIds.push(k.name);
    keyNames.push(keyLabel(k.name));
  }
  select(b, "key", "", p.score.key === "" ? "auto" : p.score.key, keyIds, keyNames, "Key signature (Auto guesses it from the notes)", (val) => {
    commit(() => {
      p.score.key = val === "auto" ? "" : val;
    });
  });
  b.leaf("span", "gl", "label", "Grid");
  select(b, "grid", "", String(v.grid), ["24", "12", "6"], ["1/8", "1/16", "1/32"], "Shortest value the notes are written in", (val) => {
    v.grid = Math.round(Number(val));
    invalidate();
  });
  b.leaf("span", "szl", "label", "Size");
  b.leaf("button", "sz-", "btn small score-size down", "A");
  b.attr("title", "Smaller music: more bars on a line");
  b.attr("aria-label", "Smaller music");
  b.on("click", (e) => setSize(v, v.size - 1));
  b.leaf("button", "sz+", "btn small score-size up", "A");
  b.attr("title", "Larger music: fewer bars on a line");
  b.attr("aria-label", "Larger music");
  b.on("click", (e) => setSize(v, v.size + 1));
  iconButton(b, "zo", "small ghost", "zoomout", "Zoom out (Ctrl+wheel)", () => zoomAt(v, zoomStep(v, -1), v.width / 2, 0));
  b.leaf("span", "z", "score-zoom", `${Math.round(v.zoom * 100)}%`);
  b.attr("title", "Zoom — double-click for 100%");
  b.on("dblclick", (e) => zoomAt(v, 1, v.width / 2, 0));
  iconButton(b, "zi", "small ghost", "zoomin", "Zoom in (Ctrl+wheel)", () => zoomAt(v, zoomStep(v, 1), v.width / 2, 0));
  b.open("button", "night", v.night ? "btn small icon on" : "btn small icon");
  b.attr("title", v.night ? "Paper: ink on ivory" : "Night: gold ink on black lacquer");
  b.on("click", (e) => {
    v.night = !v.night;
    savePrefs(v);
    invalidate();
  });
  glyph(b, "moon");
  b.close();
  iconButton(
    b,
    "colors",
    v.colors ? "small on" : "small",
    "palette",
    v.colors ? "Colors: shown — click to write everything in plain ink" : "Colors: hidden (plain ink) — click to show the colored passages",
    () => {
      v.colors = !v.colors;
      savePrefs(v);
      invalidate();
    }
  );
  iconButton(
    b,
    "ink",
    v.ink === "wet" ? "small on" : "small",
    "drop",
    v.ink === "wet" ? "Ink: wet and glossy — click for dry, faded ink" : "Ink: dry and faded — click for wet, glossy ink",
    () => {
      v.ink = v.ink === "wet" ? "dry" : "wet";
      savePrefs(v);
      invalidate();
    }
  );
  if (v.ink === "wet") {
    inkSlider(b, v, "gloss", "Gloss", "Gloss: how bright the wet ink shines (double-click to reset)", v.gloss, GLOSS_MAX, GLOSS_DEFAULT, (x) => {
      v.gloss = x;
    });
    inkSlider(b, v, "shine", "Shine", "Shine: a small speck of light, or a broad dome (double-click to reset)", v.shine, 1, SHINE_DEFAULT, (x) => {
      v.shine = x;
    });
  }
  iconButton(b, "film", "small", "film", "Film: the camera plays the song over the pages on a desk, zooming in on the parts that carry it", () => {
    v.film = true;
    v.range.on = false;
    savePrefs(v);
    invalidate();
  });
  iconButton(b, "pdf", v.pdfMenu ? "small on" : "small", "export", "Download as PDF…", () => {
    v.pdfMenu = !v.pdfMenu;
    invalidate();
  });
  iconButton(b, "side", v.side ? "small on" : "small", "sidebar", "Parts, tracks and colors", () => {
    v.side = !v.side;
    savePrefs(v);
    invalidate();
  });
  b.close();
  b.close();
}

/** Shift+P / Shift+E in the score. */
/** function setScoreTool(tool: String) => Undefined */
export function setScoreTool(tool) {
  const v = activeView();
  v.tool = tool;
  if (tool === "write") v.range.on = false;
  invalidate();
}

/** Escape: close a part's menu, or drop the chosen passage. Returns true when there was one. */
/** function cancelScoreRange() => Boolean */
export function cancelScoreRange() {
  for (const v of [topScore, dockScore]) {
    if (v.menu.on) {
      closePartMenu(v);
      return true;
    }
  }
  for (const v of [topScore, dockScore]) {
    if (v.range.on) {
      v.range.on = false;
      invalidate();
      return true;
    }
  }
  return false;
}

/** Whether a selected note exists to act on from the score. */
/** function scoreHasSelection() => Boolean */
export function scoreHasSelection() {
  const pat = currentPattern();
  return pat !== undefined && state.selection.length > 0;
}
