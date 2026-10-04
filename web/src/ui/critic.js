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

const CATEGORIES = ["Harmony", "Melody", "Rhythm", "Arrangement", "Low end", "Mix", "Stereo", "Effects", "Master", "Project"];

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
      view.catalog = r.rules.map((x) => ({ id: String(x.id), category: String(x.category), name: String(x.name), why: String(x.why) }));
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
  return { id: id, category: "Project", name: id, why: "" };
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
        toast("The song changed meanwhile", "Nothing was applied; try again.", "info");
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
      if (n > 0) toast(what, "Ctrl+Z undoes it.", "info");
      return true;
    })
    .catch((e) => {
      view.busy = false;
      toast("Could not apply the fix", errText(e), "error");
      invalidate();
      return false;
    });
}

/** Suppress or bring back a finding (by key) or a whole check (by rule id), in the project. */
/** function setSuppressed(what: String, rule: Boolean, on: Boolean) => Undefined */
function setSuppressed(what, rule, on) {
  commit(() => {
    const c = state.project.critic;
    const list = rule ? c.off : c.suppress;
    const i = list.indexOf(what);
    if (on && i < 0) list.push(what);
    if (!on && i >= 0) list.splice(i, 1);
  });
  // Show it at once; the next refresh confirms.
  if (rule && on) view.found = view.found.filter((f) => f.rule !== what);
  if (!rule) for (const f of view.found) if (f.key === what) f.suppressed = on;
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
    b.attr("title", "Show it");
    b.on("pointerenter", (e) => hint(`Show ${f.where.label}`));
    b.on("click", (e) => reveal(f.where));
    glyph(b, f.where.kind === "insert" ? "mixer" : f.where.kind === "pattern" ? "piano" : f.where.kind === "channel" ? "rack" : "playlist");
    b.leaf("span", "l", "", f.where.label);
    b.close();
  } else if (f.where.label !== "") b.leaf("span", "where", "crit-where static", f.where.label);
  b.leaf("span", "sp", "spacer", "");
  if (f.suppressed) {
    button(b, "back", "small ghost", "Unsuppress", "Bring this finding back (saved in the project)", () => setSuppressed(f.key, false, false));
  } else {
    if (f.fix !== "") button(b, "fix", "small gold", f.fix, `Apply: ${f.fix} (one undo step)`, () => applyFixes([f.key], f.fix));
    else if (state.backend !== "local")
      button(b, "ask", "small ghost", "Ask Maestro", "Type this issue into the agent's prompt (press Enter in the terminal to send)", () => ask(askText(f)));
    iconButton(b, "sup", "small ghost", "close", "Suppress this finding (saved in the project; the agent and `rosaclef critic` leave it out too)", () =>
      setSuppressed(f.key, false, true)
    );
  }
  b.close();
  b.close();
  b.close();
}

/** The list of checks, to turn them on and off. */
/** function rulesView(b: Builder) => Undefined */
function rulesView(b) {
  const off = state.project.critic.off;
  b.open("div", "rules", "crit-rules");
  b.leaf(
    "p",
    "intro",
    "crit-intro",
    `${view.catalog.length} checks, all mechanical: they read the project — notes, clips, channels and the mixer — and nothing else. Turning one off is saved in the project.`
  );
  for (const cat of CATEGORIES) {
    b.leaf("h4", `h-${cat}`, "crit-cat", cat);
    for (const r of view.catalog) {
      if (r.category !== cat) continue;
      const on = !off.includes(r.id);
      b.open("label", r.id, on ? "crit-rule on" : "crit-rule");
      b.attr("title", r.why);
      b.leaf("input", "cb", "", "");
      b.attr("type", "checkbox");
      b.prop("checked", on ? "true" : "");
      b.on("change", (e) => setSuppressed(r.id, true, !e.checked));
      b.open("span", "txt", "crit-rule-text");
      b.leaf("b", "n", "", r.name);
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
    { id: "all", label: `All ${nSuggest + nIssue}`, tip: "Every finding" },
    { id: "suggest", label: `Suggestions ${nSuggest}`, tip: "Findings with a fix you can apply" },
    { id: "issue", label: `Issues ${nIssue}`, tip: "Findings to know about (no automatic fix)" },
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
    b.attr("title", view.showSuppressed ? "Hide the suppressed findings" : "Show the suppressed findings (and the checks turned off)");
    b.on("click", (e) => {
      view.showSuppressed = !view.showSuppressed;
      invalidate();
    });
    b.text(`Suppressed ${nSuppressed}`);
    b.close();
  }
  iconButton(b, "checks", view.rules ? "small gold" : "small ghost", "critic", view.rules ? "Back to the findings" : "Choose the checks", () => {
    view.rules = !view.rules;
    invalidate();
  });
  b.close();

  b.open("div", "scroll", "crit-scroll");
  if (view.rules) rulesView(b);
  else {
    b.open("div", "sum", "crit-summary");
    if (view.key !== "") {
      b.open("span", "key", "chip");
      b.text("Key ");
      b.leaf("b", "v", "", view.key);
      b.close();
    }
    const fixKeys = list.filter((f) => f.fix !== "" && !f.suppressed).map((f) => f.key);
    if (fixKeys.length > 1)
      button(b, "all", "small", `Apply all ${fixKeys.length} suggestions`, "Apply every suggestion shown (one undo step)", () =>
        applyFixes(fixKeys, `${fixKeys.length} suggestions applied`)
      );
    b.close();
    if (view.error !== "") b.leaf("div", "err", "crit-error", `The Critic could not read the song: ${view.error}`);
    if (view.showSuppressed && state.project.critic.off.length > 0) {
      b.open("div", "off", "crit-offlist");
      b.leaf("span", "l", "", "Checks turned off:");
      for (const id of state.project.critic.off) {
        b.open("button", id, "chip crit-offchip");
        b.attr("title", `${ruleOf(id).why} — click to turn it back on`);
        b.on("click", (e) => setSuppressed(id, true, false));
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
      b.leaf("h3", "h", "", view.edits < 0 ? "Listening…" : nothing ? "Nothing to criticise" : "Nothing here");
      b.leaf(
        "p",
        "p",
        "",
        view.edits < 0
          ? "The Critic is reading the song."
          : nothing
            ? state.project.patterns.length === 0
              ? "Write some notes and the Critic will listen in."
              : "The project passes every check the Critic knows."
            : "Every finding of this kind is suppressed or filtered out."
      );
      b.close();
    }
    // Grouped by category, then by rule.
    for (const cat of CATEGORIES) {
      const inCat = list.filter((f) => f.category === cat);
      if (inCat.length === 0) continue;
      b.open("section", cat, "crit-group");
      b.leaf("h4", "h", "crit-cat", cat);
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
          b.attr("title", `Apply all ${keys.length} fixes of this check (one undo step)`);
          b.on("click", (e) => {
            e.stopPropagation();
            applyFixes(keys, `${r.name}: ${keys.length} fixes applied`);
          });
          b.text(`Fix all ${keys.length}`);
          b.close();
        }
        b.open("span", "off", "crit-link quiet");
        b.attr("title", "Turn this check off for the project (saved in it; the checks button turns it back on)");
        b.on("click", (e) => {
          e.stopPropagation();
          setSuppressed(id, true, true);
          toast(`${r.name}: turned off`, "Saved in the project. Ctrl+Z, or the checks button, turns it back on.", "info");
        });
        b.text("Turn off");
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
