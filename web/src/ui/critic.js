// The Critic: a Maestro plugin that lints the project. The checks run in
// Rust (crates/core/src/critic, the same as `rosaclef critic`): the panel
// posts the project to /api/critic — the server, or the browser-only
// studio's worker — a moment after it changes (never during a drag), and
// lists what comes back. Suggestions apply with one click (the endpoint
// returns the fixed project; one undo step). Issues only inform — in the
// native studio they can be handed to the agent. Suppressing a finding, or
// turning a whole check off, is written into the project (`critic`), so the
// command line and the agent leave it out too; Ctrl+Z undoes it.

import { getJson, sendJson } from "#platform";
import { state, commit, invalidate, fixSelection, selectPattern, selectChannel, selectInsert, hint } from "../store.js";
import { encodeProject, decodeProject } from "../model.js";
import { button, iconButton, glyph } from "./widgets.js";
import { openDock, setTop, setView, isCompact, layoutState } from "./panes.js";
import { revealBeat } from "./playlist.js";
import { toast } from "./toast.js";
import { insertIx, noteIx, clipIx } from "#brands";
import { t, tf, tk } from "../i18n.js";

/** The categories, in the order shown: `value` is the server's category (data), `label` the key shown. */
/** const CATEGORIES: { value: String, label: String }[] */
const CATEGORIES = [
  { value: "Harmony", label: tk("critic.category.harmony") },
  { value: "Melody", label: tk("critic.category.melody") },
  { value: "Rhythm", label: tk("critic.category.rhythm") },
  { value: "Arrangement", label: tk("critic.category.arrangement") },
  { value: "Low end", label: tk("critic.category.lowEnd") },
  { value: "Mix", label: tk("critic.category.mix") },
  { value: "Stereo", label: tk("critic.category.stereo") },
  { value: "Effects", label: tk("critic.category.effects") },
  { value: "Master", label: tk("critic.category.master") },
  { value: "Project", label: tk("critic.category.project") },
  { value: "Mix check", label: tk("critic.category.mixCheck") },
];

const view = {
  /** "all", "suggest" (with a fix) or "issue" (without). */
  filter: "all",
  /** Show the list of checks (to turn them on and off). */
  rules: false,
  showSuppressed: false,
  /** Rules whose findings are folded away (by rule id). */
  folded /*: String[] */: [],
  /** The project version the findings are for (-1: none yet), and whether a request is out. */
  edits: -1,
  busy: false,
  error: "",
  key: "",
  found /*: Finding[] */: [],
  catalog /*: Rule[] */: [],
};

// ------------------------------------------------------------------ findings

/** function decodeFinding<T>(f: T) => Finding */
function decodeFinding(f) {
  const w = f.where;
  return {
    key: String(f.key),
    rule: String(f.rule),
    category: String(f.category),
    level: String(f.level),
    title: String(f.title),
    detail: String(f.detail),
    where: {
      kind: String(w.kind),
      id: String(w.id),
      channel: String(w.channel),
      index: Math.round(Number(w.index)),
      beat: Number(w.beat),
      notes: (w.notes ?? []).map((i) => Math.round(Number(i))),
      label: String(w.label),
    },
    fix: f.fix ? String(f.fix.label) : "",
    suppressed: f.suppressed === true,
  };
}

/** function took<T>(r: T) => Undefined */
function took(r) {
  view.found = r.findings.map(decodeFinding);
  view.key = String(r.key ?? "");
  view.error = "";
}

/** function errText<E>(e: E) => String */
function errText(e) {
  return String(e && e.message ? e.message : e);
}

/** Ask for the findings of the project as it is now. */
function refresh() {
  const edits = state.edits;
  view.busy = true;
  sendJson("/api/critic", "POST", { project: encodeProject(state.project) })
    .then((r) => {
      view.busy = false;
      view.edits = edits;
      took(r);
      invalidate();
      return true;
    })
    .catch((e) => {
      view.busy = false;
      view.edits = edits;
      view.error = errText(e);
      invalidate();
      return false;
    });
}

let timer = false;

/** The findings for the project (asked for again a moment after it changes, not during a drag). */
/** function findings() => Finding[] */
export function findings() {
  if (view.catalog.length === 0 && !view.busy && view.edits < 0 && state.loaded) loadCatalog();
  if (view.edits !== state.edits && !view.busy && !timer && state.loaded) {
    timer = true;
    setTimeout(
      () => {
        timer = false;
        if (layoutState.dragging) {
          invalidate();
          return undefined;
        }
        refresh();
      },
      view.edits < 0 ? 0 : 350
    );
  }
  return view.found;
}

function loadCatalog() {
  getJson("/api/critic")
    .then((r) => {
      view.catalog = r.rules.map((x) => ({
        id: String(x.id),
        category: String(x.category),
        name: String(x.name),
        why: String(x.why),
        defaultOn: x.defaultOn !== false,
      }));
      invalidate();
      return true;
    })
    .catch((e) => false);
}

