// The Projects window: the project library (open, create, duplicate,
// rename, delete, import LMMS / MIDI) and the file manager of the open
// project. Opened from the top bar or with Ctrl+O; drawn by the shell.
//
// The server owns the library (GET /api/projects, /api/files, ...); this
// module keeps a copy of what it last listed and asks again after every
// action. Opening a project makes the server broadcast `switched`, which
// reloads every client (net.js) and calls `projectSwitched` here. In the
// browser-only studio the "server" is the back end in a worker, and the
// library lives in the browser's storage: projects download as .zip files.

import {
  getJson,
  sendJson,
  uploadFile,
  pickFiles,
  download,
  previewAudio,
  stopPreview,
  fmtDate,
  fmt,
  confirmBox,
  listenWindow,
  storageEstimate,
} from "#platform";
import { state, invalidate, hint } from "../store.js";
import { PALETTE } from "../model.js";
import { glyph, iconButton, button } from "./widgets.js";
import { toast } from "./toast.js";
import { t, tf } from "../i18n.js";

/** type ProjectInfo = { name: String, folder: String, title: String, bpm: Number, beatsPerBar: Number, modified: Number, current: Boolean, channels: Number, patterns: Number, clips: Number, lengthBeats: Number, invalid: Boolean } */
/** type FileInfo = { path: String, name: String, dir: String, size: Number, modified: Number, kind: String, used: Boolean, managed: Boolean } */
/** type ImportNote = { name: String, source: String, warnings: String[], into: Boolean } */

const pm = {
  open: false,
  /** "projects" or "files". */
  tab: "projects",
  library: "",
  current: "",
  loaded: false,
  projects /*: ProjectInfo[] */: [],
  /** The name a copy of the demo song starts with (its title). */
  demoTitle: "Demo",
  files /*: FileInfo[] */: [],
  filesLoaded: false,
  filter: "",
  /** Label of the request in flight ("" when idle). */
  busy: "",
  /** What the name field is for: "", "new", "demo", "duplicate", "rename" or "file". */
  mode: "",
  target: "",
  draft: "",
  /** Path of the file being previewed. */
  preview: "",
  samplesKey: "",
  /** The last import (zero or one entry). */
  imported /*: ImportNote[] */: [],
  /** Browser storage in use and available, in bytes (the browser-only studio). */
  storage: { usage: 0, quota: 0 },
};

// ------------------------------------------------------------------ icons

const ICON_IMPORT = "M12 15.5v-11M7.5 9l4.5-4.5L16.5 9M4.5 16.5v3h15v-3";
const ICON_DOC = "M6.5 3.5h7l4 4v13h-11zM13.5 3.5v4h4";
const ICON_SEARCH = "M10.5 4.5a6 6 0 1 0 0 12 6 6 0 1 0 0-12zM15 15l5 5";
const ICON_OPEN = "M4 7.5h6l2 2h8v9H4zM4 7.5V5h6";

/** function icon(b: Builder, d: String) => Undefined */
function icon(b, d) {
  b.open("svg", "g", "glyph");
  b.attr("viewBox", "0 0 24 24");
  b.attr("aria-hidden", "true");
  b.leaf("path", "p", "", "");
  b.attr("d", d);
  b.close();
}

// ------------------------------------------------------------------ formatting

/** function errText<E>(e: E) => String */
function errText(e) {
  return String(e).replace(/^Error:\s*/, "");
}

/** function ago(ms: Number) => String */
function ago(ms) {
  if (ms <= 0) return "";
  const s = (Date.now() - ms) / 1000;
  if (s < 60) return t("just now");
  if (s < 3600) return tf("{0} min ago", [String(Math.floor(s / 60))]);
  if (s < 86400) return tf("{0} h ago", [String(Math.floor(s / 3600))]);
  if (s < 86400 * 2) return t("yesterday");
  if (s < 86400 * 7) return tf("{0} days ago", [String(Math.floor(s / 86400))]);
  return fmtDate(ms);
}

/** function bytes(n: Number) => String */
function bytes(n) {
  if (n < 1024) return `${Math.round(n)} B`;
  if (n < 1024 * 1024) return `${fmt(n / 1024, 0)} KB`;
  if (n < 1024 * 1024 * 1024) return `${fmt(n / 1024 / 1024, 1)} MB`;
  return `${fmt(n / 1024 / 1024 / 1024, 2)} GB`;
}

