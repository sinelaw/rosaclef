// The playlist (arrangement).
//
// Same structure as the piano roll: `geometry()` + `layout()` compute every
// clip rectangle; rendering and hit-testing read them. Clip contents (note
// previews, waveforms) are small canvas leaves.

import { drag, getJson, promptBox } from "#platform";
import { state, commit, begin, changed, invalidate, selectPattern, selectChannel, showDock, currentPattern, hint } from "../store.js";
import { snapTo, snapDown, songLength } from "../model.js";
import { seek, followPattern, setMode } from "../audio.js";
import { select, iconButton, glyph } from "./widgets.js";
import { dragSample } from "./browser.js";
import { toast } from "./toast.js";
import { clipIx, clipIndex, trackIx, trackIndex, insertIx } from "#brands";

const view = {
  zoom /*: Number */: 22,
  trackH /*: Number */: 54,
  scrollLeft /*: Number */: 0,
  scrollTop /*: Number */: 0,
  width /*: Number */: 900,
  height /*: Number */: 300,
  selected /*: ClipIx[] */: [],
};

/** type PGeo = { zoom: Number, trackH: Number, beats: Number, width: Number, height: Number } */
/** type CBox = { i: ClipIx, x: Number, y: Number, w: Number, h: Number } */

/** function geometry() => PGeo */
function geometry() {
  const p = state.project;
  const bpb = p.transport.beatsPerBar;
  const beats = Math.max(songLength(p) + bpb * 8, Math.ceil(view.width / view.zoom / bpb + 1) * bpb);
  return { zoom: view.zoom, trackH: view.trackH, beats: beats, width: beats * view.zoom, height: Math.max(p.playlist.tracks.length, 1) * view.trackH };
}

/** function layout(g: PGeo) => CBox[] */
function layout(g) {
  /** const out: CBox[] */
  const out = [];
  const clips = state.project.playlist.clips;
  for (let i = 0; i < clips.length; i++) {
    const c = clips[i];
    out.push({ i: clipIx(i), x: c.start * g.zoom, y: trackIndex(c.track) * g.trackH + 2, w: Math.max(6, c.length * g.zoom), h: g.trackH - 4 });
  }
  return out;
}

/** function hit(boxes: CBox[], x: Number, y: Number) => Int */
function hit(boxes, x, y) {
  for (let k = boxes.length - 1; k >= 0; k--) {
    const r = boxes[k];
    if (x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h) return k;
  }
  return -1;
}

/** function isSel(i: ClipIx) => Boolean */
function isSel(i) {
  return view.selected.some((s) => clipIndex(s) === clipIndex(i));
}

// ------------------------------------------------------------------ peaks cache

/** const peaks: { path: String, duration: Number, data: Number[][] }[] */
const peaks = [];
/** const peaksLoading: String[] */
const peaksLoading = [];

/** function samplePeaks(path: String) => { path: String, duration: Number, data: Number[][] }? */
function samplePeaks(path) {
  for (const p of peaks) if (p.path === path) return p;
  if (!peaksLoading.includes(path)) {
    peaksLoading.push(path);
    getJson(`/api/peaks?path=${encodeURIComponent(path)}&n=2000`)
      .then((r) => {
        peaks.push({ path: path, duration: Number(r.duration), data: r.peaks });
        invalidate();
        return Promise.resolve(true);
      })
      .catch((e) => Promise.resolve(false));
  }
  return undefined;
}

// ------------------------------------------------------------------ editing

export function deleteSelectedClips() {
  if (view.selected.length === 0) return;
  const gone = view.selected.map(clipIndex);
  commit(() => {
    const clips = state.project.playlist.clips;
    /** const keep: Clip[] */
    const keep = [];
    for (let i = 0; i < clips.length; i++) if (!gone.includes(i)) keep.push(clips[i]);
    state.project.playlist.clips = keep;
    return undefined;
  });
  view.selected = [];
}

/** function patternById(id: String) => Pattern? */
function patternById(id) {
  return state.project.patterns.find((p) => p.id === id);
}

