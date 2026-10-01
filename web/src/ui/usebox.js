// Patterns used by reference (Pattern.uses), in the piano roll. What each
// use brings in is drawn as ghost notes inside a box, labeled with the used
// pattern and how it is changed; the notes are not the pattern's own, so
// they cannot be selected or dragged. The label opens the use's menu (Open,
// Make unique, transpose, verse, remove); Make reference turns selected
// notes into a new use. The edits themselves are in reuse.js.

import { state, commit, currentPattern, selectPattern, selectChannel, invalidate, hint, reportContext } from "../store.js";
import { patternIndex, verseCount, sings } from "../expand.js";
import { useBoxes, useLabel, useSpan, makeUnique, makeReference, removeUse } from "../reuse.js";
import { semitonesText } from "../model.js";
import { contextMenu, item } from "./menu.js";
import { toast } from "./toast.js";
import { noteIx, noteIndex } from "#brands";

/** The use whose menu is open: in which pattern, which use (-1: none), where (viewport px). */
const menu = { pattern: "", use: -1, x: 0, y: 0 };

/** function openUseMenu(pattern: String, k: Int, x: Number, y: Number) => Undefined */
function openUseMenu(pattern, k, x, y) {
  menu.pattern = pattern;
  menu.use = k;
  menu.x = x;
  menu.y = y;
  invalidate();
}

function closeUseMenu() {
  menu.use = -1;
  invalidate();
}

/** The color a use is drawn in: its pattern's. */
/** function useColor(p: Project, u: Use) => String */
function useColor(p, u) {
  const j = patternIndex(p, u.pattern);
  return j >= 0 ? p.patterns[j].color : "#8a7f6c";
}

/** Draw the uses of patterns[index] into the grid (x0..x1: the visible pixels): ghost notes,
 * those on `channel` in its color, in a labeled box per use. */
/** function useLayer(b: Builder, zoom: Number, rowH: Number, index: Int, channel: Channel, x0: Number, x1: Number) => Undefined */
export function useLayer(b, zoom, rowH, index, channel, x0, x1) {
  const p = state.project;
  const pat = p.patterns[index];
  for (const box of useBoxes(p, index, 1)) {
    const left = box.start * zoom;
    if (left > x1 || box.end * zoom < x0) continue;
    const u = pat.uses[box.use];
    const k = box.use;
    for (let i = 0; i < box.notes.length; i++) {
      const n = box.notes[i];
      const mine = n.channel === channel.id;
      b.leaf("div", `u${k}n${i}`, mine ? "note used" : "note ghost", "");
      b.style("left", `${n.start * zoom}px`);
      b.style("top", `${(127 - n.pitch) * rowH}px`);
      b.style("width", `${Math.max(3, n.length * zoom - 1)}px`);
      b.style("height", `${rowH - 1}px`);
      b.style("--c", mine ? channel.color : "#8a7f6c");
      b.style("--vel", "0.5");
    }
    const open = menu.use === k && menu.pattern === pat.id;
    b.open("div", `u${k}`, open ? "use-box open" : "use-box");
    b.style("left", `${left}px`);
    b.style("top", `${(127 - box.high) * rowH - 3}px`);
    b.style("width", `${(box.end - box.start) * zoom}px`);
    b.style("height", `${(box.high - box.low + 1) * rowH + 6}px`);
    b.style("--c", useColor(p, u));
    useTab(b, pat.id, k, box.label);
    b.close();
  }
}

/** A use box's label: click (or right-click) for its menu, double-click to open the used pattern. */
/** function useTab(b: Builder, pattern: String, k: Int, label: String) => Undefined */
function useTab(b, pattern, k, label) {
  b.leaf("div", "tab", "use-tab", label);
  b.attr("title", `${label} — played by reference: click for its menu, double-click to open it`);
  b.on("pointerenter", (e) => hint(`${label} — a pattern played here by reference: click for its menu, double-click to open it`));
  b.on("pointerdown", (e) => {
    e.stopPropagation();
  });
  b.on("click", (e) => openUseMenu(pattern, k, e.clientX, e.clientY));
  b.on("contextmenu", (e) => {
    e.preventDefault();
    e.stopPropagation();
    openUseMenu(pattern, k, e.clientX, e.clientY);
  });
  b.on("dblclick", (e) => {
    closeUseMenu();
    openUsed(pattern, k);
  });
}

