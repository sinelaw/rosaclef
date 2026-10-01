// End-to-end smoke test of patterns used by reference and of lyrics in a
// real browser, against the browser-only studio (docs/static.md): the
// piano roll's use boxes and lyric row, the playlist's clip verse and the
// words in the score. Not part of `npm test`: it needs Playwright and a
// served build.
//
//   tools/build-static.sh && python3 -m http.server -d dist 8765 &
//   node web/test/lyrics-smoke.mjs http://localhost:8765/
//
// Set CHROMIUM to use a specific browser binary.

import { chromium } from "playwright";

const base = process.argv[2] || "http://localhost:8765/";
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined });
const page = await (await browser.newContext({ viewport: { width: 1500, height: 950 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
const ok = (s) => console.log("ok  ", s);
const texts = (sel) => page.locator(sel).allTextContents();
/** Wait until the texts of `sel` start with `want`. */
async function shows(sel, want) {
  await page.waitForFunction(
    ([s, w]) => {
      const got = [...document.querySelectorAll(s)].map((e) => e.textContent);
      return w.every((x, i) => got[i] === x);
    },
    [sel, want],
    { timeout: 10000 }
  );
}
/** The next frames drawn (the studio redraws on animation frames). */
const frames = () => page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(true)))));
/** Type into the lyric cell being edited (once it has the focus), then press a key and let the row redraw. */
async function typeIn(text, key) {
  await page.waitForSelector(".lyr-input:focus");
  await page.keyboard.type(text);
  await page.keyboard.press(key);
  await frames();
}

await page.goto(base);
await page.waitForFunction(() => document.querySelector(".song-title")?.textContent === "Demo", null, { timeout: 30000 });
const harp = page.locator(".clip", { hasText: "Harp · Prologue" }).first();
await harp.dblclick();
await page.waitForSelector(".lyr-lane .lyr-cell");

// The lyric row: type under the notes, Tab on, "_" holds, Escape drops.
await page.locator(".lyr-cell").first().click();
await typeIn("Hel-", "Tab");
await typeIn("lo", "Tab");
await typeIn("_", "Tab");
await typeIn("world", "Enter");
await shows(".lyr-cell", ["Hel-", "lo", "_", "world"]);
ok("syllables typed under the notes, Tab going on, _ holding");
await page.locator(".lyr-cell").nth(1).click();
await typeIn("nope", "Escape");
await shows(".lyr-cell", ["Hel-", "lo"]);
ok("Escape drops what was typed");

// A second verse, and the whole verse as text.
await page.click(".lyr-verse.add");
await page.waitForSelector(".lyr-verse.on:has-text('2')");
await page.locator(".lyr-cell").first().click();
await typeIn("Good-", "Tab");
await typeIn("bye", "Enter");
await shows(".lyr-cell", ["Good-", "bye"]);
await page.click(".lyr-verse:has-text('1')");
await shows(".lyr-cell", ["Hel-", "lo"]);
ok("verse 2 is written and shown on its own");
await page.click(".lyr-label");
await page.waitForSelector(".lyr-sheet");
await page.fill(".lyr-sheet-text", "Hel-lo [dark");
await page.waitForSelector(".lyr-sheet-verdict.error");
if (!(await page.isDisabled(".lyr-sheet button:has-text('Apply')"))) throw new Error("words that do not parse can be applied");
await page.fill(".lyr-sheet-text", "Hel-lo dark-ness my old friend");
await page.waitForFunction(() => /^7 syllables for \d+ notes/.test(document.querySelector(".lyr-sheet-verdict")?.textContent ?? ""));
await page.click(".lyr-sheet button:has-text('Apply')");
await shows(".lyr-cell", ["Hel-", "lo", "dark-", "ness"]);
ok("Lyrics… edits the verse as text, refusing words that do not parse");

// The score writes the words under the staff, verses stacked and numbered.
await page.keyboard.press("F10");
await page.waitForSelector(".score-dock text.lyric");
const nums = await texts(".score-dock text.lyric.num");
if (!nums.includes("1.") || !nums.includes("2.")) throw new Error(`verses are not numbered: ${nums}`);
ok(`the score sets ${await page.locator(".score-dock text.lyric:not(.num)").count()} syllables, two verses numbered`);

// The clip sings verse 2.
await harp.click();
const sings = page.locator(".tools select[title^='The verse']");
await sings.selectOption("2");
await page.waitForSelector(".clip-verse:has-text('v2')");
await page.keyboard.press("Control+z");
await page.waitForFunction(() => document.querySelector(".clip-verse") === null);
ok("a clip is set to sing verse 2 (v2), and Ctrl+Z takes it back");

// Make reference, then the use's menu: transpose, make unique, undo.
await harp.dblclick();
await page.waitForSelector(".editor-main.pr .note:not(.ghost)");
await page.keyboard.press("Control+a");
await page.click("button[title^='Make reference']");
await page.waitForSelector(".use-tab");
const own = await page.locator(".editor-main.pr .note:not(.ghost):not(.used)").count();
if (own !== 0 || (await page.locator(".note.used").count()) === 0) throw new Error("the notes did not become a use");
await shows(".lyr-cell", ["Hel-", "lo"]);
ok(`the notes become a pattern played by reference (${await page.textContent(".use-tab")}), their words with them`);
await page.click(".use-tab");
await page.click(".ctx-menu-item:has-text('Transpose +1')");
await page.waitForFunction(() => document.querySelector(".use-tab")?.textContent?.endsWith("+1"));
await page.click(".use-tab");
await page.click(".ctx-menu-item:has-text('Make unique')");
await page.waitForFunction(() => document.querySelector(".use-tab") === null);
await shows(".lyr-cell", ["Hel-", "lo"]);
ok("a use is transposed, then made unique, keeping its words");
for (let i = 0; i < 3; i++) await page.keyboard.press("Control+z");
// The pattern Make reference made is gone again (Make unique leaves it).
await page.waitForFunction(() => ![...document.querySelectorAll(".b-item")].some((e) => e.textContent.includes("Prologue part 1")));
await shows(".lyr-cell", ["Hel-", "lo"]);
if ((await page.locator(".use-tab").count()) > 0 || (await page.locator(".editor-main.pr .note:not(.ghost):not(.used)").count()) === 0)
  throw new Error("the notes are not the pattern's own again");
ok("Ctrl+Z undoes it all");

if (errors.length > 0) throw new Error(`page errors: ${errors.join("; ")}`);
await browser.close();
console.log("all uses and lyrics checks passed");