/** Show the editor that fits a pattern: the piano roll (with its main
 * melodic channel) for melodic patterns, the channel rack for drum-only
 * ones. `forcePiano` (double click) always opens the piano roll. */
/** function focusEditor(patternId: String, forcePiano: Boolean) => Undefined */
function focusEditor(patternId, forcePiano) {
  const pat = patternById(patternId);
  if (!pat) return undefined;
  // Count notes per channel; prefer melodic (non-drum) channels.
  let best = "";
  let bestCount = 0;
  let bestDrum = "";
  let bestDrumCount = 0;
  for (const ch of state.project.channels) {
    let n = 0;
    for (const note of pat.notes) if (note.channel === ch.id) n = n + 1;
    if (n === 0) continue;
    if (ch.instrument.type === "drum") {
      if (n > bestDrumCount) {
        bestDrum = ch.id;
        bestDrumCount = n;
      }
    } else if (n > bestCount) {
      best = ch.id;
      bestCount = n;
    }
  }
  if (best !== "") {
    selectChannel(best);
    showDock("piano");
  } else if (bestDrum !== "") {
    selectChannel(bestDrum);
    showDock(forcePiano ? "piano" : "rack");
  }
  return undefined;
}

/** function onLaneDown(e: Ev, g: PGeo) => Undefined */
function onLaneDown(e, g) {
  e.preventDefault();
  const p = state.project;
  const x = e.clientX - e.targetLeft + e.scrollLeft;
  const y = e.clientY - e.targetTop + e.scrollTop;
  const boxes = layout(g);
  const k = hit(boxes, x, y);
  const snap = Math.max(state.snap, 1);
  const track = Math.max(0, Math.min(p.playlist.tracks.length - 1, Math.floor(y / g.trackH)));
  state.track = trackIx(track);

  if (e.button === 2) {
    if (k >= 0) {
      const idx = clipIndex(boxes[k].i);
      commit(() => {
        p.playlist.clips.splice(idx, 1);
        return undefined;
      });
      view.selected = [];
    }
    return undefined;
  }

  if (k >= 0) {
    const box = boxes[k];
    const idx = clipIndex(box.i);
    const clip = p.playlist.clips[idx];
    if (!isSel(box.i)) view.selected = e.shiftKey ? view.selected.concat([box.i]) : [box.i];
    if (clip.pattern !== "") {
      selectPattern(clip.pattern);
      followPattern();
      focusEditor(clip.pattern, e.detail >= 2);
    }
    const resizing = x > box.x + box.w - 8;
    const orig = view.selected.map((s) => {
      const c = p.playlist.clips[clipIndex(s)];
      return { i: clipIndex(s), start: c.start, length: c.length, track: trackIndex(c.track) };
    });
    // Alt/Ctrl-drag duplicates.
    if (!resizing && (e.altKey || e.ctrlKey)) {
      commit(() => {
        for (const o of orig) {
          const c = p.playlist.clips[o.i];
          p.playlist.clips.push({ pattern: c.pattern, sample: c.sample, track: c.track, start: c.start, length: c.length, offset: c.offset, gain: c.gain, mixer: c.mixer });
        }
        return undefined;
      });
    }
    const x0 = e.clientX;
    const y0 = e.clientY;
    begin();
    drag(e, (m) => {
      const db = (m.clientX - x0) / g.zoom;
      const dt = Math.round((m.clientY - y0) / g.trackH);
      for (const o of orig) {
        const c = p.playlist.clips[o.i];
        if (resizing) c.length = Math.max(snap, snapTo(o.length + db, snap));
        else {
          c.start = Math.max(0, snapTo(o.start + db, snap));
          c.track = trackIx(Math.max(0, Math.min(p.playlist.tracks.length - 1, o.track + dt)));
        }
      }
      changed(true);
      return undefined;
    }, (u) => undefined);
    return undefined;
  }

  // Empty lane: place the selected pattern.
  const pat = currentPattern();
  if (!pat) return undefined;
  const start = snapDown(x / g.zoom, snap);
  begin();
  p.playlist.clips.push({ pattern: pat.id, sample: "", track: trackIx(track), start: start, length: pat.length, offset: 0, gain: 1, mixer: insertIx(0) });
  const idx = p.playlist.clips.length - 1;
  view.selected = [clipIx(idx)];
  changed(true);
  const x0 = e.clientX;
  drag(e, (m) => {
    const db = (m.clientX - x0) / g.zoom;
    p.playlist.clips[idx].length = Math.max(snap, snapTo(pat.length + db, snap));
    changed(true);
    return undefined;
  }, (u) => undefined);
  return undefined;
}

