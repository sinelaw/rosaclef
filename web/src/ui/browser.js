// The browser (left panel): instruments, plugins, patterns, samples, project.

import { uploadFile, pickFiles } from "#platform";
import { state, commit, selectPattern, selectChannel, showDock, hint } from "../store.js";
import { newDevice, setOption, uniqueId, paletteColor } from "../model.js";
import { followPattern } from "../audio.js";
import { glyph, iconButton, textInput } from "./widgets.js";
import { toast } from "./toast.js";
import { insertIx } from "#brands";

/** Add a channel with a new instrument; returns its id. */
/** function addChannel(type: String, name: String, setup: (Device) => Undefined) => String */
export function addChannel(type, name, setup) {
  const p = state.project;
  const id = uniqueId(name, p.channels.map((c) => c.id));
  const dev = newDevice(type);
  setup(dev);
  // Route to the first insert no channel uses yet (FL-style one insert per channel).
  let mixer = 1;
  while (mixer < p.mixer.inserts.length && p.channels.some((c) => c.mixer === insertIx(mixer))) mixer = mixer + 1;
  if (mixer >= p.mixer.inserts.length) mixer = 0;
  commit(() => {
    p.channels.push({ id: id, name: name, color: paletteColor(p.channels.length), instrument: dev, volume: 0.8, pan: 0, mute: false, mixer: insertIx(mixer) });
    return undefined;
  });
  selectChannel(id);
  return id;
}

/** function addPattern() => Undefined */
export function addPattern() {
  const p = state.project;
  const n = p.patterns.length + 1;
  const id = uniqueId(`pattern-${n}`, p.patterns.map((x) => x.id));
  commit(() => {
    p.patterns.push({ id: id, name: `Pattern ${n}`, color: paletteColor(n + 2), length: 4, notes: [] });
    return undefined;
  });
  selectPattern(id);
  followPattern();
  return undefined;
}

/** function addSampler(path: String) => Undefined */
function addSampler(path) {
  const name = (path.split("/").pop() ?? path).replace(/\.[a-z0-9]+$/i, "");
  addChannel("sampler", name, (d) => {
    setOption(d, "sample", path);
    setOption(d, "mode", "oneshot");
    return undefined;
  });
  showDock("rack");
  return undefined;
}

/** function uploadAll(files: FileRef[]) => Undefined */
export function uploadAll(files) {
  for (const f of files) {
    uploadFile(`/api/samples?name=${encodeURIComponent(f.name)}`, f)
      .then((r) => {
        toast("Sample added", String(r.path), "info");
        return Promise.resolve(true);
      })
      .catch((e) => {
        toast("Upload failed", String(e), "error");
        return Promise.resolve(false);
      });
  }
  return undefined;
}

const DEVICE_ICONS = [
  { type: "synth", icon: "wave" },
  { type: "fm", icon: "spark" },
  { type: "drum", icon: "rack" },
  { type: "sampler", icon: "folder" },
];

