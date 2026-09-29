// The Projects window: the project library (open, create, duplicate,
// rename, delete, import LMMS / MIDI) and the file manager of the open
// project. Opened from the top bar or with Ctrl+O; drawn by the shell.
//
// The server owns the library (GET /api/projects, /api/files, ...); this
// module keeps a copy of what it last listed and asks again after every
// action. Opening a project makes the server broadcast `switched`, which
// reloads every client (net.js) and calls `projectSwitched` here.

import { getJson, sendJson, uploadFile, pickFiles, download, previewAudio, stopPreview, fmtDate, fmt, confirmBox, listenWindow } from "#platform";
import { state, invalidate, hint } from "../store.js";
import { PALETTE } from "../model.js";
import { glyph, iconButton, button } from "./widgets.js";
import { toast } from "./toast.js";

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
  return undefined;
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
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  if (s < 86400 * 2) return "yesterday";
  if (s < 86400 * 7) return `${Math.floor(s / 86400)} days ago`;
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
  if (p.bpm <= 0 || p.lengthBeats <= 0) return "empty";
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

function refreshProjects() {
  getJson("/api/projects")
    .then((r) => {
      pm.library = String(r.library);
      pm.current = String(r.current);
      pm.projects = r.projects;
      pm.loaded = true;
      invalidate();
      return Promise.resolve(true);
    })
    .catch((e) => {
      toast("Could not list the projects", errText(e), "error");
      return Promise.resolve(false);
    });
}

function refreshFiles() {
  getJson("/api/files")
    .then((r) => {
      pm.files = r.files;
      pm.filesLoaded = true;
      invalidate();
      return Promise.resolve(true);
    })
    .catch((e) => {
      toast("Could not list the files", errText(e), "error");
      return Promise.resolve(false);
    });
}

/** Run a request with a busy indicator; errors become toasts. */
/** function act<T>(label: String, p: Promise<T>, done: (T) => Undefined) => Undefined */
function act(label, p, done) {
  pm.busy = label;
  invalidate();
  p.then((r) => {
    pm.busy = "";
    done(r);
    invalidate();
    return Promise.resolve(true);
  }).catch((e) => {
    pm.busy = "";
    toast(label.replace("…", " failed"), errText(e), "error");
    invalidate();
    return Promise.resolve(false);
  });
  return undefined;
}

/** function openProject(name: String) => Undefined */
function openProject(name) {
  act(`Opening “${name}”…`, sendJson("/api/projects/open", "POST", { name: name }), (r) => {
    pm.imported = [];
    closeProjects();
    return undefined;
  });
  return undefined;
}

/** function createProject(name: String, demo: Boolean) => Undefined */
function createProject(name, demo) {
  act(`Creating “${name}”…`, sendJson("/api/projects", "POST", { name: name, demo: demo }), (r) => {
    openProject(String(r.name));
    return undefined;
  });
  return undefined;
}

/** function duplicateProject(name: String, to: String) => Undefined */
function duplicateProject(name, to) {
  act(`Duplicating “${name}”…`, sendJson("/api/projects/duplicate", "POST", { name: name, to: to }), (r) => {
    toast("Project duplicated", `“${name}” → “${String(r.name)}”`, "info");
    refreshProjects();
    return undefined;
  });
  return undefined;
}

/** function renameProject(name: String, to: String) => Undefined */
function renameProject(name, to) {
  act(`Renaming “${name}”…`, sendJson("/api/projects/rename", "POST", { name: name, to: to }), (r) => {
    toast("Project renamed", `“${name}” → “${String(r.name)}”`, "info");
    refreshProjects();
    return undefined;
  });
  return undefined;
}

/** function deleteProject(p: ProjectInfo) => Undefined */
function deleteProject(p) {
  if (!confirmBox(`Move “${p.title}” (${p.name}) to the library trash?\n\nIt is kept in ${pm.library}/.trash and can be restored from there.`)) return undefined;
  act(`Deleting “${p.name}”…`, sendJson(`/api/projects/${encodeURIComponent(p.name)}`, "DELETE", {}), (r) => {
    toast("Moved to the trash", p.name, "info");
    refreshProjects();
    return undefined;
  });
  return undefined;
}

