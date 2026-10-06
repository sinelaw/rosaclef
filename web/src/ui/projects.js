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
  if (s < 60) return t("projects.ago.justNow");
  if (s < 3600) return tf("projects.ago.minutes", [String(Math.floor(s / 60))]);
  if (s < 86400) return tf("projects.ago.hours", [String(Math.floor(s / 3600))]);
  if (s < 86400 * 2) return t("projects.ago.yesterday");
  if (s < 86400 * 7) return tf("projects.ago.days", [String(Math.floor(s / 86400))]);
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
  if (p.bpm <= 0 || p.lengthBeats <= 0) return t("common.empty");
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
      toast(t("projects.list.failed.title"), errText(e), "error");
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
      toast(t("projects.files.list.failed.title"), errText(e), "error");
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
  act(tf("projects.open.busy", [name]), tf("projects.open.failed", [name]), sendJson("/api/projects/open", "POST", { name: name }), (r) => {
    pm.imported = [];
    closeProjects();
  });
}

/** function createProject(name: String, demo: Boolean) => Undefined */
function createProject(name, demo) {
  act(tf("projects.create.busy", [name]), tf("projects.create.failed", [name]), sendJson("/api/projects", "POST", { name: name, demo: demo }), (r) => {
    openProject(String(r.name));
  });
}

/** function duplicateProject(name: String, to: String) => Undefined */
function duplicateProject(name, to) {
  act(
    tf("projects.duplicate.busy", [name]),
    tf("projects.duplicate.failed", [name]),
    sendJson("/api/projects/duplicate", "POST", { name: name, to: to }),
    (r) => {
      toast(t("projects.duplicate.done.title"), `“${name}” → “${String(r.name)}”`, "info");
      refreshProjects();
    }
  );
}

/** function renameProject(name: String, to: String) => Undefined */
function renameProject(name, to) {
  act(tf("projects.rename.busy", [name]), tf("projects.rename.failed", [name]), sendJson("/api/projects/rename", "POST", { name: name, to: to }), (r) => {
    toast(t("projects.rename.done.title"), `“${name}” → “${String(r.name)}”`, "info");
    refreshProjects();
  });
}

/** function deleteProject(p: ProjectInfo) => Undefined */
function deleteProject(p) {
  const kept = state.backend === "local" ? t("projects.delete.confirm.keptLocal") : tf("projects.delete.confirm.keptLibrary", [pm.library]);
  if (!confirmBox(tf("projects.delete.confirm", [p.title, p.name, kept]))) return undefined;
  act(
    tf("projects.delete.busy", [p.name]),
    tf("projects.delete.failed", [p.name]),
    sendJson(`/api/projects/${encodeURIComponent(p.name)}`, "DELETE", {}),
    (r) => {
      toast(t("common.movedToTrash"), p.name, "info");
      refreshProjects();
    }
  );
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
      tf("projects.import.busy", [f.name]),
      tf("projects.import.failed", [f.name]),
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
    act(tf("projects.addMidi.busy", [f.name]), tf("projects.addMidi.failed", [f.name]), uploadFile("/api/import-midi?into=current", f), (r) => {
      pm.imported = [{ name: "", source: f.name, warnings: r.warnings, into: true }];
      toast(t("projects.addMidi.done.title"), tf("projects.addMidi.done.body", [String(r.channels), f.name]), "info");
    });
  });
}

