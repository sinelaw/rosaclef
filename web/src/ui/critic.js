// The Critic: a Maestro plugin that lints the project (see ../critic.js).
// Suggestions apply with one click (one undo step each); issues only inform —
// in the native studio they can be handed to the agent. Findings are
// recomputed shortly after the project changes, never while a drag is
// running. Turned-off checks and ignored findings are remembered in this
// browser (ignores per project).

import { state, commit, invalidate, fixSelection, selectPattern, selectChannel, selectInsert, hint } from "../store.js";
import { critique, songKey, RULES, CATEGORIES, ruleOf } from "../critic.js";
import { button, iconButton, glyph } from "./widgets.js";
import { openDock, setTop, setView, isCompact, layoutState } from "./panes.js";
import { revealBeat } from "./playlist.js";
import { toast } from "./toast.js";
import { insertIx, noteIx, clipIx } from "#brands";

const view = {
  /** "all", "suggest" (with a fix) or "issue" (without). */
  filter: "all",
  /** Show the list of checks (to turn them on and off). */
  rules: false,
  showIgnored: false,
  /** Rules whose instances are folded away (by rule id). */
  folded /*: String[] */: [],
  /** The project version the findings are for (-1: none yet). */
  edits: -1,
  key: "",
  pending: false,
  found /*: Finding[] */: [],
};

// ------------------------------------------------------------------ settings

const OFF_KEY = "rosaclef.critic.off";
const IGNORED_KEY = "rosaclef.critic.ignored";

/** function readList(key: String, sep: String) => String[] */
function readList(key, sep) {
  const v = localStorage.getItem(key) ?? "";
  return v === "" ? [] : v.split(sep);
}

/** const off: String[] */
const off = readList(OFF_KEY, ",");
/** const ignored: String[] */
const ignored = readList(IGNORED_KEY, "\n");

function save() {
  localStorage.setItem(OFF_KEY, off.join(","));
  // Keep the newest few hundred ignores.
  while (ignored.length > 400) ignored.shift();
  localStorage.setItem(IGNORED_KEY, ignored.join("\n"));
}

/** An ignore is remembered per project folder. */
/** function ignoreKey(f: Finding) => String */
function ignoreKey(f) {
  return `${state.folder}|${f.key}`;
}

/** function isIgnored(f: Finding) => Boolean */
function isIgnored(f) {
  return ignored.includes(ignoreKey(f));
}

// ------------------------------------------------------------------ findings

function refresh() {
  view.found = critique(state.project, off);
  view.key = songKey(state.project);
  view.edits = state.edits;
}

/** The findings for the project as it is (recomputed a moment after it changes, not during a drag). */
/** function findings() => Finding[] */
export function findings() {
  if (view.edits === state.edits || view.pending) return view.found;
  if (view.edits < 0) {
    refresh();
    return view.found;
  }
  view.pending = true;
  setTimeout(() => {
    view.pending = false;
    if (layoutState.dragging) return undefined;
    refresh();
    invalidate();
  }, 350);
  return view.found;
}

/** The number of findings to show on the Critic's tab (not ignored). */
/** function criticCount() => Int */
export function criticCount() {
  return findings().filter((f) => !isIgnored(f)).length;
}

/** Apply fixes as one undo step. Each is looked up again on the project as it is by then
 * (an earlier fix can move the notes a later one points at). */
/** function applyFixes(keys: String[], what: String) => Undefined */
function applyFixes(keys, what) {
  let done = 0;
  commit(() => {
    for (const k of keys) {
      const f = critique(state.project, off).find((x) => x.key === k);
      if (!f || f.fix === "") continue;
      f.apply(state.project);
      done = done + 1;
    }
  });
  fixSelection();
  refresh();
  if (done > 0) toast(what, "Ctrl+Z undoes it.", "info");
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
  return `The Critic says (${f.where.label}): ${f.title}. ${f.detail} Please look into it and fix it if it should be fixed.`;
}

// ------------------------------------------------------------------ view

/** One finding. `ask` hands an issue to the agent (when there is one). */
/** function findingView(b: Builder, f: Finding, ask: (String) => Undefined) => Undefined */
function findingView(b, f, ask) {
  const ign = isIgnored(f);
  b.open("div", f.key, `crit-item ${f.level}${f.fix !== "" ? " fixable" : ""}${ign ? " ignored" : ""}`);
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
  if (f.fix !== "" && !ign) {
    button(b, "fix", "small gold", f.fix, `Apply: ${f.fix} (one undo step)`, () => applyFixes([f.key], f.fix));
  } else if (f.fix === "" && !ign && state.backend !== "local") {
    button(b, "ask", "small ghost", "Ask Maestro", "Type this issue into the agent's prompt (press Enter in the terminal to send)", () => ask(askText(f)));
  }
  iconButton(b, "ign", "small ghost", ign ? "restart" : "close", ign ? "Bring it back" : "Ignore this one (in this project)", () => {
    const k = ignoreKey(f);
    if (ign) ignored.splice(ignored.indexOf(k), 1);
    else ignored.push(k);
    save();
    invalidate();
  });
  b.close();
  b.close();
  b.close();
}

