// The Drums dock (docs/drums.md): one screen that always works on a drum
// pattern — the target — and says where in the song it plays.
//
// - On top, the target: the drum pattern at the song cursor or the selected
//   clip (or picked), the track and bars it plays, its time signature and
//   kit; with none there, a button makes one at the cursor.
// - On the left, the groove library, the target's time signature first: a
//   click plays a groove in the target (or makes a pattern at the cursor),
//   ▶ tries it first without changing the song.
// - In the middle, the target's recipe (A or B, a fill, a crash, its bars,
//   kit and feel) and its own notes as a step grid to click.
// - Below, the song's drum clips along the bars (a click targets one), and
//   the song drummer: the whole song's drums at once, from sections
//   guessed from the playlist (the project's `drums` part).
//
// A pattern's recipe lives on the pattern (`patterns[].drums`) and its notes
// are made by the server (POST /api/drums/pattern), like the song drummer's
// (POST /api/drums): the same code the command line and the browser-only
// studio run, so the agent can edit both.

import { getJson, sendJson, now, audioPost } from "#platform";
import { state, commit, invalidate, hint, currentPattern, hooks, engineJson, selectPattern } from "../store.js";
import { decodeProject, encodeProject, meterMap, barAt, barLines, cloneProject, uniqueId, songLength, optionValue, drumName, noteName } from "../model.js";
import { startAudio, seek, setMode, play, stop } from "../audio.js";
import { button, iconButton, select, textInput, glyph } from "./widgets.js";
import { toast } from "./toast.js";
import { trackIndex, clipIndex, trackIx, clipIx, insertIx } from "#brands";

export const drums = {
  catalog /*: GrooveCatalog */: { grooves: [], kits: [] },
  /** The catalog was asked for. */
  asked: false,
  /** A request is out (writing, guessing). */
  busy: false,
  /** The selected section. */
  section /*: Int */: 0,
  /** The written part plays with the song (the project is not changed). */
  previewing: false,
  previewAt: 0,
  /** Counts preview requests, so a late answer does not replace a newer one. */
  previewSeq: 0,
  /** Where the producer last pointed in the song (the cursor's bar, or a
   * selected clip): when it moves, the tab targets what plays there. */
  focus: "",
  /** The drum pattern the tab works on ("" none). */
  target: "",
  /** The groove library's search. */
  query: "",
  /** The song drummer's panel is open. */
  arrange: false,
  /** Requests out to make a pattern. */
  pending: 0,
  /** A groove being tried, and since when (ms). */
  trying: "",
  trySeq: 0,
  tryAt: 0,
  /** The song cursor (beats), kept while a pattern or groove plays on its own. */
  songAt: 0,
  /** The pattern loops on its own from the song (stopping it goes back). */
  looped: false,
  /** The song drummer's panel was just opened (scroll down to it). */
  opened: false,
  /** Drums added to the grid that have no notes yet. */
  extraRows /*: GridRow[] */: [],
};

const PLAY_LABELS = [
  { key: "a", value: "Groove A" },
  { key: "b", value: "Groove B" },
  { key: "hits", value: "Hits" },
  { key: "count", value: "Count-in" },
  { key: "rest", value: "Rest" },
];
const PLAY_SHORT = [
  { key: "a", value: "A" },
  { key: "b", value: "B" },
  { key: "hits", value: "Hits" },
  { key: "count", value: "Count" },
  { key: "rest", value: "Rest" },
];
const FILL_LABELS = [
  { key: "none", value: "No fill" },
  { key: "beat", value: "1-beat fill" },
  { key: "half", value: "2-beat fill" },
  { key: "bar", value: "1-bar fill" },
];
const ROLE_LABELS = [
  { key: "kick", value: "Kick" },
  { key: "snare", value: "Snare" },
  { key: "rim", value: "Side stick" },
  { key: "clap", value: "Clap" },
  { key: "hat", value: "Hi-hat" },
  { key: "pedal", value: "Hat foot" },
  { key: "openhat", value: "Open hat" },
  { key: "ride", value: "Ride" },
  { key: "bell", value: "Ride bell" },
  { key: "crash", value: "Crash" },
  { key: "tom1", value: "High tom" },
  { key: "tom2", value: "Mid tom" },
  { key: "tom3", value: "Floor tom" },
  { key: "cowbell", value: "Cowbell" },
  { key: "shaker", value: "Shaker" },
  { key: "tamb", value: "Tambourine" },
];
const FEELS = ["tight", "natural", "loose"];
const FEEL_LABELS = ["Tight", "Natural", "Loose"];
const SWINGS = ["0", "0.2", "0.3", "0.4", "0.5", "0.7"];
const SWING_LABELS = ["Straight", "Swing 20%", "Swing 30%", "Swing 40%", "Swing 50%", "Swing 70%"];

/** function labelOf(list: KS[], key: String) => String */
function labelOf(list, key) {
  const e = list.find((x) => x.key === key);
  return e ? e.value : key;
}

/** function errText<E>(e: E) => String */
function errText(e) {
  return String(e).replace(/^Error:\s*/, "");
}

// ------------------------------------------------------------------ catalog

/** function decodeRows<T>(rows: T[]) => String[][] */
function decodeRows(rows) {
  return rows.map((r) => [String(r[0]), String(r[1])]);
}

/** Ask the server for the groove library (once). */
function loadCatalog() {
  if (drums.asked) return undefined;
  drums.asked = true;
  getJson("/api/grooves")
    .then((r) => {
      drums.catalog = {
        grooves: r.grooves.map((g) => ({
          id: String(g.id),
          style: String(g.style),
          name: String(g.name),
          meter: String(g.meter),
          barBeats: Math.round(Number(g.barBeats)),
          steps: Math.round(Number(g.steps)),
          tempo: [Math.round(Number(g.tempo[0])), Math.round(Number(g.tempo[1]))],
          kit: String(g.kit),
          swing: Number(g.swing),
          a: decodeRows(g.a),
          b: decodeRows(g.b),
        })),
        kits: r.kits.map((k) => String(k)),
      };
      invalidate();
      return true;
    })
    .catch((e) => {
      drums.asked = false;
      toast("Could not load the drum grooves", errText(e), "error");
      return false;
    });
}

/** function grooveOf(id: String) => GrooveInfo? */
function grooveOf(id) {
  return drums.catalog.grooves.find((g) => g.id === id);
}

/** function grooveLabel(g: GrooveInfo) => String */
function grooveLabel(g) {
  return `${g.style} · ${g.name}`;
}

// ------------------------------------------------------------------ meters

/** type BarRun = { bar: Number, barBeats: Number, label: String } */

/** The time signatures of the bars a groove would play (one run per bar
 * length): the bars of the sections that groove in it — section `at`, or
 * those on the part's groove when `at` is -1 — or bar 1 before there is a part. */
/** function metersOf(at: Int) => BarRun[] */
function metersOf(at) {
  const map = meterMap(state.project.transport);
  const d = state.project.drums;
  /** const out: BarRun[] */
  const out = [];
  /** function add(bar: Number) => Undefined */
  const add = (bar) => {
    let s = map[0];
    for (const m of map) if (m.bar <= bar) s = m;
    // (bar 0 stands for the whole song when it keeps one time signature)
    if (!out.some((r) => Math.abs(r.barBeats - s.barBeats) < 1e-6)) out.push({ bar: map.length === 1 ? 0 : bar, barBeats: s.barBeats, label: s.label });
  };
  if (!d.on) {
    add(0);
    return out;
  }
  let bar = d.start - 1;
  for (let i = 0; i < d.sections.length; i++) {
    const s = d.sections[i];
    const uses = at >= 0 ? i === at : s.groove === "";
    if (uses && (s.play === "a" || s.play === "b")) for (let k = 0; k < s.bars; k++) add(bar + k);
    bar = bar + s.bars;
  }
  return out;
}

/** The time signature of bar `bar` (counted from 0). */
/** function meterAt(bar: Number) => BarRun */
function meterAt(bar) {
  const map = meterMap(state.project.transport);
  let s = map[0];
  for (const m of map) if (m.bar <= bar) s = m;
  return { bar: map.length === 1 ? 0 : bar, barBeats: s.barBeats, label: s.label };
}

/** The bar (from 0) the producer points at: the first selected clip's, else
 * the song cursor's; and a key that changes when either moves. */
/** function pointedBar() => { bar: Number, key: String } */
function pointedBar() {
  const t = state.project.transport;
  const clips = state.project.playlist.clips;
  if (state.clipSelection.length > 0) {
    const i = clipIndex(state.clipSelection[0]);
    if (i < clips.length) {
      const c = clips[i];
      return { bar: barAt(t, c.start).bar, key: `clip ${i} ${c.start}` };
    }
  }
  // The song cursor; while a pattern or a groove plays on its own, where it was.
  if (state.mode === "song" && drums.trying === "") drums.songAt = Math.max(0, state.position);
  const bar = barAt(t, drums.songAt).bar;
  return { bar: bar, key: `bar ${bar}` };
}

/** The section of the part that plays bar `bar` (-1: none). */
/** function sectionAt(d: DrumPart, bar: Number) => Int */
function sectionAt(d, bar) {
  let first = d.start - 1;
  for (let i = 0; i < d.sections.length; i++) {
    if (bar >= first && bar < first + d.sections[i].bars) return i;
    first = first + d.sections[i].bars;
  }
  return -1;
}

/** The first bar of section `at`. */
/** function sectionStart(d: DrumPart, at: Int) => Number */
function sectionStart(d, at) {
  let first = d.start - 1;
  for (let i = 0; i < at && i < d.sections.length; i++) first = first + d.sections[i].bars;
  return first;
}

/** The time signature the producer is working in: the selected section's
 * (its first bar), or — before there is a part, or past its end — where
 * the song cursor or the selected clip is. */
/** function activeMeter() => BarRun */
function activeMeter() {
  const d = state.project.drums;
  if (d.on && drums.section < d.sections.length) return meterAt(sectionStart(d, drums.section));
  return meterAt(pointedBar().bar);
}

/** Whether a groove fits bars of a time signature. */
/** function fits(g: GrooveInfo, run: BarRun) => Boolean */
function fits(g, run) {
  return Math.abs(g.barBeats - run.barBeats) < 1e-6;
}

/** The first section whose groove does not fit its bars, and why ("" when all fit). */
/** function partMisfit(d: DrumPart) => { at: Int, why: String } */
function partMisfit(d) {
  let first = d.start - 1;
  for (let i = 0; i < d.sections.length; i++) {
    const s = d.sections[i];
    const g = grooveOf(s.groove !== "" ? s.groove : d.groove);
    if (g && (s.play === "a" || s.play === "b")) {
      for (let k = 0; k < s.bars; k++) {
        const r = meterAt(first + k);
        if (!fits(g, r)) {
          const name = s.name || `Section ${i + 1}`;
          return {
            at: i,
            why: `${name} (bars ${first + 1}–${first + s.bars}) is in ${r.label}, but plays ${g.name} (${g.meter}). Select it — or put the song cursor there — and pick a ${r.label} groove.`,
          };
        }
      }
    }
    first = first + s.bars;
  }
  return { at: -1, why: "" };
}

/** Why a groove cannot play those bars ("" when it can). */
/** function misfit(g: GrooveInfo, runs: BarRun[]) => String */
function misfit(g, runs) {
  for (const r of runs) {
    if (Math.abs(g.barBeats - r.barBeats) > 1e-6) {
      const where = r.bar > 0 ? `bar ${r.bar + 1} is in ${r.label}` : `the song is in ${r.label}`;
      return `${g.name} is in ${g.meter}, but ${where}. Pick a groove in ${r.label}, or change the time signature (Time, in the top bar).`;
    }
  }
  return "";
}

// ------------------------------------------------------------------ editing

/** Change the drum part (one undo step); a playing preview follows (see playPreview). */
/** function edit(fn: (DrumPart) => Undefined) => Undefined */
function edit(fn) {
  commit(() => fn(state.project.drums));
}

/** Bars of the song before the drum part's first section, as song beats. */
/** function barStart(bar: Number) => Number */
function barStart(bar) {
  const map = meterMap(state.project.transport);
  let s = map[0];
  for (const m of map) if (m.bar <= bar) s = m;
  return s.beat + (bar - s.bar) * s.barBeats;
}

