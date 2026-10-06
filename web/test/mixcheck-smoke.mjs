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
// A mixer fault, as the agent's shell would make it: the master limiter
// driven 14 dB (the demo has 2).
const gain = () =>
  page.evaluate(async () => {
    const { state } = await import("./src/store.js");
    return state.project.mixer.inserts[0].effects[1].params.find((k) => k.key === "gain")?.value;
  });
await page.click(".agent-choice");
await page.click("#agent-term");
await page.keyboard.type("set /mixer/inserts/0/effects/1/params/gain 14\r");
await page.waitForFunction(
  () => import("./src/store.js").then((m) => m.state.project.mixer.inserts[0].effects[1].params.find((k) => k.key === "gain")?.value === 14),
  null,
  {
    timeout: 10000,
  }
);
ok("the limiter is driven 14 dB");

await page.click(".maestro-tab:has-text('Mix check')");
await page.waitForSelector(".mx-empty");
ok("the Mix check tab opens");

// Bars 77–79 of the demo (the sax solo).
await page.selectOption(".mx-bar .mx-select >> nth=0", "bars");
await page.fill(".mx-num >> nth=0", "77");
await page.press(".mx-num >> nth=0", "Tab");
await page.fill(".mx-num >> nth=1", "79");
await page.press(".mx-num >> nth=1", "Tab");
await page.click(".mixcheck .btn:has-text('Measure')");
await page.waitForSelector(".mx-progress", { timeout: 5000 });
// What the back end says as it renders: how far in, then the measuring.
await page.waitForFunction(() => /Rendering \d+%|Measuring/.test(document.querySelector(".mx-progress-label")?.textContent ?? ""), null, { timeout: 60000 });
ok(`measuring shows how far the render has come (${await page.textContent(".mx-progress-label")})`);
await page.waitForSelector(".mx-master", { timeout: 120000 });
const lufs = Number((await page.textContent(".mx-big-value")).replace("−", "-"));
if (!(lufs < -3 && lufs > -40)) throw new Error(`integrated loudness reads ${lufs}`);
ok(`bars 77–79 measure ${lufs} LUFS integrated`);
for (const label of ["True peak", "Pre-limiter", "PLR", "LRA", "Mono"]) {
  if ((await page.locator(".mx-readout", { hasText: label }).count()) !== 1) throw new Error(`no ${label} readout`);
}
if ((await page.locator(".mx-el").count()) < 3) throw new Error("the parts are missing");
ok("loudness, peaks, dynamics, phase and the parts show");

// The overload names the drive; its fix is the drive, on the mixer.
const overload = page.locator(".mx-find", { hasText: "master overload" });
await overload.waitFor({ timeout: 5000 });
const detail = await overload.textContent();
if (!detail.includes("input gain")) throw new Error(`the overload does not name the drive: ${detail}`);
await overload.locator(".btn:has-text('Try')").click();
await page.waitForSelector(".mx-tried", { timeout: 120000 });
const tried = await page.textContent(".mx-tried");
if (!tried.includes("resolved")) throw new Error(`the what-if says: ${tried}`);
ok(`a fix can be tried without making it (${tried.trim()})`);

// Quick fix: every fix, each with a box to leave it out.
const quick = page.locator(".mx-quick");
await quick.waitFor({ timeout: 5000 });
const ticked = async () => Number(((await quick.locator(".mx-quick-sub").textContent()).match(/\d+/) ?? ["0"])[0]);
const all = await ticked();
await page.locator(".mx-find .mx-include").first().uncheck();
await page.waitForFunction((n) => (document.querySelector(".mx-quick-sub")?.textContent ?? "").startsWith(`${n} `), all - 1, { timeout: 5000 });
await page.locator(".mx-find .mx-include").first().check();
await page.waitForFunction((n) => (document.querySelector(".mx-quick-sub")?.textContent ?? "").startsWith(`${n} `), all, { timeout: 5000 });
ok(`Quick fix gathers the ${all} fixes; a box leaves one out`);

