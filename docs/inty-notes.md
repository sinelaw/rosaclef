# inty quirks found while building the Rosaclef frontend

A running log of the issues found while type-checking `web/` with
[inty](https://github.com/sinelaw/inty), for fixing later in inty. inty
version: `1713277` (main, 2026-09). Each entry has a minimal repro, the
symptom, and what Rosaclef does in the meantime.

Legend: 🐞 bug · ⚠️ confusing diagnostic · 🐢 performance · 📚 stdlib gap · 💡 feature request

Items 1–4 and 9 are being fixed on a local `rosaclef-fixes` branch of inty (not yet
upstreamed); the others are open.

## 1. 🐞 Comparing deferred operands: `Number` vs `Int` inside functions

```js
const state = { insert /*: Number */: 1, xs /*: Number[] */: [] };
function fix() { if (state.insert >= state.xs.length) state.insert = 0; }
// Error: Type mismatch: expected 'Number', found 'Int'
```
The same comparison passes at top level or when the operands are parameters.
Cause: property reads on outer bindings are deferred `HasProp` constraints,
so both sides of `>=` are still type variables; the comparison arm then unifies
them (`subsume_either`) instead of applying "numbers compare whatever their
kind". Workaround: type index-like values as `Int` (or a newtype, see 11).

## 2. 🐞 Narrowing does not apply to reads of outer bindings

```js
/** const XS: { name: String, v: String }[] */
const XS = [{ name: "a", v: "x" }];
/** function g(n: String) => String */
function g(n) { const found = XS.find((i) => i.name === n); return found ? found.v : ""; }
// Error: Property 'v' not found in type Undefined
```
Also `found !== undefined ? found[1] : ""` → "Type 'Undefined | String[]' is not
an instance of Indexable". Works when the array is a parameter. Same root cause
as 1. Workaround: a `for … of` loop.

## 3. 🐞 Forward references between type aliases

```js
/** type R = { a: Number, xs: R2[] } */
/** type R2 = { b: Number } */
/** function f() => R */
function f() { return { a: 1, xs: [{ b: 0 }] }; }
// Error: Type mismatch: expected 'Undefined', found '{b: p11}'
```
Declaring `R2` first works. Workaround: order aliases bottom-up.

## 4. ⚠️ Errors inside an imported module are reported at the importer

When `main.js` imports `store.js` and the error is in `store.js`, the
diagnostic points at a span in `main.js` (the last line, or the import). The
message sometimes names the wrong thing too ("from parameter 'pan'" for a
comparison in another module). Workaround: run `inty` on each module directly
(`web/check.sh src/store.js`) to find the real location.

## 5. ⚠️ Unbound type variable in a function annotation names the parameter

`/** function f(o: T) => Number */` reports `unknown type 'o'`; it should name
`T` and suggest `function f<T>(o: T) => …`.

## 6. 🐞 Nominal classes in a `--lib` file are invisible to aliases in the same file

```js
// lib.d.js
class InsertIx {}
/** type Channel = { mixer: InsertIx } */
// Error: unknown type 'InsertIx'
```
Works when the classes live in a separate `--lib` file loaded first
(Rosaclef: `web/types/newtypes.d.js`).

## 7. 🐢 Checking time grows steeply with the module graph

`web/check.sh` (entry `src/main.js`, ~2k lines across 9 modules) takes
**~90 s**; single modules take 1–30 s (`src/audio.js`: 29 s). Each import
seems to re-check its dependencies, or inference over the big DOM rows is slow.

## 8. ⚠️ "Presence mismatch: expected present, found absent"

Reported at `console.log(mount, play)` in `main.js` although the cause is
elsewhere; the message does not say which field is present/absent or in
which types. Checking `src/ui/mixer.js` on its own located it at a
`window.prompt(...)` call inside an event handler; the same code checks fine
in isolation, so it depends on how `window` is used across the module graph.
Workaround: go through the typed platform layer (`promptBox`).

## 9. 📚 DOM events are typed as elements

`stdlib/dom.d.js` types `addEventListener`'s callback argument as the element
type `T`, so `el.addEventListener("pointerdown", (e) => e.clientX)` fails with
"Property 'clientX' not found in type {…element…}". There are also no
declarations for canvas 2D, `WebSocket`, Web Audio or `ResizeObserver`.
Workaround: a small typed FFI layer (`web/lib/platform.js` +
`web/types/platform.d.js`).

## 10. 💡 No dictionary / index-signature type for plain objects

JSON objects used as maps (`{"cutoff": 1200, "resonance": 0.3}`) cannot be
annotated; `Map<K, V>` is only the JS `Map` class and cannot be named in an
annotation either (`unknown type 'Map'`). Workaround: decode into `{key,
value}[]` entry lists (`web/src/model.js`).

## 11. 💡 Zero-cost newtypes

Empty classes declared in a `.d.js` work as nominal types (`TrackIx` is
rejected where `InsertIx` is expected, and so is a bare `3`), with identity
casts at runtime (`web/lib/brands.js`). A first-class `/** newtype X = Int */`
with built-in casts (and optionally arithmetic lifting) would remove the
boilerplate.

## 12. 🐞 `Int` does not flow into `Number` in some positions

```js
/** type S = { elements: Number } */
/** const f: () => S */
const f = () => ({ elements: [1, 2].length });
// Error: expected '() => {elements: Number}', found '() => {elements: Int}'
```
Literals widen, but a non-literal `Int` (e.g. `.length`, `Math.round(x)`) is
not accepted where a `Number` field is declared. Array indexing requires
`Int`, so index data has to be typed `Int` end to end.

## 13. ⚠️ Other small things

- `arr.map(() => [])` (callback ignoring its argument) does not type-check;
  `arr.map((_) => [])` does.
- `a && b` requires both operands to have the same type, so
  `cond && maybeObj ? x : y` must be rewritten with `if`.
- `arr.filter((x, i) => …)` / `arr.map((x, i) => …)`: callbacks taking the
  index are rejected (stdlib declares one-argument callbacks); use a loop.
- Heterogeneous tuples (`[["l", 0.5]]`) are rejected; use records.
- `promise.catch((e) => false)` must return a `Promise` (`Promise.resolve(false)`).
- An AudioWorklet processor must `extend AudioWorkletProcessor`; with no
  class inheritance, `web/engine/worklet.js` stays unchecked.

## 14. 🐞 `new Date(...)` fails with "Presence mismatch: expected present, found absent"

```js
function g() { return new Date().toISOString(); }
// Error: Presence mismatch: expected present, found absent   (at `new Date()`)
```
Any `new Date(...)` (with or without arguments, whatever method follows)
fails; checked from an importer, the error surfaces at an unrelated line
(`ui.flush()` at the end of `main.js`, see 4 and 8). `Date.now()` is fine.
Workaround: construct dates in the platform layer (`nowIso`, `fmtDate` in
`web/lib/platform.js`).

## 15. ⚠️ An unknown type in a function annotation is reported as the parameter name

`/** function errText(e: Error) => String */` reports `unknown type 'e'`
(the real problem: `Error` is not a nameable type). Like 5, the message should
name the type. Workaround: a type parameter, `function errText<E>(e: E) => String`.
