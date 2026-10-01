// The context menu: a small panel at the pointer — a title, a line under
// it and a list of actions — over a backdrop that closes it. A view that
// has one keeps whether it is open and draws it every frame from the
// current state, so an item never acts on something that is gone.

import { glyph } from "./widgets.js";

/** One action. `on` marks the current choice (a verse, a mode). */
/** type MenuItem = { key: String, icon: String, label: String, on: Boolean, run: () => Undefined } */
/** At viewport pixel (x, y): a title, a line under it, a note when there is nothing to do, the items. */
/** type Menu = { x: Number, y: Number, title: String, sub: String, note: String, items: MenuItem[] } */

/** function item(key: String, icon: String, label: String, run: () => Undefined) => MenuItem */
export function item(key, icon, label, run) {
  return { key: key, icon: icon, label: label, on: false, run: run };
}

/** Draw a menu; `close` runs on a press outside it and after an item. */
/** function contextMenu(b: Builder, m: Menu, close: () => Undefined) => Undefined */
export function contextMenu(b, m, close) {
  b.leaf("div", "ctx-backdrop", "ctx-backdrop", "");
  b.on("pointerdown", (e) => {
    e.preventDefault();
    close();
  });
  b.on("contextmenu", (e) => {
    e.preventDefault();
    close();
  });
  b.open("div", "ctx-menu", "ctx-menu");
  b.style("left", `min(${m.x}px, calc(100vw - 250px))`);
  b.style("top", `min(${m.y}px, calc(100vh - ${70 + m.items.length * 31}px))`);
  b.on("contextmenu", (e) => {
    e.preventDefault();
  });
  b.leaf("div", "t", "ctx-menu-title", m.title);
  if (m.sub !== "") b.leaf("div", "s", "ctx-menu-sub", m.sub);
  if (m.note !== "") b.leaf("div", "x", "ctx-menu-note", m.note);
  for (const it of m.items) {
    const run = it.run;
    b.open("button", it.key, it.on ? "ctx-menu-item on" : "ctx-menu-item");
    b.on("click", (e) => {
      close();
      run();
    });
    glyph(b, it.icon);
    b.leaf("span", "l", "", it.label);
    b.close();
  }
  b.close();
}
