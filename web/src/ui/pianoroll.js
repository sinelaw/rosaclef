// The piano roll.
//
// One source of geometry: `geometry()` turns the view state into the
// transform between (beat, pitch) and pixels, and `layout()` computes the
// rectangle of every note from it. Rendering emits DOM nodes at those
// rectangles; hit-testing reads the same list. Only visible rows and notes
// become nodes.

import { drag, fmt, pressOrTap } from "#platform";
import { state, commit, begin, changed, currentPattern, currentChannel, selectChannel, invalidate, reportContext, hint } from "../store.js";
import { isBlackKey, noteName, snapTo, snapDown } from "../model.js";
import { preview, noteOn, noteOff, seek } from "../audio.js";
import { select, iconButton } from "./widgets.js";
import { followButton } from "./playlist.js";
import { noteIx, noteIndex } from "#brands";

const view = {
  zoom: 72,
  rowH: 14,
  scrollLeft: 0,
  scrollTop: 0,
  width: 800,
  height: 300,
  tool: "draw",
  lastLength: 0.25,
  centered: false,
  focus: "",
  marquee /*: { x0: Number, y0: Number, x1: Number, y1: Number } */: { x0: 0, y0: 0, x1: 0, y1: 0 },
  marqueeOn: false,
  keyDown: -1,
};

/** type Geo = { zoom: Number, rowH: Number, width: Number, height: Number, beats: Number } */
/** type Box = { i: Int, x: Number, y: Number, w: Number, h: Number } */

/** function geometry(pat: Pattern) => Geo */
function geometry(pat) {
  const beats = Math.max(pat.length, Math.ceil(view.width / view.zoom / 4) * 4);
  return { zoom: view.zoom, rowH: view.rowH, width: beats * view.zoom, height: 128 * view.rowH, beats: beats };
}

/** function pitchY(g: Geo, pitch: Number) => Number */
function pitchY(g, pitch) {
  return (127 - pitch) * g.rowH;
}

/** Rectangles of the current channel's notes (all of them; culling happens at render). */
/** function layout(g: Geo, pat: Pattern, ch: String) => Box[] */
function layout(g, pat, ch) {
  /** const out: Box[] */
  const out = [];
  for (let i = 0; i < pat.notes.length; i++) {
    const n = pat.notes[i];
    if (n.channel !== ch) continue;
    out.push({ i: i, x: n.start * g.zoom, y: pitchY(g, n.pitch), w: Math.max(4, n.length * g.zoom), h: g.rowH });
  }
  return out;
}

/** function hit(boxes: Box[], x: Number, y: Number) => Int */
function hit(boxes, x, y) {
  for (let k = boxes.length - 1; k >= 0; k--) {
    const r = boxes[k];
    if (x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h) return k;
  }
  return -1;
}

/** function isSelected(i: Int) => Boolean */
function isSelected(i) {
  return state.selection.some((s) => noteIndex(s) === i);
}

/** function setSelection(list: Int[]) => Undefined */
function setSelection(list) {
  state.selection = list.map(noteIx);
  reportContext();
}

// ------------------------------------------------------------------ editing ops

/** Operations used by keyboard shortcuts. */
export function deleteSelection() {
  const pat = currentPattern();
  if (!pat || state.selection.length === 0) return;
  const gone = state.selection.map(noteIndex);
  commit(() => {
    /** const keep: Note[] */
    const keep = [];
    for (let i = 0; i < pat.notes.length; i++) if (!gone.includes(i)) keep.push(pat.notes[i]);
    pat.notes = keep;
  });
  setSelection([]);
}

export function selectAll() {
  const pat = currentPattern();
  const ch = currentChannel();
  if (!pat || !ch) return;
  /** const all: Int[] */
  const all = [];
  for (let i = 0; i < pat.notes.length; i++) if (pat.notes[i].channel === ch.id) all.push(i);
  setSelection(all);
  invalidate();
}