/** function importKind(fileName: String) => String */
function importKind(fileName) {
  const lower = fileName.toLowerCase();
  if (lower.endsWith(".mid") || lower.endsWith(".midi") || lower.endsWith(".kar") || lower.endsWith(".rmi")) return "midi";
  return "lmms";
}

/** Import an LMMS project or a MIDI file as a new project. */
function importProject() {
  pickFiles(".mmp,.mmpz,.mid,.midi,.kar,.rmi", (files) => {
    if (files.length === 0) return undefined;
    const f = files[0];
    const kind = importKind(f.name);
    act(`Importing ${f.name}…`, uploadFile(`/api/projects/import-${kind}?filename=${encodeURIComponent(f.name)}`, f), (r) => {
      pm.imported = [{ name: String(r.name), source: f.name, warnings: r.warnings, into: false }];
      pm.tab = "projects";
      refreshProjects();
      return undefined;
    });
    return undefined;
  });
  return undefined;
}

/** Add the parts of a MIDI file to the open song (one undo step). */
function importMidiHere() {
  pickFiles(".mid,.midi,.kar,.rmi", (files) => {
    if (files.length === 0) return undefined;
    const f = files[0];
    act(`Adding ${f.name}…`, uploadFile("/api/import-midi?into=current", f), (r) => {
      pm.imported = [{ name: "", source: f.name, warnings: r.warnings, into: true }];
      toast("MIDI parts added", `${String(r.channels)} new channel(s) from ${f.name} — Ctrl+Z undoes it`, "info");
      return undefined;
    });
    return undefined;
  });
  return undefined;
}

/** Upload audio files into samples/. */
function importAudio() {
  pickFiles("audio/*", (files) => {
    for (const f of files) {
      act(`Uploading ${f.name}…`, uploadFile(`/api/samples?name=${encodeURIComponent(f.name)}`, f), (r) => {
        toast("Sample added", String(r.path), "info");
        refreshFiles();
        return undefined;
      });
    }
    return undefined;
  });
  return undefined;
}

/** function renameFile(path: String, to: String) => Undefined */
function renameFile(path, to) {
  act(`Renaming ${path}…`, sendJson("/api/files/rename", "POST", { path: path, to: to }), (r) => {
    const refs = Number(r.references);
    toast("File renamed", refs > 0 ? `${String(r.path)} — ${refs} reference(s) in the song updated` : String(r.path), "info");
    refreshFiles();
    return undefined;
  });
  return undefined;
}

/** function deleteFile(f: FileInfo) => Undefined */
function deleteFile(f) {
  const warn = f.used ? "\n\nThe song uses this file: those parts will fall silent." : "";
  if (!confirmBox(`Move ${f.path} to the project's trash (.trash/)?${warn}`)) return undefined;
  if (pm.preview === f.path) stopPlaying();
  act(`Deleting ${f.name}…`, sendJson(`/api/files?path=${encodeURIComponent(f.path)}`, "DELETE", {}), (r) => {
    toast("Moved to the trash", String(r.trashed), "info");
    refreshFiles();
    return undefined;
  });
  return undefined;
}

// ------------------------------------------------------------------ opening

export function openProjects() {
  pm.open = true;
  refreshProjects();
  if (pm.tab === "files") refreshFiles();
  invalidate();
  return undefined;
}

export function closeProjects() {
  pm.open = false;
  pm.mode = "";
  stopPlaying();
  invalidate();
  return undefined;
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
  toast(`Opened “${state.project.meta.title}”`, state.folder, "info");
  return undefined;
}

/** function showTab(tab: String) => Undefined */
function showTab(tab) {
  pm.tab = tab;
  pm.mode = "";
  if (tab === "files") refreshFiles();
  invalidate();
  return undefined;
}

/** function compose(mode: String, target: String, draft: String) => Undefined */
function compose(mode, target, draft) {
  pm.mode = mode;
  pm.target = target;
  pm.draft = draft;
  invalidate();
  return undefined;
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
  return undefined;
}

