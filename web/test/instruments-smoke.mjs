// End-to-end smoke test of picking instruments, in a real browser, against the
// browser-only studio (docs/static.md): the browser's searchable instrument
// tree, trying an instrument on the keys, adding it, swapping a channel's
// instrument in place (⇄, the inspector, dragging onto the rack), and the
// piano naming what it plays. Not part of `npm test`: it needs Playwright and
// a served build.
//
//   tools/build-static.sh && python3 -m http.server -d dist 8765 &
//   node web/test/instruments-smoke.mjs http://localhost:8765/
//
// Set CHROMIUM to use a specific browser binary.

import { chromium } from "playwright";

const base = process.argv[2] || "http://localhost:8765/";
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined, args: ["--autoplay-policy=no-user-gesture-required"] });
const page = await (await browser.newContext({ viewport: { width: 1500, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
const ok = (s) => console.log("ok  ", s);
/** Notes sent to the audio engine, by channel. */
await page.addInitScript(() => {
  window.__notes = [];
  const post = MessagePort.prototype.postMessage;
  MessagePort.prototype.postMessage = function (m, ...rest) {
    if (m && m.t === "note" && m.on) window.__notes.push(m.channel);
    return post.call(this, m, ...rest);
  };
});
const until = (fn, arg, what) =>
  page.waitForFunction(fn, arg, { timeout: 15000 }).catch(() => {
    throw new Error(`timed out: ${what}`);
  });
const channels = () =>
  page.evaluate(async () => (await import("./src/store.js")).state.project.channels.map((c) => ({ id: c.id, name: c.name, instrument: c.instrument })));
const keysSay = () => page.textContent(".kb-head .kb-ch");

await page.goto(base);
await page.waitForFunction(() => document.querySelector(".song-title")?.textContent === "Arietta in J", null, { timeout: 30000 });
await page.waitForSelector(".rack-row");
// Notices (the browser studio's welcome) pass in a few seconds; they would cover the rack.
await page.waitForTimeout(500);

// The rack says what it is, and counts bars and beats over the steps.
await page.waitForSelector(".rack-guide");
await page.waitForSelector(".rack-ruler .rr-beat.bar");
ok("the channel rack explains its rows and steps");

// The song's channels, then every instrument's sounds, in one tree.
await page.waitForSelector(".b-item.folder:has-text('In this song')");
const n0 = (await channels()).length;
if ((await page.$$(".b-item.pick")).length < n0) throw new Error("the song's channels are not listed");
await page.click(".b-item.folder:has-text('Grand Orchestra')");
await until(() => document.querySelectorAll(".b-item.folder").length > 20, null, "the soundfont opens into General MIDI families");
ok("Grand Orchestra opens into its General MIDI families");

// The search filters the tree; a click tries the instrument on the keys.
await page.fill(".b-search input", "cello");
await until(() => [...document.querySelectorAll(".b-item.pick .b-name")].some((e) => e.textContent === "Cello"), null, "the search finds the cello");
await page.click(".b-item.pick:has(.b-name:text-is('Cello'))");
await until(() => document.querySelector(".kb-target.trying")?.textContent.includes("Cello"), null, "the keys say they play the cello");
await page.keyboard.down("KeyZ");
await page.keyboard.up("KeyZ");
await until(() => window.__notes.includes("__audition__"), null, "a typed key plays the tried instrument");
if ((await channels()).length !== n0) throw new Error("trying an instrument changed the song");
ok(`the keys play a tried instrument (${(await keysSay()).trim()}) without touching the song`);

// Selecting a channel gives the keys back to it.
await page.click(".rack-row:not(.rack-ruler) .ch-name:has-text('Piano')");
await until(
  () => !document.querySelector(".kb-target.trying") && document.querySelector(".kb-target")?.textContent.includes("Piano"),
  null,
  "the keys play Piano"
);
ok("selecting a channel names it on the keys");

// Swap the piano's instrument with ⇄; undo brings it back.
await page.fill(".b-search input", "electric piano 1");
await page.hover(".b-item.pick:has(.b-name:text-is('Electric Piano 1'))");
await page.click(".b-item.pick:has(.b-name:text-is('Electric Piano 1')) button[title^='Replace']");
const program = async (id) => (await channels()).find((c) => c.id === id)?.instrument.options.find((o) => o.key === "program")?.value;
if ((await program("piano")) !== "Electric Piano 1") throw new Error("⇄ did not replace the instrument");
if ((await channels()).length !== n0) throw new Error("⇄ added a channel");
await page.keyboard.press("Control+z");
await page.waitForTimeout(100);
if ((await program("piano")) !== "Acoustic Grand Piano") throw new Error("undo did not bring the piano back");
ok("⇄ replaces a channel's instrument in place, and Ctrl+Z undoes it");

// The inspector's Instrument choice swaps it too.
await page.selectOption(".insp-swap select", "fm");
await page.waitForTimeout(100);
if ((await channels()).find((c) => c.id === "piano")?.instrument.type !== "fm") throw new Error("the inspector did not swap the instrument");
await page.keyboard.press("Control+z");
ok("the inspector swaps the instrument");

// Dragging onto a row replaces; onto the list adds a channel.
await page.fill(".b-search input", "clap");
await page.dragAndDrop(".b-item.pick:has(.b-name:text-is('Clap'))", ".rack-row:not(.rack-ruler):has(.ch-name:has-text('Drums'))");
const drums = (await channels()).find((c) => c.id === "drums");
if (drums?.instrument.type !== "drum") throw new Error("dropping on a row did not replace its instrument");
// Just under the last channel: the rack itself, not a row.
const below = await page.evaluate(() => {
  const list = document.querySelector(".rack-list").getBoundingClientRect();
  const rows = document.querySelectorAll(".rack-row:not(.rack-ruler)");
  return { x: 300, y: rows[rows.length - 1].getBoundingClientRect().bottom - list.top + 12 };
});
await page.dragAndDrop(".b-item.pick:has(.b-name:text-is('Clap'))", ".rack-list", { targetPosition: below });
if ((await channels()).length !== n0 + 1) throw new Error("dropping on the rack did not add a channel");
ok("dragging an instrument onto the rack replaces or adds");

// Double-click adds the instrument as a channel; the keys then play it.
await page.fill(".b-search input", "glass");
await page.dblclick(".b-item.pick >> nth=0");
await until((n) => document.querySelectorAll(".rack-row:not(.rack-ruler)").length === n, n0 + 2, "double-click adds a channel");
await until(() => !document.querySelector(".kb-target.trying"), null, "the keys play the new channel");
ok("double-click adds a channel and the keys play it");

await page.fill(".b-search input", "zzzz-nothing");
await page.waitForSelector(".browser-body .b-empty:has-text('Nothing matches')");
ok("a search with no match says so");

// Drums: with a 4/4 intro before the 3/4 waltz, the groove picker follows
// the time signature where the song cursor is.
await page.fill(".b-search input", "");
await page.evaluate(async () => {
  const store = await import("./src/store.js");
  store.commit(() => {
    const t = store.state.project.transport;
    t.meters = [
      { bar: 1, numerator: 4, denominator: 4 },
      { bar: 9, numerator: 3, denominator: 4 },
    ];
  });
});
await page.keyboard.press("F4");
await page.waitForSelector(".drums-groove .drums-label");
const label = () => page.textContent(".drums-groove .drums-label");
const seek = (beat) => page.evaluate(async (b) => (await import("./src/audio.js")).seek(b), beat);
await seek(8 * 4 + 12 * 3);
await until(() => document.querySelector(".drums-groove .drums-label")?.textContent.includes("3/4"), null, "the groove picker reads 3/4 in the waltz");
await seek(2 * 4);
await until(() => document.querySelector(".drums-groove .drums-label")?.textContent.includes("4/4"), null, "the groove picker reads 4/4 in the intro");
ok(`the drummer follows the time signature at the song cursor (${(await label()).trim()})`);

if (errors.length > 0) throw new Error(`page errors: ${errors.join("\n")}`);
console.log("all instrument checks passed");
await browser.close();