/** Start a drum part on `groove`, its sections guessed from the playlist. */
/** function startPart(groove: String) => Undefined */
function startPart(groove) {
  const body = encodeProject(state.project);
  body.drums = { groove: groove };
  drums.busy = true;
  invalidate();
  sendJson("/api/drums?guess=true&write=false", "POST", body)
    .then((r) => {
      drums.busy = false;
      const part = decodeProject(r.project).drums;
      const g = grooveOf(groove);
      if (g) part.swing = g.swing;
      commit(() => {
        state.project.drums = part;
      });
      drums.section = 0;
      return true;
    })
    .catch((e) => {
      drums.busy = false;
      toast("Could not start the drum part", errText(e), "error");
      invalidate();
      return false;
    });
}

/** Guess the sections again from the playlist (keeps the groove and kit). */
function guessAgain() {
  drums.busy = true;
  invalidate();
  sendJson("/api/drums?guess=true&write=false", "POST", encodeProject(state.project))
    .then((r) => {
      drums.busy = false;
      const part = decodeProject(r.project).drums;
      edit((d) => {
        d.start = part.start;
        d.sections = part.sections;
      });
      drums.section = 0;
      return true;
    })
    .catch((e) => {
      drums.busy = false;
      toast("Could not guess the sections", errText(e), "error");
      invalidate();
      return false;
    });
}

/** Write the drum part into the song: patterns, clips and channels, one undo step. */
export function writeDrums() {
  if (!state.project.drums.on || drums.busy) return undefined;
  stopPreview();
  drums.busy = true;
  invalidate();
  const edits = state.edits;
  sendJson("/api/drums", "POST", encodeProject(state.project))
    .then((r) => {
      drums.busy = false;
      if (state.edits !== edits) {
        // The song changed while the part was being written: write it again.
        writeDrums();
        return false;
      }
      const np = decodeProject(r.project);
      commit(() => {
        const p = state.project;
        p.channels = np.channels;
        p.patterns = np.patterns;
        p.playlist = np.playlist;
        // (Channels the write removed leave the score's settings too.)
        p.score = np.score;
        p.drums = np.drums;
      });
      toast(
        "Drums written",
        `${Number(r.report.patterns)} patterns in ${Number(r.report.clips)} clips on track ${Math.round(Number(r.report.track)) + 1} (${np.playlist.tracks[Math.round(Number(r.report.track))]?.name ?? "Drums"})${Number(r.report.kept) > 0 ? `, ${Number(r.report.kept)} kept as you edited them` : ""}${Number(r.report.left ?? 0) > 0 ? `; ${Number(r.report.left)} bar${Number(r.report.left) === 1 ? "" : "s"} left to your own drum patterns` : ""}. Ctrl+Z undoes it.`,
        "info"
      );
      return true;
    })
    .catch((e) => {
      drums.busy = false;
      toast("Could not write the drums", errText(e), "error");
      invalidate();
      return false;
    });
}

// ------------------------------------------------------------------ preview

/** Ask for the written song and hand it to the engine (the project itself
 * is left alone). */
/** function sendPreview(seek: Boolean) => Promise<Boolean> */
function sendPreview(seek) {
  drums.previewSeq = drums.previewSeq + 1;
  const seq = drums.previewSeq;
  return sendJson("/api/drums", "POST", encodeProject(state.project))
    .then((r) => {
      if (seq !== drums.previewSeq || !drums.previewing) return false;
      audioPost({ t: "project", json: JSON.stringify(r.project) });
      if (seek) {
        audioPost({ t: "mode", pattern: "" });
        audioPost({ t: "seek", beat: barStart(state.project.drums.start - 1) });
        audioPost({ t: "play" });
        drums.previewAt = now();
        previewTick();
      }
      return true;
    })
    .catch((e) => {
      stopPreview();
      toast("Could not play the drums", errText(e), "error");
      return false;
    });
}

/** Play the song with the drum part as it would be written, from its start. */
async function playPreview() {
  if (!state.project.drums.on) return false;
  await startAudio();
  drums.previewing = true;
  // Every change to the song (the part, the kit, a note) plays on in the
  // preview, instead of the song without its drums replacing it.
  hooks.preview = refreshPreview;
  hooks.previewing = true;
  invalidate();
  return await sendPreview(true);
}

function refreshPreview() {
  // (An undo can take the part away.)
  if (!state.project.drums.on) stopPreview();
  else sendPreview(false);
}

/** Stop the preview and give the engine the song back. */
export function stopPreview() {
  if (!drums.previewing) return undefined;
  drums.previewing = false;
  hooks.previewing = false;
  drums.previewSeq = drums.previewSeq + 1;
  audioPost({ t: "stop" });
  audioPost({ t: "project", json: engineJson() });
  const pat = currentPattern();
  audioPost({ t: "mode", pattern: state.mode === "pattern" && pat ? pat.id : "" });
  invalidate();
}

/** While previewing: notice the transport stopping it. */
function previewTick() {
  if (!drums.previewing) return undefined;
  if (state.output === "browser" && !state.playing && now() - drums.previewAt > 800) {
    stopPreview();
    return undefined;
  }
  setTimeout(previewTick, 200);
}

// ------------------------------------------------------------------ views

/** A labelled dropdown. */
/** function field(b: Builder, key: String, label: String, value: String, choices: String[], labels: String[], tip: String, onSet: (String) => Undefined) => Undefined */
function field(b, key, label, value, choices, labels, tip, onSet) {
  b.open("label", key, "drums-field");
  b.leaf("span", "l", "drums-label", label);
  select(b, "s", "", value, choices, labels, tip, onSet);
  b.close();
}

/** A labelled groove dropdown: the grooves that do not fit the bars are
 * greyed out, their tooltip saying why (`why[i]`, "" = fits). */
/** function grooveField(b: Builder, key: String, label: String, value: String, choices: String[], labels: String[], why: String[], tip: String, onSet: (String) => Undefined) => Undefined */
function grooveField(b, key, label, value, choices, labels, why, tip, onSet) {
  b.open("label", key, "drums-field");
  b.leaf("span", "l", "drums-label", label);
  b.open("select", "s", "select");
  b.prop("value", value);
  b.attr("title", tip);
  b.on("pointerenter", (e) => hint(tip));
  b.on("change", (e) => {
    onSet(e.value);
  });
  for (let i = 0; i < choices.length; i++) {
    const no = i < why.length && why[i] !== "";
    b.leaf("option", choices[i], no ? "misfit" : "", no ? `${labels[i]} (${grooveOf(choices[i])?.meter ?? ""})` : labels[i]);
    b.attr("value", choices[i]);
    if (no) {
      b.attr("disabled", "true");
      b.attr("title", why[i]);
    }
  }
  b.close();
  b.close();
}

/** A number with − and + buttons; `onStep` gets −1 or +1 and applies it to
 * the current value (clicks quicker than a redraw all count). */
/** function stepper(b: Builder, key: String, label: String, value: Number, tip: String, onStep: (Number) => Undefined) => Undefined */
function stepper(b, key, label, value, tip, onStep) {
  b.open("div", key, "drums-field");
  b.leaf("span", "l", "drums-label", label);
  b.open("div", "n", "drums-stepper");
  iconButton(b, "dn", "small", "minus", `${tip}: fewer`, () => onStep(-1));
  b.leaf("span", "v", "drums-num", String(value));
  iconButton(b, "up", "small", "plus", `${tip}: more`, () => onStep(1));
  b.close();
  b.close();
}

/** The rows of a groove part in this song: its edit, else the library's. */
/** function partRows(d: DrumPart, g: GrooveInfo, isB: Boolean) => String[][] */
function partRows(d, g, isB) {
  const e = d.grooves.find((x) => x.groove === g.id);
  if (e) {
    const rows = isB ? e.b : e.a;
    if (rows.length > 0) return rows;
  }
  return isB ? g.b : g.a;
}

/** Do two parts have the same strokes (spacing aside)? */
/** function sameRows(x: String[][], y: String[][]) => Boolean */
function sameRows(x, y) {
  const flat = (rows) => rows.map((r) => `${r[0]}:${r[1].replace(/[ |]/g, "")}`).join(",");
  return flat(x) === flat(y);
}

/** function copyRows(rows: String[][]) => String[][] */
function copyRows(rows) {
  return rows.map((r) => [r[0], r[1]]);
}

/** Hand edits in the written patterns of the slots `which` picks are given
 * back to the drummer: what those patterns hold now is no longer taken as an
 * edit (an empty print, see Written in crates/core/src/drums), so the next
 * write makes them afresh instead of keeping them. */
/** function forgetEdits(d: DrumPart, which: (String) => Boolean) => Undefined */
function forgetEdits(d, which) {
  for (const w of d.written) if (which(w.slot)) w.print = "";
}

/** Change a groove part's rows for this song (copied from the library on the
 * first change). The grid is then the groove's source: a hand-kept copy of
 * its plain pattern goes, and so does an edit to it not yet written. */
/** function editRows(g: GrooveInfo, isB: Boolean, fn: (String[][]) => Undefined) => Undefined */
function editRows(g, isB, fn) {
  edit((d) => {
    const found = d.grooves.find((x) => x.groove === g.id);
    /** const e: GrooveEdit */
    const e = found ?? { groove: g.id, a: copyRows(g.a), b: copyRows(g.b) };
    if (!found) d.grooves.push(e);
    if (e.a.length === 0) e.a = copyRows(g.a);
    if (e.b.length === 0) e.b = copyRows(g.b);
    fn(isB ? e.b : e.a);
    const slot = `${g.id}/${isB ? "b" : "a"}`;
    d.kept = d.kept.filter((k) => k.slot !== slot);
    forgetEdits(d, (s) => s === slot);
  });
}

/** Steps as stored: a space between beats. */
/** function grouped(steps: String, perBeat: Number) => String */
function grouped(steps, perBeat) {
  /** const out: String[] */
  const out = [];
  for (let i = 0; i < steps.length; i += perBeat) out.push(steps.slice(i, i + perBeat));
  return out.join(" ");
}

/** The next stroke of a clicked cell: rest → hit → accent → ghost → rest. */
/** function nextStroke(c: String) => String */
function nextStroke(c) {
  if (c === "x") return "X";
  if (c === "X") return "g";
  if (c === "g" || c === "f") return ".";
  return "x";
}

/** A groove part as a step grid you can click: what the groove is, and the
 * place to change it for the whole song. */
/** function grooveGrid(b: Builder, key: String, title: String, g: GrooveInfo, isB: Boolean, rows: String[][], edited: Boolean) => Undefined */
function grooveGrid(b, key, title, g, isB, rows, edited) {
  const perBeat = g.steps / g.barBeats;
  b.open("div", key, "drums-grid");
  b.open("div", "t", "drums-grid-title");
  b.text(title);
  if (edited) b.leaf("span", "e", "drums-edited", "edited");
  b.close();
  for (const row of rows) {
    const role = row[0];
    const steps = row[1].replace(/[ |]/g, "");
    b.open("div", role, "drums-row");
    b.leaf("span", "r", "drums-role", labelOf(ROLE_LABELS, role));
    b.open("div", "c", "drums-cells");
    for (let i = 0; i < steps.length; i++) {
      const c = steps[i];
      const kind = c === "X" ? "acc" : c === "x" ? "hit" : c === "g" ? "ghost" : c === "f" ? "feather" : "rest";
      b.open("span", `${i}`, `drums-cell ${kind}${i % perBeat === 0 ? " beat" : ""}`);
      b.attr("title", `${labelOf(ROLE_LABELS, role)}, step ${i + 1}: click for a hit, then an accent, a ghost note, a rest`);
      b.on("click", (e) => {
        editRows(g, isB, (rs) => {
          const k = rs.findIndex((x) => x[0] === role);
          if (k < 0) return undefined;
          const st = rs[k][1].replace(/[ |]/g, "");
          rs[k] = [role, grouped(st.slice(0, i) + nextStroke(st.charAt(i)) + st.slice(i + 1), perBeat)];
        });
      });
      b.close();
    }
    b.close();
    b.close();
  }
  /** const free: String[] */
  const free = [""];
  /** const freeLabels: String[] */
  const freeLabels = ["+ Drum…"];
  for (const e of ROLE_LABELS) {
    if (!rows.some((r) => r[0] === e.key)) {
      free.push(e.key);
      freeLabels.push(e.value);
    }
  }
  select(b, "add", "drums-add", "", free, freeLabels, "Add a drum to this part", (v) => {
    if (v === "") return undefined;
    const len = rows.length > 0 ? rows[0][1].replace(/[ |]/g, "").length : g.steps;
    editRows(g, isB, (rs) => {
      rs.push([v, grouped(".".repeat(len), perBeat)]);
    });
  });
  b.close();
}