function stopPlaying() {
  stopPreview();
  pm.preview = "";
  return undefined;
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
      return undefined;
    });
  }
  invalidate();
  return undefined;
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
  return undefined;
});

// ------------------------------------------------------------------ views

/** The top-bar button. */
/** function projectsButton(b: Builder) => Undefined */
export function projectsButton(b) {
  b.open("button", "projects", pm.open ? "btn pm-open on" : "btn pm-open");
  b.attr("title", "Projects and files (Ctrl+O)");
  b.on("pointerenter", (e) => hint("Projects — open, create, duplicate or import (LMMS, MIDI) songs, and manage this project's files (Ctrl+O)"));
  b.on("click", (e) => {
    if (pm.open) closeProjects();
    else openProjects();
    return undefined;
  });
  glyph(b, "folder");
  b.leaf("span", "t", "", "Projects");
  b.close();
  return undefined;
}

/** The name field shared by New / Duplicate / Rename. */
/** function composer(b: Builder) => Undefined */
function composer(b) {
  let label = "Name your new project";
  let action = "Create & open";
  if (pm.mode === "demo") label = "Name the new project (a copy of the demo song)";
  if (pm.mode === "duplicate") {
    label = `Duplicate “${pm.target}” as`;
    action = "Duplicate";
  }
  if (pm.mode === "rename") {
    label = `Rename “${pm.target}” to`;
    action = "Rename";
  }
  if (pm.mode === "file") {
    label = `Rename ${pm.target} to`;
    action = "Rename";
  }
  b.open("div", "composer", "pm-composer");
  b.leaf("label", "l", "pm-composer-label", label);
  b.leaf("input", `in-${pm.mode}-${pm.target}`, "text-input pm-name", "");
  b.attr("spellcheck", "false");
  b.attr("placeholder", pm.mode === "file" ? "new file name" : "project name");
  b.prop("value", pm.draft);
  b.prop("focus", "true");
  b.on("input", (e) => {
    pm.draft = e.value;
    return undefined;
  });
  b.on("keydown", (e) => {
    if (e.key === "Enter") {
      pm.draft = e.value;
      confirmCompose();
    }
    return undefined;
  });
  button(b, "ok", "gold", action, "Confirm (Enter)", () => {
    confirmCompose();
    return undefined;
  });
  button(b, "cancel", "ghost", "Cancel", "Cancel (Esc)", () => {
    pm.mode = "";
    invalidate();
    return undefined;
  });
  b.close();
  return undefined;
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
  b.leaf("b", "t", "", im.into ? `The parts of “${im.source}” were added to this song` : `“${im.source}” was imported as “${im.name}”`);
  b.leaf("span", "s", "", n === 0 ? "Everything came across cleanly." : `${n} note${n === 1 ? "" : "s"} on what was approximated or left out:`);
  if (n > 0) {
    b.open("ul", "w", "pm-warnings");
    for (let i = 0; i < n; i++) b.leaf("li", `w${i}`, "", im.warnings[i]);
    b.close();
  }
  b.close();
  b.open("div", "act", "pm-imp-actions");
  if (!im.into) {
    button(b, "open", "gold", "Open it", `Open “${im.name}”`, () => {
      openProject(im.name);
      return undefined;
    });
  }
  button(b, "dismiss", "ghost", "Dismiss", "Hide this report", () => {
    pm.imported = [];
    invalidate();
    return undefined;
  });
  b.close();
  b.close();
  return undefined;
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
  if (p.current) b.leaf("span", "badge", "pm-badge", "Open now");
  else if (p.invalid) b.leaf("span", "badge", "pm-badge bad", "Needs repair");
  b.close();
  return undefined;
}

