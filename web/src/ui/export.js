// The top bar's Export menu: the song as a WAV mixdown, or the song — or
// the pattern in the piano roll, singing a chosen verse — as MIDI with
// lyrics, MusicXML, singing-synthesizer projects (OpenUtau, Synthesizer V,
// DiffSinger) and lyric files (UltraStar, LRC, TTML, timed words, tagged
// lyrics, SSML). The back end lists the formats (GET /api/export/formats)
// and writes the files (GET /api/export); the menu is drawn by the shell.

import { getJson, download } from "#platform";
import { state, invalidate, hint } from "../store.js";
import { verseCount } from "../expand.js";
import { button, select } from "./widgets.js";
import { toast } from "./toast.js";

/** type ExportFormat = { id: String, extension: String, label: String, description: String } */

const ex = {
  open: false,
  x: 0,
  y: 0,
  /** "song", or "pattern": the pattern in the piano roll. */
  scope: "song",
  verse: 1,
  formats /*: ExportFormat[] */: [],
  /** Renders the WAV mixdown (the top bar's). */
  mixdown: () => undefined,
};

/** function openExport(x: Number, y: Number, mixdown: () => Undefined) => Undefined */
function openExport(x, y, mixdown) {
  ex.open = true;
  ex.x = x;
  ex.y = y;
  ex.mixdown = mixdown;
  if (ex.formats.length === 0) loadFormats();
  invalidate();
}

function closeExport() {
  ex.open = false;
  invalidate();
}

function loadFormats() {
  getJson("/api/export/formats")
    .then((r) => {
      ex.formats = r.formats;
      invalidate();
      return true;
    })
    .catch((e) => {
      toast("Export formats unavailable", String(e), "error");
      return false;
    });
}

/** A file name friendly form of a title, as the back end makes it. */
/** function slug(s: String) => String */
function slug(s) {
  const out = s
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return out === "" ? "untitled" : out;
}

/** Whether the export is of the pattern in the piano roll. */
function ofPattern() {
  return ex.scope === "pattern" && state.pattern !== "";
}

/** Download the song (or pattern) in format `f`. */
/** function exportAs(f: ExportFormat) => Undefined */
function exportAs(f) {
  let url = `/api/export?format=${encodeURIComponent(f.id)}`;
  let name = slug(state.project.meta.title);
  if (ofPattern()) {
    url = `${url}&pattern=${encodeURIComponent(state.pattern)}&verse=${ex.verse}`;
    name = `${name}-${slug(state.pattern)}-v${ex.verse}`;
  }
  // Formats sharing an extension (.txt) are told apart by their id.
  const shared = ex.formats.filter((g) => g.extension === f.extension).length > 1;
  download(url, `${name}${shared ? `-${f.id}` : ""}.${f.extension}`);
  closeExport();
}

/** The verses the pattern in the piano roll sings. */
function verses() {
  const i = state.project.patterns.findIndex((p) => p.id === state.pattern);
  return i < 0 ? 1 : verseCount(state.project, i);
}

/** The top bar's Export button: opens the menu; `mixdown` renders the WAV. */
/** function exportButton(b: Builder, mixdown: () => Undefined) => Undefined */
export function exportButton(b, mixdown) {
  const tip = "Export: a WAV mixdown, MIDI with lyrics, MusicXML, singing-synthesizer projects and lyric files";
  b.open("button", "export", "btn gold");
  b.attr("title", tip);
  b.on("pointerenter", (e) => hint(tip));
  b.on("click", (e) => {
    openExport(e.clientX, e.clientY, mixdown);
  });
  b.leaf("span", "t", "", "Export");
  b.close();
}

/** function item(b: Builder, key: String, label: String, extension: String, tip: String, onClick: () => Undefined) => Undefined */
function item(b, key, label, extension, tip, onClick) {
  b.open("button", key, "auto-menu-item");
  b.attr("title", tip);
  b.on("click", (e) => {
    onClick();
  });
  b.leaf("span", "l", "", label);
  b.leaf("span", "x", "", extension);
  b.style("margin-left", "auto");
  b.style("color", "var(--dim)");
  b.style("font", "500 10px/1 var(--mono)");
  b.close();
}

/** Song or pattern, and the pattern's verse. */
/** function scopeRow(b: Builder) => Undefined */
function scopeRow(b) {
  b.open("div", "scope", "seg");
  b.style("margin", "2px 6px 6px");
  button(b, "song", ex.scope === "song" ? "on" : "", "Song", "Export the whole song, as it plays", () => {
    ex.scope = "song";
    invalidate();
  });
  if (state.pattern !== "")
    button(b, "pattern", ex.scope === "pattern" ? "on" : "", "Pattern", "Export the pattern in the piano roll, played once", () => {
      ex.scope = "pattern";
      invalidate();
    });
  b.close();
  if (!ofPattern()) return undefined;
  const n = verses();
  if (ex.verse > n) ex.verse = 1;
  /** const choices: String[] */
  const choices = [];
  for (let v = 1; v <= n; v++) choices.push(String(v));
  const labels = choices.map((v) => `Verse ${v}`);
  select(b, "verse", "", String(ex.verse), choices, labels, "The verse the pattern sings", (v) => {
    ex.verse = Math.max(1, Math.round(Number(v)));
    invalidate();
  });
}

/** The Export menu (rendered by the shell, above every panel). */
/** function exportMenu(b: Builder) => Undefined */
export function exportMenu(b) {
  // A stable container, as for the automation menu.
  b.open("div", "export-menu-root", "auto-menu-root");
  if (ex.open) exportMenuBody(b);
  b.close();
}

/** function exportMenuBody(b: Builder) => Undefined */
function exportMenuBody(b) {
  b.leaf("div", "backdrop", "auto-backdrop", "");
  b.on("pointerdown", (e) => {
    e.preventDefault();
    closeExport();
  });
  b.open("div", "menu", "auto-menu");
  b.style("left", `min(${ex.x}px, calc(100vw - 270px))`);
  b.style("top", `min(${ex.y + 10}px, calc(100vh - 480px))`);
  b.leaf("div", "t", "auto-menu-title", "Export");
  b.leaf("div", "s", "auto-menu-sub", ofPattern() ? `pattern ${state.pattern}, verse ${ex.verse}` : "the whole song");
  scopeRow(b);
  if (!ofPattern())
    item(b, "wav", "Mixdown", ".wav", "Render the song offline to a 24-bit WAV (saved in renders/, and downloaded)", () => {
      closeExport();
      ex.mixdown();
    });
  if (ex.formats.length === 0) b.leaf("div", "loading", "auto-menu-note", "Loading the formats…");
  for (const f of ex.formats) item(b, f.id, f.label, `.${f.extension}`, f.description, () => exportAs(f));
  b.close();
}
