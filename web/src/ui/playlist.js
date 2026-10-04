// The playlist (arrangement).
//
// Same structure as the piano roll: `geometry()` + `layout()` compute every
// clip rectangle; rendering and hit-testing read them. Clip contents (note
// previews, waveforms) are small canvas leaves.

import { drag, getJson, promptBox, pressOrTap } from "#platform";
import { state, commit, begin, changed, invalidate, selectPattern, selectChannel, showDock, currentPattern, hint, reportContext } from "../store.js";
import { snapTo, snapDown, songLength, barAt, barLines, meterChangeAt } from "../model.js";
import { openMeterMenu } from "./meter.js";
import { passesText } from "../notation.js";
import { seek, followPattern, setMode } from "../audio.js";
import { select, iconButton, glyph } from "./widgets.js";
import { dragSample } from "./browser.js";
import { revealDock } from "./panes.js";
import { toast } from "./toast.js";
import { autoHeight, autoHeads, autoBody, onAutoDown, onAutoDblClick, autoHint, revealOffset, LANE_H } from "./lanes.js";
import { clipIx, clipIndex, trackIx, trackIndex, insertIx } from "#brands";

const view = {
  zoom: 22,
  trackH: 54,
  scrollLeft: 0,
  scrollTop: 0,
  width: 900,
  height: 300,
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
  return state.clipSelection.some((s) => clipIndex(s) === clipIndex(i));
}

// ------------------------------------------------------------------ peaks cache

/** const peaks: { path: String, duration: Number, data: Number[][] }[] */
const peaks = [];
/** const peaksLoading: String[] */
const peaksLoading = [];

/** The project folder the cached peaks were read from. */
const peaksFrom = { folder: "" };

/** function samplePeaks(path: String) => { path: String, duration: Number, data: Number[][] }? */
function samplePeaks(path) {
  if (peaksFrom.folder !== state.folder) {
    // Another project was opened: the same path may be another file.
    peaks.length = 0;
    peaksLoading.length = 0;
    peaksFrom.folder = state.folder;
  }
  for (const p of peaks) if (p.path === path) return p;
  if (!peaksLoading.includes(path)) {
    peaksLoading.push(path);
    getJson(`/api/peaks?path=${encodeURIComponent(path)}&n=2000`)
      .then((r) => {
        peaks.push({ path: path, duration: Number(r.duration), data: r.peaks });
        invalidate();
        return true;
      })
      .catch((e) => false);
  }
}

// ------------------------------------------------------------------ editing

export function deleteSelectedClips() {
  if (state.clipSelection.length === 0) return;
  const gone = state.clipSelection.map(clipIndex);
  commit(() => {
    const clips = state.project.playlist.clips;
    /** const keep: Clip[] */
    const keep = [];
    for (let i = 0; i < clips.length; i++) if (!gone.includes(i)) keep.push(clips[i]);
    state.project.playlist.clips = keep;
  });
  state.clipSelection = [];
}

/** function patternById(id: String) => Pattern? */
function patternById(id) {
  return state.project.patterns.find((p) => p.id === id);
}

/** A double click asks for the editor (on a phone, switch to it); a click only follows along. */
/** function showEditor(name: String, reveal: Boolean) => Undefined */
function showEditor(name, reveal) {
  if (reveal) revealDock(name);
  else showDock(name);
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
    showEditor("piano", forcePiano);
  } else if (bestDrum !== "") {
    selectChannel(bestDrum);
    showEditor(forcePiano ? "piano" : "rack", forcePiano);
  }
}