/** function card(b: Builder, p: ProjectInfo) => Undefined */
function card(b, p) {
  const busy = pm.busy !== "";
  b.open("div", `p-${p.name}`, p.current ? "pm-card current" : "pm-card");
  b.attr("title", `${p.folder}\nDouble-click to open`);
  b.on("dblclick", (e) => {
    if (!p.current && !busy) openProject(p.name);
    return undefined;
  });
  cover(b, p);
  b.open("div", "body", "pm-card-body");
  b.leaf("div", "t", "pm-card-title", p.title);
  b.leaf("div", "f", "pm-card-folder", p.name === p.title ? " " : p.name);
  b.open("div", "meta", "pm-card-meta");
  b.leaf("span", "bpm", "pm-chip", `${fmt(p.bpm, p.bpm === Math.round(p.bpm) ? 0 : 1)} BPM`);
  b.leaf("span", "len", "pm-chip", duration(p));
  b.leaf("span", "ch", "pm-chip", `${p.channels} ch · ${p.patterns} pat`);
  b.close();
  b.close();
  b.open("div", "foot", "pm-card-foot");
  b.leaf("span", "ago", "pm-ago", ago(p.modified));
  b.attr("title", `Last saved ${fmtDate(p.modified)}`);
  b.leaf("span", "sp", "spacer", "");
  if (!p.current) {
    button(b, "open", "small gold", "Open", `Open “${p.title}” in the studio`, () => {
      openProject(p.name);
      return undefined;
    });
  }
  iconButton(b, "dup", "small ghost", "copy", `Duplicate “${p.name}”`, () => compose("duplicate", p.name, uniqueName(`${p.name} copy`, pm.projects)));
  iconButton(b, "ren", "small ghost", "draw", `Rename “${p.name}”`, () => compose("rename", p.name, p.name));
  iconButton(b, "del", "small ghost danger", "trash", p.current ? "The open project cannot be deleted" : `Move “${p.name}” to the trash`, () => deleteProject(p));
  if (p.current) b.attr("disabled", "true");
  b.close();
  b.close();
  return undefined;
}