// Before / after: from the playhead, the bars as they are, then with the fix.
await page.evaluate(() => import("./src/audio.js").then((m) => m.seek(4 * 76)));
await quick.locator(".btn:has-text('Before / after')").click();
await quick.locator(".btn:has-text('Before…')").waitFor({ timeout: 10000 });
await quick.locator(".btn:has-text('After…')").waitFor({ timeout: 30000 });
await quick.locator(".btn:has-text('After…')").click();
await quick.locator(".btn:has-text('Before / after')").waitFor({ timeout: 5000 });
await page.waitForTimeout(400);
const back = await page.evaluate(() => import("./src/store.js").then((m) => m.state.position));
if (Math.abs(back - 4 * 76) > 0.5) throw new Error(`the playhead did not come back (${back})`);
ok("Before / after plays from the playhead, then the fixed version; the playhead comes back");

const before = await gain();
await overload.locator(".btn:has-text('Apply fix')").click();
await page.waitForSelector(".mx-status.stale", { timeout: 15000 });
const after = await gain();
if (!(after < before)) throw new Error(`the fix did not lower the drive (${before} → ${after})`);
await overload.locator(".mx-applied").waitFor({ timeout: 5000 });
ok(`applying the fix lowers the limiter's drive (${before} → ${after}); it reads Applied`);
// Another fix of the same report applies without measuring again — unless
// it sets what the applied one set.
const pumping = page.locator(".mx-find", { hasText: "limiter pumping" });
if ((await pumping.count()) > 0) {
  await pumping.locator(".btn:has-text('Apply fix')").click();
  await page.waitForSelector(".toast:has-text('A fix you applied changed that already')", { timeout: 5000 });
  if ((await gain()) !== after) throw new Error("a fix setting the same drive was applied");
  ok("a fix setting what an applied one set is refused");
}
const ceiling = () =>
  page.evaluate(async () => {
    const { state } = await import("./src/store.js");
    return Number(state.project.mixer.inserts[0].effects[1].params.find((k) => k.key === "ceiling")?.value);
  });
const peak = page.locator(".mx-find", { hasText: "true peak" });
let applied = 1;
if ((await peak.count()) > 0) {
  const c0 = await ceiling();
  await peak.locator(".btn:has-text('Apply fix')").click();
  await peak.locator(".mx-applied").waitFor({ timeout: 10000 });
  if ((await ceiling()) === c0) throw new Error("the second fix did not apply");
  applied = 2;
  ok(`another fix applies without measuring again (ceiling ${c0} → ${await ceiling()})`);
}
// Any other edit: the report is stale; its fixes wait for a new measure.
await page.click(".maestro-tab:has-text('Terminal')");
await page.click("#agent-term");
await page.keyboard.type("set /mixer/inserts/0/volume 0.9\r");
await page.waitForFunction(() => import("./src/store.js").then((m) => m.state.project.mixer.inserts[0].volume === 0.9), null, { timeout: 10000 });
await page.click(".maestro-tab:has-text('Mix check')");
const g2 = await gain();
await page.locator(".mx-find .btn:has-text('Apply fix')").first().click();
await page.waitForSelector(".toast:has-text('The song changed since this report')", { timeout: 5000 });
if ((await gain()) !== g2) throw new Error("a stale report's fix was applied");
ok("after another edit, the report's fixes wait for a new measure");
await page.click("button[title^='Undo']");
for (let i = 0; i < applied; i++) await page.click("button[title^='Undo']");
await page.waitForFunction(
  (g) => import("./src/store.js").then((m) => m.state.project.mixer.inserts[0].effects[1].params.find((k) => k.key === "gain")?.value === g),
  before,
  {
    timeout: 5000,
  }
);
ok("Ctrl+Z (undo) puts the drive back");

// Export says how far into the song its render has come.
const [wav] = await Promise.all([
  page.waitForEvent("download", { timeout: 120000 }),
  page.click(".btn.export"),
  page.waitForFunction(() => /Exporting [1-9]\d*%/.test(document.querySelector(".btn.export")?.textContent ?? ""), null, { timeout: 60000 }),
]);
ok(`Export shows its progress, then downloads ${wav.suggestedFilename()}`);

if (errors.length > 0) throw new Error(`page errors:\n${errors.join("\n")}`);
await browser.close();
console.log("mixcheck smoke: all ok");
