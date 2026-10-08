// The browser (left panel): a searchable tree of every instrument — the
// song's channels, each instrument's presets, drum sounds and soundfont
// programs, samples, plugins — then patterns, samples and the project.
// Clicking an instrument plays it on the keys (instruments.js); double-click
// or + adds it as a channel, ⇄ swaps it into the selected channel.

import { uploadFile, pickFiles } from "#platform";
import { state, commit, selectPattern, selectChannel, invalidate, hint, setFocus, currentChannel } from "../store.js";
import { uniqueId, paletteColor, noPatternDrums } from "../model.js";
import { followPattern } from "../audio.js";
import { glyph, iconButton, textInput } from "./widgets.js";
import { toast } from "./toast.js";
import { collectionFor, showCredits } from "./credits.js";
import { paneHeader, paneControls, revealDock, sideMode, setSide } from "./panes.js";
import { addPick, tryPick, replaceInstrument, instrumentLabel, startPickDrag, endPickDrag } from "./instruments.js";
import { t, tf, tk, language } from "../i18n.js";

/** function addPattern() => Undefined */
export function addPattern() {
  const p = state.project;
  const n = p.patterns.length + 1;
  const id = uniqueId(
    `pattern-${n}`,
    p.patterns.map((x) => x.id)
  );
  commit(() => {
    p.patterns.push({ id: id, name: `Pattern ${n}`, color: paletteColor(n + 2), length: 4, notes: [], drums: noPatternDrums() });
  });
  selectPattern(id);
  followPattern();
}

/** A sampler playing the sample at `path`, one-shot. */
/** function samplerPick(path: String) => Pick */
function samplerPick(path) {
  const name = (path.split("/").pop() ?? path).replace(/\.[a-z0-9]+$/i, "");
  return devicePick(
    "sampler",
    `smp-${path}`,
    name,
    [
      { key: "sample", value: path },
      { key: "mode", value: "oneshot" },
    ],
    []
  );
}

/** function uploadAll(files: FileRef[]) => Undefined */
export function uploadAll(files) {
  for (const f of files) {
    uploadFile(`/api/samples?name=${encodeURIComponent(f.name)}`, f)
      .then((r) => {
        toast(t("browser.sampleAdded"), String(r.path), "info");
        return true;
      })
      .catch((e) => {
        toast(t("browser.upload.failed.title"), String(e), "error");
        return false;
      });
  }
}

const DEVICE_ICONS = [
  { type: "soundfont", icon: "piano" },
  { type: "drum", icon: "drum" },
  { type: "sampler", icon: "folder" },
  { type: "additive", icon: "spark" },
  { type: "fm", icon: "mixer" },
  { type: "wavetable", icon: "pattern" },
  { type: "analog", icon: "wave" },
  { type: "granular", icon: "loop" },
  { type: "generative", icon: "select" },
  { type: "transition", icon: "export" },
];

/** The General MIDI families, eight programs each: the English name (searched)
 * and the key of the shown label. */
/** const GM_FAMILIES: { name: String, label: String }[] */
const GM_FAMILIES = [
  { name: "Pianos", label: tk("browser.gmFamily.pianos.label") },
  { name: "Chromatic percussion", label: tk("browser.gmFamily.chromaticPercussion.label") },
  { name: "Organs", label: tk("browser.gmFamily.organs.label") },
  { name: "Guitars", label: tk("browser.gmFamily.guitars.label") },
  { name: "Basses", label: tk("browser.gmFamily.basses.label") },
  { name: "Strings", label: tk("browser.gmFamily.strings.label") },
  { name: "Ensembles & choirs", label: tk("browser.gmFamily.ensemblesChoirs.label") },
  { name: "Brass", label: tk("browser.gmFamily.brass.label") },
  { name: "Reeds", label: tk("browser.gmFamily.reeds.label") },
  { name: "Pipes & flutes", label: tk("browser.gmFamily.pipesFlutes.label") },
  { name: "Synth leads", label: tk("browser.gmFamily.synthLeads.label") },
  { name: "Synth pads", label: tk("browser.gmFamily.synthPads.label") },
  { name: "Synth effects", label: tk("browser.gmFamily.synthEffects.label") },
  { name: "World", label: tk("browser.gmFamily.world.label") },
  { name: "Percussive", label: tk("browser.gmFamily.percussive.label") },
  { name: "Sound effects", label: tk("browser.gmFamily.soundEffects.label") },
];

