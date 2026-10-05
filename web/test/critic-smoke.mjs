// End-to-end smoke test of the Critic (the Maestro panel's lint plugin), in a
// real browser. Not part of `npm test`: it needs Playwright and a served build.
//
//   tools/build-static.sh && python3 -m http.server -d dist 8765 &
//   node web/test/critic-smoke.mjs http://localhost:8765/
//
// Set CHROMIUM to use a specific browser binary.

import { chromium } from "playwright";

const base = process.argv[2] || "http://localhost:8765/";
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined });
const page = await (await browser.newContext({ viewport: { width: 1500, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
const ok = (s) => console.log("ok  ", s);
const count = () => page.$$eval(".crit-title", (xs) => xs.length);

await page.goto(base);
await page.waitForFunction(() => document.querySelector(".song-title")?.textContent === "Arietta in J", null, { timeout: 30000 });
await page.click(".maestro-tab:has-text('Critic')");
// The findings follow the project a moment after it changes (here: after the demo loads).
await page.waitForFunction(
  () =>
    document.querySelectorAll(".crit-title").length > 5 &&
    Number(document.querySelector(".maestro-count")?.textContent) === document.querySelectorAll(".crit-title").length,
  null,
  { timeout: 15000 }
);
const before = await count();
const badge = Number(await page.textContent(".maestro-count"));
if (badge !== before) throw new Error(`the tab says ${badge}, the list has ${before}`);
ok(`the Critic lists ${before} findings on the demo`);

const keyOf = (sel) => page.getAttribute(sel, "data-key");
const shows = (key, yes) =>
  page.waitForFunction(([k, y]) => [...document.querySelectorAll(".crit-item:not(.suppressed)")].some((e) => e.dataset.key === k) === y, [key, yes], {
    timeout: 15000,
  });
const fixed = await keyOf(".crit-item.fixable >> nth=0");
await page.click(".crit-item.fixable .btn.gold >> nth=0");
await shows(fixed, false);
ok(`a suggestion applies (${fixed})`);
await page.click("button[title^='Undo']");
await shows(fixed, true);
ok("Ctrl+Z (undo) brings it back");

await page.click(".crit-item .crit-where >> nth=0");
await page.waitForFunction(() => document.querySelector(".hintbar")?.textContent.includes("Show "), null, { timeout: 5000 });
ok("a finding shows where it is");

const hushed = await keyOf(".crit-item >> nth=0");
await page.click(".crit-item .btn.icon[title^='Suppress'] >> nth=0");
await shows(hushed, false);
await page.waitForSelector(".crit-seg:has-text('Suppressed 1')");
ok("a finding can be suppressed");
await page.click(".crit-seg:has-text('Suppressed 1')");
await page.click(`.crit-item.suppressed[data-key="${hushed}"] .btn:has-text('Unsuppress')`);
await shows(hushed, true);
await page.click(".crit-seg:has-text('Suppressed')").catch(() => {});
ok("and brought back");

// A whole check, from its heading: saved in the project, so undo brings it back.
const heading = await page.textContent(".crit-rulehead b >> nth=0");
await page.click(".crit-rulehead .crit-link.quiet >> nth=0");
await page.waitForFunction((h) => ![...document.querySelectorAll(".crit-rulehead b")].some((b) => b.textContent === h), heading, { timeout: 10000 });
await page.click("button[title^='Undo']");
await page.waitForFunction((h) => [...document.querySelectorAll(".crit-rulehead b")].some((b) => b.textContent === h), heading, { timeout: 10000 });
ok(`a check ("${heading}") turns off, and undo turns it back on`);

const suggestions = await page.$$eval(".crit-item.fixable:not(.suppressed)", (xs) => xs.map((x) => x.dataset.key));
await page.click(".crit-summary .btn");
await page.waitForFunction((keys) => !keys.some((k) => [...document.querySelectorAll(".crit-item")].some((e) => e.dataset.key === k)), suggestions, {
  timeout: 15000,
});
ok(`all ${suggestions.length} suggestions apply at once`);

// The classical theory checks start off; the demo's piano has parallel fifths.
const heads = (name) =>
  page.waitForFunction((n) => [...document.querySelectorAll(".crit-rulehead b")].some((b) => b.textContent === n), name, { timeout: 15000 });
if (await page.isVisible(".crit-rulehead:has-text('Parallel fifths')")) throw new Error("a theory check reports by default");
await page.click(".crit-bar .btn.icon");
await page.waitForSelector(".crit-rule:has-text('Parallel fifths') .crit-default");
await page.click(".crit-rule:has-text('Parallel fifths and octaves') input");
await page.click(".crit-rule:has-text('Beyond the instrument') input");
await page.click(".crit-bar .btn.icon");
await heads("Parallel fifths and octaves");
ok("a theory check, off by default, turns on");
if (await page.isVisible(".crit-rulehead:has-text('Beyond the instrument')")) throw new Error("a check turned off still reports");
ok("checks turn off");

await page.click(".maestro-tab:has-text('Terminal')");
await page.waitForSelector(".agent-choice");
ok("the terminal is still there");

if (errors.length > 0) throw new Error(`page errors: ${errors.join("; ")}`);
await browser.close();