/** function browser(b: Builder) => Undefined */
export function browser(b) {
  const p = state.project;
  b.open("aside", "browser", "browser panel");
  b.open("div", "head", "panel-head");
  b.leaf("div", "t", "panel-title", "Browser");
  b.close();
  b.open("div", "body", "browser-body");

  // Instruments.
  b.open("div", "inst-title", "section-title");
  b.text("Instruments");
  b.close();
  for (const d of state.catalog.devices) {
    if (d.category !== "instrument" || d.type === "plugin") continue;
    let icon = "wave";
    for (const di of DEVICE_ICONS) if (di.type === d.type) icon = di.icon;
    b.open("div", `inst-${d.type}`, "b-item");
    b.attr("title", d.doc);
    b.on("pointerenter", (e) => hint(`${d.label} — ${d.doc} Click to add a channel.`));
    b.on("click", (e) => {
      if (d.type === "sampler") {
        pickFiles("audio/*", (files) => {
          uploadAll(files);
          return undefined;
        });
        return undefined;
      }
      addChannel(d.type, d.label, (dev) => undefined);
      showDock("rack");
      return undefined;
    });
    glyph(b, icon);
    b.leaf("span", "n", "b-name", d.label);
    b.leaf("span", "s", "b-sub", d.type);
    b.close();
  }
  for (const pl of state.catalog.plugins) {
    if (!pl.instrument) continue;
    b.open("div", `plug-${pl.id}`, "b-item");
    b.attr("title", `${pl.vendor} — ${pl.description}\n${pl.path}`);
    b.on("pointerenter", (e) => hint(`${pl.name} (CLAP, ${pl.vendor}) — plays through the Studio (native) output`));
    b.on("click", (e) => {
      addChannel("plugin", pl.name, (dev) => {
        setOption(dev, "format", pl.format);
        setOption(dev, "path", pl.path);
        setOption(dev, "id", pl.id);
        return undefined;
      });
      showDock("rack");
      return undefined;
    });
    glyph(b, "plug");
    b.leaf("span", "n", "b-name", pl.name);
    b.leaf("span", "s", "b-sub", "CLAP");
    b.close();
  }

  // Patterns.
  b.open("div", "pat-title", "section-title");
  b.text("Patterns");
  iconButton(b, "add", "small ghost", "plus", "New pattern", addPattern);
  b.close();
  for (const pat of p.patterns) {
    b.open("div", `pat-${pat.id}`, pat.id === state.pattern ? "b-item on" : "b-item");
    b.on("click", (e) => {
      selectPattern(pat.id);
      followPattern();
      return undefined;
    });
    b.on("dblclick", (e) => {
      showDock("piano");
      return undefined;
    });
    b.leaf("span", "sw", "swatch", "");
    b.style("--c", pat.color);
    b.leaf("span", "n", "b-name", pat.name);
    b.leaf("span", "s", "b-sub", `${pat.notes.length} notes`);
    b.close();
  }

  // Samples.
  b.open("div", "smp-title", "section-title");
  b.text("Samples");
  iconButton(b, "up", "small ghost", "plus", "Import audio files into samples/", () => {
    pickFiles("audio/*", (files) => {
      uploadAll(files);
      return undefined;
    });
    return undefined;
  });
  b.close();
  if (state.samples.length === 0) b.leaf("div", "none", "b-empty", "Drop audio files here");
  for (const s of state.samples) {
    b.open("div", `smp-${s}`, "b-item");
    b.attr("title", `${s}\nClick: new sampler channel. Drag onto the playlist: audio clip.`);
    b.attr("draggable", "true");
    b.on("dragstart", (e) => {
      dragSample.path = s;
      return undefined;
    });
    b.on("click", (e) => addSampler(s));
    glyph(b, "wave");
    b.leaf("span", "n", "b-name", s.replace("samples/", ""));
    b.close();
  }

  // Project.
  b.open("div", "prj-title", "section-title");
  b.text("Project");
  b.close();
  b.open("div", "f-title", "field");
  b.leaf("label", "l", "", "Title");
  textInput(b, "in", "", p.meta.title, "Untitled", (v) => {
    commit(() => {
      state.project.meta.title = v;
      return undefined;
    });
    return undefined;
  });
  b.close();
  b.open("div", "f-author", "field");
  b.leaf("label", "l", "", "Author");
  textInput(b, "in", "", p.meta.author, "", (v) => {
    commit(() => {
      state.project.meta.author = v;
      return undefined;
    });
    return undefined;
  });
  b.close();
  b.leaf("div", "folder", "b-empty", state.folder.split("/").pop() ?? state.folder);
  b.attr("title", state.folder);

  b.close();
  b.close();
  return undefined;
}

/** The sample being dragged from the browser (read by the playlist). */
export const dragSample = { path: "" };
