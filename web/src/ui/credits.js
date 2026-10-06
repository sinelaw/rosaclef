// Credits and licenses: where the sample collections come from, who made
// them and under which license, shown from the instruments that play them
// (the browser's instrument list, a channel's instrument panel). The license,
// readme and per-instrument sample sources are the files published with the
// collection, served with the site (web/soundfonts/gm/).

import { siteText, listenWindow } from "#platform";
import { state, invalidate } from "../store.js";
import { button, iconButton, glyph } from "./widgets.js";
import { t, tf } from "../i18n.js";

/** type CreditRow = { bank: Int, program: Int, name: String, details: String, credit: String, changes: String } */

export const credits = {
  open: false,
  /** The collection shown (a SampleCollection id). */
  id: "",
  /** The instrument it was opened from ("" = none). */
  preset: "",
  /** The license text, and which collection it is for. */
  text: "",
  textFor: "",
  rows /*: CreditRow[] */: [],
  rowsFor: "",
};

/** Other third-party work in Rosaclef, with its license file. */
const OTHERS = [
  { name: "Bravura music font", by: "Steinberg Media Technologies", license: "SIL Open Font License 1.1", file: "fonts/Bravura-OFL.txt" },
  { name: "xterm.js", by: "The xterm.js authors", license: "MIT", file: "vendor/xterm/LICENSE" },
  { name: "mp4-muxer", by: "Vanilagy", license: "MIT", file: "vendor/mp4-muxer/LICENSE" },
];

/** The collection an instrument type plays, if any. */
/** function collectionFor(type: String) => SampleCollection? */
export function collectionFor(type) {
  return state.catalog.collections.find((c) => c.instrument === type);
}

/** Open the credits of a collection, for one of its instruments ("" = none). */
/** function showCredits(id: String, preset: String) => Undefined */
export function showCredits(id, preset) {
  credits.open = true;
  credits.id = id;
  credits.preset = preset;
  load();
  invalidate();
}

export function closeCredits() {
  credits.open = false;
  invalidate();
}

/** Fetch the license text and the sample sources (once per collection). */
function load() {
  const c = state.catalog.collections.find((x) => x.id === credits.id);
  if (!c) return undefined;
  if (credits.textFor !== c.id) {
    credits.textFor = c.id;
    credits.text = "";
    siteText(c.licenseFile)
      .then((s) => {
        credits.text = s;
        invalidate();
        return true;
      })
      .catch((e) => {
        credits.text = tf("credits.license.loadFailed", [c.licenseFile]);
        invalidate();
        return false;
      });
  }
  if (credits.rowsFor !== c.id && c.sourcesFile !== "") {
    credits.rowsFor = c.id;
    credits.rows = [];
    siteText(c.sourcesFile)
      .then((s) => {
        credits.rows = parseSources(s);
        invalidate();
        return true;
      })
      .catch((e) => false);
  }
}

/** The rows of a CSV file (quoted fields may hold commas, quotes and line breaks). */
/** function csvRows(text: String) => String[][] */
export function csvRows(text) {
  /** const rows: String[][] */
  const rows = [];
  /** let row: String[] */
  let row = [];
  let field = "";
  let quoted = false;
  for (let i = 0; i < text.length; i++) {
    const ch = text.charAt(i);
    if (quoted) {
      if (ch === '"' && text.charAt(i + 1) === '"') {
        field = field + '"';
        i = i + 1;
      } else if (ch === '"') quoted = false;
      else field = field + ch;
    } else if (ch === '"') quoted = true;
    else if (ch === ",") {
      row.push(field);
      field = "";
    } else if (ch === "\n") {
      row.push(field);
      rows.push(row);
      row = [];
      field = "";
    } else if (ch !== "\r") field = field + ch;
  }
  if (field !== "" || row.length > 0) {
    row.push(field);
    rows.push(row);
  }
  return rows;
}

/** MuseScore General's sample sources: bank, program, name, details, credit, changes. */
/** function parseSources(text: String) => CreditRow[] */
export function parseSources(text) {
  /** const out: CreditRow[] */
  const out = [];
  const rows = csvRows(text);
  for (let i = 1; i < rows.length; i++) {
    const r = rows[i];
    if (r.length < 6) continue;
    out.push({
      bank: Math.round(Number(r[0])),
      program: Math.round(Number(r[2])),
      name: r[3].trim(),
      details: r[4].trim(),
      credit: r[5].trim(),
      changes: r.length > 6 ? r[6].trim() : "",
    });
  }
  return out;
}

/** What the sample sources say about one instrument of a collection. */
/** function creditOf(c: SampleCollection, preset: String) => CreditRow? */
export function creditOf(c, preset) {
  const p = c.presets.find((x) => x.name === preset);
  if (!p) return undefined;
  return credits.rows.find((r) => r.bank === p.bank && r.program === p.program);
}