/** function transpose(semis: Number) => Undefined */
export function transpose(semis) {
  const pat = currentPattern();
  if (!pat || state.selection.length === 0) return undefined;
  commit(() => {
    for (const s of state.selection) {
      const n = pat.notes[noteIndex(s)];
      n.pitch = Math.max(0, Math.min(127, n.pitch + semis));
    }
  });
}

export function quantize() {
  const pat = currentPattern();
  const ch = currentChannel();
  if (!pat || !ch) return;
  const grid = state.snap > 0 ? state.snap : 0.25;
  const targets = state.selection.length > 0 ? state.selection.map(noteIndex) : [];
  commit(() => {
    for (let i = 0; i < pat.notes.length; i++) {
      const n = pat.notes[i];
      if (targets.length > 0 ? !targets.includes(i) : n.channel !== ch.id) continue;
      n.start = Math.max(0, snapTo(n.start, grid));
    }
  });
}

export function duplicateSelection() {
  const pat = currentPattern();
  if (!pat || state.selection.length === 0) return;
  let lo = 1e9;
  let hi = 0;
  for (const s of state.selection) {
    const n = pat.notes[noteIndex(s)];
    lo = Math.min(lo, n.start);
    hi = Math.max(hi, n.start + n.length);
  }
  const shift = Math.max(state.snap, snapTo(hi - lo, Math.max(state.snap, 0.25)));
  /** const fresh: Int[] */
  const fresh = [];
  commit(() => {
    for (const s of state.selection) {
      const n = pat.notes[noteIndex(s)];
      pat.notes.push({ channel: n.channel, pitch: n.pitch, start: n.start + shift, length: n.length, velocity: n.velocity });
      fresh.push(pat.notes.length - 1);
    }
  });
  setSelection(fresh);
}

// ------------------------------------------------------------------ pointer