/** The list of checks, to turn them on and off. */
/** function rulesView(b: Builder) => Undefined */
function rulesView(b) {
  b.open("div", "rules", "crit-rules");
  b.leaf(
    "p",
    "intro",
    "crit-intro",
    `${RULES.length} checks, all mechanical: they read the project — notes, clips, channels and the mixer — and nothing else.`
  );
  for (const cat of CATEGORIES) {
    b.leaf("h4", `h-${cat}`, "crit-cat", cat);
    for (const r of RULES) {
      if (r.category !== cat) continue;
      const on = !off.includes(r.id);
      b.open("label", r.id, on ? "crit-rule on" : "crit-rule");
      b.attr("title", r.why);
      b.leaf("input", "cb", "", "");
      b.attr("type", "checkbox");
      b.prop("checked", on ? "true" : "");
      b.on("change", (e) => {
        if (e.checked) off.splice(off.indexOf(r.id), 1);
        else off.push(r.id);
        save();
        refresh();
        invalidate();
      });
      b.open("span", "txt", "crit-rule-text");
      b.leaf("b", "n", "", r.name);
      b.leaf("span", "w", "", r.why);
      b.close();
      b.close();
    }
  }
  b.close();
}

/** The Critic's panel (in the Maestro panel, in place of the terminal). */
/** function criticPanel(b: Builder, ask: (String) => Undefined) => Undefined */
export function criticPanel(b, ask) {
  const all = findings();
  const shown = all.filter((f) => view.showIgnored || !isIgnored(f));
  const nSuggest = shown.filter((f) => f.fix !== "").length;
  const nIssue = shown.length - nSuggest;
  const nIgnored = all.filter(isIgnored).length;
  const list = shown.filter((f) => view.filter === "all" || (view.filter === "suggest") === (f.fix !== ""));

  b.open("div", "critic", "critic");
  b.open("div", "bar", "crit-bar");
  for (const x of [
    { id: "all", label: `All ${shown.length}`, tip: "Every finding" },
    { id: "suggest", label: `Suggestions ${nSuggest}`, tip: "Findings with a fix you can apply" },
    { id: "issue", label: `Issues ${nIssue}`, tip: "Findings to know about (no automatic fix)" },
  ]) {
    b.open("button", x.id, view.filter === x.id ? "crit-seg on" : "crit-seg");
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
  if (nIgnored > 0) {
    b.open("button", "ign", view.showIgnored ? "crit-seg on" : "crit-seg");
    b.attr("title", view.showIgnored ? "Hide the findings you ignored" : "Show the findings you ignored");
    b.on("click", (e) => {
      view.showIgnored = !view.showIgnored;
      invalidate();
    });
    b.text(`Ignored ${nIgnored}`);
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
    const fixKeys = list.filter((f) => f.fix !== "" && !isIgnored(f)).map((f) => f.key);
    if (fixKeys.length > 1)
      button(b, "all", "small", `Apply all ${fixKeys.length} suggestions`, "Apply every suggestion shown (one undo step)", () =>
        applyFixes(fixKeys, `${fixKeys.length} suggestions applied`)
      );
    b.close();
    if (list.length === 0) {
      b.open("div", "empty", "crit-empty");
      glyph(b, "spark");
      b.leaf("h3", "h", "", all.length === 0 ? "Nothing to criticise" : "Nothing here");
      b.leaf(
        "p",
        "p",
        "",
        all.length === 0
          ? state.project.patterns.length === 0
            ? "Write some notes and the Critic will listen in."
            : "The project passes every check the Critic knows."
          : "Every finding of this kind is ignored or filtered out."
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
        const keys = fs.filter((f) => f.fix !== "" && !isIgnored(f)).map((f) => f.key);
        if (keys.length > 1) {
          b.open("span", "fixall", "crit-fixall");
          b.on("click", (e) => {
            e.stopPropagation();
            applyFixes(keys, `${r.name}: ${keys.length} fixes applied`);
          });
          b.text(`Fix all ${keys.length}`);
          b.close();
        }
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