/** The browser's tree: which folders are open, and the search. A search
 * opens every folder holding a match and hides what does not match. */
/** const tree: { open: String[], query: String, first: Pick[], enter: Boolean } */
const tree = { open: ["song"], query: "", first: [], enter: false };

/** The words of the search ([] for none). */
/** function words() => String[] */
function words() {
  return tree.query
    .toLowerCase()
    .split(" ")
    .filter((w) => w !== "");
}

/** Whether `text` has every word of the search. */
/** function hit(text: String) => Boolean */
function hit(text) {
  const t = text.toLowerCase();
  return words().every((w) => t.includes(w));
}

/** function isOpen(key: String) => Boolean */
function isOpen(key) {
  return tree.open.includes(key);
}

/** function toggle(key: String) => Undefined */
function toggle(key) {
  const at = tree.open.indexOf(key);
  if (at >= 0) tree.open.splice(at, 1);
  else tree.open.push(key);
  invalidate();
}

/** function openFolder(key: String) => Undefined */
function openFolder(key) {
  if (!tree.open.includes(key)) tree.open.push(key);
}

/** A folder row of the tree; `act` runs on a click of the row (the caret
 * always opens and closes it). */
/** function folderRow(b: Builder, key: String, depth: Int, icon: String, label: String, sub: String, doc: String, shownOpen: Boolean, act: () => Undefined) => Undefined */
function folderRow(b, key, depth, icon, label, sub, doc, shownOpen, act) {
  b.open("div", `f-${key}`, shownOpen ? "b-item folder open" : "b-item folder");
  b.style("--depth", String(depth));
  if (doc !== "") b.attr("title", doc);
  b.on("pointerenter", (e) => hint(`${label} — ${doc}`));
  b.on("click", (e) => act());
  b.leaf("span", "caret", "b-caret", shownOpen ? "▾" : "▸");
  b.attr("title", shownOpen ? t("browser.folder.close.title") : t("common.open"));
  b.on("click", (e) => {
    e.stopPropagation();
    if (words().length > 0 && shownOpen && !isOpen(key)) return undefined;
    toggle(key);
  });
  if (icon !== "") glyph(b, icon);
  b.leaf("span", "n", "b-name", label);
  b.leaf("span", "s", "b-sub", sub);
}

/** An instrument of the tree: a click tries it on the piano, a double click
 * (or +) adds it as a channel, ⇄ swaps it into the selected channel; it can
 * be dragged onto a channel of the rack. */
/** function pickRow(b: Builder, depth: Int, pick: Pick, sub: String, doc: String, icon: String) => Undefined */
function pickRow(b, depth, pick, sub, doc, icon) {
  tree.first.push(pick);
  const on = state.audition.on && state.audition.key === pick.key;
  b.open("div", `p-${pick.key}`, on ? "b-item pick on" : "b-item pick");
  b.style("--depth", String(depth));
  b.attr("title", `${doc}${doc !== "" ? "\n" : ""}${t("browser.pick.title")}`);
  b.attr("draggable", "true");
  b.on("dragstart", (e) => {
    startPickDrag(pick);
  });
  b.on("dragend", (e) => {
    endPickDrag();
    invalidate();
  });
  b.on("pointerenter", (e) => hint(doc !== "" ? tf("browser.pick.withDoc.hint", [pick.name, doc]) : tf("browser.pick.hint", [pick.name])));
  b.on("click", (e) => tryPick(pick));
  b.on("dblclick", (e) => {
    addPick(pick);
    revealDock("rack");
  });
  if (icon !== "") glyph(b, icon);
  b.leaf("span", "n", "b-name", pick.name);
  if (sub !== "") b.leaf("span", "s", "b-sub", sub);
  b.open("span", "acts", "b-acts");
  const ch = currentChannel();
  if (ch) {
    const tip = tf("browser.pick.swap.title", [ch.name, pick.name]);
    b.open("button", "swap", "btn icon small ghost");
    b.attr("title", tip);
    b.attr("aria-label", tip);
    b.on("pointerenter", (e) => hint(tip));
    b.on("click", (e) => {
      e.stopPropagation();
      const target = currentChannel();
      if (target) replaceInstrument(target, pick);
    });
    glyph(b, "swap");
    b.close();
  }
  b.open("button", "add", "btn icon small ghost");
  b.attr("title", t("browser.pick.add.title"));
  b.attr("aria-label", t("browser.pick.add.title"));
  b.on("click", (e) => {
    e.stopPropagation();
    addPick(pick);
    revealDock("rack");
  });
  glyph(b, "plus");
  b.close();
  b.close();
  b.close();
}