/** function onGridDown(e: Ev, pat: Pattern, ch: Channel, g: Geo) => Undefined */
function onGridDown(e, pat, ch, g) {
  e.preventDefault();
  const x = e.clientX - e.targetLeft + e.scrollLeft;
  const y = e.clientY - e.targetTop + e.scrollTop;
  const boxes = layout(g, pat, ch.id);
  const k = hit(boxes, x, y);
  const snap = state.snap;

  // Right button: delete.
  if (e.button === 2) {
    if (k >= 0) {
      const idx = boxes[k].i;
      commit(() => {
        pat.notes.splice(idx, 1);
      });
      setSelection([]);
    }
    return undefined;
  }

  if (k >= 0) {
    const box = boxes[k];
    const idx = box.i;
    if (!isSelected(idx)) {
      if (e.shiftKey) setSelection(state.selection.map(noteIndex).concat([idx]));
      else setSelection([idx]);
    }
    const n0 = pat.notes[idx];
    preview(ch.id, n0.pitch, n0.velocity);
    const resizing = x > box.x + box.w - 7;
    const picked = state.selection.map(noteIndex);
    const orig = picked.map((i) => ({ i: i, start: pat.notes[i].start, pitch: pat.notes[i].pitch, length: pat.notes[i].length }));
    const x0 = e.clientX;
    const y0 = e.clientY;
    begin();
    let lastPitch = n0.pitch;
    drag(
      e,
      (m) => {
        const db = (m.clientX - x0) / g.zoom;
        if (resizing) {
          for (const o of orig) {
            const len = snap > 0 ? Math.max(snap, snapTo(o.length + db, snap)) : Math.max(0.03, o.length + db);
            pat.notes[o.i].length = len;
            view.lastLength = len;
          }
        } else {
          const dp = Math.round((y0 - m.clientY) / g.rowH);
          for (const o of orig) {
            const start = snap > 0 ? snapTo(o.start + db, snap) : o.start + db;
            pat.notes[o.i].start = Math.max(0, start);
            pat.notes[o.i].pitch = Math.max(0, Math.min(127, o.pitch + dp));
          }
          const p = pat.notes[idx].pitch;
          if (p !== lastPitch) {
            lastPitch = p;
            preview(ch.id, p, n0.velocity);
          }
        }
        changed(true);
      },
      (u) => undefined
    );
    return undefined;
  }

  // Empty space: marquee (select tool or Shift) or draw a new note.
  if (view.tool === "select" || e.shiftKey) {
    view.marqueeOn = true;
    view.marquee = { x0: x, y0: y, x1: x, y1: y };
    const sx = e.clientX - x;
    const sy = e.clientY - y;
    drag(
      e,
      (m) => {
        view.marquee = { x0: view.marquee.x0, y0: view.marquee.y0, x1: m.clientX - sx, y1: m.clientY - sy };
        invalidate();
      },
      (u) => {
        const mq = view.marquee;
        const lx = Math.min(mq.x0, mq.x1);
        const hx = Math.max(mq.x0, mq.x1);
        const ly = Math.min(mq.y0, mq.y1);
        const hy = Math.max(mq.y0, mq.y1);
        const inside = layout(g, pat, ch.id)
          .filter((r) => r.x < hx && r.x + r.w > lx && r.y < hy && r.y + r.h > ly)
          .map((r) => r.i);
        setSelection(inside);
        view.marqueeOn = false;
        invalidate();
      }
    );
    return undefined;
  }

  const beat = x / g.zoom;
  const pitch = Math.max(0, Math.min(127, 127 - Math.floor(y / g.rowH)));
  const start = snap > 0 ? snapDown(beat, snap) : beat;
  const len = view.lastLength;
  begin();
  pat.notes.push({ channel: ch.id, pitch: pitch, start: start, length: len, velocity: 0.8 });
  const idx = pat.notes.length - 1;
  setSelection([idx]);
  preview(ch.id, pitch, 0.8);
  changed(true);
  const x0 = e.clientX;
  drag(
    e,
    (m) => {
      const db = (m.clientX - x0) / g.zoom;
      const l = snap > 0 ? Math.max(snap, snapTo(len + db, snap)) : Math.max(0.03, len + db);
      pat.notes[idx].length = l;
      view.lastLength = l;
      changed(true);
    },
    (u) => undefined
  );
}

// ------------------------------------------------------------------ render

/** function rulerView(b: Builder, g: Geo, pat: Pattern) => Undefined */
function rulerView(b, g, pat) {
  const bpb = state.project.transport.beatsPerBar;
  b.open("div", "ruler", "ruler");
  b.on("pointerdown", (e) => {
    const beat = (e.clientX - e.targetLeft + view.scrollLeft) / g.zoom;
    seek(Math.max(0, snapDown(beat, 1)));
  });
  b.open("div", "in", "");
  b.style("transform", `translateX(${-view.scrollLeft}px)`);
  b.style("position", "absolute");
  b.style("inset", "0");
  const first = Math.max(0, Math.floor(view.scrollLeft / g.zoom));
  const last = Math.min(g.beats, Math.ceil((view.scrollLeft + view.width) / g.zoom));
  for (let beat = first; beat <= last; beat++) {
    const isBar = beat % bpb === 0;
    if (!isBar && g.zoom < 28) continue;
    b.leaf("div", `m${beat}`, isBar ? "ruler-mark" : "ruler-mark beat", isBar ? String(beat / bpb + 1) : "");
    b.style("left", `${beat * g.zoom}px`);
  }
  if (state.mode === "pattern") {
    b.leaf("div", "ph", "playhead", "");
    b.style("left", `${state.position * g.zoom}px`);
  }
  b.close();
  b.close();
}