/** The number of findings to show on the Critic's tab (not suppressed). */
/** function criticCount() => Int */
export function criticCount() {
  return findings().filter((f) => !f.suppressed).length;
}

/** function ruleOf(id: String) => Rule */
function ruleOf(id) {
  for (const r of view.catalog) if (r.id === id) return r;
  return { id: id, category: "Project", name: id, why: "", defaultOn: true };
}

/** Apply fixes (finding keys or rule ids) as one undo step: the endpoint fixes the project as it is
 * and sends it back. If the song changed meanwhile, nothing is applied. */
/** function applyFixes(keys: String[], what: String) => Undefined */
function applyFixes(keys, what) {
  const edits = state.edits;
  view.busy = true;
  invalidate();
  sendJson("/api/critic", "POST", { project: encodeProject(state.project), fix: keys })
    .then((r) => {
      view.busy = false;
      if (state.edits !== edits) {
        toast(t("fix.songChanged"), t("fix.nothingApplied"), "info");
        invalidate();
        return false;
      }
      const fixed = decodeProject(r.project);
      const n = r.applied.length;
      commit(() => {
        state.project = fixed;
      });
      fixSelection();
      view.edits = state.edits;
      took(r);
      if (n > 0) toast(what, t("critic.fix.applied.toast.body"), "info");
      return true;
    })
    .catch((e) => {
      view.busy = false;
      toast(t("critic.fix.failed.toast.title"), errText(e), "error");
      invalidate();
      return false;
    });
}

/** Suppress or bring back a finding (by key), in the project. */
/** function setSuppressed(key: String, on: Boolean) => Undefined */
function setSuppressed(key, on) {
  commit(() => {
    const list = state.project.critic.suppress;
    const i = list.indexOf(key);
    if (on && i < 0) list.push(key);
    if (!on && i >= 0) list.splice(i, 1);
  });
  // Show it at once; the next refresh confirms.
  for (const f of view.found) if (f.key === key) f.suppressed = on;
}

/** Does the project run this check? Its default, unless the project turns it off or on. */
/** function ruleEnabled(r: Rule) => Boolean */
function ruleEnabled(r) {
  const c = state.project.critic;
  return r.defaultOn ? !c.off.includes(r.id) : c.on.includes(r.id);
}

/** Turn a check on or off for the project (only departures from its default are written). */
/** function setEnabled(id: String, on: Boolean) => Undefined */
function setEnabled(id, on) {
  const r = ruleOf(id);
  commit(() => {
    const c = state.project.critic;
    c.off = c.off.filter((x) => x !== id);
    c.on = c.on.filter((x) => x !== id);
    if (on && !r.defaultOn) c.on.push(id);
    if (!on && r.defaultOn) c.off.push(id);
  });
  if (!on) view.found = view.found.filter((f) => f.rule !== id);
}

/** Show where a finding points: the notes in the piano roll, the insert in the mixer, the bar in the playlist. */
/** function reveal(w: Where) => Undefined */
function reveal(w) {
  if (w.kind === "pattern") {
    selectPattern(w.id);
    if (w.channel !== "") selectChannel(w.channel);
    state.selection = w.notes.map((i) => noteIx(i));
    openDock(w.channel !== "" || w.notes.length > 0 ? "piano" : "rack");
  } else if (w.kind === "channel") {
    selectChannel(w.id);
    openDock("rack");
  } else if (w.kind === "insert") {
    selectInsert(insertIx(w.index));
    openDock("mixer");
  } else if (w.kind === "song" || w.kind === "lane") {
    setTop("playlist");
    if (isCompact(window.innerWidth, window.innerHeight)) setView("playlist");
    if (w.kind === "song" && w.index >= 0) state.clipSelection = [clipIx(w.index)];
    revealBeat(Math.max(0, w.beat));
  }
  invalidate();
}

/** Hand an issue to the agent in the terminal (the producer presses Enter). */
/** function askText(f: Finding) => String */
function askText(f) {
  return `The Critic says (${f.where.label}): ${f.title}. ${f.detail} Please look into it and fix it if it should be fixed (rosaclef critic lists it as ${f.key}).`;
}

// ------------------------------------------------------------------ view