/** The groove, kit and feel. */
/** function grooveView(b: Builder, d: DrumPart) => Undefined */
function grooveView(b, d) {
  const gs = drums.catalog.grooves;
  // The groove of the time signature being worked in: the part's own, or —
  // where the song changes time signature — the one its sections there play.
  const run = activeMeter();
  const several = meterMap(state.project.transport).length > 1;
  const main = grooveOf(d.groove);
  const forMain = !main || fits(main, run);
  const sec = drums.section < d.sections.length ? d.sections[drums.section] : undefined;
  const value = forMain ? d.groove : sec && sec.groove !== "" ? sec.groove : "";
  const g = grooveOf(value);
  b.open("section", "groove", "drums-step drums-groove");
  b.open("div", "pick", "drums-pick");
  const runs = [run];
  /** const ids: String[] */
  const ids = forMain ? [] : [""];
  /** const labels: String[] */
  const labels = forMain ? [] : [`Pick a ${run.label} groove…`];
  /** const why: String[] */
  const why = forMain ? [] : [""];
  for (const x of gs) {
    ids.push(x.id);
    labels.push(grooveLabel(x));
    why.push(x.id === value ? "" : misfit(x, runs));
  }
  grooveField(
    b,
    "groove",
    several ? `Groove · ${run.label}` : "Groove",
    value,
    ids,
    labels,
    why,
    several
      ? `The groove the drummer plays in ${run.label} (where the song cursor or the selected section is): A in verses, the bigger B in choruses`
      : "The groove the drummer plays: A in verses, the bigger B in choruses",
    (v) => setGrooveIn(run, v)
  );
  b.open("div", "nav", "drums-nav");
  iconButton(b, "prev", "small", "left", "The previous groove in this time signature (keeps playing)", () => stepGroove(-1));
  iconButton(b, "next", "small", "right", "The next groove in this time signature (keeps playing)", () => stepGroove(1));
  iconButton(
    b,
    "play",
    drums.previewing ? "small on" : "small",
    drums.previewing ? "stop" : "play",
    drums.previewing ? "Stop" : "Play the song with these drums, from where they start (nothing is written until you press Write drums)",
    () => {
      if (drums.previewing) stopPreview();
      else playPreview();
    }
  );
  b.close();
  b.close();
  const wrong = partMisfit(d);
  if (wrong.why !== "") {
    b.open("div", "meter", "drums-info warn");
    b.text(wrong.why);
    if (wrong.at !== drums.section) {
      button(b, "go", "small", "Select it", "Select that section (the groove above then picks for its time signature)", () => {
        drums.section = wrong.at;
        invalidate();
      });
    }
    b.close();
  }
  if (g) {
    const bpm = state.project.transport.bpm;
    const fits = bpm >= g.tempo[0] && bpm <= g.tempo[1];
    b.leaf(
      "div",
      "info",
      fits ? "drums-info" : "drums-info warn",
      `${g.meter} · ${g.tempo[0]}–${g.tempo[1]} BPM${fits ? "" : ` (the song is at ${Math.round(bpm)})`} · suggests ${g.kit === "Ebony" ? "the drum machine" : g.kit}`
    );
  }
  b.open("div", "opts", "drums-opts");
  /** const kits: String[] */
  const kits = [""];
  /** const kitLabels: String[] */
  const kitLabels = [g ? `Suggested (${g.kit === "Ebony" ? "drum machine" : g.kit})` : "Suggested"];
  for (const k of drums.catalog.kits) {
    kits.push(k);
    kitLabels.push(k === "Ebony" ? "Ebony drum machine" : k);
  }
  field(b, "kit", "Kit", d.kit, kits, kitLabels, "A General MIDI kit (one channel), or the Ebony drum machine (a channel per drum)", (v) =>
    edit((x) => {
      x.kit = v;
    })
  );
  field(
    b,
    "feel",
    "Feel",
    d.feel,
    FEELS,
    FEEL_LABELS,
    "Tight: on the grid. Natural: the backbeat sits a little late, small differences. Loose: more of both.",
    (v) =>
      edit((x) => {
        x.feel = v;
      })
  );
  if (g && g.steps / g.barBeats === 4) {
    const sw = String(d.swing);
    /** const swings: String[] */
    const swings = SWINGS.includes(sw) ? SWINGS : SWINGS.concat([sw]);
    /** const swingLabels: String[] */
    const swingLabels = SWINGS.includes(sw) ? SWING_LABELS : SWING_LABELS.concat([`Swing ${Math.round(d.swing * 100)}%`]);
    field(b, "swing", "Swing", sw, swings, swingLabels, "Delays the off 16th notes (the song's own swing applies on top)", (v) =>
      edit((x) => {
        x.swing = Number(v);
      })
    );
  }
  b.close();
  if (g) {
    const changed = d.grooves.some((x) => x.groove === g.id);
    b.open("div", "grids", "drums-grids");
    grooveGrid(b, "a", "A · verse", g, false, partRows(d, g, false), changed && !sameRows(partRows(d, g, false), g.a));
    grooveGrid(b, "b", "B · chorus", g, true, partRows(d, g, true), changed && !sameRows(partRows(d, g, true), g.b));
    if (changed) {
      button(b, "reset", "small", "Reset groove", `Back to the library's ${g.name} (your changes to this groove, and its kept patterns, go)`, () =>
        edit((x) => {
          x.grooves = x.grooves.filter((e) => e.groove !== g.id);
          x.kept = x.kept.filter((k) => !k.slot.startsWith(`${g.id}/`));
          forgetEdits(x, (s) => s.startsWith(`${g.id}/`));
        })
      );
    }
    b.close();
  }
  b.close();
}

/** Play groove `id` in time signature `run`: as the part's groove when that
 * is in `run` (or there is none), else on the sections in `run` that play
 * the part's groove or another groove in `run`. */
/** function setGrooveIn(run: BarRun, id: String) => Undefined */
function setGrooveIn(run, id) {
  const d = state.project.drums;
  const main = grooveOf(d.groove);
  if (!main || fits(main, run)) {
    setGroove(id);
    return undefined;
  }
  if (id === "") return undefined;
  edit((x) => {
    let first = x.start - 1;
    for (const s of x.sections) {
      const own = grooveOf(s.groove);
      const inRun = Math.abs(meterAt(first).barBeats - run.barBeats) < 1e-6;
      if (inRun && (s.groove === "" || (own !== undefined && fits(own, run)))) s.groove = id;
      first = first + s.bars;
    }
  });
}

/** function setGroove(id: String) => Undefined */
function setGroove(id) {
  const g = grooveOf(id);
  edit((d) => {
    d.groove = id;
    if (g) d.swing = g.swing;
  });
}

/** function stepGroove(by: Int) => Undefined */
function stepGroove(by) {
  const gs = drums.catalog.grooves;
  if (gs.length === 0) return undefined;
  const d = state.project.drums;
  const run = activeMeter();
  const main = grooveOf(d.groove);
  const sec = drums.section < d.sections.length ? d.sections[drums.section] : undefined;
  const cur = !main || fits(main, run) ? d.groove : sec ? sec.groove : "";
  let i = gs.findIndex((g) => g.id === cur);
  // The next one in the time signature being worked in.
  for (let n = 0; n < gs.length; n++) {
    i = (i + by + gs.length) % gs.length;
    if (fits(gs[i], run)) {
      setGrooveIn(run, gs[i].id);
      return undefined;
    }
  }
}

/** The sections, as a strip the width of the part, and the selected one's settings. */
/** function sectionsView(b: Builder, d: DrumPart) => Undefined */
function sectionsView(b, d) {
  const total = d.sections.reduce((n, s) => n + s.bars, 0);
  if (drums.section >= d.sections.length) drums.section = Math.max(0, d.sections.length - 1);
  b.open("section", "sections", "drums-step drums-sections");
  b.open("div", "head", "drums-head");
  b.leaf("div", "t", "drums-title", "Sections");
  b.leaf(
    "div",
    "sub",
    "drums-sub",
    total > 0 ? `bars ${d.start}–${d.start + total - 1}${d.ending === "hit" ? `, ending on ${d.start + total}` : ""}` : "none yet"
  );
  b.open("div", "acts", "drums-actions");
  button(b, "guess", "small", "Guess from playlist", "Make the sections again from the playlist: a new one wherever the arrangement changes", () =>
    guessAgain()
  );
  button(b, "add", "small", "Add section", "Add a section at the end", () =>
    edit((x) => {
      x.sections.push({ name: `Section ${x.sections.length + 1}`, bars: 8, play: "a", fill: "beat", crash: x.sections.length > 0, groove: "" });
      drums.section = x.sections.length - 1;
    })
  );
  b.close();
  b.close();

  b.open("div", "strip", "drums-strip");
  for (let i = 0; i < d.sections.length; i++) {
    const s = d.sections[i];
    b.open("button", `s${i}`, `drums-sec play-${s.play}${i === drums.section ? " on" : ""}`);
    b.style("flex", `${Math.max(1, s.bars)} 1 0`);
    b.attr(
      "title",
      `${s.name || `Section ${i + 1}`}: ${s.bars} bars, ${labelOf(PLAY_LABELS, s.play)}${s.crash ? ", crash" : ""}${s.fill !== "none" ? `, ${labelOf(FILL_LABELS, s.fill)}` : ""}`
    );
    b.on("click", (e) => {
      drums.section = i;
      invalidate();
    });
    b.leaf("span", "n", "drums-sec-name", s.name || `Section ${i + 1}`);
    b.leaf(
      "span",
      "m",
      "drums-sec-meta",
      `${s.crash ? "✶ " : ""}${s.bars} · ${labelOf(PLAY_SHORT, s.play)}${s.groove !== "" ? "*" : ""}${s.fill !== "none" ? ` ▸${s.fill === "bar" ? "bar" : s.fill === "half" ? "2" : "1"}` : ""}`
    );
    b.close();
  }
  b.close();

  const s = d.sections[drums.section];
  if (s) {
    const at = drums.section;
    b.open("div", "edit", "drums-edit");
    b.open("label", "name", "drums-field");
    b.leaf("span", "l", "drums-label", "Name");
    textInput(b, "i", "drums-name", s.name, `Section ${at + 1}`, (v) =>
      edit((x) => {
        x.sections[at].name = v.trim();
      })
    );
    b.close();
    stepper(b, "bars", "Bars", s.bars, "Bars in this section", (by) =>
      edit((x) => {
        x.sections[at].bars = Math.min(999, Math.max(1, x.sections[at].bars + by));
      })
    );
    field(
      b,
      "play",
      "Plays",
      s.play,
      PLAY_LABELS.map((x) => x.key),
      PLAY_LABELS.map((x) => x.value),
      "Groove A (verse), the bigger Groove B (chorus), hits (crash and kick on each downbeat), a count-in on the side stick, or rest",
      (v) =>
        edit((x) => {
          x.sections[at].play = v;
        })
    );
    field(
      b,
      "fill",
      "Fill",
      s.fill,
      FILL_LABELS.map((x) => x.key),
      FILL_LABELS.map((x) => x.value),
      "A fill at the end of the section, into the next one",
      (v) =>
        edit((x) => {
          x.sections[at].fill = v;
        })
    );
    b.open("div", "crash", "drums-field");
    b.leaf("span", "l", "drums-label", "Crash");
    button(b, "t", s.crash ? "small on" : "small", s.crash ? "On the 1" : "Off", "A crash cymbal on the section's first downbeat", () =>
      edit((x) => {
        x.sections[at].crash = !x.sections[at].crash;
      })
    );
    b.close();
    /** const ids: String[] */
    const ids = [""];
    /** const labels: String[] */
    const labels = ["Same groove"];
    /** const why: String[] */
    const why = [""];
    const runs = metersOf(at);
    for (const g of drums.catalog.grooves) {
      ids.push(g.id);
      labels.push(grooveLabel(g));
      why.push(g.id === s.groove ? "" : misfit(g, runs));
    }
    grooveField(b, "groove", "Groove", s.groove, ids, labels, why, "Another groove for this section only (a half-time bridge)", (v) =>
      edit((x) => {
        x.sections[at].groove = v;
      })
    );
    b.open("div", "acts", "drums-actions drums-sec-acts");
    iconButton(b, "left", "small", "left", "Move the section earlier", () => moveSection(at, -1));
    iconButton(b, "right", "small", "right", "Move the section later", () => moveSection(at, 1));
    iconButton(b, "del", "small", "trash", "Remove the section", () =>
      edit((x) => {
        x.sections.splice(at, 1);
      })
    );
    b.close();
    b.close();
  }
  b.close();
}