/** function keysView(b: Builder, g: Geo, ch: Channel) => Undefined */
function keysView(b, g, ch) {
  b.open("div", "keys", "keys");
  b.open("div", "in", "");
  b.style("transform", `translateY(${-view.scrollTop}px)`);
  b.style("position", "absolute");
  b.style("left", "0");
  b.style("right", "0");
  b.style("height", `${g.height}px`);
  const top = Math.max(0, 127 - Math.floor((view.scrollTop + view.height) / g.rowH) - 1);
  const bottom = Math.min(127, 127 - Math.floor(view.scrollTop / g.rowH) + 1);
  for (let p = top; p <= bottom; p++) {
    const black = isBlackKey(p);
    let cls = black ? "key black" : "key white";
    if (p % 12 === 0) cls = `${cls} c`;
    if (p === view.keyDown) cls = `${cls} down`;
    b.open("div", `k${p}`, cls);
    b.style("top", `${pitchY(g, p)}px`);
    b.style("height", `${g.rowH}px`);
    b.on("pointerdown", (e) => {
      e.preventDefault();
      view.keyDown = p;
      noteOn(ch.id, p, 0.85);
      invalidate();
      drag(
        e,
        (m) => undefined,
        (u) => {
          noteOff(ch.id, p);
          view.keyDown = -1;
          invalidate();
        }
      );
    });
    if (p % 12 === 0) b.leaf("span", "l", "", noteName(p));
    b.close();
  }
  b.close();
  b.close();
}

/** function gridView(b: Builder, g: Geo, pat: Pattern, ch: Channel) => Undefined */
function gridView(b, g, pat, ch) {
  b.open("div", "grid", "scroller");
  b.on("scroll", (e) => {
    view.scrollLeft = e.scrollLeft;
    view.scrollTop = e.scrollTop;
    invalidate();
  });
  b.on("resize", (e) => {
    view.width = e.targetWidth;
    view.height = e.targetHeight;
    if (!view.centered) {
      view.centered = true;
      view.scrollTop = pitchY(g, 76) - 10;
    }
    invalidate();
  });
  b.on("pointerdown", (e) => pressOrTap(e, (d) => onGridDown(d, pat, ch, g)));
  b.on("contextmenu", (e) => {
    e.preventDefault();
  });
  b.on("wheel", (e) => {
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      view.zoom = Math.max(12, Math.min(400, view.zoom * (e.deltaY < 0 ? 1.12 : 1 / 1.12)));
      invalidate();
    } else if (e.altKey) {
      e.preventDefault();
      view.rowH = Math.max(8, Math.min(30, view.rowH + (e.deltaY < 0 ? 1 : -1)));
      invalidate();
    }
  });
  b.on("pointermove", (e) => {
    const y = e.clientY - e.targetTop + e.scrollTop;
    const pitch = 127 - Math.floor(y / g.rowH);
    const beat = (e.clientX - e.targetLeft + e.scrollLeft) / g.zoom;
    hint(`${noteName(pitch)} · beat ${fmt(beat, 2)} — click to draw, drag to move, right-click to delete, Shift-drag to select, Ctrl+wheel to zoom`);
  });
  if (view.centered) b.prop("scrollTop", String(view.scrollTop));
  b.prop("scrollLeft", String(view.scrollLeft));

  b.open("div", "content", "canvas-grid");
  b.style("width", `${g.width}px`);
  b.style("height", `${g.height}px`);

  // Visible key lanes.
  const top = Math.max(0, 127 - Math.floor((view.scrollTop + view.height) / g.rowH) - 1);
  const bottom = Math.min(127, 127 - Math.floor(view.scrollTop / g.rowH) + 1);
  for (let p = top; p <= bottom; p++) {
    if (!isBlackKey(p) && p % 12 !== 0) continue;
    b.leaf("div", `l${p}`, isBlackKey(p) ? "lane black" : "lane oct", "");
    b.style("top", `${pitchY(g, p)}px`);
    b.style("height", `${g.rowH}px`);
  }
  const bpb = state.project.transport.beatsPerBar;
  b.leaf("div", "bg", "grid-bg", "");
  b.style("--bar", `${g.zoom * bpb}px`);
  b.style("--beat", `${g.zoom}px`);
  b.style("--step", `${g.zoom / 4}px`);

  // Pattern end marker.
  b.leaf("div", "end", "playhead", "");
  b.style("left", `${pat.length * g.zoom}px`);
  b.style("background", "rgba(208,138,147,0.6)");
  b.style("box-shadow", "none");

  const x0 = view.scrollLeft - 40;
  const x1 = view.scrollLeft + view.width + 40;
  const y0 = view.scrollTop - g.rowH;
  const y1 = view.scrollTop + view.height + g.rowH;

  // Ghost notes of other channels.
  for (let i = 0; i < pat.notes.length; i++) {
    const n = pat.notes[i];
    if (n.channel === ch.id) continue;
    const x = n.start * g.zoom;
    const y = pitchY(g, n.pitch);
    const w = Math.max(3, n.length * g.zoom);
    if (x > x1 || x + w < x0 || y > y1 || y < y0) continue;
    b.leaf("div", `g${i}`, "note ghost", "");
    b.style("left", `${x}px`);
    b.style("top", `${y}px`);
    b.style("width", `${w}px`);
    b.style("height", `${g.rowH - 1}px`);
    b.style("--c", "#8a7f6c");
    b.style("--vel", "0.5");
  }

  // Notes of the selected channel.
  for (const r of layout(g, pat, ch.id)) {
    if (r.x > x1 || r.x + r.w < x0 || r.y > y1 || r.y < y0) continue;
    const n = pat.notes[r.i];
    b.open("div", `n${r.i}`, isSelected(r.i) ? "note sel" : "note");
    b.style("left", `${r.x}px`);
    b.style("top", `${r.y}px`);
    b.style("width", `${r.w - 1}px`);
    b.style("height", `${r.h - 1}px`);
    b.style("--c", ch.color);
    b.style("--vel", fmt(n.velocity, 2));
    if (r.w > 30 && g.rowH >= 12) b.text(noteName(n.pitch));
    b.leaf("div", "edge", "note-edge", "");
    b.close();
  }

  if (state.mode === "pattern") {
    b.leaf("div", "ph", "playhead", "");
    b.style("left", `${state.position * g.zoom}px`);
  }
  if (view.marqueeOn) {
    const mq = view.marquee;
    b.leaf("div", "mq", "marquee", "");
    b.style("left", `${Math.min(mq.x0, mq.x1)}px`);
    b.style("top", `${Math.min(mq.y0, mq.y1)}px`);
    b.style("width", `${Math.abs(mq.x1 - mq.x0)}px`);
    b.style("height", `${Math.abs(mq.y1 - mq.y0)}px`);
  }
  b.close();
  b.close();
}