/** function projectsView(b: Builder) => Undefined */
function projectsView(b) {
  b.open("div", "tools", "pm-tools");
  b.open("button", "new", "btn gold");
  b.attr("title", "Create an empty project and open it");
  b.on("click", (e) => compose("new", "", uniqueName("Untitled", pm.projects)));
  glyph(b, "plus");
  b.leaf("span", "t", "", "New project");
  b.close();
  button(b, "demo", "", "New from demo", "Create a project from the bundled demo song", () => compose("demo", "", uniqueName("Demo", pm.projects)));
  b.open("button", "import", "btn");
  b.attr("title", "Import an LMMS project (.mmp, .mmpz) or a MIDI file (.mid) as a new project");
  b.on("click", (e) => importProject());
  icon(b, ICON_IMPORT);
  b.leaf("span", "t", "", "Import LMMS / MIDI…");
  b.close();
  b.leaf("div", "sp", "spacer", "");
  b.open("label", "search", "pm-search");
  icon(b, ICON_SEARCH);
  b.leaf("input", "q", "", "");
  b.attr("placeholder", "Filter projects");
  b.attr("spellcheck", "false");
  b.prop("value", pm.filter);
  b.on("input", (e) => {
    pm.filter = e.value;
    invalidate();
    return undefined;
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
    b.leaf("h3", "h", "", q !== "" ? "No project matches" : "The library is empty");
    b.leaf("p", "p", "", q !== "" ? `Nothing is called “${pm.filter}”.` : "Create a project, start from the demo, or import an LMMS or MIDI file.");
    b.close();
  }
  return undefined;
}

/** function fileRow(b: Builder, f: FileInfo) => Undefined */
function fileRow(b, f) {
  const playing = pm.preview === f.path;
  const url = `/files/${encodePath(f.path)}`;
  b.open("div", `f-${f.path}`, playing ? "pm-file playing" : "pm-file");
  if (f.kind === "audio") {
    iconButton(b, "play", playing ? "small on" : "small", playing ? "stop" : "play", playing ? "Stop the preview" : `Listen to ${f.name}`, () => togglePreview(f));
  } else {
    b.open("span", "doc", "pm-doc");
    icon(b, ICON_DOC);
    b.close();
  }
  b.open("div", "name", "pm-file-name");
  b.leaf("span", "t", "", f.name);
  b.attr("title", f.path);
  if (f.used) b.leaf("span", "used", "pm-tag used", "in the song");
  if (f.managed) b.leaf("span", "managed", "pm-tag", "studio file");
  b.close();
  b.leaf("span", "size", "pm-size", bytes(f.size));
  b.leaf("span", "mod", "pm-date", ago(f.modified));
  b.attr("title", fmtDate(f.modified));
  b.open("div", "act", "pm-file-actions");
  iconButton(b, "dl", "small ghost", "export", `Download ${f.name}`, () => {
    download(url, f.name);
    return undefined;
  });
  iconButton(b, "ren", "small ghost", "draw", f.managed ? "Managed by the studio" : `Rename ${f.name}`, () => compose("file", f.path, f.name));
  if (f.managed) b.attr("disabled", "true");
  iconButton(b, "del", "small ghost danger", "trash", f.managed ? "Managed by the studio" : `Move ${f.name} to the trash`, () => deleteFile(f));
  if (f.managed) b.attr("disabled", "true");
  b.close();
  b.close();
  return undefined;
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
  b.leaf("span", "s", "", `${pm.files.length} files · ${bytes(total)}`);
  b.close();
  b.leaf("div", "sp", "spacer", "");
  b.open("button", "audio", "btn gold");
  b.attr("title", "Copy audio files into samples/");
  b.on("click", (e) => importAudio());
  icon(b, ICON_IMPORT);
  b.leaf("span", "t", "", "Import audio…");
  b.close();
  b.open("button", "midi", "btn");
  b.attr("title", "Add the parts of a MIDI file to this song as new channels and patterns (Ctrl+Z undoes it)");
  b.on("click", (e) => importMidiHere());
  glyph(b, "piano");
  b.leaf("span", "t", "", "Add MIDI parts…");
  b.close();
  iconButton(b, "refresh", "", "restart", "Refresh the list", () => {
    refreshFiles();
    return undefined;
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
    b.leaf("b", "n", "", dir === "" ? "Project folder" : `${dir}/`);
    b.leaf("span", "c", "", inDir.length === 0 ? "empty" : `${inDir.length} file${inDir.length === 1 ? "" : "s"} · ${bytes(size)}`);
    b.close();
    if (inDir.length === 0) {
      b.leaf("div", "none", "pm-none", dir === "samples" ? "No samples yet — import audio, or let the agent render some." : "Exports and mixdowns land here.");
    }
    for (const f of inDir) fileRow(b, f);
    b.close();
  }
  if (!pm.filesLoaded) b.leaf("div", "loading", "pm-none", "Reading the project folder…");
  b.close();
  return undefined;
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
  b.attr("aria-label", "Projects");

  b.open("header", "head", "pm-head");
  b.open("div", "mark", "pm-mark");
  glyph(b, "folder");
  b.close();
  b.open("div", "title", "pm-title");
  b.leaf("h2", "h", "", pm.tab === "files" ? "Files" : "Projects");
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
  button(b, "projects", pm.tab === "projects" ? "on" : "", `Projects · ${pm.projects.length}`, "The project library", () => showTab("projects"));
  button(b, "files", pm.tab === "files" ? "on" : "", "Files", "Files of the open project", () => showTab("files"));
  b.close();
  iconButton(b, "close", "ghost", "close", "Close (Esc)", closeProjects);
  b.close();

  b.open("div", "body", pm.busy !== "" ? "pm-body busy" : "pm-body");
  if (pm.tab === "files") filesView(b);
  else projectsView(b);
  b.close();

  b.open("footer", "foot", "pm-foot");
  b.leaf("span", "l", "", pm.tab === "files" ? "Deleted files go to .trash/ in the project · renaming a sample updates the song" : "Double-click a card to open it · deleted projects go to the library's .trash/");
  b.leaf("span", "sp", "spacer", "");
  b.open("span", "k", "pm-keys");
  b.leaf("kbd", "k1", "", "Ctrl+O");
  b.leaf("span", "t1", "", "toggle");
  b.leaf("kbd", "k2", "", "Esc");
  b.leaf("span", "t2", "", "close");
  b.close();
  b.close();

  b.close();
  b.close();
  return undefined;
}
