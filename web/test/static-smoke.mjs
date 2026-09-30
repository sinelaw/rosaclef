// End-to-end smoke test of the browser-only studio (docs/static.md), in a
// real browser. Not part of `npm test`: it needs Playwright and a served build.
//
//   tools/build-static.sh && python3 -m http.server -d dist 8765 &
//   node web/test/static-smoke.mjs http://localhost:8765/
//
// Set CHROMIUM to use a specific browser binary.

import { chromium } from "playwright";

const base = process.argv[2] || "http://localhost:8765/";
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined, args: ["--autoplay-policy=no-user-gesture-required"] });
const ctx = await browser.newContext({ viewport: { width: 1500, height: 900 }, acceptDownloads: true });
const page = await ctx.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
const ok = (s) => console.log("ok  ", s);
const hasText = (p, t) => p.waitForFunction((x) => document.body.textContent.includes(x), t, { timeout: 30000 });

await page.goto(base);
await page.waitForFunction(() => document.querySelector(".song-title")?.textContent === "Demo", null, { timeout: 30000 });
ok("the first visit opens the demo song");
if (await page.isVisible('.seg button:has-text("Studio")')) throw new Error("the Studio output needs a server");

const before = await page.textContent(".lcd.static .lcd-value");
await page.mouse.click(700, 400);
await page.keyboard.press("Space");
await page.waitForTimeout(1500);
await page.keyboard.press("Space");
if ((await page.textContent(".lcd.static .lcd-value")) === before) throw new Error("the playhead did not move");
ok("it plays");

await page.click(".agent-choice");
await page.click("#agent-term");
await page.keyboard.type("set /transport/bpm 133\r");
await hasText(page, "133.00");
ok("the shell edits the song");

const other = await ctx.newPage();
await other.goto(base);
await hasText(other, "133.00");
await page.click("#agent-term");
await page.keyboard.type("set /transport/bpm 101\r");
await hasText(other, "101.00");
await other.close();
ok("another tab follows the edits");

await page.reload();
await hasText(page, "101.00");
ok("edits survive a reload");

await page.keyboard.press("Control+o");
await page.waitForSelector(".pm-card");
await page.click("text=New project");
await page.fill(".pm-name", "Second song");
await page.keyboard.press("Enter");
await page.waitForFunction(() => document.querySelector(".song-title")?.textContent === "Second song", null, { timeout: 20000 });
ok("a new project is created and opened");

await page.keyboard.press("Control+o");
await page.waitForSelector('.pm-card:has-text("Demo")');
const [zip] = await Promise.all([page.waitForEvent("download"), page.click('.pm-card:has-text("Demo") button[title*=".zip"]')]);
ok(`a project downloads as ${zip.suggestedFilename()}`);
await page.keyboard.press("Escape");
const [wav] = await Promise.all([page.waitForEvent("download", { timeout: 60000 }), page.click("button.btn.gold:has-text('Export')")]);
ok(`the song exports to ${wav.suggestedFilename()}`);

await browser.close();
if (errors.length > 0) {
  console.error("page errors:", errors);
  process.exit(1);
}
console.log("all browser-only studio checks passed");