/** function moveSection(i: Int, by: Int) => Undefined */
function moveSection(i, by) {
  const j = i + by;
  const d = state.project.drums;
  if (j < 0 || j >= d.sections.length) return undefined;
  edit((x) => {
    const s = x.sections[i];
    x.sections[i] = x.sections[j];
    x.sections[j] = s;
  });
  drums.section = j;
}

/** Where the part starts and ends, and the Write button. */
/** function writeView(b: Builder, d: DrumPart) => Undefined */
function writeView(b, d) {
  b.open("section", "write", "drums-step drums-write");
  b.open("div", "opts", "drums-opts");
  stepper(b, "start", "Starts on bar", d.start, "The bar the first section starts on", (by) =>
    edit((x) => {
      x.start = Math.min(9999, Math.max(1, x.start + by));
    })
  );
  field(b, "ending", "Ending", d.ending, ["hit", "none"], ["Final hit", "None"], "A crash and kick on the downbeat after the last section", (v) =>
    edit((x) => {
      x.ending = v;
    })
  );
  b.open("div", "var", "drums-field");
  b.leaf("span", "l", "drums-label", "Variations");
  button(
    b,
    "t",
    d.variations ? "small on" : "small",
    d.variations ? "Every 4 bars" : "Off",
    "A small turnaround every 4th bar (an open hat or a pickup kick)",
    () =>
      edit((x) => {
        x.variations = !x.variations;
      })
  );
  b.close();
  b.open("div", "seed", "drums-field");
  b.leaf("span", "l", "drums-label", "Fills");
  button(b, "t", "small", "Other fills", "Pick other fills (and other small timing differences)", () =>
    edit((x) => {
      x.seed = x.seed + 1;
    })
  );
  b.close();
  b.close();
  const written = d.written.length;
  if (d.kept.length > 0) {
    b.open("div", "kept", "drums-kept");
    b.leaf("div", "t", "drums-label", "Edited by hand — kept as you left them");
    for (const k of d.kept) {
      const inSong = d.written.some((w) => w.slot === k.slot);
      b.open("div", k.slot, inSong ? "drums-kept-row" : "drums-kept-row gone");
      b.leaf("span", "n", "drums-kept-name", inSong ? k.name : `${k.name} (not in the song now)`);
      button(b, "reset", "small", "Reset", "Give this pattern back to the drummer: the next write makes it from the groove again", () =>
        edit((x) => {
          x.kept = x.kept.filter((e) => e.slot !== k.slot);
          forgetEdits(x, (s) => s === k.slot);
        })
      );
      b.close();
    }
    b.close();
  }
  const others = otherDrumTracks(d);
  if (others.length > 0) {
    b.leaf(
      "div",
      "others",
      "drums-status warn",
      `The song already has other drums (track ${others.join(", ")}); they play along with these. Mute or remove them if they should not.`
    );
  }
  b.leaf(
    "div",
    "status",
    "drums-status",
    (written > 0
      ? `Written: ${written} pattern${written === 1 ? "" : "s"}. Edit them in the piano roll as you like: writing again keeps your edits.`
      : "Not written yet: the song plays no drums from this part until you write it.") +
      (ownClips() > 0 ? " Where your own drum patterns play (made or taken over above), writing leaves those bars to them." : "")
  );
  button(
    b,
    "go",
    drums.busy ? "drums-writebtn" : "gold drums-writebtn",
    drums.busy ? "Working…" : written > 0 ? "Write drums again" : "Write drums",
    "Write the drum part into patterns and clips on a Drums track (Ctrl+Z undoes it)",
    () => writeDrums()
  );
  button(
    b,
    "remove",
    "small drums-remove",
    "Remove drum part",
    "Forget the drum part and go back to the grooves (the written patterns stay in the song; Ctrl+Z undoes it)",
    () => removePart()
  );
  b.close();
}

/** Clips of the song's own drum patterns (made from a recipe): the song
 * drummer leaves their bars to them. */
function ownClips() {
  const p = state.project;
  return p.playlist.clips.filter((c) => p.patterns.some((x) => x.id === c.pattern && x.drums.on)).length;
}

/** Does a channel play drums (an Ebony channel or a General MIDI kit)? */
/** function isDrumChannel(c: Channel) => Boolean */
function isDrumChannel(c) {
  if (c.instrument.type === "drum") return true;
  const prog = c.instrument.options.find((o) => o.key === "program");
  return c.instrument.type === "soundfont" && prog !== undefined && drums.catalog.kits.includes(prog.value);
}

/** Playlist tracks (counted from 1) with drum clips the part did not write. */
/** function otherDrumTracks(d: DrumPart) => Number[] */
function otherDrumTracks(d) {
  const p = state.project;
  const ours = d.written.map((w) => w.id);
  /** const out: Number[] */
  const out = [];
  for (const c of p.playlist.clips) {
    if (c.pattern === "" || ours.includes(c.pattern)) continue;
    const pat = p.patterns.find((x) => x.id === c.pattern);
    // (The song's own drum patterns replace the part where they play.)
    if (!pat || pat.notes.length === 0 || pat.drums.on) continue;
    const drumsOnly = pat.notes.every((n) => {
      const ch = p.channels.find((x) => x.id === n.channel);
      return ch !== undefined && isDrumChannel(ch);
    });
    const track = trackIndex(c.track) + 1;
    if (drumsOnly && !out.includes(track)) out.push(track);
  }
  return out.sort((a, b) => a - b);
}

// ------------------------------------------------------------------ the target

/** A drum clip of the playlist: its index and the clip. */
/** type DrumClip = { i: Int, clip: Clip } */
/** A row of a pattern's step grid: a drum (a role, `gm:<key>` or
 * `ch:<channel>:<pitch>`), its name, and the channel and key it plays. */
/** type GridRow = { role: String, label: String, channel: String, pitch: Number } */

/** General MIDI drum keys of the roles (crates/core/src/drums `gm_key`). */
const GM_KEYS = [
  { key: "kick", value: 36 },
  { key: "rim", value: 37 },
  { key: "snare", value: 38 },
  { key: "clap", value: 39 },
  { key: "hat", value: 42 },
  { key: "pedal", value: 44 },
  { key: "openhat", value: 46 },
  { key: "crash", value: 49 },
  { key: "ride", value: 51 },
  { key: "bell", value: 53 },
  { key: "tom1", value: 48 },
  { key: "tom2", value: 45 },
  { key: "tom3", value: 43 },
  { key: "tamb", value: 54 },
  { key: "cowbell", value: 56 },
  { key: "shaker", value: 70 },
];
/** The Ebony Drum Machine sound (`kind`) and key of the roles (`ebony`). */
const EBONY_KEYS = [
  { role: "kick", kind: "kick", pitch: 60 },
  { role: "snare", kind: "snare", pitch: 60 },
  { role: "rim", kind: "rim", pitch: 60 },
  { role: "clap", kind: "clap", pitch: 60 },
  { role: "hat", kind: "hat", pitch: 60 },
  { role: "openhat", kind: "openhat", pitch: 60 },
  { role: "tom1", kind: "tom", pitch: 65 },
  { role: "tom2", kind: "tom", pitch: 60 },
  { role: "tom3", kind: "tom", pitch: 55 },
  { role: "cowbell", kind: "cowbell", pitch: 60 },
  { role: "shaker", kind: "shaker", pitch: 60 },
];
const PLAY_EDIT = [
  { key: "a", value: "A · verse" },
  { key: "b", value: "B · chorus" },
  { key: "hits", value: "Hits" },
  { key: "count", value: "Count-in" },
  { key: "rest", value: "Rest" },
];
const FILL_EDIT = [
  { key: "none", value: "None" },
  { key: "beat", value: "1 beat" },
  { key: "half", value: "2 beats" },
  { key: "bar", value: "1 bar" },
];

/** function gmKey(role: String) => Number */
function gmKey(role) {
  const e = GM_KEYS.find((x) => x.key === role);
  return e ? e.value : -1;
}

/** The role nearest to a General MIDI drum key (`gm_role`). */
/** function gmRole(key: Number) => String */
function gmRole(key) {
  if (key === 35 || key === 36) return "kick";
  if (key === 37) return "rim";
  if (key === 38 || key === 40) return "snare";
  if (key === 39) return "clap";
  if (key === 42) return "hat";
  if (key === 44) return "pedal";
  if (key === 46) return "openhat";
  if (key === 49 || key === 52 || key === 55 || key === 57) return "crash";
  if (key === 51 || key === 59) return "ride";
  if (key === 53) return "bell";
  if (key === 48 || key === 50) return "tom1";
  if (key === 45 || key === 47) return "tom2";
  if (key === 41 || key === 43) return "tom3";
  if (key === 54) return "tamb";
  if (key === 56) return "cowbell";
  if (key === 69 || key === 70 || key === 82) return "shaker";
  return "";
}

/** function isKitChannel(c: Channel) => Boolean */
function isKitChannel(c) {
  const prog = c.instrument.options.find((o) => o.key === "program");
  return c.instrument.type === "soundfont" && prog !== undefined && drums.catalog.kits.includes(prog.value);
}

/** The drum a note plays, as a grid row. */
/** function noteRow(n: Note) => GridRow */
function noteRow(n) {
  const c = state.project.channels.find((x) => x.id === n.channel);
  const own = { role: `ch:${n.channel}:${n.pitch}`, label: `${c ? c.name : n.channel} ${noteName(n.pitch)}`, channel: n.channel, pitch: n.pitch };
  if (!c) return own;
  if (isKitChannel(c)) {
    const r = gmRole(n.pitch);
    if (r !== "" && gmKey(r) === n.pitch) return { role: r, label: labelOf(ROLE_LABELS, r), channel: n.channel, pitch: n.pitch };
    const name = drumName(c, n.pitch);
    return { role: `gm:${n.pitch}`, label: name !== "" ? name : own.label, channel: n.channel, pitch: n.pitch };
  }
  if (c.instrument.type === "drum") {
    const kind = optionValue(c.instrument, "kind") || "kick";
    const lower = c.name.toLowerCase();
    let r = kind;
    if (kind === "tom") r = n.pitch >= 63 ? "tom1" : n.pitch >= 58 ? "tom2" : "tom3";
    else if (kind === "openhat" && lower.includes("crash")) r = "crash";
    else if (kind === "openhat" && lower.includes("ride")) r = n.pitch > 63 ? "bell" : "ride";
    if (ROLE_LABELS.some((x) => x.key === r)) return { role: `${r}@${n.channel}`, label: labelOf(ROLE_LABELS, r), channel: n.channel, pitch: n.pitch };
  }
  return own;
}

/** Does a pattern hold drums: made from a groove, written by the song's
 * drummer, or notes only on drum channels? */
/** function isDrumPattern(pat: Pattern) => Boolean */
function isDrumPattern(pat) {
  if (pat.drums.on) return true;
  if (state.project.drums.written.some((w) => w.id === pat.id)) return true;
  const p = state.project;
  return (
    pat.notes.length > 0 &&
    pat.notes.every((n) => {
      const c = p.channels.find((x) => x.id === n.channel);
      return c !== undefined && isDrumChannel(c);
    })
  );
}

/** function patternOf(id: String) => Pattern? */
function patternOf(id) {
  return state.project.patterns.find((x) => x.id === id);
}

/** The playlist's drum clips. */
/** function drumClips() => DrumClip[] */
function drumClips() {
  /** const out: DrumClip[] */
  const out = [];
  const clips = state.project.playlist.clips;
  for (let i = 0; i < clips.length; i++) {
    const pat = clips[i].pattern === "" ? undefined : patternOf(clips[i].pattern);
    if (pat && isDrumPattern(pat)) out.push({ i: i, clip: clips[i] });
  }
  return out;
}

/** The drum clip playing at bar `bar` (from 0), if any. */
/** function drumClipAt(bar: Number) => DrumClip? */
function drumClipAt(bar) {
  const at = barStart(bar) + 1e-6;
  return drumClips().find((d) => d.clip.start <= at && at < d.clip.start + d.clip.length);
}

/** The track drums go on (counted from 0): the one with drum clips, else
 * one named Drums, else -1 (a new one). */
function drumTrack() {
  const dc = drumClips();
  if (dc.length > 0) return trackIndex(dc[0].clip.track);
  return state.project.playlist.tracks.findIndex((t) => t.name.toLowerCase() === "drums");
}

/** The pattern the tab works on (none when it is gone). */
/** function targetPattern() => Pattern? */
function targetPattern() {
  return drums.target === "" ? undefined : patternOf(drums.target);
}

