// Time signatures: the one the song starts in (the Time LCD in the top bar)
// and changes from any bar on (right-click a bar in the playlist's ruler).
// The project keeps them in `transport`: `beatsPerBar` quarter notes a bar
// until the first of `meters` (one may be at bar 1, for 6/8 and the like).

import { listenWindow } from "#platform";
import { state, commit, invalidate, hint } from "../store.js";
import { meterMap } from "../model.js";
import { t, tf } from "../i18n.js";

/** The time signatures offered. */
export const METERS = ["2/4", "3/4", "4/4", "5/4", "6/4", "7/4", "3/8", "5/8", "6/8", "7/8", "9/8", "12/8"];

/** The time signature of `bar` (counted from 0). */
/** function meterOf(t: Transport, bar: Number) => String */
export function meterOf(t, bar) {
  const map = meterMap(t);
  let s = map[0];
  for (const m of map) if (m.bar <= bar) s = m;
  return s.label;
}

/** Make the song change to `label` ("6/8") at `bar` (counted from 0; 0 = the
 * song's own time signature). A change to the meter already playing there is
 * no change at all. One undo step. */
/** function setMeter(bar: Number, label: String) => Undefined */
export function setMeter(bar, label) {
  const parts = label.split("/");
  const n = Math.round(Number(parts[0]));
  const d = Math.round(Number(parts.length > 1 ? parts[1] : "4"));
  if (!(n > 0 && n <= 32 && d > 0)) return undefined;
  commit(() => {
    const t = state.project.transport;
    t.meters = t.meters.filter((m) => m.bar !== bar + 1);
    if (bar === 0) {
      if (d === 4) t.beatsPerBar = n;
      else t.meters.unshift({ bar: 1, numerator: n, denominator: d });
    } else if (meterOf(t, bar) !== label) {
      t.meters.push({ bar: bar + 1, numerator: n, denominator: d });
      t.meters.sort((a, b) => a.bar - b.bar);
    }
  });
}

/** Take away the change at `bar` (counted from 0, after the first). */
/** function removeMeter(bar: Number) => Undefined */
export function removeMeter(bar) {
  commit(() => {
    const t = state.project.transport;
    t.meters = t.meters.filter((m) => m.bar !== bar + 1);
  });
}

/** The Time LCD: the song's time signature, picked from a list. */
/** function meterLcd(b: Builder) => Undefined */
export function meterLcd(b) {
  const now = meterOf(state.project.transport, 0);
  const tip = t("Time signature the song starts in — right-click a bar in the playlist's ruler to change it from there");
  b.open("label", "meter", "lcd timesig");
  b.attr("title", tip);
  b.on("pointerenter", (e) => hint(tip));
  b.leaf("span", "label", "lcd-label", t("Time"));
  b.open("span", "value", "lcd-value");
  b.open("select", "s", "lcd-select");
  b.attr("aria-label", t("Time signature"));
  b.prop("value", now);
  b.on("change", (e) => setMeter(0, e.value));
  const choices = METERS.includes(now) ? METERS : [now].concat(METERS);
  for (const m of choices) {
    b.leaf("option", m, "", m);
    b.attr("value", m);
  }
  b.close();
  b.close();
  b.close();
}

// ------------------------------------------------------------------ menu

const menu = { open: false, bar: 0, x: 0, y: 0 };

/** The time signature menu of a bar (counted from 0), at the pointer. */
/** function openMeterMenu(bar: Number, x: Number, y: Number) => Undefined */
export function openMeterMenu(bar, x, y) {
  menu.open = true;
  menu.bar = bar;
  menu.x = x;
  menu.y = y;
  invalidate();
}

function closeMeterMenu() {
  menu.open = false;
  invalidate();
}

listenWindow("keydown", (e) => {
  if (menu.open && e.key === "Escape") closeMeterMenu();
});

/** The menu (rendered by the shell, in a container of its own). */
/** function meterMenu(b: Builder) => Undefined */
export function meterMenu(b) {
  b.open("div", "meter-menu-root", "meter-menu-root");
  if (menu.open) meterMenuBody(b);
  b.close();
}

/** function meterMenuBody(b: Builder) => Undefined */
function meterMenuBody(b) {
  const tr = state.project.transport;
  const bar = menu.bar;
  const now = meterOf(tr, bar);
  const changes = bar === 0 || tr.meters.some((m) => m.bar === bar + 1);
  b.leaf("div", "backdrop", "auto-backdrop", "");
  b.on("pointerdown", (e) => {
    e.preventDefault();
    closeMeterMenu();
  });
  b.on("contextmenu", (e) => {
    e.preventDefault();
    closeMeterMenu();
  });
  b.open("div", "menu", "auto-menu meter-menu");
  b.attr("role", "menu");
  b.style("left", `min(${menu.x}px, calc(100vw - 250px))`);
  b.style("top", `min(${menu.y}px, calc(100vh - 230px))`);
  b.on("contextmenu", (e) => {
    e.preventDefault();
  });
  b.leaf("div", "t", "auto-menu-title", bar === 0 ? t("Time signature") : tf("Time signature from bar {0}", [String(bar + 1)]));
  b.leaf(
    "div",
    "s",
    "auto-menu-sub",
    bar === 0 ? tf("The song starts in {0}", [now]) : changes ? tf("Changes to {0} here", [now]) : tf("In {0} here (no change)", [now])
  );
  b.open("div", "grid", "meter-grid");
  for (const m of METERS) {
    b.leaf("button", m, m === now ? "meter-choice on" : "meter-choice", m);
    b.attr("role", "menuitem");
    b.on("click", (e) => {
      closeMeterMenu();
      setMeter(bar, m);
    });
  }
  b.close();
  if (bar > 0 && changes) {
    b.open("button", "rm", "auto-menu-item");
    b.on("click", (e) => {
      closeMeterMenu();
      removeMeter(bar);
    });
    b.leaf("span", "l", "", tf("No change at bar {0}", [String(bar + 1)]));
    b.close();
  }
  b.close();
}