/** function velocityView(b: Builder, g: Geo, pat: Pattern, ch: Channel) => Undefined */
function velocityView(b, g, pat, ch) {
  b.open("div", "vel", "vel-lane");
  b.on("pointerdown", (e) => {
    e.preventDefault();
    begin();
    const setAt = (m) => {
      const x = m.clientX - e.targetLeft + view.scrollLeft;
      const v = Math.max(0.02, Math.min(1, 1 - (m.clientY - e.targetTop - 6) / (e.targetHeight - 10)));
      let best = -1;
      let bestD = 8;
      for (const r of layout(g, pat, ch.id)) {
        const d = Math.abs(r.x - x);
        if (d < bestD) {
          bestD = d;
          best = r.i;
        }
      }
      if (best >= 0) {
        const targets = isSelected(best) ? state.selection.map(noteIndex) : [best];
        for (const t of targets) pat.notes[t].velocity = Math.round(v * 100) / 100;
        changed(true);
      }
    };
    setAt(e);
    drag(e, setAt, (u) => undefined);
  });
  b.open("div", "in", "");
  b.style("transform", `translateX(${-view.scrollLeft}px)`);
  b.style("position", "absolute");
  b.style("inset", "0");
  for (const r of layout(g, pat, ch.id)) {
    if (r.x < view.scrollLeft - 10 || r.x > view.scrollLeft + view.width + 10) continue;
    const n = pat.notes[r.i];
    b.leaf("div", `v${r.i}`, isSelected(r.i) ? "vel-bar sel" : "vel-bar", "");
    b.style("left", `${r.x}px`);
    b.style("height", `${Math.round(n.velocity * 62)}px`);
  }
  b.close();
  b.close();
}