/** The bars (from 0) where the target pattern's clips start. */
/** function targetSpans(id: String) => { from: Number, to: Number }[] */
function targetSpans(id) {
  const t = state.project.transport;
  return state.project.playlist.clips
    .filter((c) => c.pattern === id)
    .sort((a, b) => a.start - b.start)
    .map((c) => ({ from: barAt(t, c.start).bar, to: barAt(t, Math.max(c.start, c.start + c.length - 1e-6)).bar }));
}

/** The time signature the target plays in: at its clip under the cursor
 * (else its first), else where the producer points. */
/** function targetMeter() => BarRun */
function targetMeter() {
  const here = pointedBar().bar;
  const spans = drums.target === "" ? [] : targetSpans(drums.target);
  const at = spans.find((x) => x.from <= here && here <= x.to) ?? spans[0];
  return meterAt(at ? at.from : here);
}

/** Bars a groove's parts run before they repeat. */
/** function grooveBars(g: GrooveInfo) => Int */
function grooveBars(g) {
  let n = 1;
  for (const r of g.a.concat(g.b)) n = Math.max(n, Math.round(r[1].replace(/[ |]/g, "").length / g.steps));
  return n;
}

/** The kit new drum patterns play on: the last one made's, else the song drummer's. */
function defaultKit() {
  const made = state.project.patterns.filter((x) => x.drums.on && x.drums.kit !== "");
  if (made.length > 0) return made[made.length - 1].drums.kit;
  return state.project.drums.on ? state.project.drums.kit : "";
}

/** Follow the producer around the song: when the cursor moves (while
 * stopped) or a clip is selected, the tab targets the drum pattern playing
 * there (none: it offers to make one) and selects the song drummer's section. */
function followFocus() {
  const d = state.project.drums;
  const p = pointedBar();
  if (p.key === drums.focus) {
    // Nothing targeted, but drums play here now (made elsewhere, or back by
    // an undo): target them.
    if (drums.target === "") {
      const at = drumClipAt(p.bar);
      if (at) drums.target = at.clip.pattern;
    }
    return undefined;
  }
  // While playing, the cursor runs on its own: only a clip selection counts.
  if (state.playing && !p.key.startsWith("clip")) return undefined;
  drums.focus = p.key;
  const i = d.on ? sectionAt(d, p.bar) : -1;
  if (i >= 0) drums.section = i;
  const clips = state.project.playlist.clips;
  const sel = state.clipSelection.length > 0 ? clipIndex(state.clipSelection[0]) : -1;
  const selPat = sel >= 0 && sel < clips.length ? patternOf(clips[sel].pattern) : undefined;
  if (selPat && isDrumPattern(selPat)) {
    drums.target = selPat.id;
    return undefined;
  }
  const at = drumClipAt(p.bar);
  drums.target = at ? at.clip.pattern : "";
}

/** Point the tab at the drum clip `i` of the playlist (and select it there). */
/** function targetClip(i: Int) => Undefined */
function targetClip(i) {
  const c = state.project.playlist.clips[i];
  if (!c) return undefined;
  state.clipSelection = [clipIx(i)];
  drums.target = c.pattern;
  drums.focus = pointedBar().key;
  const d = state.project.drums;
  const s = d.on ? sectionAt(d, barAt(state.project.transport, c.start).bar) : -1;
  if (s >= 0) drums.section = s;
  invalidate();
}

// ------------------------------------------------------------------ making patterns

/** Change a copy of the song with `change` (it returns the id of the drum
 * pattern to make, "" for none), make that pattern's notes from its recipe
 * (POST /api/drums/pattern, the same code the CLI and the browser-only studio
 * run), and take the result as one undo step; then `done(id)`. */
/** function remake(change: (Project) => String, done: (String) => Undefined) => Undefined */
function remake(change, done) {
  const copy = cloneProject(state.project);
  const id = change(copy);
  if (id === "") return undefined;
  drums.pending = drums.pending + 1;
  invalidate();
  const edits = state.edits;
  sendJson(`/api/drums/pattern?id=${encodeURIComponent(id)}`, "POST", encodeProject(copy))
    .then((r) => {
      drums.pending = drums.pending - 1;
      if (state.edits !== edits) {
        // The song changed meanwhile: make it again on the song as it is now.
        remake(change, done);
        return false;
      }
      const np = decodeProject(r.project);
      commit(() => {
        const p = state.project;
        p.channels = np.channels;
        p.patterns = np.patterns;
        p.playlist = np.playlist;
        p.score = np.score;
        p.drums = np.drums;
      });
      done(id);
      return true;
    })
    .catch((e) => {
      drums.pending = drums.pending - 1;
      toast("Could not make the drum pattern", errText(e), "error");
      invalidate();
      return false;
    });
}

/** Clear room for drums from `start` to `end` (beats) on `track`: other drum
 * clips there are shortened, split or removed. */
/** function clearDrums(p: Project, track: Int, start: Number, end: Number) => Undefined */
function clearDrums(p, track, start, end) {
  /** const keep: Clip[] */
  const keep = [];
  for (const c of p.playlist.clips) {
    const pat = c.pattern === "" ? undefined : p.patterns.find((x) => x.id === c.pattern);
    const drum = pat !== undefined && (pat.drums.on || p.drums.written.some((w) => w.id === pat.id) || isDrumPattern(pat));
    const cs = c.start;
    const ce = c.start + c.length;
    if (!drum || trackIndex(c.track) !== track || ce <= start + 1e-6 || cs >= end - 1e-6) {
      keep.push(c);
      continue;
    }
    if (cs < start - 1e-6)
      keep.push({ pattern: c.pattern, sample: c.sample, track: c.track, start: cs, length: start - cs, offset: c.offset, gain: c.gain, mixer: c.mixer });
    if (ce > end + 1e-6) {
      keep.push({
        pattern: c.pattern,
        sample: c.sample,
        track: c.track,
        start: end,
        length: ce - end,
        offset: c.offset + (end - cs),
        gain: c.gain,
        mixer: c.mixer,
      });
    }
  }
  p.playlist.clips = keep;
}

/** How many bars a new drum pattern plays from bar `bar`: the selected
 * clip's bars, else 4, up to the next drum clip. */
/** function newSpan(bar: Number) => Number */
function newSpan(bar) {
  const t = state.project.transport;
  const clips = state.project.playlist.clips;
  let span = 4;
  const sel = state.clipSelection.length > 0 ? clipIndex(state.clipSelection[0]) : -1;
  if (sel >= 0 && sel < clips.length && barAt(t, clips[sel].start).bar === bar) {
    span = Math.max(1, Math.round(clips[sel].length / meterAt(bar).barBeats));
  }
  const start = barStart(bar);
  const track = drumTrack();
  for (const d of drumClips()) {
    if (trackIndex(d.clip.track) === track && d.clip.start > start + 1e-6) {
      span = Math.min(span, Math.max(1, barAt(t, d.clip.start - 1e-6).bar - bar + 1));
    }
  }
  return Math.max(1, span);
}

/** Make a new drum pattern on groove `gid` and place it at bar `bar` (from
 * 0) on the drums track (made if there is none): the tab then targets it. */
/** function newPattern(gid: String, bar: Number) => Undefined */
function newPattern(gid, bar) {
  const g = grooveOf(gid);
  if (!g) return undefined;
  const run = meterAt(bar);
  if (!fits(g, run)) {
    toast("That groove does not fit here", `${g.name} is in ${g.meter}, but bar ${bar + 1} is in ${run.label}.`, "error");
    return undefined;
  }
  const span = newSpan(bar);
  const gb = grooveBars(g);
  const bars = Math.max(gb, Math.round(Math.min(4, span) / gb) * gb);
  const kit = defaultKit();
  remake(
    (p) => {
      const id = uniqueId(
        `drums-${g.name}`,
        p.patterns.map((x) => x.id)
      );
      let n = 1;
      while (p.patterns.some((x) => x.name === `Drums · ${g.name}${n > 1 ? ` ${n}` : ""}`)) n = n + 1;
      p.patterns.push({
        id: id,
        name: `Drums · ${g.name}${n > 1 ? ` ${n}` : ""}`,
        color: "#8e3b46",
        length: bars * g.barBeats,
        notes: [],
        drums: {
          on: true,
          groove: g.id,
          play: "a",
          fill: "none",
          crash: false,
          turnaround: false,
          kit: kit,
          feel: "natural",
          swing: g.swing,
          seed: 1,
          edited: false,
        },
      });
      let track = drumTrack();
      if (track < 0) {
        p.playlist.tracks.push({ name: "Drums", mute: false });
        track = p.playlist.tracks.length - 1;
      }
      const start = barStart(bar);
      const end = start + span * g.barBeats;
      clearDrums(p, track, start, end);
      p.playlist.clips.push({ pattern: id, sample: "", track: trackIx(track), start: start, length: end - start, offset: 0, gain: 1, mixer: insertIx(0) });
      return id;
    },
    (id) => {
      drums.target = id;
      drums.focus = pointedBar().key;
      const name = patternOf(id)?.name ?? id;
      toast(
        "Drum pattern made",
        `${name} plays bars ${bar + 1}–${bar + span} on the ${state.project.playlist.tracks[drumTrack()]?.name ?? "Drums"} track. Ctrl+Z undoes it.`,
        "info"
      );
    }
  );
}

/** Make the target pattern again with its recipe changed by `fn` (a pattern
 * without one — written by the song drummer, or imported — gets one, and
 * leaves the song drummer: writing the whole song again leaves the bars it
 * plays to it). */
/** function changeTarget(fn: (PatternDrums, Pattern, Project) => Undefined) => Undefined */
function changeTarget(fn) {
  const id = drums.target;
  remake(
    (p) => {
      const pat = p.patterns.find((x) => x.id === id);
      if (!pat) return "";
      if (!pat.drums.on) {
        pat.drums = {
          on: true,
          groove: "",
          play: "a",
          fill: "none",
          crash: false,
          turnaround: false,
          kit: patternKit(pat),
          feel: "natural",
          swing: 0,
          seed: 1,
          edited: false,
        };
        p.drums.written = p.drums.written.filter((w) => w.id !== id);
      }
      fn(pat.drums, pat, p);
      return pat.drums.groove === "" ? "" : id;
    },
    (x) => undefined
  );
}

/** The kit a pattern's notes play on ("" when they are on no General MIDI kit). */
/** function patternKit(pat: Pattern) => String */
function patternKit(pat) {
  for (const n of pat.notes) {
    const c = state.project.channels.find((x) => x.id === n.channel);
    if (c && isKitChannel(c)) return optionValue(c.instrument, "program");
    if (c && c.instrument.type === "drum") return "Ebony";
  }
  return defaultKit();
}

/** Play groove `gid` on the target (or, with none, where a new pattern would go). */
/** function setTargetGroove(gid: String) => Undefined */
function setTargetGroove(gid) {
  const g = grooveOf(gid);
  if (!g) return undefined;
  const pat = targetPattern();
  if (!pat) {
    newPattern(gid, pointedBar().bar);
    return undefined;
  }
  changeTarget((r, pt) => {
    if (r.groove !== gid) r.swing = g.swing;
    r.groove = gid;
    // Whole bars of the new groove.
    const gb = grooveBars(g);
    pt.length = Math.max(gb, Math.round(pt.length / g.barBeats / gb) * gb) * g.barBeats;
  });
}

/** The next (or previous) groove that fits the target's time signature. */
/** function stepTargetGroove(by: Int) => Undefined */
function stepTargetGroove(by) {
  const gs = drums.catalog.grooves;
  const pat = targetPattern();
  if (gs.length === 0 || !pat) return undefined;
  const run = targetMeter();
  let i = gs.findIndex((g) => g.id === pat.drums.groove);
  for (let n = 0; n < gs.length; n++) {
    i = (i + by + gs.length) % gs.length;
    if (fits(gs[i], run)) {
      setTargetGroove(gs[i].id);
      return undefined;
    }
  }
}

/** Lengthen or shorten the clip of the target at the cursor (else its
 * first), by whole bars; drum clips after it make room. */
/** function stretchClip(by: Int) => Undefined */
function stretchClip(by) {
  const p = state.project;
  const c = cursorClip(drums.target);
  if (!c) return undefined;
  const t = p.transport;
  const first = barAt(t, c.start).bar;
  const bars = Math.max(1, Math.round(c.length / meterAt(first).barBeats) + by);
  const end = barStart(first + bars);
  commit(() => {
    if (end > c.start + c.length) clearDrums(p, trackIndex(c.track), c.start + c.length, end);
    c.length = end - c.start;
  });
}

/** The target's clip at the cursor (else its first). */
/** function cursorClip(id: String) => Clip? */
function cursorClip(id) {
  const at = barStart(pointedBar().bar) + 1e-6;
  const mine = state.project.playlist.clips.filter((c) => c.pattern === id).sort((a, b) => a.start - b.start);
  return mine.find((x) => x.start <= at && at < x.start + x.length) ?? mine[0];
}