/** function devicePick(type: String, key: String, name: String, options: KS[], params: KV[]) => Pick */
function devicePick(type, key, name, options, params) {
  return { key: key, name: name, device: { type: type, enabled: true, params: params, options: options } };
}

/** The channels of the song: a click plays one on the keys. */
/** function songFolder(b: Builder) => Undefined */
function songFolder(b) {
  const chans = state.project.channels.filter((c) => hit(`${c.name} ${instrumentLabel(c.instrument)} song`));
  const searching = words().length > 0;
  if (searching && chans.length === 0) return undefined;
  const open = isOpen("song") || searching;
  folderRow(b, "song", 0, "song", t("browser.song.label"), String(chans.length), t("browser.song.title"), open, () => toggle("song"));
  b.close();
  if (!open) return undefined;
  for (const ch of chans) {
    const on = !state.audition.on && ch.id === state.channel;
    b.open("div", `ch-${ch.id}`, on ? "b-item pick on" : "b-item pick");
    b.style("--depth", "1");
    const detail = instrumentLabel(ch.instrument);
    b.attr("title", tf("browser.song.channel.title", [ch.name, detail]));
    b.on("pointerenter", (e) => hint(tf("browser.song.channel.hint", [ch.name, detail])));
    b.on("click", (e) => selectChannel(ch.id));
    b.on("dblclick", (e) => {
      selectChannel(ch.id);
      revealDock("piano");
    });
    b.leaf("i", "sw", "swatch", "");
    b.style("--c", ch.color);
    b.leaf("span", "n", "b-name", ch.name);
    b.leaf("span", "s", "b-sub", detail);
    b.close();
  }
}

/** The soundfont's programs, by General MIDI family, and its drum kits. */
/** function soundfontKids(b: Builder, d: DeviceSpec, parentHit: Boolean) => Undefined */
function soundfontKids(b, d, parentHit) {
  const coll = collectionFor(d.type);
  if (!coll) return undefined;
  const families = GM_FAMILIES.concat([{ name: "Drum kits", label: tk("browser.gmFamily.drumKits.label") }]);
  for (let f = 0; f < families.length; f++) {
    const fam = families[f];
    const progs = coll.presets.filter((pr) => (pr.bank === 128 ? families.length - 1 : Math.floor(pr.program / 8)) === f);
    const shown = progs.filter((pr) => parentHit || hit(`${pr.name} ${fam.name} ${t(fam.label)}`));
    if (shown.length === 0) continue;
    const key = `gm-${f}`;
    const open = isOpen(key) || (words().length > 0 && !parentHit);
    folderRow(b, key, 1, "", t(fam.label), String(shown.length), `${d.label}: ${language() === "en" ? fam.name.toLowerCase() : t(fam.label)}`, open, () =>
      toggle(key)
    );
    b.close();
    if (!open) continue;
    for (const pr of shown) {
      const pick = devicePick(d.type, `sf-${pr.name}`, pr.name, [{ key: "program", value: pr.name }], []);
      pickRow(
        b,
        2,
        pick,
        pr.bank === 128 ? t("browser.soundfont.kit.label") : pr.bank === 0 ? `#${pr.program + 1}` : t("browser.soundfont.variation.label"),
        `${d.label} · ${t(fam.label)}`,
        ""
      );
    }
  }
}