/** Upload audio files into samples/. */
function importAudio() {
  pickFiles("audio/*", (files) => {
    for (const f of files) {
      act(
        tf("projects.importAudio.busy", [f.name]),
        tf("projects.importAudio.failed", [f.name]),
        uploadFile(`/api/samples?name=${encodeURIComponent(f.name)}`, f),
        (r) => {
          toast(t("browser.sampleAdded"), String(r.path), "info");
          refreshFiles();
        }
      );
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
  const question = scope === "library" ? t("projects.emptyTrash.confirm.library") : t("projects.emptyTrash.confirm.project");
  if (!confirmBox(question)) return undefined;
  act(t("projects.emptyTrash.busy"), t("projects.emptyTrash.failed"), sendJson("/api/trash/empty", "POST", { scope: scope }), (r) => {
    const n = Number(r.removed);
    const body = n === 0 ? "" : n === 1 ? t("projects.emptyTrash.done.body.one") : tf("projects.emptyTrash.done.body.other", [String(n)]);
    toast(n === 0 ? t("projects.emptyTrash.alreadyEmpty.title") : t("projects.emptyTrash.done.title"), body, "info");
    refreshStorage();
  });
}

/** function renameFile(path: String, to: String) => Undefined */
function renameFile(path, to) {
  act(
    tf("projects.file.rename.busy", [path]),
    tf("projects.file.rename.failed", [path]),
    sendJson("/api/files/rename", "POST", { path: path, to: to }),
    (r) => {
      const refs = Number(r.references);
      toast(t("projects.file.rename.done.title"), refs > 0 ? tf("projects.file.rename.done.body", [String(r.path), String(refs)]) : String(r.path), "info");
      refreshFiles();
    }
  );
}

/** function deleteFile(f: FileInfo) => Undefined */
function deleteFile(f) {
  const question = f.used ? tf("projects.file.delete.confirm.used", [f.path]) : tf("projects.file.delete.confirm", [f.path]);
  if (!confirmBox(question)) return undefined;
  if (pm.preview === f.path) stopPlaying();
  act(
    tf("projects.file.delete.busy", [f.name]),
    tf("projects.file.delete.failed", [f.name]),
    sendJson(`/api/files?path=${encodeURIComponent(f.path)}`, "DELETE", {}),
    (r) => {
      toast(t("common.movedToTrash"), String(r.trashed), "info");
      refreshFiles();
    }
  );
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
  toast(tf("projects.open.done.title", [state.project.meta.title]), state.folder, "info");
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
  b.attr("title", t("projects.button.title"));
  b.on("pointerenter", (e) => hint(t("projects.button.hint")));
  b.on("click", (e) => {
    if (pm.open) closeProjects();
    else openProjects();
  });
  glyph(b, "folder");
  b.leaf("span", "t", "", t("panel.projects"));
  b.close();
}

/** The name field shared by New / Duplicate / Rename. */
/** function composer(b: Builder) => Undefined */
function composer(b) {
  let label = t("projects.compose.new.label");
  let action = t("projects.compose.create.label");
  if (pm.mode === "demo") label = t("projects.compose.demo.label");
  if (pm.mode === "duplicate") {
    label = tf("projects.compose.duplicate.label", [pm.target]);
    action = t("common.duplicate");
  }
  if (pm.mode === "rename") {
    label = tf("projects.compose.rename.label", [pm.target]);
    action = t("common.rename");
  }
  if (pm.mode === "file") {
    label = tf("projects.compose.renameFile.label", [pm.target]);
    action = t("common.rename");
  }
  b.open("div", "composer", "pm-composer");
  b.leaf("label", "l", "pm-composer-label", label);
  b.leaf("input", `in-${pm.mode}-${pm.target}`, "text-input pm-name", "");
  b.attr("spellcheck", "false");
  b.attr("placeholder", pm.mode === "file" ? t("projects.compose.file.placeholder") : t("projects.compose.name.placeholder"));
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
  button(b, "ok", "gold", action, t("projects.compose.confirm.title"), () => {
    confirmCompose();
  });
  button(b, "cancel", "ghost", t("projects.compose.cancel.label"), t("projects.compose.cancel.title"), () => {
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
  b.leaf("b", "t", "", im.into ? tf("projects.imported.into.label", [im.source]) : tf("projects.imported.asNew.label", [im.source, im.name]));
  const summary = n === 0 ? t("projects.imported.clean") : n === 1 ? t("projects.imported.notes.one") : tf("projects.imported.notes.other", [String(n)]);
  b.leaf("span", "s", "", summary);
  if (n > 0) {
    b.open("ul", "w", "pm-warnings");
    for (let i = 0; i < n; i++) b.leaf("li", `w${i}`, "", im.warnings[i]);
    b.close();
  }
  b.close();
  b.open("div", "act", "pm-imp-actions");
  if (!im.into) {
    button(b, "open", "gold", t("projects.imported.open.label"), tf("projects.imported.open.title", [im.name]), () => {
      openProject(im.name);
    });
  }
  button(b, "dismiss", "ghost", t("projects.imported.dismiss.label"), t("projects.imported.dismiss.title"), () => {
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
  if (p.current) b.leaf("span", "badge", "pm-badge", t("projects.card.badge.openNow"));
  else if (p.invalid) b.leaf("span", "badge", "pm-badge bad", t("projects.card.badge.needsRepair"));
  b.close();
}

/** function card(b: Builder, p: ProjectInfo) => Undefined */
function card(b, p) {
  const busy = pm.busy !== "";
  b.open("div", `p-${p.name}`, p.current ? "pm-card current" : "pm-card");
  b.attr("title", tf("projects.card.title", [p.folder]));
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
  b.leaf("span", "ch", "pm-chip", tf("projects.card.counts", [String(p.channels), String(p.patterns)]));
  b.close();
  b.close();
  b.open("div", "foot", "pm-card-foot");
  b.leaf("span", "ago", "pm-ago", ago(p.modified));
  b.attr("title", tf("projects.card.saved.title", [fmtDate(p.modified)]));
  b.leaf("span", "sp", "spacer", "");
  if (!p.current) {
    button(b, "open", "small gold", t("common.open"), tf("projects.card.open.title", [p.title]), () => {
      openProject(p.name);
    });
  }
  iconButton(b, "zip", "small ghost", "export", tf("projects.card.zip.title", [p.name]), () => exportProject(p));
  iconButton(b, "dup", "small ghost", "copy", tf("projects.card.duplicate.title", [p.name]), () =>
    compose("duplicate", p.name, uniqueName(`${nameFrom(p.title)} copy`, pm.projects))
  );
  iconButton(b, "ren", "small ghost", "draw", tf("projects.card.rename.title", [p.name]), () => compose("rename", p.name, p.name));
  iconButton(b, "del", "small ghost danger", "trash", p.current ? t("projects.card.delete.titleOpen") : tf("projects.card.delete.title", [p.name]), () =>
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
  b.attr("title", t("projects.new.title"));
  b.on("click", (e) => compose("new", "", uniqueName("Untitled", pm.projects)));
  glyph(b, "plus");
  b.leaf("span", "t", "", t("projects.new.label"));
  b.close();
  button(b, "demo", "", t("projects.demo.label"), t("projects.demo.title"), () => compose("demo", "", uniqueName(pm.demoTitle, pm.projects)));
  b.open("button", "import", "btn");
  b.attr("title", t("projects.import.title"));
  b.on("click", (e) => importProject());
  icon(b, ICON_IMPORT);
  b.leaf("span", "t", "", t("projects.import.label"));
  b.close();
  b.leaf("div", "sp", "spacer", "");
  b.open("label", "search", "pm-search");
  icon(b, ICON_SEARCH);
  b.leaf("input", "q", "", "");
  b.attr("placeholder", t("projects.filter.placeholder"));
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
    b.leaf("h3", "h", "", q !== "" ? t("projects.list.noMatch.label") : t("projects.list.empty.label"));
    b.leaf("p", "p", "", q !== "" ? tf("projects.list.noMatch.body", [pm.filter]) : t("projects.list.empty.body"));
    b.close();
  }
}

/** function fileRow(b: Builder, f: FileInfo) => Undefined */
function fileRow(b, f) {
  const playing = pm.preview === f.path;
  const url = `/files/${encodePath(f.path)}`;
  b.open("div", `f-${f.path}`, playing ? "pm-file playing" : "pm-file");
  if (f.kind === "audio") {
    iconButton(
      b,
      "play",
      playing ? "small on" : "small",
      playing ? "stop" : "play",
      playing ? t("projects.file.preview.stop.title") : tf("projects.file.preview.play.title", [f.name]),
      () => togglePreview(f)
    );
  } else {
    b.open("span", "doc", "pm-doc");
    icon(b, ICON_DOC);
    b.close();
  }
  b.open("div", "name", "pm-file-name");
  b.leaf("span", "t", "", f.name);
  b.attr("title", f.path);
  if (f.used) b.leaf("span", "used", "pm-tag used", t("projects.file.tag.used"));
  if (f.managed) b.leaf("span", "managed", "pm-tag", t("projects.file.tag.managed"));
  b.close();
  b.leaf("span", "size", "pm-size", bytes(f.size));
  b.leaf("span", "mod", "pm-date", ago(f.modified));
  b.attr("title", fmtDate(f.modified));
  b.open("div", "act", "pm-file-actions");
  iconButton(b, "dl", "small ghost", "export", tf("projects.file.download.title", [f.name]), () => {
    download(url, f.name);
  });
  iconButton(b, "ren", "small ghost", "draw", f.managed ? t("projects.file.managed.title") : tf("projects.file.rename.title", [f.name]), () =>
    compose("file", f.path, f.name)
  );
  if (f.managed) b.attr("disabled", "true");
  iconButton(b, "del", "small ghost danger", "trash", f.managed ? t("projects.file.managed.title") : tf("projects.file.delete.title", [f.name]), () =>
    deleteFile(f)
  );
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
  b.leaf("span", "s", "", tf("projects.files.summary", [String(pm.files.length), bytes(total)]));
  b.close();
  b.leaf("div", "sp", "spacer", "");
  b.open("button", "audio", "btn gold");
  b.attr("title", t("projects.files.importAudio.title"));
  b.on("click", (e) => importAudio());
  icon(b, ICON_IMPORT);
  b.leaf("span", "t", "", t("projects.files.importAudio.label"));
  b.close();
  b.open("button", "midi", "btn");
  b.attr("title", t("projects.files.addMidi.title"));
  b.on("click", (e) => importMidiHere());
  glyph(b, "piano");
  b.leaf("span", "t", "", t("projects.files.addMidi.label"));
  b.close();
  iconButton(b, "refresh", "", "restart", t("projects.files.refresh.title"), () => {
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
    b.leaf("b", "n", "", dir === "" ? t("projects.files.folder.root.label") : `${dir}/`);
    const count =
      inDir.length === 0
        ? t("common.empty")
        : inDir.length === 1
          ? tf("projects.files.folder.size.one", [bytes(size)])
          : tf("projects.files.folder.size.other", [String(inDir.length), bytes(size)]);
    b.leaf("span", "c", "", count);
    b.close();
    if (inDir.length === 0) {
      b.leaf("div", "none", "pm-none", dir === "samples" ? t("projects.files.folder.samples.empty") : t("projects.files.folder.renders.empty"));
    }
    for (const f of inDir) fileRow(b, f);
    b.close();
  }
  if (!pm.filesLoaded) b.leaf("div", "loading", "pm-none", t("projects.files.loading"));
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
  b.attr("aria-label", t("panel.projects"));

  b.open("header", "head", "pm-head");
  b.open("div", "mark", "pm-mark");
  glyph(b, "folder");
  b.close();
  b.open("div", "title", "pm-title");
  b.leaf("h2", "h", "", pm.tab === "files" ? t("projects.tab.files.label") : t("panel.projects"));
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
  button(
    b,
    "projects",
    pm.tab === "projects" ? "on" : "",
    tf("projects.tab.projects.label", [String(pm.projects.length)]),
    t("projects.tab.projects.title"),
    () => showTab("projects")
  );
  button(b, "files", pm.tab === "files" ? "on" : "", t("projects.tab.files.label"), t("projects.tab.files.title"), () => showTab("files"));
  b.close();
  iconButton(b, "close", "ghost", "close", t("common.closeEsc"), closeProjects);
  b.close();

  b.open("div", "body", pm.busy !== "" ? "pm-body busy" : "pm-body");
  if (pm.tab === "files") filesView(b);
  else projectsView(b);
  b.close();

  b.open("footer", "foot", "pm-foot");
  b.leaf("span", "l", "", pm.tab === "files" ? t("projects.footer.files.label") : t("projects.footer.projects.label"));
  button(
    b,
    "trash",
    "small ghost",
    t("projects.emptyTrash.label"),
    pm.tab === "files" ? t("projects.emptyTrash.project.title") : t("projects.emptyTrash.library.title"),
    () => emptyTrash(pm.tab === "files" ? "project" : "library")
  );
  if (state.backend === "local" && pm.storage.quota > 0) {
    b.leaf("span", "store", "pm-store", tf("projects.storage.label", [bytes(pm.storage.usage), bytes(pm.storage.quota)]));
    b.attr("title", t("projects.storage.title"));
  }
  b.leaf("span", "sp", "spacer", "");
  b.open("span", "k", "pm-keys");
  b.leaf("kbd", "k1", "", "Ctrl+O");
  b.leaf("span", "t1", "", t("projects.footer.keys.toggle"));
  b.leaf("kbd", "k2", "", "Esc");
  b.leaf("span", "t2", "", t("projects.footer.keys.close"));
  b.close();
  b.close();

  b.close();
  b.close();
}