/** Show the pattern a use plays in the piano roll (on a channel it has notes on, when the use moves them). */
/** function openUsed(pattern: String, k: Int) => Undefined */
function openUsed(pattern, k) {
  const p = state.project;
  const i = patternIndex(p, pattern);
  if (i < 0 || k >= p.patterns[i].uses.length) return undefined;
  const u = p.patterns[i].uses[k];
  const j = patternIndex(p, u.pattern);
  if (j < 0) return undefined;
  selectPattern(u.pattern);
  const notes = p.patterns[j].notes;
  if (u.channel !== "" && notes.length > 0 && !notes.some((n) => n.channel === state.channel)) selectChannel(notes[0].channel);
}

/** The open use menu, if it belongs to patterns[index]. */
/** function useMenu(b: Builder, index: Int) => Undefined */
export function useMenu(b, index) {
  // A stable container: a child coming and going would re-append (and so
  // scroll back) the grid beside it.
  b.open("div", "use-menu-root", "use-menu-root");
  useMenuBody(b, index);
  b.close();
}

/** function useMenuBody(b: Builder, index: Int) => Undefined */
function useMenuBody(b, index) {
  const p = state.project;
  const pat = p.patterns[index];
  if (menu.use < 0 || menu.pattern !== pat.id) return undefined;
  if (menu.use >= pat.uses.length) {
    menu.use = -1;
    return undefined;
  }
  const k = menu.use;
  const u = pat.uses[k];
  const where = `at beat ${Math.round(u.start * 100) / 100} · ${Math.round(useSpan(p, u) * 100) / 100} beats`;
  contextMenu(b, { x: menu.x, y: menu.y, title: useLabel(p, u), sub: where, note: "", items: useItems(p, index, k) }, closeUseMenu);
}

/** What a use's menu offers. */
/** function useItems(p: Project, index: Int, k: Int) => MenuItem[] */
function useItems(p, index, k) {
  const pat = p.patterns[index];
  const u = pat.uses[k];
  const j = patternIndex(p, u.pattern);
  const name = j >= 0 ? p.patterns[j].name : u.pattern;
  const items = [
    item("open", "piano", `Open “${name}”`, () => openUsed(pat.id, k)),
    item("unique", "copy", "Make unique (its notes become this pattern's own)", () => uniqueUse(index, k)),
  ];
  for (const d of [1, -1, 12, -12]) {
    const label = `Transpose ${semitonesText(d)}${Math.abs(d) === 12 ? " (an octave)" : ""}`;
    items.push(item(`t${d}`, d > 0 ? "plus" : "minus", label, () => commit(() => setTranspose(u, u.transpose + d))));
  }
  if (j >= 0 && sings(p, j)) {
    const auto = item("v0", "mic", "Sings this pattern's verse", () => commit(() => setVerse(u, 0)));
    auto.on = u.verse === 0;
    items.push(auto);
    for (let v = 1; v <= Math.max(verseCount(p, j), u.verse); v++) {
      const it = item(`v${v}`, "mic", `Sings verse ${v}`, () => commit(() => setVerse(u, v)));
      it.on = u.verse === v;
      items.push(it);
    }
  }
  items.push(item("rm", "trash", "Remove", () => commit(() => removeUse(p, index, k))));
  return items;
}

/** function setTranspose(u: Use, semis: Int) => Undefined */
function setTranspose(u, semis) {
  u.transpose = Math.max(-48, Math.min(48, semis));
}

/** function setVerse(u: Use, verse: Int) => Undefined */
function setVerse(u, verse) {
  u.verse = verse;
}

/** Make a use plain notes and select them. */
/** function uniqueUse(index: Int, k: Int) => Undefined */
function uniqueUse(index, k) {
  /** let fresh: Int[] */
  let fresh = [];
  commit(() => {
    fresh = makeUnique(state.project, index, k);
  });
  state.selection = fresh.map(noteIx);
  reportContext();
}

/** Make the selected notes a new pattern, used where they were. */
export function referSelection() {
  const pat = currentPattern();
  if (!pat || state.selection.length === 0) return undefined;
  const index = state.project.patterns.indexOf(pat);
  const picked = state.selection.map(noteIndex);
  let id = "";
  commit(() => {
    id = makeReference(state.project, index, picked);
  });
  state.selection = [];
  reportContext();
  const made = state.project.patterns.find((x) => x.id === id);
  if (made) toast(`Made “${made.name}”`, `${picked.length} notes now play by reference. Ctrl+Z undoes it.`, "info");
}