/** Drop a sample (from the browser or the desktop) onto a track. */
/** function dropSample(path: String, x: Number, y: Number, g: PGeo) => Undefined */
function dropSample(path, x, y, g) {
  const p = state.project;
  const track = Math.max(0, Math.min(p.playlist.tracks.length - 1, Math.floor(y / g.trackH)));
  const start = snapDown(x / g.zoom, 1);
  getJson(`/api/peaks?path=${encodeURIComponent(path)}&n=16`)
    .then((r) => {
      const beats = Number(r.duration) / (60 / p.transport.bpm);
      commit(() => {
        p.playlist.clips.push({ pattern: "", sample: path, track: trackIx(track), start: start, length: Math.max(0.25, Math.round(beats * 100) / 100), offset: 0, gain: 1, mixer: insertIx(0) });
        return undefined;
      });
      return Promise.resolve(true);
    })
    .catch((e) => {
      toast("Could not read sample", path, "error");
      return Promise.resolve(false);
    });
  return undefined;
}

// ------------------------------------------------------------------ render

/** function clipBody(b: Builder, c: Clip, w: Number) => Undefined */
function clipBody(b, c, w) {
  if (c.pattern !== "") {
    const pat = patternById(c.pattern);
    if (!pat) return undefined;
    b.canvas("prev", "", (g2, cw, ch) => {
      if (pat.notes.length === 0) return undefined;
      let lo = 127;
      let hi = 0;
      for (const n of pat.notes) {
        lo = Math.min(lo, n.pitch);
        hi = Math.max(hi, n.pitch);
      }
      const span = Math.max(12, hi - lo + 1);
      const scale = cw / c.length;
      const nh = Math.max(1.5, (ch - 4) / span);
      g2.fillStyle = "rgba(255, 244, 220, 0.8)";
      for (let rep = -c.offset; rep < c.length; rep = rep + pat.length) {
        for (const n of pat.notes) {
          const t = rep + n.start;
          if (t < 0 || t >= c.length) continue;
          g2.fillRect(t * scale, ch - 2 - (n.pitch - lo + 1) * nh, Math.max(1.5, n.length * scale - 1), Math.max(1.2, nh - 0.6));
        }
        if (pat.length <= 0) break;
      }
      return undefined;
    });
  } else {
    const pk = samplePeaks(c.sample);
    const spb = 60 / state.project.transport.bpm;
    b.canvas("wave", "", (g2, cw, ch) => {
      if (!pk || pk.data.length === 0) return undefined;
      const mid = ch / 2;
      g2.strokeStyle = "rgba(255, 244, 220, 0.85)";
      g2.lineWidth = 1;
      g2.beginPath();
      for (let px = 0; px < cw; px++) {
        const sec = (c.offset + (px / cw) * c.length) * spb;
        const idx = Math.floor((sec / pk.duration) * pk.data.length);
        if (idx < 0 || idx >= pk.data.length) continue;
        const pr = pk.data[idx];
        g2.moveTo(px + 0.5, mid - pr[1] * mid * 0.95);
        g2.lineTo(px + 0.5, mid - pr[0] * mid * 0.95);
      }
      g2.stroke();
      return undefined;
    });
  }
  return undefined;
}