/** function onLaneDown(e: Ev, g: PGeo) => Undefined */
function onLaneDown(e, g) {
  e.preventDefault();
  const p = state.project;
  const x = e.clientX - e.targetLeft + e.scrollLeft;
  const y = e.clientY - e.targetTop + e.scrollTop;
  if (y >= g.height) {
    // Automation lanes below the tracks.
    onAutoDown(e, { zoom: g.zoom, top: g.height, x0: 0, x1: g.width }, x, y);
    return undefined;
  }
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
      });
      state.clipSelection = [];
    }
    return undefined;
  }

  if (k >= 0) {
    const box = boxes[k];
    const idx = clipIndex(box.i);
    const clip = p.playlist.clips[idx];
    if (!isSel(box.i)) {
      state.clipSelection = e.shiftKey ? state.clipSelection.concat([box.i]) : [box.i];
      reportContext();
    }
    if (clip.pattern !== "") {
      selectPattern(clip.pattern);
      followPattern();
      focusEditor(clip.pattern, e.detail >= 2);
    }
    const resizing = x > box.x + box.w - 8;
    const orig = state.clipSelection.map((s) => {
      const c = p.playlist.clips[clipIndex(s)];
      return { i: clipIndex(s), start: c.start, length: c.length, track: trackIndex(c.track) };
    });
    // Alt/Ctrl-drag duplicates.
    if (!resizing && (e.altKey || e.ctrlKey)) {
      commit(() => {
        for (const o of orig) {
          const c = p.playlist.clips[o.i];
          p.playlist.clips.push({
            pattern: c.pattern,
            sample: c.sample,
            track: c.track,
            start: c.start,
            length: c.length,
            offset: c.offset,
            gain: c.gain,
            mixer: c.mixer,
          });
        }
      });
    }
    const x0 = e.clientX;
    const y0 = e.clientY;
    begin();
    drag(
      e,
      (m) => {
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
      },
      (u) => undefined
    );
    return undefined;
  }

  // Empty lane: place the selected pattern.
  const pat = currentPattern();
  if (!pat) return undefined;
  const start = snapDown(x / g.zoom, snap);
  begin();
  p.playlist.clips.push({ pattern: pat.id, sample: "", track: trackIx(track), start: start, length: pat.length, offset: 0, gain: 1, mixer: insertIx(0) });
  const idx = p.playlist.clips.length - 1;
  state.clipSelection = [clipIx(idx)];
  changed(true);
  const x0 = e.clientX;
  drag(
    e,
    (m) => {
      const db = (m.clientX - x0) / g.zoom;
      p.playlist.clips[idx].length = Math.max(snap, snapTo(pat.length + db, snap));
      changed(true);
    },
    (u) => undefined
  );
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
        p.playlist.clips.push({
          pattern: "",
          sample: path,
          track: trackIx(track),
          start: start,
          length: Math.max(0.25, Math.round(beats * 100) / 100),
          offset: 0,
          gain: 1,
          mixer: insertIx(0),
        });
      });
      return true;
    })
    .catch((e) => {
      toast("Could not read sample", path, "error");
      return false;
    });
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
    });
  }
}

