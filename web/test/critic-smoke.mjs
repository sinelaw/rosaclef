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
const until = (n) => page.waitForFunction((x) => document.querySelectorAll(".crit-title").length === x, n, { timeout: 10000 });

await page.goto(base);
await page.waitForFunction(() => document.querySelector(".song-title")?.textContent === "Demo", null, { timeout: 30000 });
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

await page.click(".crit-item.fixable .btn.gold >> nth=0");
await until(before - 1);
ok("a suggestion applies");
await page.click("button[title^='Undo']");
await until(before);
ok("Ctrl+Z (undo) brings it back");

await page.click(".crit-item .crit-where >> nth=0");
await page.waitForFunction(() => document.querySelector(".hintbar")?.textContent.includes("Show Bar"), null, { timeout: 5000 });
ok("a finding shows where it is");

await page.click(".crit-item .btn.icon[title^='Ignore'] >> nth=0");
await until(before - 1);
await page.waitForSelector(".crit-seg:has-text('Ignored 1')");
ok("a finding can be ignored");

await page.click(".crit-summary .btn");
await page.waitForFunction(() => document.querySelector(".crit-bar")?.textContent.includes("Suggestions 0"), null, { timeout: 15000 });
ok("every suggestion applies at once");

await page.click(".crit-bar .btn.icon");
await page.waitForSelector(".crit-rule");
await page.click(".crit-rule:has-text('Sustained semitone clashes') input");
await page.click(".crit-bar .btn.icon");
if (await page.isVisible(".crit-rulehead:has-text('Sustained semitone clashes')")) throw new Error("a check turned off still reports");
ok("checks turn off");

await page.click(".maestro-tab:has-text('Terminal')");
await page.waitForSelector(".agent-choice");
ok("the terminal is still there");

if (errors.length > 0) throw new Error(`page errors: ${errors.join("; ")}`);
await browser.close();