/** function playlist(b: Builder) => Undefined */
export function playlist(b) {
  const p = state.project;
  const g = geometry();
  followPlayhead(g);
  const bpb = p.transport.beatsPerBar;
  b.open("div", "pl", "editor");
  b.open("div", "main", "editor-main pl");

  b.open("div", "corner", "corner");
  b.text("Playlist");
  b.close();

  // Ruler.
  b.open("div", "ruler", "ruler");
  b.on("pointerdown", (e) => {
    const beat = (e.clientX - e.targetLeft + view.scrollLeft) / g.zoom;
    if (state.mode !== "song") setMode("song");
    seek(Math.max(0, snapDown(beat, bpb)));
    return undefined;
  });
  b.open("div", "in", "");
  b.style("transform", `translateX(${-view.scrollLeft}px)`);
  b.style("position", "absolute");
  b.style("inset", "0");
  const firstBar = Math.max(0, Math.floor(view.scrollLeft / g.zoom / bpb));
  const lastBar = Math.ceil((view.scrollLeft + view.width) / g.zoom / bpb);
  for (let bar = firstBar; bar <= lastBar; bar++) {
    if (g.zoom * bpb < 40 && bar % 2 === 1) continue;
    b.leaf("div", `m${bar}`, "ruler-mark", String(bar + 1));
    b.style("left", `${bar * bpb * g.zoom}px`);
  }
  if (state.mode === "song") {
    b.leaf("div", "ph", "playhead", "");
    b.style("left", `${state.position * g.zoom}px`);
  }
  b.close();
  b.close();

  // Track headers.
  b.open("div", "heads", "tracks-head");
  b.open("div", "in", "");
  b.style("transform", `translateY(${-view.scrollTop}px)`);
  b.style("position", "absolute");
  b.style("left", "0");
  b.style("right", "0");
  const tracks = p.playlist.tracks;
  for (let t = 0; t < tracks.length; t++) {
    const tr = tracks[t];
    b.open("div", `t${t}`, trackIndex(state.track) === t ? "track-head sel" : "track-head");
    b.style("top", `${t * g.trackH}px`);
    b.style("height", `${g.trackH}px`);
    b.on("click", (e) => {
      state.track = trackIx(t);
      invalidate();
      return undefined;
    });
    b.leaf("span", "num", "t-num", String(t + 1));
    b.leaf("span", "name", "t-name", tr.name);
    b.attr("title", "Double-click to rename");
    b.on("dblclick", (e) => {
      const name = promptBox("Track name", tr.name);
      if (name !== "") {
        commit(() => {
          tr.name = name;
          return undefined;
        });
      }
      return undefined;
    });
    b.leaf("div", "mute", tr.mute ? "ch-mute off" : "ch-mute", "");
    b.attr("title", tr.mute ? "Unmute track" : "Mute track");
    b.on("click", (e) => {
      e.stopPropagation();
      commit(() => {
        tr.mute = !tr.mute;
        return undefined;
      });
      return undefined;
    });
    b.close();
  }
  b.close();
  b.close();

  // Lanes.
  b.open("div", "lanes", "scroller");
  b.on("scroll", (e) => {
    view.scrollLeft = e.scrollLeft;
    view.scrollTop = e.scrollTop;
    invalidate();
    return undefined;
  });
  b.on("resize", (e) => {
    view.width = e.targetWidth;
    view.height = e.targetHeight;
    invalidate();
    return undefined;
  });
  b.on("pointerdown", (e) => onLaneDown(e, g));
  b.prop("scrollLeft", String(view.scrollLeft));
  b.on("contextmenu", (e) => {
    e.preventDefault();
    return undefined;
  });
  b.on("dragover", (e) => {
    if (dragSample.path !== "") e.preventDefault();
    return undefined;
  });
  b.on("drop", (e) => {
    if (dragSample.path === "") return undefined;
    e.preventDefault();
    dropSample(dragSample.path, e.clientX - e.targetLeft + e.scrollLeft, e.clientY - e.targetTop + e.scrollTop, g);
    dragSample.path = "";
    return undefined;
  });
  b.on("wheel", (e) => {
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      view.zoom = Math.max(4, Math.min(160, view.zoom * (e.deltaY < 0 ? 1.12 : 1 / 1.12)));
      invalidate();
    } else if (e.altKey) {
      e.preventDefault();
      view.trackH = Math.max(28, Math.min(120, view.trackH + (e.deltaY < 0 ? 4 : -4)));
      invalidate();
    }
    return undefined;
  });
  b.on("pointermove", (e) => {
    const beat = (e.clientX - e.targetLeft + e.scrollLeft) / g.zoom;
    const bar = Math.floor(beat / bpb) + 1;
    const pat = currentPattern();
    hint(`Bar ${bar} — click to place “${pat ? pat.name : "a pattern"}”, drag clips to move, edge to resize, Alt-drag to copy, right-click to delete`);
    return undefined;
  });

  b.open("div", "content", "canvas-grid");
  b.style("width", `${g.width}px`);
  b.style("height", `${Math.max(g.height, view.height - 2)}px`);
  for (let t = 0; t < tracks.length; t++) {
    b.leaf("div", `lane${t}`, "track-lane", "");
    b.style("top", `${t * g.trackH}px`);
    b.style("height", `${g.trackH}px`);
  }
  b.leaf("div", "bg", "grid-bg", "");
  b.style("--bar", `${g.zoom * bpb}px`);
  b.style("--beat", `${g.zoom}px`);
  b.style("--step", `${g.zoom * bpb * 4}px`);

  const x0 = view.scrollLeft - 60;
  const x1 = view.scrollLeft + view.width + 60;
  for (const r of layout(g)) {
    if (r.x > x1 || r.x + r.w < x0) continue;
    const c = p.playlist.clips[clipIndex(r.i)];
    const pat = c.pattern !== "" ? patternById(c.pattern) : undefined;
    const color = pat ? pat.color : "#6fa3a0";
    const title = pat ? pat.name : (c.sample.split("/").pop() ?? c.sample);
    const muted = trackIndex(c.track) < tracks.length && tracks[trackIndex(c.track)].mute;
    let cls = "clip";
    if (isSel(r.i)) cls = `${cls} sel`;
    if (muted) cls = `${cls} muted`;
    b.open("div", `c${clipIndex(r.i)}`, cls);
    b.style("left", `${r.x}px`);
    b.style("top", `${r.y}px`);
    b.style("width", `${r.w - 1}px`);
    b.style("height", `${r.h}px`);
    b.style("--c", color);
    b.leaf("div", "title", "clip-title", title);
    clipBody(b, c, r.w);
    b.leaf("div", "edge", "clip-edge", "");
    b.close();
  }
  if (state.mode === "song") {
    b.leaf("div", "ph", "playhead", "");
    b.style("left", `${state.position * g.zoom}px`);
  }
  b.close();
  b.close();

  b.close();
  b.close();
  return undefined;
}