/** function duration(p: ProjectInfo) => String */
function duration(p) {
  if (p.bpm <= 0 || p.lengthBeats <= 0) return t("empty");
  const secs = Math.round((p.lengthBeats * 60) / p.bpm);
  const m = Math.floor(secs / 60);
  const s = secs - m * 60;
  return `${m}:${s < 10 ? "0" : ""}${s}`;
}

/** A stable number from a string (cover colors and waveform). */
/** function hash(s: String) => Number */
function hash(s) {
  let h = 7;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) % 1000003;
  return h;
}

/** function encodePath(path: String) => String */
function encodePath(path) {
  return path
    .split("/")
    .map((s) => encodeURIComponent(s))
    .join("/");
}

/** A valid project (folder) name made from a song title (the server's rules:
 * letters, digits, spaces and _-.()+,&' only, no leading or trailing dot). */
/** function nameFrom(title: String) => String */
function nameFrom(title) {
  const s = title
    .replace(/[^\p{L}\p{N} _\-.()+,&']+/gu, "-")
    .replace(/^[-. ]+/, "")
    .slice(0, 52)
    .replace(/[-. ]+$/, "");
  return s === "" ? "Untitled" : s;
}

/** function uniqueName(base: String, projects: ProjectInfo[]) => String */
function uniqueName(base, projects) {
  let name = base;
  let n = 2;
  while (projects.some((p) => p.name === name)) {
    name = `${base} ${n}`;
    n = n + 1;
  }
  return name;
}

// ------------------------------------------------------------------ server

function refreshStorage() {
  if (state.backend !== "local") return undefined;
  storageEstimate()
    .then((s) => {
      pm.storage = s;
      invalidate();
      return true;
    })
    .catch((e) => false);
}

function refreshProjects() {
  refreshStorage();
  getJson("/api/projects")
    .then((r) => {
      pm.library = String(r.library);
      pm.current = String(r.current);
      pm.projects = r.projects;
      if (typeof r.demoTitle === "string" && r.demoTitle !== "") pm.demoTitle = r.demoTitle;
      pm.loaded = true;
      invalidate();
      return true;
    })
    .catch((e) => {
      toast(t("Could not list the projects"), errText(e), "error");
      return false;
    });
}

function refreshFiles() {
  refreshStorage();
  getJson("/api/files")
    .then((r) => {
      pm.files = r.files;
      pm.filesLoaded = true;
      invalidate();
      return true;
    })
    .catch((e) => {
      toast(t("Could not list the files"), errText(e), "error");
      return false;
    });
}

/** Run a request with a busy indicator (`label`); errors become toasts titled `failed`. */
/** function act<T>(label: String, failed: String, p: Promise<T>, done: (T) => Undefined) => Undefined */
function act(label, failed, p, done) {
  pm.busy = label;
  invalidate();
  p.then((r) => {
    pm.busy = "";
    done(r);
    invalidate();
    return true;
  }).catch((e) => {
    pm.busy = "";
    toast(failed, errText(e), "error");
    invalidate();
    return false;
  });
}

/** function openProject(name: String) => Undefined */
function openProject(name) {
  act(tf("Opening “{0}”…", [name]), tf("Opening “{0}” failed", [name]), sendJson("/api/projects/open", "POST", { name: name }), (r) => {
    pm.imported = [];
    closeProjects();
  });
}

/** function createProject(name: String, demo: Boolean) => Undefined */
function createProject(name, demo) {
  act(tf("Creating “{0}”…", [name]), tf("Creating “{0}” failed", [name]), sendJson("/api/projects", "POST", { name: name, demo: demo }), (r) => {
    openProject(String(r.name));
  });
}

/** function duplicateProject(name: String, to: String) => Undefined */
function duplicateProject(name, to) {
  act(tf("Duplicating “{0}”…", [name]), tf("Duplicating “{0}” failed", [name]), sendJson("/api/projects/duplicate", "POST", { name: name, to: to }), (r) => {
    toast(t("Project duplicated"), `“${name}” → “${String(r.name)}”`, "info");
    refreshProjects();
  });
}

/** function renameProject(name: String, to: String) => Undefined */
function renameProject(name, to) {
  act(tf("Renaming “{0}”…", [name]), tf("Renaming “{0}” failed", [name]), sendJson("/api/projects/rename", "POST", { name: name, to: to }), (r) => {
    toast(t("Project renamed"), `“${name}” → “${String(r.name)}”`, "info");
    refreshProjects();
  });
}

/** function deleteProject(p: ProjectInfo) => Undefined */
function deleteProject(p) {
  const kept =
    state.backend === "local" ? t("It stays in the trash until you empty it.") : tf("It is kept in {0}/.trash and can be restored from there.", [pm.library]);
  if (!confirmBox(tf("Move “{0}” ({1}) to the library trash?\n\n{2}", [p.title, p.name, kept]))) return undefined;
  act(tf("Deleting “{0}”…", [p.name]), tf("Deleting “{0}” failed", [p.name]), sendJson(`/api/projects/${encodeURIComponent(p.name)}`, "DELETE", {}), (r) => {
    toast(t("Moved to the trash"), p.name, "info");
    refreshProjects();
  });
}

/** function importKind(fileName: String) => String */
function importKind(fileName) {
  const lower = fileName.toLowerCase();
  if (lower.endsWith(".mid") || lower.endsWith(".midi") || lower.endsWith(".kar") || lower.endsWith(".rmi")) return "midi";
  if (lower.endsWith(".zip")) return "zip";
  return "lmms";
}

/** Import an LMMS project, a MIDI file or a project .zip as a new project. */
function importProject() {
  pickFiles(".mmp,.mmpz,.mid,.midi,.kar,.rmi,.zip", (files) => {
    if (files.length === 0) return undefined;
    const f = files[0];
    const kind = importKind(f.name);
    act(
      tf("Importing {0}…", [f.name]),
      tf("Importing {0} failed", [f.name]),
      uploadFile(`/api/projects/import-${kind}?filename=${encodeURIComponent(f.name)}`, f),
      (r) => {
        pm.imported = [{ name: String(r.name), source: f.name, warnings: r.warnings, into: false }];
        pm.tab = "projects";
        refreshProjects();
      }
    );
  });
}

/** Add the parts of a MIDI file to the open song (one undo step). */
function importMidiHere() {
  pickFiles(".mid,.midi,.kar,.rmi", (files) => {
    if (files.length === 0) return undefined;
    const f = files[0];
    act(tf("Adding {0}…", [f.name]), tf("Adding {0} failed", [f.name]), uploadFile("/api/import-midi?into=current", f), (r) => {
      pm.imported = [{ name: "", source: f.name, warnings: r.warnings, into: true }];
      toast(t("MIDI parts added"), tf("{0} new channel(s) from {1} — Ctrl+Z undoes it", [String(r.channels), f.name]), "info");
    });
  });
}

/** Upload audio files into samples/. */
function importAudio() {
  pickFiles("audio/*", (files) => {
    for (const f of files) {
      act(tf("Uploading {0}…", [f.name]), tf("Uploading {0} failed", [f.name]), uploadFile(`/api/samples?name=${encodeURIComponent(f.name)}`, f), (r) => {
        toast(t("Sample added"), String(r.path), "info");
        refreshFiles();
      });
    }
  });
}

/** Download a project folder as a .zip (a backup, or to open it elsewhere). */
/** function exportProject(p: ProjectInfo) => Undefined */
function exportProject(p) {
  download(`/api/projects/export?name=${encodeURIComponent(p.name)}`, `${p.name}.zip`);
}

/** Delete the trash for good: deleted projects, or this project's deleted files. */
/** function emptyTrash(scope: String) => Undefined */
function emptyTrash(scope) {
  const question =
    scope === "library"
      ? t("Delete the projects in the library's trash for good? This cannot be undone.")
      : t("Delete the files in this project's trash for good? This cannot be undone.");
  if (!confirmBox(question)) return undefined;
  act(t("Emptying the trash…"), t("Emptying the trash failed"), sendJson("/api/trash/empty", "POST", { scope: scope }), (r) => {
    const n = Number(r.removed);
    const body = n === 0 ? "" : n === 1 ? t("1 item deleted for good") : tf("{0} items deleted for good", [String(n)]);
    toast(n === 0 ? t("The trash was already empty") : t("Trash emptied"), body, "info");
    refreshStorage();
  });
}

/** function renameFile(path: String, to: String) => Undefined */
function renameFile(path, to) {
  act(tf("Renaming {0}…", [path]), tf("Renaming {0} failed", [path]), sendJson("/api/files/rename", "POST", { path: path, to: to }), (r) => {
    const refs = Number(r.references);
    toast(t("File renamed"), refs > 0 ? tf("{0} — {1} reference(s) in the song updated", [String(r.path), String(refs)]) : String(r.path), "info");
    refreshFiles();
  });
}

/** function deleteFile(f: FileInfo) => Undefined */
function deleteFile(f) {
  const question = f.used
    ? tf("Move {0} to the project's trash (.trash/)?\n\nThe song uses this file: those parts will fall silent.", [f.path])
    : tf("Move {0} to the project's trash (.trash/)?", [f.path]);
  if (!confirmBox(question)) return undefined;
  if (pm.preview === f.path) stopPlaying();
  act(tf("Deleting {0}…", [f.name]), tf("Deleting {0} failed", [f.name]), sendJson(`/api/files?path=${encodeURIComponent(f.path)}`, "DELETE", {}), (r) => {
    toast(t("Moved to the trash"), String(r.trashed), "info");
    refreshFiles();
  });
}

// ------------------------------------------------------------------ opening

export function openProjects() {
  pm.open = true;
  refreshProjects();
  if (pm.tab === "files") refreshFiles();
  invalidate();
}

export function closeProjects() {
  pm.open = false;
  pm.mode = "";
  stopPlaying();
  invalidate();
}

/** Called by net.js after the server opened another project. */
export function projectSwitched() {
  pm.files = [];
  pm.filesLoaded = false;
  stopPlaying();
  if (pm.open) {
    refreshProjects();
    if (pm.tab === "files") refreshFiles();
  }
  toast(tf("Opened “{0}”", [state.project.meta.title]), state.folder, "info");
}

/** function showTab(tab: String) => Undefined */
function showTab(tab) {
  pm.tab = tab;
  pm.mode = "";
  if (tab === "files") refreshFiles();
  invalidate();
}

/** function compose(mode: String, target: String, draft: String) => Undefined */
function compose(mode, target, draft) {
  pm.mode = mode;
  pm.target = target;
  pm.draft = draft;
  invalidate();
}

function confirmCompose() {
  const name = pm.draft.trim();
  if (name === "") return undefined;
  const mode = pm.mode;
  const target = pm.target;
  pm.mode = "";
  if (mode === "new") createProject(name, false);
  else if (mode === "demo") createProject(name, true);
  else if (mode === "duplicate") duplicateProject(target, name);
  else if (mode === "rename" && name !== target) renameProject(target, name);
  else if (mode === "file") renameFile(target, name);
  invalidate();
}

function stopPlaying() {
  stopPreview();
  pm.preview = "";
}

/** function togglePreview(f: FileInfo) => Undefined */
function togglePreview(f) {
  if (pm.preview === f.path) {
    stopPlaying();
  } else {
    pm.preview = f.path;
    const path = f.path;
    previewAudio(`/files/${encodePath(path)}`, () => {
      if (pm.preview === path) pm.preview = "";
      invalidate();
    });
  }
  invalidate();
}

listenWindow("keydown", (e) => {
  if (pm.open && e.key === "Escape") {
    if (pm.mode !== "") pm.mode = "";
    else closeProjects();
    invalidate();
    return undefined;
  }
  const mod = e.ctrlKey || e.metaKey;
  if (mod && (e.key === "o" || e.key === "O")) {
    e.preventDefault();
    if (pm.open) closeProjects();
    else openProjects();
  }
});

// ------------------------------------------------------------------ views

/** The top-bar button. */
/** function projectsButton(b: Builder) => Undefined */
export function projectsButton(b) {
  b.open("button", "projects", pm.open ? "btn pm-open on" : "btn pm-open");
  b.attr("title", t("Projects and files (Ctrl+O)"));
  b.on("pointerenter", (e) => hint(t("Projects — open, create, duplicate or import (LMMS, MIDI) songs, and manage this project's files (Ctrl+O)")));
  b.on("click", (e) => {
    if (pm.open) closeProjects();
    else openProjects();
  });
  glyph(b, "folder");
  b.leaf("span", "t", "", t("Projects"));
  b.close();
}

/** The name field shared by New / Duplicate / Rename. */
/** function composer(b: Builder) => Undefined */
function composer(b) {
  let label = t("Name your new project");
  let action = t("Create & open");
  if (pm.mode === "demo") label = t("Name the new project (a copy of the demo song)");
  if (pm.mode === "duplicate") {
    label = tf("Duplicate “{0}” as", [pm.target]);
    action = t("Duplicate");
  }
  if (pm.mode === "rename") {
    label = tf("Rename “{0}” to", [pm.target]);
    action = t("Rename");
  }
  if (pm.mode === "file") {
    label = tf("Rename {0} to", [pm.target]);
    action = t("Rename");
  }
  b.open("div", "composer", "pm-composer");
  b.leaf("label", "l", "pm-composer-label", label);
  b.leaf("input", `in-${pm.mode}-${pm.target}`, "text-input pm-name", "");
  b.attr("spellcheck", "false");
  b.attr("placeholder", pm.mode === "file" ? t("new file name") : t("project name"));
  b.prop("value", pm.draft);
  b.prop("focus", "true");
  b.on("input", (e) => {
    pm.draft = e.value;
  });
  b.on("keydown", (e) => {
    if (e.key === "Enter") {
      pm.draft = e.value;
      confirmCompose();
    }
  });
  button(b, "ok", "gold", action, t("Confirm (Enter)"), () => {
    confirmCompose();
  });
  button(b, "cancel", "ghost", t("Cancel"), t("Cancel (Esc)"), () => {
    pm.mode = "";
    invalidate();
  });
  b.close();
}

/** The report of the last import. */
/** function importNote(b: Builder, im: ImportNote) => Undefined */
function importNote(b, im) {
  const n = im.warnings.length;
  b.open("div", "imported", "pm-imported");
  b.open("div", "mark", "pm-imp-mark");
  glyph(b, "spark");
  b.close();
  b.open("div", "txt", "pm-imp-text");
  b.leaf("b", "t", "", im.into ? tf("The parts of “{0}” were added to this song", [im.source]) : tf("“{0}” was imported as “{1}”", [im.source, im.name]));
  const summary =
    n === 0
      ? t("Everything came across cleanly.")
      : n === 1
        ? t("1 note on what was approximated or left out:")
        : tf("{0} notes on what was approximated or left out:", [String(n)]);
  b.leaf("span", "s", "", summary);
  if (n > 0) {
    b.open("ul", "w", "pm-warnings");
    for (let i = 0; i < n; i++) b.leaf("li", `w${i}`, "", im.warnings[i]);
    b.close();
  }
  b.close();
  b.open("div", "act", "pm-imp-actions");
  if (!im.into) {
    button(b, "open", "gold", t("Open it"), tf("Open “{0}”", [im.name]), () => {
      openProject(im.name);
    });
  }
  button(b, "dismiss", "ghost", t("Dismiss"), t("Hide this report"), () => {
    pm.imported = [];
    invalidate();
  });
  b.close();
  b.close();
}

/** Cover art: the song's colors, monogram and a waveform drawn from its name. */
/** function cover(b: Builder, p: ProjectInfo) => Undefined */
function cover(b, p) {
  const h = hash(p.name);
  const c1 = PALETTE[Math.floor(h % PALETTE.length)];
  const c2 = PALETTE[Math.floor((h / 7) % PALETTE.length)];
  b.open("div", "cover", "pm-cover");
  b.style("--c1", c1);
  b.style("--c2", c2);
  b.open("div", "wave", "pm-wave");
  for (let k = 0; k < 36; k++) {
    const v = 14 + Math.round(78 * Math.abs(Math.sin(k * 0.47 + h * 0.013) * Math.cos(k * 0.19 + h * 0.029)));
    b.leaf("i", `w${k}`, "", "");
    b.style("height", `${v}%`);
  }
  b.close();
  b.leaf("span", "mono", "pm-mono", p.title.length > 0 ? p.title.slice(0, 1).toUpperCase() : "·");
  if (p.current) b.leaf("span", "badge", "pm-badge", t("Open now"));
  else if (p.invalid) b.leaf("span", "badge", "pm-badge bad", t("Needs repair"));
  b.close();
}

/** function card(b: Builder, p: ProjectInfo) => Undefined */
function card(b, p) {
  const busy = pm.busy !== "";
  b.open("div", `p-${p.name}`, p.current ? "pm-card current" : "pm-card");
  b.attr("title", tf("{0}\nDouble-click to open", [p.folder]));
  b.on("dblclick", (e) => {
    if (!p.current && !busy) openProject(p.name);
  });
  cover(b, p);
  b.open("div", "body", "pm-card-body");
  b.leaf("div", "t", "pm-card-title", p.title);
  b.leaf("div", "f", "pm-card-folder", p.name === p.title ? " " : p.name);
  b.open("div", "meta", "pm-card-meta");
  b.leaf("span", "bpm", "pm-chip", `${fmt(p.bpm, p.bpm === Math.round(p.bpm) ? 0 : 1)} BPM`);
  b.leaf("span", "len", "pm-chip", duration(p));
  b.leaf("span", "ch", "pm-chip", tf("{0} ch · {1} pat", [String(p.channels), String(p.patterns)]));
  b.close();
  b.close();
  b.open("div", "foot", "pm-card-foot");
  b.leaf("span", "ago", "pm-ago", ago(p.modified));
  b.attr("title", tf("Last saved {0}", [fmtDate(p.modified)]));
  b.leaf("span", "sp", "spacer", "");
  if (!p.current) {
    button(b, "open", "small gold", t("Open"), tf("Open “{0}” in the studio", [p.title]), () => {
      openProject(p.name);
    });
  }
  iconButton(b, "zip", "small ghost", "export", tf("Download “{0}” as a .zip — a backup, or to open it in another studio", [p.name]), () => exportProject(p));
  iconButton(b, "dup", "small ghost", "copy", tf("Duplicate “{0}”", [p.name]), () =>
    compose("duplicate", p.name, uniqueName(`${nameFrom(p.title)} copy`, pm.projects))
  );
  iconButton(b, "ren", "small ghost", "draw", tf("Rename “{0}”", [p.name]), () => compose("rename", p.name, p.name));
  iconButton(b, "del", "small ghost danger", "trash", p.current ? t("The open project cannot be deleted") : tf("Move “{0}” to the trash", [p.name]), () =>
    deleteProject(p)
  );
  if (p.current) b.attr("disabled", "true");
  b.close();
  b.close();
}

/** function projectsView(b: Builder) => Undefined */
function projectsView(b) {
  b.open("div", "tools", "pm-tools");
  b.open("button", "new", "btn gold");
  b.attr("title", t("Create an empty project and open it"));
  b.on("click", (e) => compose("new", "", uniqueName("Untitled", pm.projects)));
  glyph(b, "plus");
  b.leaf("span", "t", "", t("New project"));
  b.close();
  button(b, "demo", "", t("New from demo"), t("Create a project from the bundled demo song"), () => compose("demo", "", uniqueName(pm.demoTitle, pm.projects)));
  b.open("button", "import", "btn");
  b.attr("title", t("Import an LMMS project (.mmp, .mmpz), a MIDI file (.mid) or a project .zip as a new project"));
  b.on("click", (e) => importProject());
  icon(b, ICON_IMPORT);
  b.leaf("span", "t", "", t("Import…"));
  b.close();
  b.leaf("div", "sp", "spacer", "");
  b.open("label", "search", "pm-search");
  icon(b, ICON_SEARCH);
  b.leaf("input", "q", "", "");
  b.attr("placeholder", t("Filter projects"));
  b.attr("spellcheck", "false");
  b.prop("value", pm.filter);
  b.on("input", (e) => {
    pm.filter = e.value;
    invalidate();
  });
  b.close();
  b.close();

  if (pm.mode !== "" && pm.mode !== "file") composer(b);
  for (const im of pm.imported) {
    if (!im.into) importNote(b, im);
  }

  const q = pm.filter.trim().toLowerCase();
  b.open("div", "grid", "pm-grid");
  let shown = 0;
  for (const p of pm.projects) {
    if (q !== "" && !p.title.toLowerCase().includes(q) && !p.name.toLowerCase().includes(q)) continue;
    shown = shown + 1;
    card(b, p);
  }
  b.close();
  if (pm.loaded && shown === 0) {
    b.open("div", "empty", "pm-empty");
    b.leaf("h3", "h", "", q !== "" ? t("No project matches") : t("The library is empty"));
    b.leaf(
      "p",
      "p",
      "",
      q !== "" ? tf("Nothing is called “{0}”.", [pm.filter]) : t("Create a project, start from the demo, or import an LMMS or MIDI file or a project .zip.")
    );
    b.close();
  }
}

/** function fileRow(b: Builder, f: FileInfo) => Undefined */
function fileRow(b, f) {
  const playing = pm.preview === f.path;
  const url = `/files/${encodePath(f.path)}`;
  b.open("div", `f-${f.path}`, playing ? "pm-file playing" : "pm-file");
  if (f.kind === "audio") {
    iconButton(b, "play", playing ? "small on" : "small", playing ? "stop" : "play", playing ? t("Stop the preview") : tf("Listen to {0}", [f.name]), () =>
      togglePreview(f)
    );
  } else {
    b.open("span", "doc", "pm-doc");
    icon(b, ICON_DOC);
    b.close();
  }
  b.open("div", "name", "pm-file-name");
  b.leaf("span", "t", "", f.name);
  b.attr("title", f.path);
  if (f.used) b.leaf("span", "used", "pm-tag used", t("in the song"));
  if (f.managed) b.leaf("span", "managed", "pm-tag", t("studio file"));
  b.close();
  b.leaf("span", "size", "pm-size", bytes(f.size));
  b.leaf("span", "mod", "pm-date", ago(f.modified));
  b.attr("title", fmtDate(f.modified));
  b.open("div", "act", "pm-file-actions");
  iconButton(b, "dl", "small ghost", "export", tf("Download {0}", [f.name]), () => {
    download(url, f.name);
  });
  iconButton(b, "ren", "small ghost", "draw", f.managed ? t("Managed by the studio") : tf("Rename {0}", [f.name]), () => compose("file", f.path, f.name));
  if (f.managed) b.attr("disabled", "true");
  iconButton(b, "del", "small ghost danger", "trash", f.managed ? t("Managed by the studio") : tf("Move {0} to the trash", [f.name]), () => deleteFile(f));
  if (f.managed) b.attr("disabled", "true");
  b.close();
  b.close();
}

/** The folders to show, in order: samples, renders, others, then the top level. */
/** function folders(files: FileInfo[]) => String[] */
function folders(files) {
  /** const out: String[] */
  const out = ["samples", "renders"];
  /** const extra: String[] */
  const extra = [];
  for (const f of files) {
    if (f.dir !== "" && !out.includes(f.dir) && !extra.includes(f.dir)) extra.push(f.dir);
  }
  extra.sort();
  for (const d of extra) out.push(d);
  out.push("");
  return out;
}

/** function filesView(b: Builder) => Undefined */
function filesView(b) {
  let total = 0;
  for (const f of pm.files) total = total + f.size;
  b.open("div", "tools", "pm-tools");
  b.open("div", "what", "pm-files-title");
  b.leaf("b", "t", "", state.project.meta.title);
  b.leaf("span", "s", "", tf("{0} files · {1}", [String(pm.files.length), bytes(total)]));
  b.close();
  b.leaf("div", "sp", "spacer", "");
  b.open("button", "audio", "btn gold");
  b.attr("title", t("Copy audio files into samples/"));
  b.on("click", (e) => importAudio());
  icon(b, ICON_IMPORT);
  b.leaf("span", "t", "", t("Import audio…"));
  b.close();
  b.open("button", "midi", "btn");
  b.attr("title", t("Add the parts of a MIDI file to this song as new channels and patterns (Ctrl+Z undoes it)"));
  b.on("click", (e) => importMidiHere());
  glyph(b, "piano");
  b.leaf("span", "t", "", t("Add MIDI parts…"));
  b.close();
  iconButton(b, "refresh", "", "restart", t("Refresh the list"), () => {
    refreshFiles();
  });
  b.close();

  if (pm.mode === "file") composer(b);
  for (const im of pm.imported) {
    if (im.into) importNote(b, im);
  }

  b.open("div", "list", "pm-files");
  for (const dir of folders(pm.files)) {
    /** const inDir: FileInfo[] */
    const inDir = [];
    let size = 0;
    for (const f of pm.files) {
      if (f.dir === dir) {
        inDir.push(f);
        size = size + f.size;
      }
    }
    if (dir !== "samples" && dir !== "renders" && inDir.length === 0) continue;
    b.open("section", `d-${dir}`, "pm-folder");
    b.open("div", "head", "pm-folder-head");
    b.open("span", "i", "pm-folder-icon");
    icon(b, ICON_OPEN);
    b.close();
    b.leaf("b", "n", "", dir === "" ? t("Project folder") : `${dir}/`);
    const count =
      inDir.length === 0 ? t("empty") : inDir.length === 1 ? tf("1 file · {0}", [bytes(size)]) : tf("{0} files · {1}", [String(inDir.length), bytes(size)]);
    b.leaf("span", "c", "", count);
    b.close();
    if (inDir.length === 0) {
      b.leaf(
        "div",
        "none",
        "pm-none",
        dir === "samples" ? t("No samples yet — import audio, or let the agent render some.") : t("Exports and mixdowns land here.")
      );
    }
    for (const f of inDir) fileRow(b, f);
    b.close();
  }
  if (!pm.filesLoaded) b.leaf("div", "loading", "pm-none", t("Reading the project folder…"));
  b.close();
}

/** The Projects window (nothing when closed). */
/** function projectsOverlay(b: Builder) => Undefined */
export function projectsOverlay(b) {
  if (!pm.open) return undefined;
  // New or removed samples (uploads, recordings, the agent) refresh the list.
  const key = state.samples.join("|");
  if (key !== pm.samplesKey) {
    pm.samplesKey = key;
    if (pm.tab === "files") refreshFiles();
  }
  b.open("div", "pm-overlay", "overlay pm-overlay");
  b.leaf("div", "scrim", "pm-scrim", "");
  b.on("click", (e) => closeProjects());
  b.open("div", "dialog", "pm");
  b.attr("role", "dialog");
  b.attr("aria-label", t("Projects"));

  b.open("header", "head", "pm-head");
  b.open("div", "mark", "pm-mark");
  glyph(b, "folder");
  b.close();
  b.open("div", "title", "pm-title");
  b.leaf("h2", "h", "", pm.tab === "files" ? t("Files") : t("Projects"));
  b.leaf("span", "lib", "pm-lib", pm.tab === "files" ? state.folder : pm.library);
  b.close();
  b.leaf("div", "sp", "spacer", "");
  if (pm.busy !== "") {
    b.open("span", "busy", "pm-busy");
    b.leaf("i", "spin", "pm-spin", "");
    b.leaf("span", "t", "", pm.busy);
    b.close();
  }
  b.open("div", "tabs", "seg pm-tabs");
  button(b, "projects", pm.tab === "projects" ? "on" : "", tf("Projects · {0}", [String(pm.projects.length)]), t("The project library"), () =>
    showTab("projects")
  );
  button(b, "files", pm.tab === "files" ? "on" : "", t("Files"), t("Files of the open project"), () => showTab("files"));
  b.close();
  iconButton(b, "close", "ghost", "close", t("Close (Esc)"), closeProjects);
  b.close();

  b.open("div", "body", pm.busy !== "" ? "pm-body busy" : "pm-body");
  if (pm.tab === "files") filesView(b);
  else projectsView(b);
  b.close();

  b.open("footer", "foot", "pm-foot");
  b.leaf(
    "span",
    "l",
    "",
    pm.tab === "files"
      ? t("Deleted files go to .trash/ in the project · renaming a sample updates the song")
      : t("Double-click a card to open it · deleted projects go to the library's .trash/")
  );
  button(
    b,
    "trash",
    "small ghost",
    t("Empty trash"),
    pm.tab === "files" ? t("Delete this project's deleted files for good") : t("Delete the projects in the library's trash for good"),
    () => emptyTrash(pm.tab === "files" ? "project" : "library")
  );
  if (state.backend === "local" && pm.storage.quota > 0) {
    b.leaf("span", "store", "pm-store", tf("Browser storage · {0} of {1}", [bytes(pm.storage.usage), bytes(pm.storage.quota)]));
    b.attr("title", t("Projects are saved in this browser. Download them as .zip files to back them up."));
  }
  b.leaf("span", "sp", "spacer", "");
  b.open("span", "k", "pm-keys");
  b.leaf("kbd", "k1", "", "Ctrl+O");
  b.leaf("span", "t1", "", t("toggle"));
  b.leaf("kbd", "k2", "", "Esc");
  b.leaf("span", "t2", "", t("close"));
  b.close();
  b.close();

  b.close();
  b.close();
}