/** One line for a channel's instrument panel: whose samples, which license. */
/** function sampleCredit(b: Builder, type: String, preset: String) => Undefined */
export function sampleCredit(b, type, preset) {
  const c = collectionFor(type);
  if (!c) return undefined;
  b.open("div", "credit", "sample-credit");
  glyph(b, "info");
  // (Text and elements do not mix in one node: every piece is its own.)
  b.open("span", "t", "sample-credit-text");
  b.leaf("span", "a", "", t("credits.sampleCredit.label"));
  b.leaf("b", "n", "", `${c.name} ${c.version.split(" ")[0]}`);
  b.leaf("span", "l", "", "· " + tf("credits.licenseName", [c.license]));
  b.close();
  const tip = preset === "" ? t("credits.sampleCredit.title") : tf("credits.sampleCredit.titleForPreset", [preset]);
  button(b, "open", "small", t("credits.sampleCredit.open.label"), tip, () => showCredits(c.id, preset));
  b.close();
}

/** function link(b: Builder, key: String, label: String, href: String) => Undefined */
function link(b, key, label, href) {
  b.leaf("a", key, "cr-link", label);
  b.attr("href", href);
  b.attr("target", "_blank");
  b.attr("rel", "noopener");
}

/** The credits dialog. */
/** function creditsOverlay(b: Builder) => Undefined */
export function creditsOverlay(b) {
  if (!credits.open) return undefined;
  const c = state.catalog.collections.find((x) => x.id === credits.id);
  if (!c) return undefined;
  b.open("div", "cr-overlay", "overlay cr-overlay");
  b.leaf("div", "scrim", "cr-scrim", "");
  b.on("click", (e) => closeCredits());
  b.open("div", "dialog", "cr");
  b.attr("role", "dialog");
  b.attr("aria-label", tf("credits.dialog.aria", [c.name]));

  b.open("header", "head", "cr-head");
  b.open("div", "mark", "cr-mark");
  glyph(b, "info");
  b.close();
  b.open("div", "title", "cr-title");
  b.leaf("h2", "h", "", c.name);
  b.leaf("span", "v", "cr-version", tf("credits.dialog.version", [c.version]));
  b.close();
  b.leaf("span", "lic", "cr-license", tf("credits.licenseName", [c.license]));
  iconButton(b, "close", "ghost", "close", t("common.closeEsc"), closeCredits);
  b.close();

  b.open("div", "body", "cr-body");
  b.leaf("p", "sum", "cr-summary", c.summary);
  b.open("div", "by", "cr-by");
  b.leaf("span", "l", "cr-label", t("credits.dialog.madeBy.label"));
  b.leaf("span", "a", "", c.authors);
  b.close();

  if (credits.preset !== "") {
    const row = creditOf(c, credits.preset);
    b.open("section", "inst", "cr-inst");
    b.leaf("div", "l", "cr-label", t("credits.dialog.instrument.label"));
    b.leaf("div", "n", "cr-inst-name", row && row.name !== credits.preset ? `${credits.preset} (${row.name})` : credits.preset);
    if (row) {
      if (row.details !== "") b.leaf("div", "d", "cr-inst-details", row.details);
      b.leaf("div", "c", "cr-inst-credit", row.credit !== "" ? row.credit : t("credits.dialog.instrument.defaultCredit"));
      if (row.changes !== "") b.leaf("div", "x", "cr-inst-details", row.changes);
    } else if (credits.rows.length === 0) b.leaf("div", "c", "cr-inst-details", t("credits.dialog.instrument.loading"));
    b.close();
  }

  b.open("div", "links", "cr-links");
  link(b, "src", t("credits.dialog.link.source"), c.source);
  link(b, "readme", t("credits.dialog.link.readme"), c.readmeFile);
  if (c.sourcesFile !== "") link(b, "sources", t("credits.dialog.link.sources"), c.sourcesFile);
  link(b, "lic", t("credits.dialog.link.licenseFile"), c.licenseFile);
  b.close();

  b.leaf("div", "lt", "cr-label", t("credits.dialog.licenseText.label"));
  b.leaf("pre", "text", "cr-text", credits.text === "" ? t("credits.dialog.licenseText.loading") : credits.text);

  b.leaf("div", "ot", "cr-label", t("credits.dialog.others.label"));
  b.open("ul", "others", "cr-others");
  for (const o of OTHERS) {
    b.open("li", o.name, "");
    b.leaf("b", "n", "", o.name);
    b.leaf("span", "t", "", " " + tf("credits.dialog.others.byLine", [o.by, o.license]) + " · ");
    link(b, "f", t("credits.dialog.others.license"), o.file);
    b.close();
  }
  b.close();
  b.close();
  b.close();
  b.close();
}

listenWindow("keydown", (e) => {
  if (credits.open && e.key === "Escape") closeCredits();
});
