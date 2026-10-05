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
await page.waitForFunction(() => document.querySelector(".song-title")?.textContent === "Arietta in J", null, { timeout: 30000 });
ok("the first visit opens the demo song");
if (await page.isVisible('.seg button:has-text("Studio")')) throw new Error("the Studio output needs a server");

const before = await page.textContent(".lcd.static .lcd-value");
await page.mouse.click(700, 400);
await page.keyboard.press("Space");
// Playing moves the playhead (shown in the song position display).
await page.waitForFunction((b) => document.querySelector(".lcd.static .lcd-value")?.textContent !== b, before, { timeout: 20000 });
await page.keyboard.press("Space");
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

// The master transpose, beside the tempo: saved with the song.
const transposed = (pg, t) => pg.waitForFunction((v) => document.querySelector(".lcd.transpose .lcd-value")?.textContent.startsWith(v), t);
await page.click(".lcd.transpose button[aria-label$='higher']");
await transposed(page, "+1");
await page.click(".lcd.transpose button[aria-label$='higher']");
await transposed(page, "+2");
// Another tab shows it once it is saved.
const third = await ctx.newPage();
await third.goto(base);
await transposed(third, "+2");
await third.close();
await page.reload();
await page.waitForSelector(".lcd.transpose.shifted");
await transposed(page, "+2");
await page.dblclick(".lcd.transpose .lcd-label");
await transposed(page, "0");
ok("the song transposes by semitones, and stays transposed");

// The Drums tab: the demo's drum part (the song drummer, folded under the
// song) is a jazz waltz; writing it again keeps it in the song.
await page.keyboard.press("F4");
await page.click(".drums-arrange-head");
await page.waitForSelector(".drums-sec");
await hasText(page, "Jazz waltz");
await page.click(".drums-writebtn");
await hasText(page, "Drums written");
await hasText(page, "Drums · Jazz waltz A");
ok("the Drums tab writes a drum part into the song");

// Grooves in another time signature are greyed out, saying why; changing the
// song's time signature (the Time LCD) opens them up.
const greyed = () => page.locator(".drums-groove option[disabled]").count();
const inWaltz = await greyed();
const why = await page.locator(".drums-groove option[disabled]").first().getAttribute("title");
if (inWaltz === 0 || !why.includes("the song is in 3/4")) throw new Error(`greyed grooves: ${inWaltz} (${why})`);
await page.locator(".lcd.timesig select").selectOption("4/4");
await page.waitForFunction(() => document.querySelector(".drums-info.warn")?.textContent.includes("4/4"));
if ((await greyed()) >= inWaltz) throw new Error("4/4 grooves stay greyed out in a 4/4 song");
await page.keyboard.press("Control+z");
await page.waitForFunction(() => document.querySelector(".lcd.timesig select")?.value === "3/4");
ok(`grooves that do not fit the time signature are greyed out (${inWaltz} in 3/4)`);

// A click on the tempo makes it a field to type into.
const tempo0 = await page.textContent(".lcd.tempo .lcd-value");
await page.click(".lcd.tempo");
await page.waitForSelector(".lcd.tempo input.lcd-input");
await page.keyboard.type("90");
await page.keyboard.press("Enter");
await page.waitForFunction(() => document.querySelector(".lcd.tempo .lcd-value")?.textContent.startsWith("90.00"));
await page.keyboard.press("Control+z");
await page.waitForFunction((t) => document.querySelector(".lcd.tempo .lcd-value")?.textContent === t, tempo0);
ok("the tempo can be typed");

await page.keyboard.press("Control+o");
await page.waitForSelector(".pm-card");
await page.click("text=New project");
await page.fill(".pm-name", "Second song");
await page.keyboard.press("Enter");
await page.waitForFunction(() => document.querySelector(".song-title")?.textContent === "Second song", null, { timeout: 20000 });
ok("a new project is created and opened");

await page.keyboard.press("Control+o");
await page.waitForSelector('.pm-card:has-text("Arietta in J")');
const [zip] = await Promise.all([page.waitForEvent("download"), page.click('.pm-card:has-text("Arietta in J") button[title*=".zip"]')]);
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
