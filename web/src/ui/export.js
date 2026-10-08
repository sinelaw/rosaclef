// The Export window, opened from the top bar: render the song to an audio
// file (WAV, the one format there is), or take the project away — its
// project.json alone, or the whole folder as a .zip. Drawn by the shell.

import { fmt, download, loadPref, savePref, listenWindow } from "#platform";
import { state, invalidate } from "../store.js";
import { glyph, iconButton, button } from "./widgets.js";
import { toast } from "./toast.js";
import { t, tf, tk } from "../i18n.js";
import { followJob, jobLabel, jobFraction } from "./progress.js";

/** The bit depths the renderer writes (32 is floating point), with their labels. */
const BITS = [
  { bits: 16, label: tk("export.render.bits.16.label"), title: tk("export.render.bits.16.title") },
  { bits: 24, label: tk("export.render.bits.24.label"), title: tk("export.render.bits.24.title") },
  { bits: 32, label: tk("export.render.bits.32.label"), title: tk("export.render.bits.32.title") },
];

/** The sample rates offered (Hz). */
const RATES = [44100, 48000];

/** function prefInt(key: String, allowed: Int[], def: Int) => Int */
function prefInt(key, allowed, def) {
  const saved = loadPref(key);
  return allowed.find((v) => String(v) === saved) ?? def;
}

export const exp = {
  open: false,
  bits: prefInt(
    "rosaclef.export.bits",
    BITS.map((b) => b.bits),
    24
  ),
  rate: prefInt("rosaclef.export.rate", RATES, 48000),
  /** A render in flight, and its job id (its progress). */
  rendering: false,
  job: 0,
};

export function openExport() {
  exp.open = true;
  invalidate();
}

export function closeExport() {
  exp.open = false;
  invalidate();
}

/** The render's format in a few words: "WAV · 24-bit PCM · 48 kHz · stereo". */
/** function formatLine(bits: Int, rate: Int) => String */
export function formatLine(bits, rate) {
  const depth = bits === 32 ? t("export.render.depth.float") : tf("export.render.depth.pcm", [String(bits)]);
  return tf("export.render.format.line", [depth, rateText(rate)]);
}

/** function rateText(rate: Int) => String */
function rateText(rate) {
  return tf("export.render.rate.khz", [rate % 1000 === 0 ? String(rate / 1000) : fmt(rate / 1000, 1)]);
}

/** A file name made from the open project's name (its folder), else the song's title
 * (the .zip's name is the back end's). */