/** How many bars that clip plays. */
/** function clipBars(id: String) => Number */
function clipBars(id) {
  const c = cursorClip(id);
  if (!c) return 0;
  return Math.max(1, Math.round(c.length / meterAt(barAt(state.project.transport, c.start).bar).barBeats));
}

/** Place the target at bar `bar` (it is in no clip yet, or once more). */
/** function placeTarget(bar: Number) => Undefined */
function placeTarget(bar) {
  const pat = targetPattern();
  if (!pat) return undefined;
  const p = state.project;
  commit(() => {
    let track = drumTrack();
    if (track < 0) {
      p.playlist.tracks.push({ name: "Drums", mute: false });
      track = p.playlist.tracks.length - 1;
    }
    const start = barStart(bar);
    clearDrums(p, track, start, start + pat.length);
    p.playlist.clips.push({ pattern: pat.id, sample: "", track: trackIx(track), start: start, length: pat.length, offset: 0, gain: 1, mixer: insertIx(0) });
  });
}

/** A copy of the target that plays where the cursor is instead of it: a
 * variation to change on its own. */
function duplicateTarget() {
  const pat = targetPattern();
  if (!pat) return undefined;
  const p = state.project;
  const id = uniqueId(
    `${pat.id}-2`,
    p.patterns.map((x) => x.id)
  );
  let n = 2;
  while (p.patterns.some((x) => x.name === `${pat.name} ${n}`)) n = n + 1;
  const at = barStart(pointedBar().bar) + 1e-6;
  commit(() => {
    p.patterns.push({
      id: id,
      name: `${pat.name} ${n}`,
      color: pat.color,
      length: pat.length,
      notes: pat.notes.map((x) => ({ channel: x.channel, pitch: x.pitch, start: x.start, length: x.length, velocity: x.velocity })),
      drums: {
        on: pat.drums.on,
        groove: pat.drums.groove,
        play: pat.drums.play,
        fill: pat.drums.fill,
        crash: pat.drums.crash,
        turnaround: pat.drums.turnaround,
        kit: pat.drums.kit,
        feel: pat.drums.feel,
        swing: pat.drums.swing,
        seed: pat.drums.seed,
        edited: pat.drums.edited,
      },
    });
    const c = p.playlist.clips.find((x) => x.pattern === pat.id && x.start <= at && at < x.start + x.length);
    if (c) c.pattern = id;
  });
  drums.target = id;
  toast("Pattern copied", `${pat.name} ${n} plays here now; change it without changing the others.`, "info");
}

/** Remove the target pattern and its clips. */
function removeTarget() {
  const pat = targetPattern();
  if (!pat) return undefined;
  const p = state.project;
  commit(() => {
    p.playlist.clips = p.playlist.clips.filter((c) => c.pattern !== pat.id);
    p.patterns = p.patterns.filter((x) => x.id !== pat.id);
    p.drums.written = p.drums.written.filter((w) => w.id !== pat.id);
  });
  state.clipSelection = [];
  drums.target = "";
  toast("Drum pattern removed", `${pat.name} and its clips are gone. Ctrl+Z brings them back.`, "info");
}

/** Play the target on its own, looping (again: stop). */
function loopTarget() {
  const pat = targetPattern();
  if (!pat) return undefined;
  if (state.playing && state.mode === "pattern" && state.pattern === pat.id) {
    stop();
    // Back to the song, where its cursor was.
    if (drums.looped) {
      drums.looped = false;
      setMode("song");
      seek(drums.songAt);
    }
    return undefined;
  }
  stopTrying();
  drums.looped = state.mode === "song";
  selectPattern(pat.id);
  setMode("pattern");
  play();
}

// ------------------------------------------------------------------ trying grooves

/** Hear groove `gid` on the target (or a bar of it where a new one would
 * go), looping, before choosing it: the song is not changed. Again: stop. */
/** function tryGroove(gid: String) => Undefined */
function tryGroove(gid) {
  if (drums.trying === gid) {
    stopTrying();
    return undefined;
  }
  const g = grooveOf(gid);
  if (!g) return undefined;
  stopPreview();
  drums.trying = gid;
  drums.trySeq = drums.trySeq + 1;
  const seq = drums.trySeq;
  const copy = cloneProject(state.project);
  const pat = targetPattern();
  const id = pat ? pat.id : "drums-try";
  if (pat) {
    const pt = copy.patterns.find((x) => x.id === pat.id);
    if (pt) {
      if (!pt.drums.on)
        pt.drums = {
          on: true,
          groove: gid,
          play: "a",
          fill: "none",
          crash: false,
          turnaround: false,
          kit: patternKit(pt),
          feel: "natural",
          swing: g.swing,
          seed: 1,
          edited: false,
        };
      pt.drums.groove = gid;
      pt.drums.swing = g.swing;
      pt.length = Math.max(1, Math.round(pt.length / g.barBeats)) * g.barBeats;
    }
  } else {
    copy.patterns.push({
      id: id,
      name: "Drums · trying",
      color: "#8e3b46",
      length: grooveBars(g) * g.barBeats * (grooveBars(g) === 1 ? 2 : 1),
      notes: [],
      drums: {
        on: true,
        groove: gid,
        play: "a",
        fill: "none",
        crash: false,
        turnaround: false,
        kit: defaultKit(),
        feel: "natural",
        swing: g.swing,
        seed: 1,
        edited: false,
      },
    });
  }
  invalidate();
  startAudio()
    .then((ok) => sendJson(`/api/drums/pattern?id=${encodeURIComponent(id)}`, "POST", encodeProject(copy)))
    .then((r) => {
      if (seq !== drums.trySeq || drums.trying !== gid) return false;
      // Changes to the song meanwhile do not replace what is trying.
      hooks.preview = stopTrying;
      hooks.previewing = true;
      audioPost({ t: "project", json: JSON.stringify(r.project) });
      audioPost({ t: "mode", pattern: id });
      audioPost({ t: "seek", beat: 0 });
      audioPost({ t: "play" });
      drums.tryAt = now();
      tryTick(seq);
      return true;
    })
    .catch((e) => {
      if (seq === drums.trySeq) stopTrying();
      toast("Could not play the groove", errText(e), "error");
      return false;
    });
}

/** Stop trying a groove and give the engine the song back. */
export function stopTrying() {
  if (drums.trying === "") return undefined;
  drums.trying = "";
  drums.trySeq = drums.trySeq + 1;
  hooks.previewing = false;
  audioPost({ t: "stop" });
  audioPost({ t: "project", json: engineJson() });
  const pat = currentPattern();
  audioPost({ t: "mode", pattern: state.mode === "pattern" && pat ? pat.id : "" });
  // Back to where the song cursor was.
  if (state.mode === "song") seek(drums.songAt);
  invalidate();
}

/** While trying: notice the transport stopping it. */
/** function tryTick(seq: Int) => Undefined */
function tryTick(seq) {
  if (seq !== drums.trySeq || drums.trying === "") return undefined;
  if (state.output === "browser" && !state.playing && now() - drums.tryAt > 800) {
    stopTrying();
    return undefined;
  }
  setTimeout(() => tryTick(seq), 200);
}

// ------------------------------------------------------------------ views: target

/** Bars as text: "9–16, 25–32 and 2 more". */
/** function spansText(spans: { from: Number, to: Number }[]) => String */
function spansText(spans) {
  const shown = spans.slice(0, 3).map((s) => (s.from === s.to ? `${s.from + 1}` : `${s.from + 1}–${s.to + 1}`));
  const more = spans.length > 3 ? ` and ${spans.length - 3} more` : "";
  return `${spans.length === 1 && spans[0].from === spans[0].to ? "bar" : "bars"} ${shown.join(", ")}${more}`;
}

/** What the tab works on, and where it plays: always on top. */
/** function targetView(b: Builder) => Undefined */
function targetView(b) {
  const pat = targetPattern();
  const here = pointedBar().bar;
  const run = targetMeter();
  const mine = drumClips()
    .map((d) => patternOf(d.clip.pattern))
    .filter((x) => x !== undefined);
  /** const ids: String[] */
  const ids = [];
  for (const x of state.project.patterns) if (isDrumPattern(x)) ids.push(x.id);
  if (pat && !ids.includes(pat.id)) ids.push(pat.id);
  b.open("section", "target", "drums-target");
  b.leaf("span", "l", "drums-label", "Drums");
  b.open("label", "pick", "drums-target-pick");
  b.attr("title", "The drum pattern this tab edits: the one at the song cursor or the selected clip, or pick one");
  b.open("select", "s", "select");
  b.prop("value", pat ? pat.id : "");
  b.on("change", (e) => {
    drums.target = e.value;
    invalidate();
  });
  b.leaf("option", "-", "", `No drums at bar ${here + 1}`);
  b.attr("value", "");
  for (const id of ids) {
    const x = patternOf(id);
    b.leaf("option", id, "", x ? x.name : id);
    b.attr("value", id);
  }
  b.close();
  b.close();
  if (pat) {
    const spans = targetSpans(pat.id);
    const track = state.project.playlist.clips.find((c) => c.pattern === pat.id);
    const tname = track ? (state.project.playlist.tracks[trackIndex(track.track)]?.name ?? "") : "";
    b.open("span", "where", spans.length > 0 ? "drums-where" : "drums-where none");
    b.text(
      spans.length > 0
        ? `→ ${tname !== "" ? `${tname} track` : `track ${trackIndex(track?.track ?? trackIx(0)) + 1}`}, ${spansText(spans)}`
        : "→ not in the song yet"
    );
    b.close();
    b.leaf("span", "meter", "drums-badge", run.label);
    const kitName = pat.drums.on
      ? pat.drums.kit !== ""
        ? pat.drums.kit
        : `${grooveOf(pat.drums.groove)?.kit ?? "Standard Kit"} (suggested)`
      : patternKit(pat);
    b.leaf("span", "kit", "drums-badge", kitName === "Ebony" ? "Ebony drum machine" : kitName);
    b.open("div", "acts", "drums-actions");
    if (spans.length > 0) {
      b.open("div", "len", "drums-stepper");
      b.attr("title", "How many bars the clip at the cursor plays (the pattern loops)");
      iconButton(b, "dn", "small", "minus", "The clip at the cursor plays a bar less", () => stretchClip(-1));
      b.leaf("span", "v", "drums-num", `${clipBars(pat.id)} bars`);
      iconButton(b, "up", "small", "plus", "The clip at the cursor plays a bar more (drum clips after it make room)", () => stretchClip(1));
      b.close();
    } else {
      button(b, "place", "small", `Place at bar ${here + 1}`, "Put this pattern in the song at the song cursor", () => placeTarget(here));
    }
    iconButton(
      b,
      "loop",
      state.playing && state.mode === "pattern" && state.pattern === pat.id ? "small on" : "small",
      state.playing && state.mode === "pattern" && state.pattern === pat.id ? "stop" : "play",
      "Play this pattern on its own, looping",
      () => loopTarget()
    );
    iconButton(b, "dup", "small", "copy", "Copy it to play here instead: a variation to change on its own", () => duplicateTarget());
    iconButton(b, "del", "small", "trash", "Remove this pattern and its clips", () => removeTarget());
    b.close();
  } else {
    b.leaf("span", "where", "drums-where none", `Nothing plays drums at bar ${here + 1}. Pick a groove to make a pattern here.`);
    b.leaf("span", "meter", "drums-badge", run.label);
    b.open("div", "acts", "drums-actions");
    const first = drums.catalog.grooves.find((g) => fits(g, run));
    button(
      b,
      "new",
      "small gold",
      `+ New pattern at bar ${here + 1}`,
      `Make a drum pattern at the song cursor${first ? ` on ${first.name} (pick another groove after)` : ""}`,
      () => {
        if (first) newPattern(first.id, here);
      }
    );
    b.close();
  }
  if (pat) crossingView(b, pat);
  if (drums.pending > 0) b.leaf("span", "busy", "drums-busy", "Making…");
  b.close();
}

/** The time signature changes inside the target's clip at the cursor:
 * where it changes before the cursor, and to what ("" none). */