/** The instruments of the catalog, each with what it can play. */
/** function instrumentFolders(b: Builder) => Undefined */
function instrumentFolders(b) {
  const searching = words().length > 0;
  for (const d of state.catalog.devices) {
    if (d.category !== "instrument" || d.type === "plugin") continue;
    let icon = "wave";
    for (const di of DEVICE_ICONS) if (di.type === d.type) icon = di.icon;
    const doc = d.bestFor !== "" ? tf("browser.instrument.bestFor", [d.bestFor, d.doc]) : d.doc;
    const selfHit = searching && hit(`${d.label} ${d.type}`);
    // What the instrument holds: presets, drum sounds, soundfont programs, samples.
    const presets = state.catalog.presets.filter((pr) => pr.type === d.type);
    const kindSpec = d.type === "drum" ? d.options.find((o) => o.key === "kind") : undefined;
    const kinds = kindSpec ? kindSpec.choices : [];
    const coll = d.type === "soundfont" ? collectionFor(d.type) : undefined;
    let count = presets.length + kinds.length + (coll ? coll.presets.length : 0);
    if (d.type === "sampler") count = state.samples.length;
    let matches = 0;
    if (searching && !selfHit) {
      matches = presets.filter((pr) => hit(`${pr.name} ${pr.tags}`)).length + kinds.filter((k) => hit(`${k} drum`)).length;
      if (coll)
        matches =
          matches + coll.presets.filter((pr) => hit(`${pr.name} ${GM_FAMILIES[Math.floor(pr.program / 8)].name} ${pr.bank === 128 ? "drum kits" : ""}`)).length;
      if (d.type === "sampler") matches = matches + state.samples.filter((smp) => hit(smp)).length;
      if (matches === 0) continue;
    }
    const key = `dev-${d.type}`;
    const open = isOpen(key) || (searching && !selfHit);
    const base = devicePick(d.type, `dev-${d.type}`, d.label, [], []);
    folderRow(
      b,
      key,
      0,
      icon,
      d.label,
      searching && !selfHit ? tf("browser.instrument.matchCount", [String(matches), String(count)]) : String(count),
      doc,
      open,
      () => {
        if (d.type === "sampler") {
          toggle(key);
          return undefined;
        }
        // The row plays the instrument as it comes, and shows what it holds.
        tryPick(base);
        openFolder(key);
      }
    );
    const credit = collectionFor(d.type);
    if (credit) {
      // Sampled instruments: who made the samples, and their license.
      const tip = tf("browser.instrument.credits.title", [credit.name, credit.license]);
      b.open("button", "credit", "btn icon small ghost b-credit");
      b.attr("title", tip);
      b.attr("aria-label", tip);
      b.on("click", (e) => {
        e.stopPropagation();
        showCredits(credit.id, "");
      });
      glyph(b, "info");
      b.close();
    }
    if (d.type === "sampler") {
      b.open("button", "up", "btn icon small ghost");
      b.attr("title", t("browser.importSamples.title"));
      b.attr("aria-label", t("browser.importSamples.title"));
      b.on("click", (e) => {
        e.stopPropagation();
        pickFiles("audio/*", (files) => {
          uploadAll(files);
        });
      });
      glyph(b, "plus");
      b.close();
    }
    b.close();
    if (!open) continue;
    if (d.type === "soundfont") soundfontKids(b, d, selfHit || !searching);
    for (const k of kinds) {
      if (searching && !selfHit && !hit(`${k} drum`)) continue;
      const at = kindSpec ? kindSpec.choices.indexOf(k) : -1;
      const kdoc = kindSpec && at >= 0 && at < kindSpec.choiceDocs.length ? kindSpec.choiceDocs[at] : "";
      const name = k.charAt(0).toUpperCase() + k.slice(1);
      pickRow(b, 1, devicePick(d.type, `drum-${k}`, name, [{ key: "kind", value: k }], []), t("browser.drum.sound.label"), kdoc, "");
    }
    for (const pr of presets) {
      if (searching && !selfHit && !hit(`${pr.name} ${pr.tags}`)) continue;
      pickRow(
        b,
        1,
        devicePick(
          pr.type,
          `pr-${pr.type}-${pr.name}`,
          pr.name,
          pr.options.map((o) => ({ key: o.key, value: o.value })),
          pr.params.map((x) => ({ key: x.key, value: x.value }))
        ),
        pr.tags.split(",")[0],
        `${pr.doc} (${pr.tags})`,
        ""
      );
    }
    if (d.type === "sampler") {
      if (state.samples.length === 0) b.leaf("div", "none", "b-empty", t("browser.sampler.empty"));
      for (const smp of state.samples) {
        if (searching && !selfHit && !hit(smp)) continue;
        pickRow(b, 1, samplerPick(smp), "", smp, "wave");
      }
    }
  }
  for (const pl of state.catalog.plugins) {
    if (!pl.instrument || !hit(`${pl.name} ${pl.vendor} clap plugin`)) continue;
    const pick = devicePick(
      "plugin",
      `plug-${pl.id}`,
      pl.name,
      [
        { key: "format", value: pl.format },
        { key: "path", value: pl.path },
        { key: "id", value: pl.id },
      ],
      []
    );
    pickRow(b, 0, pick, "CLAP", tf("browser.plugin.title", [pl.vendor, pl.description]), "plug");
  }
}