/** function playlist(b: Builder) => Undefined */
export function playlist(b) {
  const p = state.project;
  const g = geometry();
  followPlayhead(g);
  revealLane(g);
  reportViewport(g);
  const bpb = p.transport.beatsPerBar;
  b.open("div", "pl", "editor");
  b.open("div", "main", "editor-main pl");

  b.open("div", "corner", "corner");
  b.text("Playlist");
  b.close();

  // Ruler.
  b.open("div", "ruler", "ruler");
  b.attr("title", "Click to play from a bar · right-click to change the time signature from it");
  b.on("pointerdown", (e) => {
    if (e.button === 2) return undefined;
    const beat = (e.clientX - e.targetLeft + view.scrollLeft) / g.zoom;
    if (state.mode !== "song") setMode("song");
    seek(barAt(p.transport, Math.max(0, beat)).start);
  });
  b.on("contextmenu", (e) => {
    e.preventDefault();
    const beat = (e.clientX - e.targetLeft + view.scrollLeft) / g.zoom;
    openMeterMenu(barAt(p.transport, Math.max(0, beat)).bar, e.clientX, e.clientY);
  });
  b.open("div", "in", "");
  b.style("transform", `translateX(${-view.scrollLeft}px)`);
  b.style("position", "absolute");
  b.style("inset", "0");
  const bars = barLines(p.transport, view.scrollLeft / g.zoom, (view.scrollLeft + view.width) / g.zoom);
  for (const bl of bars) {
    const meter = meterChangeAt(p.transport, bl.bar);
    if (g.zoom * bl.length < 40 && bl.bar % 2 === 1 && meter === "") continue;
    b.leaf("div", `m${bl.bar}`, "ruler-mark", meter === "" ? String(bl.bar + 1) : `${bl.bar + 1} · ${meter}`);
    b.style("left", `${bl.start * g.zoom}px`);
  }
  // Repeats: a band from the start sign to the end sign, its endings beneath.
  for (let i = 0; i < p.repeats.length; i++) {
    const r = p.repeats[i];
    b.open("div", `r${i}`, "ruler-repeat");
    b.style("left", `${r.start * g.zoom}px`);
    b.style("width", `${Math.max(2, (r.end - r.start) * g.zoom)}px`);
    b.attr("title", `Repeat: plays ${r.times} times${r.endings.length > 0 ? ", with endings" : ""}`);
    b.leaf("span", "t", "ruler-repeat-times", `×${r.times}`);
    b.close();
    for (let k = 0; k < r.endings.length; k++) {
      const e = r.endings[k];
      b.leaf("div", `r${i}e${k}`, "ruler-ending", passesText(e.passes));
      b.style("left", `${e.start * g.zoom}px`);
      b.style("width", `${Math.max(2, (e.end - e.start) * g.zoom)}px`);
      b.attr("title", `Ending: plays on pass${e.passes.length > 1 ? "es" : ""} ${e.passes.join(", ")}`);
    }
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
    });
    b.leaf("span", "num", "t-num", String(t + 1));
    b.leaf("span", "name", "t-name", tr.name);
    b.attr("title", "Double-click to rename");
    b.on("dblclick", (e) => {
      const name = promptBox("Track name", tr.name);
      if (name !== "") {
        commit(() => {
          tr.name = name;
        });
      }
    });
    b.leaf("div", "mute", tr.mute ? "ch-mute off" : "ch-mute", "");
    b.attr("title", tr.mute ? "Unmute track" : "Mute track");
    b.on("click", (e) => {
      e.stopPropagation();
      commit(() => {
        tr.mute = !tr.mute;
      });
    });
    b.close();
  }
  autoHeads(b, g.height);
  b.close();
  b.close();

  // Lanes.
  b.open("div", "lanes", "scroller");
  b.on("scroll", (e) => {
    view.scrollLeft = e.scrollLeft;
    view.scrollTop = e.scrollTop;
    invalidate();
  });
  b.on("resize", (e) => {
    view.width = e.targetWidth;
    view.height = e.targetHeight;
    invalidate();
  });
  b.on("pointerdown", (e) => pressOrTap(e, (d) => onLaneDown(d, g)));
  b.on("dblclick", (e) => {
    const y = e.clientY - e.targetTop + e.scrollTop;
    if (y >= g.height) onAutoDblClick(e, { zoom: g.zoom, top: g.height, x0: 0, x1: g.width }, e.clientX - e.targetLeft + e.scrollLeft, y);
  });
  b.prop("scrollLeft", String(view.scrollLeft));
  b.prop("scrollTop", String(view.scrollTop));
  b.on("contextmenu", (e) => {
    e.preventDefault();
  });
  b.on("dragover", (e) => {
    if (dragSample.path !== "") e.preventDefault();
  });
  b.on("drop", (e) => {
    if (dragSample.path === "") return undefined;
    e.preventDefault();
    dropSample(dragSample.path, e.clientX - e.targetLeft + e.scrollLeft, e.clientY - e.targetTop + e.scrollTop, g);
    dragSample.path = "";
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
  });
  b.on("pointermove", (e) => {
    const y = e.clientY - e.targetTop + e.scrollTop;
    if (y >= g.height) {
      hint(autoHint({ zoom: g.zoom, top: g.height, x0: 0, x1: g.width }, e.clientX - e.targetLeft + e.scrollLeft, y));
      return undefined;
    }
    const beat = (e.clientX - e.targetLeft + e.scrollLeft) / g.zoom;
    const bar = barAt(p.transport, beat).bar + 1;
    const pat = currentPattern();
    hint(`Bar ${bar} — click to place “${pat ? pat.name : "a pattern"}”, drag clips to move, edge to resize, Alt-drag to copy, right-click to delete`);
  });

  b.open("div", "content", "canvas-grid");
  b.style("width", `${g.width}px`);
  b.style("height", `${Math.max(g.height + autoHeight(), view.height - 2)}px`);
  for (let t = 0; t < tracks.length; t++) {
    b.leaf("div", `lane${t}`, "track-lane", "");
    b.style("top", `${t * g.trackH}px`);
    b.style("height", `${g.trackH}px`);
  }
  b.leaf("div", "bg", "grid-bg", "");
  b.style("--beat", `${g.zoom}px`);
  if (p.transport.meters.length === 0) {
    b.style("--bar", `${g.zoom * bpb}px`);
    b.style("--step", `${g.zoom * bpb * 4}px`);
  } else {
    // Bars of changing length: drawn one by one below.
    b.style("--bar", `${g.width + 1}px`);
    b.style("--step", `${g.width + 1}px`);
    for (const bl of barLines(p.transport, view.scrollLeft / g.zoom, (view.scrollLeft + view.width) / g.zoom)) {
      b.leaf("div", `bl${bl.bar}`, "bar-line", "");
      b.style("left", `${bl.start * g.zoom}px`);
    }
  }

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
  autoBody(b, { zoom: g.zoom, top: g.height, x0: x0, x1: x1 });
  if (state.mode === "song") {
    b.leaf("div", "ph", "playhead", "");
    b.style("left", `${state.position * g.zoom}px`);
  }
  b.close();
  b.close();

  b.close();
  b.close();
}