/** function crossing(pat: Pattern) => { bar: Number, label: String } */
function crossing(pat) {
  const c = cursorClip(pat.id);
  const here = pointedBar().bar;
  const t = state.project.transport;
  if (!c) return { bar: -1, label: "" };
  const first = barAt(t, c.start).bar;
  const last = barAt(t, c.start + c.length - 1e-6).bar;
  if (here < first || here > last) return { bar: -1, label: "" };
  const began = meterAt(first);
  const now = meterAt(here);
  if (Math.abs(began.barBeats - now.barBeats) < 1e-6) return { bar: -1, label: "" };
  // The bar where the time signature at the cursor starts.
  let start = here;
  while (start > first && Math.abs(meterAt(start - 1).barBeats - now.barBeats) < 1e-6) start = start - 1;
  return { bar: start, label: now.label };
}

/** A clip that runs on into another time signature: say so, and offer to
 * end it there (the cursor's bars then take a pattern of their own). */
/** function crossingView(b: Builder, pat: Pattern) => Undefined */
function crossingView(b, pat) {
  const x = crossing(pat);
  if (x.bar < 0) return undefined;
  const c = cursorClip(pat.id);
  if (!c) return undefined;
  b.open("div", "cross", "drums-cross");
  b.leaf("span", "t", "drums-status warn", `This clip runs on into ${x.label} at bar ${x.bar + 1}, but ${pat.name} is in ${targetMeter().label}.`);
  button(
    b,
    "cut",
    "small gold",
    `End it at bar ${x.bar + 1}`,
    `Shorten the clip to end where ${x.label} starts; then pick a ${x.label} groove for the bars from ${x.bar + 1}`,
    () => {
      const at = barStart(x.bar);
      commit(() => {
        clearDrums(state.project, trackIndex(c.track), at, c.start + c.length);
      });
      // The cursor goes to where the new time signature starts: a groove
      // picked now makes the pattern for those bars.
      state.clipSelection = [];
      if (state.mode === "song") seek(at);
      drums.target = "";
    }
  );
  b.close();
}

// ------------------------------------------------------------------ views: grooves

/** The groove library: search, the grooves of the target's time signature
 * first; a click plays one on the target (or makes one), ▶ tries it first. */
/** function libraryView(b: Builder) => Undefined */
function libraryView(b) {
  const run = targetMeter();
  const pat = targetPattern();
  const current = pat && pat.drums.on ? pat.drums.groove : "";
  const words = drums.query
    .toLowerCase()
    .split(" ")
    .filter((w) => w !== "");
  const gs = drums.catalog.grooves.filter((g) => words.every((w) => `${g.style} ${g.name} ${g.meter}`.toLowerCase().includes(w)));
  b.open("section", "lib", "drums-lib");
  b.open("div", "search", "drums-search");
  glyph(b, "search");
  b.leaf("input", "in", "text-input", "");
  b.attr("placeholder", `Grooves in ${run.label}…`);
  b.attr("spellcheck", "false");
  b.attr("aria-label", "Search the grooves");
  b.prop("value", drums.query);
  b.on("input", (e) => {
    drums.query = e.value;
    invalidate();
  });
  b.on("keydown", (e) => {
    if (e.key === "Escape") {
      drums.query = "";
      invalidate();
    }
  });
  b.close();
  b.open("div", "list", "drums-lib-list");
  if (drums.catalog.grooves.length === 0) b.leaf("div", "wait", "drums-sub", "Loading the grooves…");
  // The grooves that fit first, by style; the others after, greyed out.
  for (const fit of [true, false]) {
    const some = gs.filter((g) => fits(g, run) === fit);
    if (some.length === 0) continue;
    if (!fit) b.leaf("div", "other", "drums-lib-other", `Not in ${run.label}`);
    /** const styles: String[] */
    const styles = [];
    for (const g of some) if (!styles.includes(g.style)) styles.push(g.style);
    for (const st of styles) {
      b.leaf("div", `st-${fit}-${st}`, "drums-lib-style", st);
      for (const g of some.filter((x) => x.style === st)) {
        let cls = "drums-lib-item";
        if (g.id === current) cls = `${cls} on`;
        if (!fit) cls = `${cls} misfit`;
        if (drums.trying === g.id) cls = `${cls} trying`;
        b.open("div", g.id, cls);
        const tip = fit
          ? `${g.name}: ${g.meter}, ${g.tempo[0]}–${g.tempo[1]} BPM, suggests ${g.kit === "Ebony" ? "the drum machine" : g.kit}. Click: ${pat ? `play it in ${pat.name}` : "make a drum pattern here"}`
          : `${g.name} is in ${g.meter}, but ${pat ? pat.name : "this bar"} is in ${run.label}`;
        b.attr("title", tip);
        b.on("pointerenter", (e) => hint(tip));
        b.on("click", (e) => {
          if (fit) setTargetGroove(g.id);
        });
        b.open("button", "try", drums.trying === g.id ? "btn icon small ghost on" : "btn icon small ghost");
        const tryTip = drums.trying === g.id ? "Stop" : `Hear ${g.name} first (the song is not changed)`;
        b.attr("title", tryTip);
        b.attr("aria-label", tryTip);
        b.on("click", (e) => {
          e.stopPropagation();
          if (fit) tryGroove(g.id);
        });
        glyph(b, drums.trying === g.id ? "stop" : "play");
        b.close();
        b.leaf("span", "n", "drums-lib-name", g.name);
        b.leaf("span", "m", "drums-lib-meta", `${g.meter} · ${g.tempo[0]}–${g.tempo[1]}`);
        b.close();
      }
    }
  }
  if (gs.length === 0 && drums.catalog.grooves.length > 0) b.leaf("div", "none", "drums-sub", `No groove matches “${drums.query.trim()}”`);
  b.close();
  b.close();
}

// ------------------------------------------------------------------ views: the pattern

/** A row of buttons, one lit. */
/** function segmented(b: Builder, key: String, label: String, value: String, choices: KS[], tip: String, onSet: (String) => Undefined) => Undefined */
function segmented(b, key, label, value, choices, tip, onSet) {
  b.open("div", key, "drums-field");
  b.leaf("span", "l", "drums-label", label);
  b.open("div", "seg", "drums-seg");
  b.attr("title", tip);
  for (const c of choices) {
    b.open("button", c.key, c.key === value ? "on" : "");
    b.on("click", (e) => {
      if (c.key !== value) onSet(c.key);
    });
    b.text(c.value);
    b.close();
  }
  b.close();
  b.close();
}

/** The target's recipe and its notes as a step grid. */
/** function patternView(b: Builder) => Undefined */
function patternView(b) {
  const pat = targetPattern();
  b.open("section", "pattern", "drums-pattern");
  if (!pat) {
    b.leaf("div", "t", "drums-title", "A drummer for the song");
    b.leaf(
      "div",
      "sub",
      "drums-sub",
      "Put the song cursor where the drums should play (or select a clip in the playlist), then pick a groove on the left: it becomes a drum pattern there, on a Drums track. Each pattern plays A or B, a fill, a crash; copy one to vary it. To give the whole song drums in one go, open the song drummer below."
    );
    b.close();
    return undefined;
  }
  const r = pat.drums;
  const g = r.on ? grooveOf(r.groove) : undefined;
  if (r.on && g) {
    b.open("div", "head", "drums-pattern-head");
    b.open("div", "groove", "drums-pattern-groove");
    b.leaf("span", "n", "drums-title", g.name);
    const bpm = state.project.transport.bpm;
    const fitsTempo = bpm >= g.tempo[0] && bpm <= g.tempo[1];
    b.leaf(
      "span",
      "i",
      fitsTempo ? "drums-info" : "drums-info warn",
      `${g.style} · ${g.meter} · ${g.tempo[0]}–${g.tempo[1]} BPM${fitsTempo ? "" : ` (the song is at ${Math.round(bpm)})`}`
    );
    b.close();
    iconButton(b, "prev", "small", "left", "The previous groove in this time signature", () => stepTargetGroove(-1));
    iconButton(b, "next", "small", "right", "The next groove in this time signature", () => stepTargetGroove(1));
    b.close();
    b.open("div", "opts", "drums-opts");
    segmented(
      b,
      "play",
      "Plays",
      r.play,
      PLAY_EDIT,
      "A: the groove (verse); B: its bigger part (chorus); hits: crash and kick on each downbeat; count-in on the side stick; rest",
      (v) =>
        changeTarget((x, pt) => {
          x.play = v;
        })
    );
    segmented(b, "fill", "Fill at the end", r.fill, FILL_EDIT, "A fill in the last beats of the pattern, into what comes next", (v) =>
      changeTarget((x, pt) => {
        x.fill = v;
      })
    );
    b.open("div", "crash", "drums-field");
    b.leaf("span", "l", "drums-label", "Crash");
    button(b, "t", r.crash ? "small on" : "small", r.crash ? "On the 1" : "Off", "A crash cymbal on the first downbeat", () =>
      changeTarget((x, pt) => {
        x.crash = !x.crash;
      })
    );
    b.close();
    b.open("div", "turn", "drums-field");
    b.leaf("span", "l", "drums-label", "Turnaround");
    button(
      b,
      "t",
      r.turnaround ? "small on" : "small",
      r.turnaround ? "Every 4 bars" : "Off",
      "A small turnaround every 4th bar (an open hat or a pickup kick)",
      () =>
        changeTarget((x, pt) => {
          x.turnaround = !x.turnaround;
        })
    );
    b.close();
    b.close();
    b.open("div", "opts2", "drums-opts");
    const bars = Math.max(1, Math.round(pat.length / g.barBeats));
    stepper(b, "bars", "Pattern bars", bars, "Bars in the pattern (a clip longer than that loops it)", (by) =>
      changeTarget((x, pt, song) => {
        const old = pt.length;
        pt.length = Math.max(1, Math.min(64, Math.round(pt.length / g.barBeats) + by)) * g.barBeats;
        // A clip that played the whole pattern still does (its end, the fill, too);
        // drum clips after it make room.
        for (const c of song.playlist.clips) {
          if (c.pattern !== pt.id || Math.abs(c.length - old) > 1e-6) continue;
          if (pt.length > old) clearDrums(song, trackIndex(c.track), c.start + old, c.start + pt.length);
          c.length = pt.length;
        }
      })
    );
    /** const kits: String[] */
    const kits = [""];
    /** const kitLabels: String[] */
    const kitLabels = [`Suggested (${g.kit === "Ebony" ? "drum machine" : g.kit})`];
    for (const k of drums.catalog.kits) {
      kits.push(k);
      kitLabels.push(k === "Ebony" ? "Ebony drum machine" : k);
    }
    field(
      b,
      "kit",
      "Kit",
      r.kit,
      kits,
      kitLabels,
      "A General MIDI kit (one channel), or the Ebony drum machine (a channel per drum); the other patterns keep their kit",
      (v) =>
        changeTarget((x, pt) => {
          x.kit = v;
        })
    );
    field(
      b,
      "feel",
      "Feel",
      r.feel,
      FEELS,
      FEEL_LABELS,
      "Tight: on the grid. Natural: the backbeat sits a little late, small differences. Loose: more of both.",
      (v) =>
        changeTarget((x, pt) => {
          x.feel = v;
        })
    );
    if (g.steps / g.barBeats === 4) {
      const sw = String(r.swing);
      /** const swings: String[] */
      const swings = SWINGS.includes(sw) ? SWINGS : SWINGS.concat([sw]);
      /** const swingLabels: String[] */
      const swingLabels = SWINGS.includes(sw) ? SWING_LABELS : SWING_LABELS.concat([`Swing ${Math.round(r.swing * 100)}%`]);
      field(b, "swing", "Swing", sw, swings, swingLabels, "Delays the off 16th notes (the song's own swing applies on top)", (v) =>
        changeTarget((x, pt) => {
          x.swing = Number(v);
        })
      );
    }
    b.open("div", "seed", "drums-field");
    b.leaf("span", "l", "drums-label", "Variation");
    button(b, "t", "small", "Another fill", "Pick another fill (and other small timing differences)", () =>
      changeTarget((x, pt) => {
        x.seed = x.seed + 1;
      })
    );
    b.close();
    b.close();
    if (r.edited)
      b.leaf(
        "div",
        "edited",
        "drums-status warn",
        "Edited by hand below: changing a setting above makes the pattern again from the groove (Ctrl+Z brings your edits back)."
      );
  } else {
    const w = state.project.drums.written.find((x) => x.id === pat.id);
    b.leaf("div", "t", "drums-title", pat.name);
    b.leaf(
      "div",
      "sub",
      "drums-sub",
      w
        ? "Written by the song drummer (below): edit its notes here — writing the song again keeps your edits — or pick a groove on the left to make it a pattern of its own (writing the song again then leaves its bars to it)."
        : "Drums made elsewhere (played in, imported or drawn): edit its notes here, or pick a groove on the left to make it again from one."
    );
  }
  stepGrid(b, pat, g);
  b.close();
}

