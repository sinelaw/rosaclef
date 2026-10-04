// The Drums dock: a drummer for the song (docs/drums.md). Three choices —
// a groove, a kit, and what each section plays — then Write drums turns the
// project's drum part into patterns and clips on a Drums track (one undo
// step). The part itself lives in the project (`drums`), so the agent can
// edit it too; the writing is done by the server (POST /api/drums), the same
// code the command line and the browser-only studio run.

import { getJson, sendJson, now, audioPost, confirmBox } from "#platform";
import { state, commit, invalidate, hint, currentPattern } from "../store.js";
import { decodeProject, encodeProject, projectJson, meterMap } from "../model.js";
import { startAudio } from "../audio.js";
import { button, iconButton, select, textInput } from "./widgets.js";
import { toast } from "./toast.js";

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

// ------------------------------------------------------------------ editing

/** Change the drum part (one undo step); a playing preview follows. */
/** function edit(fn: (DrumPart) => Undefined) => Undefined */
function edit(fn) {
  commit(() => fn(state.project.drums));
  if (drums.previewing) refreshPreview();
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
      const edited = r.edited.map((x) => String(x));
      if (edited.length > 0) {
        const names = edited.map((id) => state.project.patterns.find((x) => x.id === id)?.name ?? id);
        if (
          !confirmBox(
            `These drum patterns were edited by hand:\n\n${names.join("\n")}\n\nWriting the drums replaces them (Ctrl+Z brings them back). Write anyway?`
          )
        ) {
          invalidate();
          return false;
        }
      }
      const np = decodeProject(r.project);
      commit(() => {
        const p = state.project;
        p.channels = np.channels;
        p.patterns = np.patterns;
        p.playlist = np.playlist;
        p.drums = np.drums;
      });
      toast(
        "Drums written",
        `${Number(r.report.patterns)} patterns in ${Number(r.report.clips)} clips on the ${np.playlist.tracks[Math.round(Number(r.report.track))]?.name ?? "Drums"} track. Ctrl+Z undoes it.`,
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
  invalidate();
  return await sendPreview(true);
}

function refreshPreview() {
  sendPreview(false);
}

/** Stop the preview and give the engine the song back. */
export function stopPreview() {
  if (!drums.previewing) return undefined;
  drums.previewing = false;
  drums.previewSeq = drums.previewSeq + 1;
  audioPost({ t: "stop" });
  audioPost({ t: "project", json: projectJson(state.project) });
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

/** A number with − and + buttons. */
/** function stepper(b: Builder, key: String, label: String, value: Number, lo: Number, hi: Number, tip: String, onSet: (Number) => Undefined) => Undefined */
function stepper(b, key, label, value, lo, hi, tip, onSet) {
  b.open("div", key, "drums-field");
  b.leaf("span", "l", "drums-label", label);
  b.open("div", "n", "drums-stepper");
  iconButton(b, "dn", "small", "minus", `${tip}: fewer`, () => onSet(Math.max(lo, value - 1)));
  b.leaf("span", "v", "drums-num", String(value));
  iconButton(b, "up", "small", "plus", `${tip}: more`, () => onSet(Math.min(hi, value + 1)));
  b.close();
  b.close();
}

/** The groove's parts as step grids: what a groove is, at a glance. */
/** function grooveGrid(b: Builder, key: String, title: String, g: GrooveInfo, rows: String[][]) => Undefined */
function grooveGrid(b, key, title, g, rows) {
  const perBeat = g.steps / g.barBeats;
  b.open("div", key, "drums-grid");
  b.leaf("div", "t", "drums-grid-title", title);
  for (const row of rows) {
    const steps = row[1].replace(/[ |]/g, "");
    b.open("div", row[0], "drums-row");
    b.leaf("span", "r", "drums-role", labelOf(ROLE_LABELS, row[0]));
    b.open("div", "c", "drums-cells");
    for (let i = 0; i < steps.length; i++) {
      const c = steps[i];
      const kind = c === "X" ? "acc" : c === "x" ? "hit" : c === "g" ? "ghost" : c === "f" ? "feather" : "rest";
      b.leaf("span", `${i}`, `drums-cell ${kind}${i % perBeat === 0 ? " beat" : ""}`, "");
    }
    b.close();
    b.close();
  }
  b.close();
}

/** The groove, kit and feel. */
/** function grooveView(b: Builder, d: DrumPart) => Undefined */
function grooveView(b, d) {
  const gs = drums.catalog.grooves;
  const g = grooveOf(d.groove);
  b.open("section", "groove", "drums-step drums-groove");
  b.open("div", "pick", "drums-pick");
  field(
    b,
    "groove",
    "Groove",
    d.groove,
    gs.map((x) => x.id),
    gs.map(grooveLabel),
    "The groove the drummer plays: A in verses, the bigger B in choruses",
    (v) => setGroove(v)
  );
  b.open("div", "nav", "drums-nav");
  iconButton(b, "prev", "small", "left", "The previous groove (keeps playing)", () => stepGroove(-1));
  iconButton(b, "next", "small", "right", "The next groove (keeps playing)", () => stepGroove(1));
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
    b.open("div", "grids", "drums-grids");
    grooveGrid(b, "a", "A · verse", g, g.a);
    grooveGrid(b, "b", "B · chorus", g, g.b);
    b.close();
  }
  b.close();
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
  const i = gs.findIndex((g) => g.id === state.project.drums.groove);
  setGroove(gs[(i + by + gs.length) % gs.length].id);
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
    stepper(b, "bars", "Bars", s.bars, 1, 999, "Bars in this section", (v) =>
      edit((x) => {
        x.sections[at].bars = v;
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
    for (const g of drums.catalog.grooves) {
      ids.push(g.id);
      labels.push(grooveLabel(g));
    }
    field(b, "groove", "Groove", s.groove, ids, labels, "Another groove for this section only (a half-time bridge)", (v) =>
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
  stepper(b, "start", "Starts on bar", d.start, 1, 9999, "The bar the first section starts on", (v) =>
    edit((x) => {
      x.start = v;
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
  b.leaf(
    "div",
    "status",
    "drums-status",
    written > 0
      ? `Written: ${written} pattern${written === 1 ? "" : "s"}. Writing again replaces them.`
      : "Not written yet: the song plays no drums from this part until you write it."
  );
  button(
    b,
    "go",
    drums.busy ? "drums-writebtn" : "gold drums-writebtn",
    drums.busy ? "Working…" : written > 0 ? "Write drums again" : "Write drums",
    "Write the drum part into patterns and clips on a Drums track (Ctrl+Z undoes it)",
    () => writeDrums()
  );
  b.close();
}

/** Before there is a drum part: pick a groove to start from. */
/** function startView(b: Builder) => Undefined */
function startView(b) {
  b.open("div", "start", "drums-start");
  b.leaf("div", "t", "drums-title", "A drummer for the song");
  b.leaf(
    "div",
    "sub",
    "drums-sub",
    "Pick a groove. The sections are guessed from the playlist; then choose what each one plays — groove A or B, a fill, a crash — and write the drums."
  );
  const gs = drums.catalog.grooves;
  /** const styles: String[] */
  const styles = [];
  for (const g of gs) if (!styles.includes(g.style)) styles.push(g.style);
  b.open("div", "list", "drums-styles");
  for (const st of styles) {
    b.open("div", st, "drums-style");
    b.leaf("div", "n", "drums-label", st);
    b.open("div", "g", "drums-style-grooves");
    for (const g of gs.filter((x) => x.style === st)) {
      button(b, g.id, "small", g.name, `${g.meter}, ${g.tempo[0]}–${g.tempo[1]} BPM — start the drum part on this groove`, () => startPart(g.id));
    }
    b.close();
    b.close();
  }
  b.close();
  if (gs.length === 0) b.leaf("div", "wait", "drums-sub", "Loading the grooves…");
  b.close();
}

/** The Drums panel. */
/** function drumsPanel(b: Builder) => Undefined */
export function drumsPanel(b) {
  loadCatalog();
  const d = state.project.drums;
  b.open("div", "drums", "drums");
  if (!d.on) startView(b);
  else {
    b.open("div", "flow", "drums-flow");
    grooveView(b, d);
    sectionsView(b, d);
    writeView(b, d);
    b.close();
  }
  b.close();
}

/** Dock tab tools. */
/** function drumsTools(b: Builder) => Undefined */
export function drumsTools(b) {
  const d = state.project.drums;
  b.leaf("span", "l", "label", d.on ? "Groove → sections → write" : "A drummer for the song");
  if (d.on) {
    button(b, "remove", "small", "Remove part", "Forget the drum part (the written patterns stay in the song)", () => {
      stopPreview();
      commit(() => {
        state.project.drums.on = false;
        state.project.drums.written = [];
      });
      hint("");
    });
  }
}