/** Record the visible part of the song for the agent context. */
/** function reportViewport(g: PGeo) => Undefined */
function reportViewport(g) {
  const vp = state.viewport;
  const start = Math.round((view.scrollLeft / g.zoom) * 100) / 100;
  const end = Math.round(((view.scrollLeft + view.width) / g.zoom) * 100) / 100;
  const t0 = Math.floor(view.scrollTop / g.trackH);
  const t1 = Math.max(t0, Math.min(state.project.playlist.tracks.length - 1, Math.floor((view.scrollTop + view.height - 1) / g.trackH)));
  if (vp.plStart !== start || vp.plEnd !== end || vp.plTrack0 !== t0 || vp.plTrack1 !== t1) {
    vp.plStart = start;
    vp.plEnd = end;
    vp.plTrack0 = t0;
    vp.plTrack1 = t1;
    reportContext();
  }
}

/** Scroll an automation lane into view (after "Create / Go to automation"). */
/** function revealLane(g: PGeo) => Undefined */
function revealLane(g) {
  const off = revealOffset();
  if (off < 0) return undefined;
  const y = g.height + off;
  if (y < view.scrollTop || y + LANE_H > view.scrollTop + view.height) view.scrollTop = Math.max(0, y + LANE_H - view.height + 12);
}

/** Scroll the playlist so a song beat is in view (the Critic's "show me"). */
/** function revealBeat(beat: Number) => Undefined */
export function revealBeat(beat) {
  const x = beat * view.zoom;
  if (x < view.scrollLeft || x > view.scrollLeft + view.width * 0.88) view.scrollLeft = Math.max(0, x - view.width * 0.08);
  invalidate();
}

/** Page the view along with the playhead (FL Studio style) while playing. */
/** function followPlayhead(g: PGeo) => Undefined */
function followPlayhead(g) {
  if (!state.follow || !state.playing || state.mode !== "song") return undefined;
  const x = state.position * g.zoom;
  if (x < view.scrollLeft || x > view.scrollLeft + view.width * 0.88) {
    view.scrollLeft = Math.max(0, x - view.width * 0.08);
  }
}

/** function followButton(b: Builder) => Undefined */
export function followButton(b) {
  iconButton(
    b,
    "follow",
    state.follow ? "small on" : "small",
    "follow",
    state.follow ? "Follow playback: on — the view scrolls with the playhead" : "Follow playback: off",
    () => {
      state.follow = !state.follow;
      invalidate();
    }
  );
}

/** function playlistTools(b: Builder) => Undefined */
export function playlistTools(b) {
  const pat = currentPattern();
  followButton(b);
  b.leaf("span", "l", "label", "Paint");
  const ids = state.project.patterns.map((x) => x.id);
  select(
    b,
    "pat",
    "",
    state.pattern,
    ids,
    state.project.patterns.map((x) => x.name),
    "Pattern placed by clicking an empty lane",
    (v) => {
      selectPattern(v);
      followPattern();
    }
  );
  if (pat) {
    b.leaf("span", "sw", "swatch", "");
    b.style("--c", pat.color);
  }
  iconButton(b, "addtrack", "small ghost", "plus", "Add a playlist track", () => {
    commit(() => {
      state.project.playlist.tracks.push({ name: `Track ${state.project.playlist.tracks.length + 1}`, mute: false });
    });
  });
}