/** Show an instrument's sounds in the browser (opened if it was folded away). */
/** function browseInstrument(type: String) => Undefined */
export function browseInstrument(type) {
  tree.query = "";
  openFolder(`dev-${type}`);
  if (sideMode("browser") === "min") setSide("browser", "open");
  invalidate();
}

/** The search box over the tree: Enter plays the first match, Esc clears it. */
/** function searchBox(b: Builder) => Undefined */
function searchBox(b) {
  b.open("div", "search", "b-search");
  glyph(b, "search");
  b.leaf("input", "in", "text-input", "");
  b.attr("placeholder", t("browser.search.placeholder"));
  b.attr("spellcheck", "false");
  b.attr("aria-label", t("browser.search.aria"));
  b.prop("value", tree.query);
  b.on("input", (e) => {
    tree.query = e.value;
    invalidate();
  });
  b.on("keydown", (e) => {
    if (e.key === "Escape") {
      tree.query = "";
      invalidate();
    } else if (e.key === "Enter") {
      // The first match of what is typed now (the list may not show it yet).
      tree.query = e.value;
      tree.enter = true;
      invalidate();
    }
  });
  // Always there (hidden while empty): an element appearing after the input
  // would make the tree re-attach the row, and the input lose the focus.
  iconButton(
    b,
    "clear",
    tree.query === "" ? "small ghost b-search-clear empty" : "small ghost b-search-clear",
    "close",
    t("browser.search.clear.title"),
    () => {
      tree.query = "";
      invalidate();
    }
  );
  b.close();
}

/** function browser(b: Builder) => Undefined */
export function browser(b) {
  const p = state.project;
  b.open("aside", "browser", "browser panel");
  b.on("pointerdown", (e) => setFocus("browser"));
  b.open("div", "head", "panel-head");
  paneHeader(b, "browser");
  b.leaf("div", "t", "panel-title", t("panel.browser"));
  paneControls(b, "browser");
  b.close();
  searchBox(b);
  b.open("div", "body", "browser-body");
  const searching = words().length > 0;
  tree.first = [];

  // Instruments. (Sections are grouped so a maximized browser can lay them out in columns.)
  b.open("div", "sec-inst", "b-section");
  b.open("div", "inst-title", "section-title");
  b.text(t("browser.instruments.label"));
  b.close();
  songFolder(b);
  instrumentFolders(b);
  if (searching && tree.first.length === 0 && !state.project.channels.some((c) => hit(`${c.name} ${instrumentLabel(c.instrument)} song`))) {
    b.leaf("div", "none", "b-empty", tf("browser.instruments.empty", [tree.query.trim()]));
  }
  b.close();
  if (tree.enter) {
    // Enter in the search: try the first match, once the list is made.
    tree.enter = false;
    const first = tree.first.length > 0 ? tree.first[0] : undefined;
    if (first) setTimeout(() => tryPick(first), 0);
  }

  // Patterns.
  const pats = p.patterns.filter((x) => hit(`${x.name} pattern`));
  if (!searching || pats.length > 0) patternSection(b, pats);
  const smps = state.samples.filter((x) => hit(x));
  if (!searching || smps.length > 0) sampleSection(b, smps);
  if (!searching) projectSection(b);

  b.close();
  b.close();
}

