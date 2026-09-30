// Tests for the UI tree, run with: node web/test/tree.test.js
// (also type-checked by inty via web/check.sh).
import { mount } from "../src/ui/tree.js";
import { memoryBackend, blankEvent } from "../src/ui/memory.js";

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
  mem.dump().includes('li.item "a"\n    li.item "d"\n') && !mem.dump().includes('"b"') && ui.stats().created === created + 1,
);

check("handlers dispatch to the latest description", mem.fire("item", "click", blankEvent()) && state.clicks === 1);

const before = mem.ops();
ui.flush();
check("an unchanged rebuild performs no backend operations", mem.ops() === before);

if (failures > 0) throw new Error(`${failures} test(s) failed`);
console.log("all UI tree tests passed");