/** Page the view along with the playhead (FL Studio style) while playing. */
/** function followPlayhead(g: PGeo) => Undefined */
function followPlayhead(g) {
  if (!state.follow || !state.playing || state.mode !== "song") return undefined;
  const x = state.position * g.zoom;
  if (x < view.scrollLeft || x > view.scrollLeft + view.width * 0.88) {
    view.scrollLeft = Math.max(0, x - view.width * 0.08);
  }
  return undefined;
}

/** function followButton(b: Builder) => Undefined */
export function followButton(b) {
  iconButton(b, "follow", state.follow ? "small on" : "small", "follow", state.follow ? "Follow playback: on — the view scrolls with the playhead" : "Follow playback: off", () => {
    state.follow = !state.follow;
    invalidate();
    return undefined;
  });
  return undefined;
}

/** function playlistTools(b: Builder) => Undefined */
export function playlistTools(b) {
  const pat = currentPattern();
  followButton(b);
  b.leaf("span", "l", "label", "Paint");
  const ids = state.project.patterns.map((x) => x.id);
  select(b, "pat", "", state.pattern, ids, state.project.patterns.map((x) => x.name), "Pattern placed by clicking an empty lane", (v) => {
    selectPattern(v);
    followPattern();
    return undefined;
  });
  if (pat) {
    b.leaf("span", "sw", "swatch", "");
    b.style("--c", pat.color);
  }
  iconButton(b, "addtrack", "small ghost", "plus", "Add a playlist track", () => {
    commit(() => {
      state.project.playlist.tracks.push({ name: `Track ${state.project.playlist.tracks.length + 1}`, mute: false });
      return undefined;
    });
    return undefined;
  });
  return undefined;
}