/** function baseName() => String */
function baseName() {
  const s = (state.name !== "" ? state.name : state.project.meta.title).replace(/[\\/:*?"<>|]+/g, "-").trim();
  return s === "" ? "project" : s;
}

/** Render the whole song offline into renders/, and download it. */
export function renderSong() {
  if (exp.rendering) return undefined;
  exp.rendering = true;
  exp.job = 0;
  const bits = exp.bits;
  const rate = exp.rate;
  invalidate();
  followJob("render", { bits: bits, sampleRate: rate }, (id) => {
    exp.job = id;
  })
    .then((r) => {
      exp.rendering = false;
      invalidate();
      const path = String(r.path);
      toast(
        t("export.render.done.toast.title"),
        path + "\n" + tf("export.render.done.toast.body", [fmt(Number(r.duration), 1), fmt(Number(r.peakDb), 1), formatLine(bits, rate)]),
        "info"
      );
      download(String(r.url), path.split("/").pop() ?? "mixdown.wav");
      return true;
    })
    .catch((e) => {
      exp.rendering = false;
      invalidate();
      toast(t("export.render.failed.toast.title"), String(e), "error");
      return false;
    });
}

/** The open project's project.json, as the back end keeps it. */
function downloadJson() {
  download("/api/project", `${baseName()}.json`);
}

/** The open project's folder (project.json, samples, renders) as a .zip. */
function downloadZip() {
  download("/api/projects/export", `${baseName()}.zip`);
}

/** function setBits(bits: Int) => Undefined */
function setBits(bits) {
  exp.bits = bits;
  savePref("rosaclef.export.bits", String(bits));
  invalidate();
}

/** function setRate(rate: Int) => Undefined */
function setRate(rate) {
  exp.rate = rate;
  savePref("rosaclef.export.rate", String(rate));
  invalidate();
}

listenWindow("keydown", (e) => {
  if (exp.open && e.key === "Escape") closeExport();
});

// ------------------------------------------------------------------ view

/** One of the window's sections: an icon, a title and what it makes. */
/** function sectionHead(b: Builder, icon: String, title: String, about: String) => Undefined */
function sectionHead(b, icon, title, about) {
  b.open("div", "head", "ex-sec-head");
  b.open("div", "mark", "ex-sec-mark");
  glyph(b, icon);
  b.close();
  b.open("div", "text", "ex-sec-text");
  b.leaf("h3", "h", "", title);
  b.leaf("p", "p", "", about);
  b.close();
  b.close();
}

/** function renderSection(b: Builder) => Undefined */
function renderSection(b) {
  b.open("section", "render", "ex-sec ex-render");
  sectionHead(b, "wave", t("export.render.heading"), t("export.render.about"));

  b.open("div", "opts", "ex-opts");
  b.open("div", "fmt", "ex-opt");
  b.leaf("span", "l", "ex-opt-label", t("export.render.format.label"));
  b.open("div", "v", "ex-format");
  b.leaf("span", "wav", "ex-chip on", "WAV");
  b.leaf("span", "only", "ex-note", t("export.render.format.only"));
  b.close();
  b.close();

  b.open("div", "bits", "ex-opt");
  b.leaf("span", "l", "ex-opt-label", t("export.render.bits.label"));
  b.open("div", "v", "seg");
  for (const d of BITS) button(b, `b${d.bits}`, exp.bits === d.bits ? "on" : "", t(d.label), t(d.title), () => setBits(d.bits));
  b.close();
  b.close();

  b.open("div", "rate", "ex-opt");
  b.leaf("span", "l", "ex-opt-label", t("export.render.rate.label"));
  b.open("div", "v", "seg");
  for (const r of RATES) button(b, `r${r}`, exp.rate === r ? "on" : "", rateText(r), t("export.render.rate.title"), () => setRate(r));
  b.close();
  b.close();
  b.close();

  b.open("div", "go", "ex-go");
  b.open("div", "sum", "ex-summary");
  b.leaf("span", "f", "ex-summary-format", formatLine(exp.bits, exp.rate));
  b.leaf("span", "w", "ex-summary-where", exp.rendering ? jobLabel(exp.job) : t("export.render.where"));
  b.close();
  b.open("button", "run", exp.rendering ? "btn gold ex-run busy" : "btn gold ex-run");
  b.attr("title", t("export.render.button.title"));
  b.style("--done", `${Math.round(jobFraction(exp.job) * 1000) / 10}%`);
  b.on("click", (e) => renderSong());
  b.leaf(
    "span",
    "t",
    "",
    exp.rendering ? tf("export.render.progress.label", [String(Math.round(jobFraction(exp.job) * 100))]) : t("export.render.button.label")
  );
  b.close();
  b.close();
  b.close();
}

/** function projectSection(b: Builder) => Undefined */
function projectSection(b) {
  b.open("section", "project", "ex-sec ex-project");
  sectionHead(b, "folder", t("export.project.heading"), t("export.project.about"));
  b.open("div", "rows", "ex-rows");

  b.open("div", "json", "ex-row");
  b.open("div", "text", "ex-row-text");
  b.leaf("span", "n", "ex-file", t("export.project.json.name"));
  b.leaf("span", "d", "ex-row-about", t("export.project.json.about"));
  b.close();
  button(b, "dl", "", t("export.project.json.button.label"), t("export.project.json.button.title"), downloadJson);
  b.close();

  b.open("div", "zip", "ex-row");
  b.open("div", "text", "ex-row-text");
  b.leaf("span", "n", "ex-file", t("export.project.zip.name"));
  b.leaf("span", "d", "ex-row-about", t("export.project.zip.about"));
  b.close();
  button(b, "dl", "", t("export.project.zip.button.label"), t("export.project.zip.button.title"), downloadZip);
  b.close();

  b.close();
  b.close();
}

/** The Export window (nothing when closed). */
/** function exportOverlay(b: Builder) => Undefined */
export function exportOverlay(b) {
  if (!exp.open) return undefined;
  b.open("div", "ex-overlay", "overlay ex-overlay");
  b.leaf("div", "scrim", "ex-scrim", "");
  b.on("click", (e) => closeExport());
  b.open("div", "dialog", "ex");
  b.attr("role", "dialog");
  b.attr("aria-label", t("export.dialog.title"));

  b.open("header", "head", "ex-head");
  b.open("div", "title", "ex-title");
  b.leaf("h2", "h", "", t("export.dialog.title"));
  b.leaf("span", "song", "ex-song", state.project.meta.title);
  b.close();
  iconButton(b, "close", "ghost", "close", t("common.closeEsc"), closeExport);
  b.close();

  b.open("div", "body", "ex-body");
  renderSection(b);
  projectSection(b);
  b.close();

  b.close();
  b.close();
}
