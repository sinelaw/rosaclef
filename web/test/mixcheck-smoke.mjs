// End-to-end smoke test of the Mix check (the Maestro panel's metering
// plugin), in a real browser. Not part of `npm test`: it needs Playwright and a
// served build. In the browser-only build the measuring runs in the worker
// (WebAssembly), the same code as `rosaclef mixcheck`.
//
//   tools/build-static.sh && python3 -m http.server -d dist 8765 &
//   node web/test/mixcheck-smoke.mjs http://localhost:8765/
//
// Set CHROMIUM to use a specific browser binary.

import { chromium } from "playwright";

const base = process.argv[2] || "http://localhost:8765/";
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined });
const page = await (await browser.newContext({ viewport: { width: 1500, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
const ok = (s) => console.log("ok  ", s);

await page.goto(base);
await page.waitForFunction(() => document.querySelector(".song-title")?.textContent === "Arietta in J", null, { timeout: 30000 });
await page.click(".maestro-tab:has-text('Mix check')");
await page.waitForSelector(".mx-empty");
ok("the Mix check tab opens");

// Bars 77–79 of the demo: in the sax solo, the sax's C4 rubs against the
// bass's B2 in bar 78 (a minor ninth: a finding at the strict threshold).
await page.selectOption(".mx-bar .mx-select >> nth=0", "bars");
await page.selectOption(".mx-bar2 .mx-select >> nth=0", "strict");
await page.fill(".mx-num >> nth=0", "77");
await page.press(".mx-num >> nth=0", "Tab");
await page.fill(".mx-num >> nth=1", "79");
await page.press(".mx-num >> nth=1", "Tab");
await page.click(".mixcheck .btn:has-text('Measure')");
await page.waitForSelector(".mx-master", { timeout: 120000 });
const lufs = Number((await page.textContent(".mx-big-value")).replace("−", "-"));
if (!(lufs < -5 && lufs > -40)) throw new Error(`integrated loudness reads ${lufs}`);
ok(`bars 77–79 measure ${lufs} LUFS integrated`);
for (const label of ["True peak", "Pre-limiter", "PLR", "LRA", "Mono"]) {
  if ((await page.locator(".mx-readout", { hasText: label }).count()) !== 1) throw new Error(`no ${label} readout`);
}
if ((await page.locator(".mx-el").count()) < 3) throw new Error("the parts are missing");
ok("loudness, peaks, dynamics, phase and the parts show");

const clash = page.locator(".mx-find", { hasText: "harmonic clash" });
await clash.waitFor({ timeout: 5000 });
await clash.locator(".btn:has-text('Try')").click();
await page.waitForSelector(".mx-tried", { timeout: 120000 });
const tried = await page.textContent(".mx-tried");
if (!tried.includes("resolved")) throw new Error(`the what-if says: ${tried}`);
ok(`a fix can be tried without making it (${tried.trim()})`);

// The note the fix moves: the sax's C4 in its "Solo" pattern.
const pitch = () =>
  page.evaluate(async () => {
    const { state } = await import("./src/store.js");
    const p = state.project.patterns.find((x) => x.id === "sax-solo");
    return p ? p.notes[121].pitch : -1;
  });
const before = await pitch();
await clash.locator(".btn:has-text('Apply fix')").click();
await page.waitForSelector(".mx-status.stale", { timeout: 15000 });
const after = await pitch();
if (after === before) throw new Error(`the fix did not move the note (${before})`);
ok(`applying the fix moves the note (${before} → ${after}); the report is marked stale`);
await clash.locator(".btn:has-text('Apply fix')").click();
await page.waitForSelector(".toast:has-text('The song changed since this report')", { timeout: 5000 });
if ((await pitch()) !== after) throw new Error("a stale report's fix was applied");
ok("a stale report's fixes are refused until it is measured again");
await page.click("button[title^='Undo']");
await page.waitForFunction(
  (p) => import("./src/store.js").then((m) => m.state.project.patterns.find((x) => x.id === "sax-solo").notes[121].pitch === p),
  before,
  {
    timeout: 5000,
  }
);
ok("Ctrl+Z (undo) puts the note back");

if (errors.length > 0) throw new Error(`page errors:\n${errors.join("\n")}`);
await browser.close();
console.log("mixcheck smoke: all ok");