/** function patternSection(b: Builder, pats: Pattern[]) => Undefined */
function patternSection(b, pats) {
  b.open("div", "sec-pat", "b-section");
  b.open("div", "pat-title", "section-title");
  b.text(t("browser.patterns.label"));
  iconButton(b, "add", "small ghost", "plus", t("browser.patterns.add.title"), addPattern);
  b.close();
  for (const pat of pats) {
    b.open("div", `pat-${pat.id}`, pat.id === state.pattern ? "b-item on" : "b-item");
    b.on("click", (e) => {
      selectPattern(pat.id);
      followPattern();
      // Show its notes: the channel playing most of them, unless the
      // selected one plays some.
      if (pat.notes.length > 0 && !pat.notes.some((n) => n.channel === state.channel)) {
        let best = "";
        let bestCount = 0;
        for (const ch of state.project.channels) {
          const n = pat.notes.filter((x) => x.channel === ch.id).length;
          if (n > bestCount) {
            best = ch.id;
            bestCount = n;
          }
        }
        if (best !== "") selectChannel(best);
      }
    });
    b.on("dblclick", (e) => {
      revealDock("piano");
    });
    b.leaf("span", "sw", "swatch", "");
    b.style("--c", pat.color);
    b.leaf("span", "n", "b-name", pat.name);
    b.leaf("span", "s", "b-sub", pat.notes.length === 1 ? t("browser.patterns.notes.one") : tf("browser.patterns.notes.other", [String(pat.notes.length)]));
    b.close();
  }
  b.close();
}

/** function sampleSection(b: Builder, smps: String[]) => Undefined */
function sampleSection(b, smps) {
  b.open("div", "sec-smp", "b-section");
  b.open("div", "smp-title", "section-title");
  b.text(t("browser.samples.label"));
  iconButton(b, "up", "small ghost", "plus", t("browser.importSamples.title"), () => {
    pickFiles("audio/*", (files) => {
      uploadAll(files);
    });
  });
  b.close();
  if (state.samples.length === 0) b.leaf("div", "none", "b-empty", t("browser.samples.empty"));
  for (const s of smps) {
    const pick = samplerPick(s);
    b.open("div", `smp-${s}`, state.audition.on && state.audition.key === pick.key ? "b-item on" : "b-item");
    b.attr("title", tf("browser.samples.item.title", [s]));
    b.attr("draggable", "true");
    b.on("dragstart", (e) => {
      dragSample.path = s;
    });
    b.on("click", (e) => tryPick(pick));
    b.on("dblclick", (e) => {
      addPick(pick);
      revealDock("rack");
    });
    glyph(b, "wave");
    b.leaf("span", "n", "b-name", s.replace("samples/", ""));
    b.close();
  }
  b.close();
}

/** function projectSection(b: Builder) => Undefined */
function projectSection(b) {
  const p = state.project;
  b.open("div", "sec-prj", "b-section");
  b.open("div", "prj-title", "section-title");
  b.text(t("term.project"));
  b.close();
  b.open("div", "f-title", "field");
  b.leaf("label", "l", "", t("browser.project.title.label"));
  textInput(b, "in", "", p.meta.title, t("browser.project.title.placeholder"), (v) => {
    commit(() => {
      state.project.meta.title = v;
    });
  });
  b.close();
  b.open("div", "f-author", "field");
  b.leaf("label", "l", "", t("browser.project.author.label"));
  textInput(b, "in", "", p.meta.author, "", (v) => {
    commit(() => {
      state.project.meta.author = v;
    });
  });
  b.close();
  b.leaf("div", "folder", "b-empty", state.folder.split("/").pop() ?? state.folder);
  b.attr("title", state.folder);
  b.close();
}

/** The sample being dragged from the browser (read by the playlist). */
export const dragSample = { path: "" };

/** The dragged sample was dropped (the playlist took it). */
export function endSampleDrag() {
  dragSample.path = "";
}