/** One finding. `ask` hands an issue to the agent (when there is one). */
/** function findingView(b: Builder, f: Finding, ask: (String) => Undefined) => Undefined */
function findingView(b, f, ask) {
  b.open("div", f.key, `crit-item ${f.level}${f.fix !== "" ? " fixable" : ""}${f.suppressed ? " suppressed" : ""}`);
  b.attr("data-key", f.key);
  b.leaf("span", "dot", "crit-dot", "");
  b.open("div", "body", "crit-body");
  b.leaf("div", "t", "crit-title", f.title);
  b.leaf("div", "d", "crit-detail", f.detail);
  b.open("div", "acts", "crit-acts");
  if (f.where.kind !== "project") {
    b.open("button", "where", "crit-where");
    b.attr("title", t("common.showIt"));
    b.on("pointerenter", (e) => hint(tf("critic.finding.where.hint", [f.where.label])));
    b.on("click", (e) => reveal(f.where));
    glyph(b, f.where.kind === "insert" ? "mixer" : f.where.kind === "pattern" ? "piano" : f.where.kind === "channel" ? "rack" : "playlist");
    b.leaf("span", "l", "", f.where.label);
    b.close();
  } else if (f.where.label !== "") b.leaf("span", "where", "crit-where static", f.where.label);
  b.leaf("span", "sp", "spacer", "");
  if (f.suppressed) {
    button(b, "back", "small ghost", t("critic.finding.unsuppress.label"), t("critic.finding.unsuppress.title"), () => setSuppressed(f.key, false));
  } else {
    if (f.fix !== "") button(b, "fix", "small gold", f.fix, tf("fix.applyOneUndo", [f.fix]), () => applyFixes([f.key], f.fix));
    else if (state.backend !== "local") button(b, "ask", "small ghost", t("agent.askMaestro"), t("critic.finding.ask.title"), () => ask(askText(f)));
    iconButton(b, "sup", "small ghost", "close", t("critic.finding.suppress.title"), () => setSuppressed(f.key, true));
  }
  b.close();
  b.close();
  b.close();
}

/** The list of checks, to turn them on and off. */
/** function rulesView(b: Builder) => Undefined */
function rulesView(b) {
  b.open("div", "rules", "crit-rules");
  b.leaf("p", "intro", "crit-intro", tf("critic.rules.intro", [String(view.catalog.length)]));
  for (const cat of CATEGORIES) {
    b.leaf("h4", `h-${cat.value}`, "crit-cat", t(cat.label));
    for (const r of view.catalog) {
      if (r.category !== cat.value) continue;
      const on = ruleEnabled(r);
      b.open("label", r.id, on ? "crit-rule on" : "crit-rule");
      b.attr("title", r.why);
      b.leaf("input", "cb", "", "");
      b.attr("type", "checkbox");
      b.prop("checked", on ? "true" : "");
      b.on("change", (e) => setEnabled(r.id, e.checked));
      b.open("span", "txt", "crit-rule-text");
      b.open("b", "n", "");
      b.text(r.name);
      if (!r.defaultOn) b.leaf("span", "def", "crit-default", t("critic.rules.offByDefault"));
      b.close();
      b.leaf("span", "w", "", r.why);
      b.close();
      b.close();
    }
  }
  b.close();
}

