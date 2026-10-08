// End-to-end smoke test of the score's colors toggle and of the Film view in
// a real browser, against the browser-only studio (docs/static.md). Not part
// of `npm test`: it needs Playwright and a served build.
//
//   tools/build-static.sh && python3 -m http.server -d dist 8765 &
//   node web/test/film-smoke.mjs http://localhost:8765/
//
// Set CHROMIUM to use a specific browser binary.

import { chromium } from "playwright";

const base = process.argv[2] || "http://localhost:8765/";
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM || undefined,
  // WebGL in a headless browser without a GPU.
  args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"],
});
const ctx = await browser.newContext({ viewport: { width: 1500, height: 950 } });
const page = await ctx.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
const ok = (s) => console.log("ok  ", s);
const count = (sel) => page.locator(sel).count();

await page.goto(base);
await page.waitForFunction(() => document.querySelector(".song-title")?.textContent === "Arietta in J", null, { timeout: 30000 });
await page.click("button.tab:has-text('Score')");
await page.waitForSelector(".score-top .score-sys", { timeout: 20000 });

// The page fits itself to the view once it has its size, which can bring
// another system into view: count only once the systems in view stay put.
/** The number of `sel` once it has stayed the same for a second (within 15 s). */
async function settled(sel) {
  let last = -1;
  let same = 0;
  for (let i = 0; i < 60 && same < 4; i++) {
    await page.waitForTimeout(250);
    const n = await count(sel);
    same = n === last ? same + 1 : 0;
    last = n;
  }
  if (same < 4) throw new Error(`the number of ${sel} kept changing for 15 s`);
  return last;
}
await settled(".score-top .score-sys");

// Colors: color a passage, hide the colors, show them again. (The demo
// colors its own sections: `bands0` of them are in view.)
const bands0 = await count(".score-top rect.score-band");
const s = await page.locator(".score-top .score-sys").first().boundingBox();
await page.mouse.move(s.x + s.width * 0.3, s.y + s.height * 0.4);
await page.mouse.down();
await page.mouse.move(s.x + s.width * 0.5, s.y + s.height * 0.45, { steps: 6 });
await page.mouse.move(s.x + s.width * 0.7, s.y + s.height * 0.5, { steps: 6 });
await page.mouse.up();
await page.waitForSelector(".score-top .score-rangebar");
await page.click(".score-top .score-rangebar .score-swatch.big >> nth=2");
await page.waitForFunction((n) => document.querySelectorAll(".score-top rect.score-band").length > n, bands0);
ok(`a passage is colored (${(await count(".score-top rect.score-band")) - bands0} band)`);
await page.click(".score-top button[title^='Colors: shown']");
await page.waitForFunction(() => document.querySelectorAll(".score-top rect.score-band").length === 0);
ok("hiding the colors writes everything in plain ink");
await page.click(".score-top button[title^='Colors: hidden']");
await page.waitForFunction((n) => document.querySelectorAll(".score-top rect.score-band").length > n, bands0);
ok("showing them brings the colored passage back");
await page.keyboard.press("Control+z");
await page.waitForFunction((n) => document.querySelectorAll(".score-top rect.score-band").length === n, bands0);

// The film: the pages on the desk, drawn by WebGL.
await page.click(".score-top button[title^='Film']");
await page.waitForSelector(".score-top canvas.film-canvas");
await page.waitForFunction(
  () => {
    const c = document.querySelector(".score-top canvas.film-canvas");
    if (!c || c.width === 0) return false;
    const gl = c.getContext("webgl2");
    if (!gl) return false;
    const px = new Uint8Array(4);
    gl.readPixels(Math.floor(c.width / 2), Math.floor(c.height / 2), 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    return px[0] + px[1] + px[2] > 60;
  },
  null,
  { timeout: 60000 }
);
ok("the film draws the desk and its pages");
const scenes = await count(".score-top .film-scene.auto");
if (scenes < 4) throw new Error(`expected the director's scenes on the timeline, saw ${scenes}`);
ok(`the director cuts the song into ${scenes} scenes (${await page.textContent(".score-top .film-why")})`);

// Seek: the camera moves to another scene.
const tl = await page.locator(".score-top .film-timeline").boundingBox();
await page.mouse.click(tl.x + tl.width * 0.3, tl.y + 8);
await page.waitForFunction(() => !(document.querySelector(".score-top .film-why")?.textContent ?? "").includes("opening"));
ok(`seeking shows another scene: ${await page.textContent(".score-top .film-why")}`);

// Manual: a double-click on the timeline adds a shot, the panel edits it, Ctrl+Z takes it back.
await page.mouse.dblclick(tl.x + tl.width * 0.5, tl.y + 8);
await page.waitForSelector(".score-top .film-scene.shot");
await page.waitForSelector(".score-top .film-mode.on:has-text('Manual')");
await page.waitForSelector(".score-top .film-side-h:has-text('Shot 1')");
ok("double-clicking the timeline adds a shot (manual mode) and opens it in the panel");
await page.selectOption(".score-top .film-side select >> nth=1", "detail");
await page.waitForSelector(".score-top .film-scene.shot[title*='detail']");
ok("the panel changes its frame");
await page.keyboard.press("Control+z");
await page.keyboard.press("Control+z");
await page.waitForFunction(() => document.querySelector(".score-top .film-scene.shot") === null);
ok("Ctrl+Z takes the shot back");

// The director's shots, written into the project.
await page.click(".score-top .film-mode:has-text('Auto')");
await page.click('.score-top button:has-text("Write the director\'s shots")');
await page.waitForFunction(() => document.querySelectorAll(".score-top .film-scene.shot").length >= 4);
ok(`the director's shots become ${await count(".score-top .film-scene.shot")} shots to refine`);

// Back to the paper.
await page.click(".score-top button[title^='Back to the paper']");
await page.waitForSelector(".score-top .score-sys");
ok("back to the paper");

if (errors.length > 0) throw new Error(`page errors: ${errors.join("; ")}`);
await browser.close();
console.log("film smoke test passed");
