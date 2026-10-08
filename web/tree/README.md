# tree — the studio's UI library

A small retained, reconciling UI tree in the spirit of
[fresh-ui](https://github.com/sinelaw/fresh/tree/master/crates/fresh-ui).
Plain ES modules, no build step, type-checked with
[inty](https://sinelaw.github.io/inty/). The studio (`web/src`) is one
program written on it. Only `dom.js` knows a little of the studio: which
elements count as controls (`CONTROLS`: a press there does not take the
keyboard back) and as its terminal (`TERMINAL`: its keys are typed).

| file                 | what                                                                                  | checked                    |
| -------------------- | ------------------------------------------------------------------------------------- | -------------------------- |
| `tree.js`            | the description builder and `mount` (reconciliation)                                  | yes                        |
| `memory.js`          | an in-memory backend: the "test renderer"                                             | yes                        |
| `dom.js`             | the browser backend, the event layer, pointer gestures, canvas sizing                 | no (the platform boundary) |
| `types.d.js`         | the library's types: `Builder`, `Desc`, `Backend`, `Ev`, `Ctx`, `Handle`, `NodeIx`, … | —                          |
| `ids.js`, `ids.d.js` | the casts of the nominal types (`handle`, `nodeIx`, …; identity at runtime)           | —                          |

## The model

1. **Descriptions** — plain values rebuilt from state on every flush. A view
   is a function `(b: Builder, ...args) => Undefined` that writes them:

   ```js
   b.open("div", "row", "row sel"); // type, key, class
   b.on("click", (e) => select(i)); // modifiers apply to the node just opened…
   b.leaf("span", "name", "", name); // type, key, class, text
   b.close();
   b.style("top", "12px"); // …or, after close(), to the node just closed
   ```

   Descriptions are flat (each node names its parent's index), so building
   one costs one allocation.

2. **Elements** — persistent records matched to descriptions by their path of
   `type:key` pairs. They own the backend handle, the last applied properties
   and the current handlers, and survive rebuilds. A node without a key is
   keyed by its position among its siblings of the same type. Keys should be
   unique among siblings; a repeated one is told apart by its occurrence
   (`x`, `x#1`, …), as positions tell keyless ones apart.
3. **Backends** — primitive operations on numeric handles (`Backend` in
   `types.d.js`): the DOM (`dom.js`) and an in-memory tree for tests
   (`memory.js`).

State flows down (views are functions of state), events flow up (handlers are
callbacks in the description). There are no signals and no observers:
anything that changes calls the `mark()` that `mount` returns, and the next
frame rebuilds and reconciles everything.

Reconciling updates only what changed: class, text, attributes and styles,
then DOM properties (after the children, so a `<select>`'s `value` finds its
options). Children are moved as little as possible (the longest run already in
order stays). One backend listener is bound per element and event type; it
calls whatever handler the latest description holds, so handlers never go
stale and never need unbinding.

## Escape hatches

- Pseudo-events: `"mount"` (once the node exists) and `"resize"` (a
  `ResizeObserver`).
- Pseudo-properties: `focus`, `select` (focus a field with its text
  selected), `reveal` (scroll into view, only as far as it takes),
  `scrollLeft` / `scrollTop`.
- `b.canvas(key, cls, paint)` for per-pixel content: `paint(ctx, w, h)` runs
  after the flush, at the device's pixel ratio. CSS or the view's styles size
  the canvas (both width and height); `w` and `h` are its box.

## Events

Every listener gets an `Ev`: the browser's event, enriched by `dom.js` with the
target's box (`targetLeft`, …) and scroll, a field's `value` / `checked`, and
whether the user is `typing` or pressed a control (`onControl`). View code
never touches DOM nodes. `drag(e, onMove, onUp)` follows one pointer on the
window until it is released; `pressOrTap(e, act)` lets a finger scroll and
only a tap act.

## Using it

```js
import { mount } from "../tree/tree.js";
import { domBackend } from "../tree/dom.js";

const ui = mount(domBackend("app"), view);
ui.flush(); // first frame
// later, on any change:
ui.mark(); // rebuild on the next animation frame
```

The host maps `#tree-ids` to `ids.js` (an import map in the page, `imports`
in `package.json` for Node) and, for inty, to `ids.d.js` (`inty.json`), and
loads `types.d.js` with `--lib` before its own declarations.

Tests: `web/test/tree.test.js` (`node web/test/tree.test.js`).