/** The Critic's panel (in the Maestro panel, over the terminal). */
/** function criticPanel(b: Builder, ask: (String) => Undefined) => Undefined */
export function criticPanel(b, ask) {
  const all = findings();
  const shown = all.filter((f) => view.showSuppressed || !f.suppressed);
  const nSuggest = shown.filter((f) => f.fix !== "" && !f.suppressed).length;
  const nIssue = shown.filter((f) => f.fix === "" && !f.suppressed).length;
  const nSuppressed = all.filter((f) => f.suppressed).length + state.project.critic.off.length;
  const list = shown.filter((f) => view.filter === "all" || (view.filter === "suggest") === (f.fix !== ""));

  b.open("div", "critic", view.busy ? "critic busy" : "critic");
  b.open("div", "bar", "crit-bar");
  for (const x of [
    { id: "all", label: tf("critic.filter.all.label", [String(nSuggest + nIssue)]), tip: t("critic.filter.all.title") },
    { id: "suggest", label: tf("critic.filter.suggest.label", [String(nSuggest)]), tip: t("critic.filter.suggest.title") },
    { id: "issue", label: tf("critic.filter.issue.label", [String(nIssue)]), tip: t("critic.filter.issue.title") },
  ]) {
    b.open("button", x.id, view.filter === x.id && !view.rules ? "crit-seg on" : "crit-seg");
    b.attr("title", x.tip);
    b.on("click", (e) => {
      view.filter = x.id;
      view.rules = false;
      invalidate();
    });
    b.text(x.label);
    b.close();
  }
  b.leaf("span", "sp", "spacer", "");
  if (nSuppressed > 0) {
    b.open("button", "sup", view.showSuppressed ? "crit-seg on" : "crit-seg");
    b.attr("title", view.showSuppressed ? t("critic.filter.suppressed.hide.title") : t("critic.filter.suppressed.show.title"));
    b.on("click", (e) => {
      view.showSuppressed = !view.showSuppressed;
      invalidate();
    });
    b.text(tf("critic.filter.suppressed.label", [String(nSuppressed)]));
    b.close();
  }
  iconButton(
    b,
    "checks",
    view.rules ? "small gold" : "small ghost",
    "critic",
    view.rules ? t("critic.checks.back.title") : t("critic.checks.choose.title"),
    () => {
      view.rules = !view.rules;
      invalidate();
    }
  );
  b.close();

  b.open("div", "scroll", "crit-scroll");
  if (view.rules) rulesView(b);
  else {
    b.open("div", "sum", "crit-summary");
    if (view.key !== "") {
      b.open("span", "key", "chip");
      b.text(t("term.key") + " ");
      b.leaf("b", "v", "", view.key);
      b.close();
    }
    const fixKeys = list.filter((f) => f.fix !== "" && !f.suppressed).map((f) => f.key);
    if (fixKeys.length > 1)
      button(b, "all", "small", tf("critic.applyAll.label", [String(fixKeys.length)]), t("critic.applyAll.title"), () =>
        applyFixes(fixKeys, tf("critic.applyAll.done.toast.title", [String(fixKeys.length)]))
      );
    b.close();
    if (view.error !== "") b.leaf("div", "err", "crit-error", tf("critic.error.readFailed", [view.error]));
    if (view.showSuppressed && state.project.critic.off.length > 0) {
      b.open("div", "off", "crit-offlist");
      b.leaf("span", "l", "", t("critic.offList.label"));
      for (const id of state.project.critic.off) {
        b.open("button", id, "chip crit-offchip");
        b.attr("title", tf("critic.offList.chip.title", [ruleOf(id).why]));
        b.on("click", (e) => setEnabled(id, true));
        b.text(ruleOf(id).name);
        b.leaf("span", "x", "", " ↺");
        b.close();
      }
      b.close();
    }
    if (list.length === 0 && view.error === "") {
      b.open("div", "empty", "crit-empty");
      glyph(b, "spark");
      const nothing = all.filter((f) => !f.suppressed).length === 0;
      b.leaf("h3", "h", "", view.edits < 0 ? t("common.listening") : nothing ? t("critic.empty.nothing.title") : t("critic.empty.filtered.title"));
      b.leaf(
        "p",
        "p",
        "",
        view.edits < 0
          ? t("critic.empty.listening.body")
          : nothing
            ? state.project.patterns.length === 0
              ? t("critic.empty.noNotes.body")
              : t("critic.empty.allPass.body")
            : t("critic.empty.filtered.body")
      );
      b.close();
    }
    // Grouped by category, then by rule.
    for (const cat of CATEGORIES) {
      const inCat = list.filter((f) => f.category === cat.value);
      if (inCat.length === 0) continue;
      b.open("section", cat.value, "crit-group");
      b.leaf("h4", "h", "crit-cat", t(cat.label));
      /** const ids: String[] */
      const ids = [];
      for (const f of inCat) if (!ids.includes(f.rule)) ids.push(f.rule);
      for (const id of ids) {
        const r = ruleOf(id);
        const fs = inCat.filter((f) => f.rule === id);
        const folded = view.folded.includes(id);
        b.open("div", id, "crit-rulegroup");
        b.open("div", "head", "crit-rulehead");
        b.attr("title", r.why);
        b.on("click", (e) => {
          if (folded) view.folded.splice(view.folded.indexOf(id), 1);
          else view.folded.push(id);
          invalidate();
        });
        b.leaf("span", "caret", folded ? "crit-caret folded" : "crit-caret", "▾");
        b.leaf("b", "n", "", r.name);
        b.leaf("span", "c", "crit-count", String(fs.length));
        b.leaf("span", "sp", "spacer", "");
        const keys = fs.filter((f) => f.fix !== "" && !f.suppressed).map((f) => f.key);
        if (keys.length > 1) {
          b.open("span", "fixall", "crit-link");
          b.attr("title", tf("critic.rule.fixAll.title", [String(keys.length)]));
          b.on("click", (e) => {
            e.stopPropagation();
            applyFixes(keys, tf("critic.rule.fixAll.done.toast.title", [r.name, String(keys.length)]));
          });
          b.text(tf("critic.rule.fixAll.label", [String(keys.length)]));
          b.close();
        }
        b.open("span", "off", "crit-link quiet");
        b.attr("title", t("critic.rule.turnOff.title"));
        b.on("click", (e) => {
          e.stopPropagation();
          setEnabled(id, false);
          toast(tf("critic.rule.turnOff.done.toast.title", [r.name]), t("critic.rule.turnOff.done.toast.body"), "info");
        });
        b.text(t("critic.rule.turnOff.label"));
        b.close();
        b.close();
        if (!folded) {
          b.leaf("p", "why", "crit-why", r.why);
          for (const f of fs) findingView(b, f, ask);
        }
        b.close();
      }
      b.close();
    }
  }
  b.close();
  b.close();
}
