// Tests for the UI tree, run with: node web/test/tree.test.js
// (also type-checked by inty via web/check.sh).
import { mount, increasingRun } from "../tree/tree.js";
import { handleIndex } from "#tree-ids";
import { memoryBackend, blankEvent } from "../tree/memory.js";

let failures = 0;
/** function check(name: String, ok: Boolean) => Undefined */
function check(name, ok) {
  if (!ok) failures = failures + 1;
  console.log(`${ok ? "ok  " : "FAIL"} ${name}`);
}

const state = { items /*: String[] */: ["a", "b", "c"], clicks: 0, title: "Hello" };
const mem = memoryBackend();
const ui = mount(mem.backend, (b) => {
  b.open("div", "app", "app");
  b.leaf("h1", "", "title", state.title);
  b.open("ul", "", "list");
  for (const it of state.items) {
    b.leaf("li", it, "item", it);
    b.on("click", (e) => {
      state.clicks = state.clicks + 1;
    });
  }
  b.close();
  b.close();
});

ui.flush();
check("renders the description", mem.dump() === 'div.app\n  h1.title "Hello"\n  ul.list\n    li.item "a"\n    li.item "b"\n    li.item "c"\n');
const created = ui.stats().created;

state.title = "World";
ui.flush();
check("updates text in place", mem.dump().includes('h1.title "World"') && ui.stats().created === created);

state.items = ["c", "a", "b"];
ui.flush();
check("keyed children are reordered, not recreated", mem.dump().includes('li.item "c"\n    li.item "a"\n    li.item "b"') && ui.stats().created === created);

state.items = ["a", "d"];
ui.flush();
check(
  "removed keys are disposed and new keys created",
  mem.dump().includes('li.item "a"\n    li.item "d"\n') && !mem.dump().includes('"b"') && ui.stats().created === created + 1
);

check("handlers dispatch to the latest description", mem.fire("item", "click", blankEvent()) && state.clicks === 1);

const before = mem.ops();
ui.flush();
check("an unchanged rebuild performs no backend operations", mem.ops() === before);

// Children that keep their order are not moved when others come and go about them
// (a browser paints a moved node again, and a scrolled one loses its place).
/** const moved: Int[] */
const moved = [];
const mem2 = memoryBackend();
const watched = {
  ...mem2.backend,
  append: (p, c) => {
    moved.push(handleIndex(c));
    mem2.backend.append(p, c);
  },
  insert: (p, c, before) => {
    moved.push(handleIndex(c));
    mem2.backend.insert(p, c, before);
  },
};
const rows = { keys /*: String[] */: ["p", "a", "b", "c", "q"] };
const ui2 = mount(watched, (b) => {
  b.open("div", "", "list");
  for (const k of rows.keys) b.leaf("i", k, "", k);
  b.close();
});
ui2.flush();
/** function shown() => String */
function shown() {
  return mem2.dump().split("\n").slice(1).join("").replaceAll(" ", "").replaceAll("i", "").replaceAll('"', "");
}
/** Show `keys`; say how many nodes were moved or attached, and whether they show in that order. */
/** function step(keys: String[]) => Int */
function step(keys) {
  rows.keys = keys;
  moved.length = 0;
  ui2.flush();
  return shown() === keys.join("") ? moved.length : -1;
}
check("a child put first is the only one attached", step(["n", "p", "a", "b", "c", "q"]) === 1);
check("a child taken out moves no other", step(["n", "p", "b", "c", "q"]) === 0);
check("children swapped about the rest move one", step(["n", "p", "c", "b", "q"]) === 1);
check("the order reversed moves all but one", step(["q", "b", "c", "p", "n"]) === 4);
check("children come and go at both ends", step(["c", "p", "n", "x", "y"]) === 2);
check("the longest run in order stays", JSON.stringify(increasingRun([3, -1, 0, 1, 4, 2])) === "[false,false,true,true,false,true]");

// Siblings with the same key are kept apart by their occurrence: no element is
// lost to the reconciler (and left in the backend's tree, one more each flush).
const twins = { second: "second", clicked: "" };
const mem3 = memoryBackend();
const ui3 = mount(mem3.backend, (b) => {
  b.open("div", "app", "");
  b.leaf("span", "x", "one", "first");
  b.on("click", (e) => {
    twins.clicked = "first";
  });
  b.leaf("span", "x", "two", twins.second);
  b.on("click", (e) => {
    twins.clicked = "second";
  });
  b.close();
});
/** function liveSpans() => Int */
function liveSpans() {
  return mem3.nodes().filter((n) => n.alive && n.type === "span").length;
}
ui3.flush();
const twinsCreated = ui3.stats().created;
for (let i = 0; i < 5; i++) ui3.flush();
check("repeated keys show every child, in order", mem3.dump() === 'div\n  span.one "first"\n  span.two "second"\n');
check("repeated keys leave no orphans behind", liveSpans() === 2 && ui3.stats().elements === 3);
check("repeated keys are not recreated on each flush", ui3.stats().created === twinsCreated);
twins.second = "changed";
ui3.flush();
check("each repeated key keeps its own element", mem3.dump().includes('span.one "first"\n  span.two "changed"') && ui3.stats().created === twinsCreated);
check("each repeated key keeps its own handler", mem3.fire("two", "click", blankEvent()) && twins.clicked === "second");

if (failures > 0) throw new Error(`${failures} test(s) failed`);
console.log("all UI tree tests passed");