/** function pianoRoll(b: Builder) => Undefined */
export function pianoRoll(b) {
  const pat = currentPattern();
  const ch = currentChannel();
  b.open("div", "pr", "editor");
  if (!pat || !ch) {
    b.leaf("div", "none", "b-empty", "Select a pattern and a channel.");
    b.close();
    return undefined;
  }
  const g = geometry(pat);
  if (state.follow && state.playing && state.mode === "pattern") {
    const x = state.position * g.zoom;
    if (x < view.scrollLeft || x > view.scrollLeft + view.width * 0.88) view.scrollLeft = Math.max(0, x - view.width * 0.08);
  }
  // Scroll to the notes whenever another pattern/channel comes into view.
  const focus = `${pat.id}/${ch.id}`;
  if (view.focus !== focus && view.centered) {
    view.focus = focus;
    const mine = pat.notes.filter((n) => n.channel === ch.id);
    let sum = 0;
    for (const n of mine) sum = sum + n.pitch;
    const mid = mine.length > 0 ? sum / mine.length : 66;
    view.scrollTop = Math.max(0, pitchY(g, mid) - view.height / 2);
  }
  const vp = state.viewport;
  const ps = Math.round((view.scrollLeft / g.zoom) * 100) / 100;
  const pe = Math.round(((view.scrollLeft + view.width) / g.zoom) * 100) / 100;
  const hi = Math.min(127, 127 - Math.floor(view.scrollTop / g.rowH));
  const lo = Math.max(0, 127 - Math.floor((view.scrollTop + view.height) / g.rowH));
  if (!vp.prOn || vp.prStart !== ps || vp.prEnd !== pe || vp.prLow !== lo || vp.prHigh !== hi) {
    vp.prOn = true;
    vp.prStart = ps;
    vp.prEnd = pe;
    vp.prLow = lo;
    vp.prHigh = hi;
    reportContext();
  }
  b.open("div", "main", "editor-main pr");
  b.leaf("div", "corner", "corner", pat.name);
  rulerView(b, g, pat);
  keysView(b, g, ch);
  gridView(b, g, pat, ch);
  b.leaf("div", "vl", "vel-label", "Velocity");
  velocityView(b, g, pat, ch);
  b.close();
  b.close();
}

/** function pianoTools(b: Builder) => Undefined */
export function pianoTools(b) {
  const ids = state.project.channels.map((c) => c.id);
  const names = state.project.channels.map((c) => c.name);
  followButton(b);
  b.leaf("span", "cl", "label", "Channel");
  select(b, "ch", "", state.channel, ids, names, "Channel to edit", (v) => selectChannel(v));
  iconButton(b, "draw", view.tool === "draw" ? "small on" : "small", "draw", "Draw tool (P)", () => {
    view.tool = "draw";
    invalidate();
  });
  iconButton(b, "select", view.tool === "select" ? "small on" : "small", "select", "Select tool (E)", () => {
    view.tool = "select";
    invalidate();
  });
  b.leaf("span", "sl", "label", "Snap");
  select(b, "snap", "", String(state.snap), ["0", "0.125", "0.25", "0.5", "1", "4"], ["Off", "1/32", "1/16", "1/8", "Beat", "Bar"], "Grid snap", (v) => {
    state.snap = Number(v);
    invalidate();
  });
}

/** function setTool(t: String) => Undefined */
export function setTool(t) {
  view.tool = t;
  invalidate();
}