/** The steps a beat is cut into, and the beats of a bar, for a pattern. */
/** function gridShape(pat: Pattern, g: GrooveInfo?) => { perBeat: Int, barBeats: Number } */
function gridShape(pat, g) {
  if (g) return { perBeat: Math.round(g.steps / g.barBeats), barBeats: g.barBeats };
  const run = targetMeter();
  // Triplet grids where the notes sit on thirds of a beat.
  const thirds = pat.notes.length > 0 && pat.notes.every((n) => Math.abs(n.start * 3 - Math.round(n.start * 3)) < 0.06);
  const quarters = pat.notes.every((n) => Math.abs(n.start * 4 - Math.round(n.start * 4)) < 0.06);
  return { perBeat: thirds && !quarters ? 3 : 4, barBeats: run.barBeats };
}

/** The stroke a velocity draws as. */
/** function strokeOf(v: Number) => String */
function strokeOf(v) {
  if (v >= 0.9) return "acc";
  if (v >= 0.5) return "hit";
  if (v >= 0.25) return "ghost";
  return "feather";
}

/** The target's own notes as a step grid, one row per drum: a click puts a
 * hit, then an accent, a ghost note, a rest. */
/** function stepGrid(b: Builder, pat: Pattern, g: GrooveInfo?) => Undefined */
function stepGrid(b, pat, g) {
  const shape = gridShape(pat, g);
  const step = 1 / shape.perBeat;
  const n = Math.max(1, Math.round(pat.length / step));
  /** const rows: GridRow[] */
  const rows = [];
  for (const note of pat.notes) {
    const r = noteRow(note);
    if (!rows.some((x) => x.role === r.role)) rows.push(r);
  }
  const order = (r) => {
    const base = r.role.split("@")[0];
    const i = ROLE_LABELS.findIndex((x) => x.key === base);
    return i >= 0 ? i : 100;
  };
  rows.sort((a, b) => order(a) - order(b));
  // Where new drums can go: the kit's channel, or the drum machine's.
  const p = state.project;
  const kitCh = p.channels.find((c) => isKitChannel(c) && pat.notes.some((x) => x.channel === c.id));
  /** const free: String[] */
  const free = [""];
  /** const freeLabels: String[] */
  const freeLabels = ["+ Drum…"];
  /** const freeRows: GridRow[] */
  const freeRows = [];
  for (const e of ROLE_LABELS) {
    if (rows.some((r) => r.role.split("@")[0] === e.key)) continue;
    if (kitCh) {
      freeRows.push({ role: e.key, label: e.value, channel: kitCh.id, pitch: gmKey(e.key) });
    } else {
      const eb = EBONY_KEYS.find((x) => x.role === e.key);
      const c = eb ? p.channels.find((x) => x.instrument.type === "drum" && optionValue(x.instrument, "kind") === eb.kind) : undefined;
      if (eb && c) freeRows.push({ role: `${e.key}@${c.id}`, label: e.value, channel: c.id, pitch: eb.pitch });
    }
  }
  for (const r of freeRows) {
    free.push(r.role);
    freeLabels.push(r.label);
  }
  /** const extra: GridRow[] */
  const extra = drums.extraRows.filter((r) => r.channel !== "" && !rows.some((x) => x.role === r.role) && freeRows.some((x) => x.role === r.role));
  const all = rows.concat(extra);
  b.open("div", "grid", "drums-grid drums-pattern-grid");
  if (all.length === 0) b.leaf("div", "none", "drums-sub", pat.drums.on && pat.drums.play === "rest" ? "Rest: no notes." : "No notes yet.");
  for (const row of all) {
    b.open("div", row.role, "drums-row");
    b.leaf("span", "r", "drums-role", row.label);
    b.open("div", "c", "drums-cells");
    /** const cells: Int[] */
    const cells = [];
    for (let i = 0; i < n; i++) cells.push(-1);
    for (let k = 0; k < pat.notes.length; k++) {
      const x = pat.notes[k];
      if (x.channel !== row.channel || x.pitch !== row.pitch) continue;
      const i = Math.round(x.start / step);
      if (i >= 0 && i < n && cells[i] < 0) cells[i] = k;
    }
    for (let i = 0; i < n; i++) {
      const k = cells[i];
      const kind = k >= 0 ? strokeOf(pat.notes[k].velocity) : "rest";
      let cls = `drums-cell ${kind}`;
      if (i % shape.perBeat === 0) cls = `${cls} beat`;
      if (i % Math.round(shape.perBeat * shape.barBeats) === 0 && i > 0) cls = `${cls} bar`;
      b.open("span", `${i}`, cls);
      b.on("click", (e) => toggleCell(pat, row, i, step));
      b.close();
    }
    b.close();
    b.close();
  }
  if (free.length > 1) {
    select(b, "add", "drums-add", "", free, freeLabels, "Add a drum to this pattern", (v) => {
      const r = freeRows.find((x) => x.role === v);
      if (r) drums.extraRows.push(r);
      invalidate();
    });
  }
  b.close();
}

/** A click on a cell: rest → hit → accent → ghost → rest. */
/** function toggleCell(pat: Pattern, row: GridRow, i: Int, step: Number) => Undefined */
function toggleCell(pat, row, i, step) {
  commit(() => {
    const k = pat.notes.findIndex((x) => x.channel === row.channel && x.pitch === row.pitch && Math.round(x.start / step) === i);
    if (k < 0) {
      pat.notes.push({
        channel: row.channel,
        pitch: row.pitch,
        start: Math.round(i * step * 10000) / 10000,
        length: Math.round(Math.min(step, 0.25) * 10000) / 10000,
        velocity: 0.78,
      });
      pat.notes.sort((a, b) => a.start - b.start);
    } else {
      const v = pat.notes[k].velocity;
      const s = strokeOf(v);
      if (s === "hit") pat.notes[k].velocity = 1;
      else if (s === "acc") pat.notes[k].velocity = 0.32;
      else pat.notes.splice(k, 1);
    }
    if (pat.drums.on) pat.drums.edited = true;
  });
}

// ------------------------------------------------------------------ views: the song

/** The song as a strip: the drum clips along the bars, the cursor; a click
 * on a clip targets it, elsewhere moves the song cursor there. */
/** function mapView(b: Builder) => Undefined */
function mapView(b) {
  const p = state.project;
  const t = p.transport;
  const d = p.drums;
  const partEnd = d.on ? barStart(d.start - 1 + d.sections.reduce((n, s) => n + s.bars, 0)) : 0;
  const end = Math.max(Math.max(songLength(p), partEnd), Math.max(barStart(pointedBar().bar + 4), barStart(16)));
  const bars = barLines(t, 0, end - 1e-6);
  const pct = (beat) => `${Math.max(0, Math.min(100, (beat / end) * 100))}%`;
  const dc = drumClips();
  /** const tracks: Int[] */
  const tracks = [];
  for (const x of dc) if (!tracks.includes(trackIndex(x.clip.track))) tracks.push(trackIndex(x.clip.track));
  tracks.sort((a, b) => a - b);
  b.open("section", "map", "drums-map");
  b.open("div", "head", "drums-map-head");
  b.leaf("span", "l", "drums-label", "Song");
  b.leaf("span", "s", "drums-sub", dc.length > 0 ? "Click a clip to edit its pattern, or anywhere to move the song cursor" : "No drums in the song yet");
  b.close();
  b.open("div", "lanes", "drums-map-lanes");
  b.on("pointerdown", (e) => {
    const beat = ((e.clientX - e.targetLeft) / Math.max(1, e.targetWidth)) * end;
    const bar = barAt(t, Math.max(0, beat)).bar;
    state.clipSelection = [];
    if (state.mode !== "song") setMode("song");
    seek(barStart(bar));
  });
  b.open("div", "ruler", "drums-map-ruler");
  // Bar numbers every 4 bars, fewer on a long song.
  const every = bars.length <= 48 ? 4 : bars.length <= 96 ? 8 : bars.length <= 192 ? 16 : 32;
  for (const x of bars) {
    if (x.bar % every !== 0) continue;
    b.leaf("span", `b${x.bar}`, "drums-map-bar", String(x.bar + 1));
    b.style("left", pct(x.start));
  }
  b.close();
  for (const tr of tracks.length > 0 ? tracks : [-1]) {
    b.open("div", `t${tr}`, "drums-map-lane");
    for (const x of dc) {
      if (trackIndex(x.clip.track) !== tr) continue;
      const pat = patternOf(x.clip.pattern);
      b.open("div", `c${x.i}`, x.clip.pattern === drums.target ? "drums-map-clip on" : "drums-map-clip");
      b.style("left", pct(x.clip.start));
      b.style("width", pct(x.clip.length));
      const s = barAt(t, x.clip.start).bar;
      b.attr("title", `${pat ? pat.name : x.clip.pattern}: bars ${s + 1}–${barAt(t, x.clip.start + x.clip.length - 1e-6).bar + 1}`);
      b.on("pointerdown", (e) => {
        e.stopPropagation();
        targetClip(x.i);
      });
      b.leaf("span", "n", "", pat ? pat.name.replace(/^Drums · /, "") : "");
      b.close();
    }
    b.close();
  }
  if (state.mode === "song") {
    b.leaf("div", "cursor", "drums-map-cursor", "");
    b.style("left", pct(Math.max(0, state.position)));
  }
  b.close();
  b.close();
}

/** The song drummer: the whole song's drums in one go, from sections. */
/** function arrangeView(b: Builder) => Undefined */
function arrangeView(b) {
  const d = state.project.drums;
  b.open("section", "arrange", drums.arrange ? "drums-arrange open" : "drums-arrange");
  b.open("div", "head", "drums-arrange-head");
  b.on("click", (e) => {
    drums.arrange = !drums.arrange;
    drums.opened = drums.arrange;
    invalidate();
  });
  b.leaf("span", "c", "b-caret", drums.arrange ? "▾" : "▸");
  b.leaf("span", "t", "drums-title", "Song drummer");
  const total = d.sections.reduce((n, s) => n + s.bars, 0);
  b.leaf(
    "span",
    "s",
    "drums-sub",
    d.on
      ? `${d.sections.length} sections, bars ${d.start}–${d.start + total - 1} · ${d.written.length > 0 ? `${d.written.length} patterns written` : "not written yet"}`
      : "Drums for the whole song at once: sections guessed from the playlist, each playing A or B with fills and crashes"
  );
  b.close();
  if (drums.arrange) {
    if (!d.on) {
      const run = meterAt(0);
      const pat = targetPattern();
      const gid =
        pat && pat.drums.on && fits(grooveOf(pat.drums.groove) ?? drums.catalog.grooves[0], run)
          ? pat.drums.groove
          : (drums.catalog.grooves.find((g) => fits(g, run))?.id ?? "");
      const g = grooveOf(gid);
      b.open("div", "start", "drums-arrange-start");
      b.leaf(
        "div",
        "sub",
        "drums-sub",
        "The song drummer guesses the sections from the playlist; you choose what each plays, then write them as patterns and clips on a Drums track."
      );
      button(b, "go", "small gold", g ? `Start on ${g.name}` : "Start", "Guess the sections from the playlist (nothing is written yet)", () => {
        if (gid !== "") startPart(gid);
      });
      b.close();
    } else {
      b.open("div", "flow", "drums-flow");
      grooveView(b, d);
      sectionsView(b, d);
      writeView(b, d);
      b.close();
    }
  }
  b.close();
}

/** The Drums panel: one screen. */
/** function drumsPanel(b: Builder) => Undefined */
export function drumsPanel(b) {
  loadCatalog();
  followFocus();
  if (drums.target !== "" && !targetPattern()) drums.target = "";
  b.open("div", "drums", drums.arrange ? "drums arranging" : "drums");
  if (drums.opened) {
    // Just opened: the song drummer comes into view.
    drums.opened = false;
    b.prop("scrollTop", "100000");
  }
  b.open("div", "screen", "drums-screen");
  targetView(b);
  libraryView(b);
  patternView(b);
  mapView(b);
  b.close();
  arrangeView(b);
  b.close();
}

/** Dock tab tools. */
/** function drumsTools(b: Builder) => Undefined */
export function drumsTools(b) {
  const pat = targetPattern();
  b.leaf("span", "l", "label", pat ? `Editing ${pat.name}` : "Pick a groove to make drums at the cursor");
}

/** Forget the drum part; the patterns it wrote stay in the song. */
function removePart() {
  stopPreview();
  commit(() => {
    state.project.drums.on = false;
    state.project.drums.written = [];
  });
  hint("");
}
