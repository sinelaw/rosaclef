// End-to-end smoke test of the Score view in a real browser, against the
// browser-only studio (docs/static.md). Not part of `npm test`: it needs
// Playwright and a served build.
//
//   tools/build-static.sh && python3 -m http.server -d dist 8765 &
//   node web/test/score-smoke.mjs http://localhost:8765/
//
// Set CHROMIUM to use a specific browser binary.

import { chromium } from "playwright";

const base = process.argv[2] || "http://localhost:8765/";
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined });
const ctx = await browser.newContext({ viewport: { width: 1500, height: 950 } });
const page = await ctx.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
const ok = (s) => console.log("ok  ", s);
const count = (sel) => page.locator(sel).count();

await page.goto(base);
await page.waitForFunction(() => document.querySelector(".song-title")?.textContent === "Demo", null, { timeout: 30000 });

// The whole song beside the playlist.
await page.click("button.tab:has-text('Score')");
await page.waitForSelector(".score-top .score-sys", { timeout: 20000 });
await page.waitForFunction(() => document.fonts.check("16px Bravura"), null, { timeout: 20000 });
const systems = await count(".score-top .score-sys");
if (systems < 2) throw new Error(`expected several systems, saw ${systems}`);
ok(`the song is engraved (${systems} systems in view, Bravura loaded)`);
const title = await page.textContent(".score-top .score-title");
if (!title || title.trim() === "") throw new Error("no title");
ok(`the page is titled "${title}"`);

// The pattern of the piano roll, in the dock (F10); write a note into it.
await page.keyboard.press("Control+Alt+KeyP");
await page.keyboard.press("F10");
await page.waitForSelector(".score-dock .score-sys");
// The page is laid out for the dock's real width (it lays out again when it learns it).
await page.waitForFunction(() => {
  const el = document.querySelector(".score-dock .score-scroll");
  return el !== null && Number(el.dataset.width) === Math.round(el.getBoundingClientRect().width);
});
// Noteheads drawn in the dock's score (not the Write ghost or the selection drawn over a note).
const HEADS = `[...document.querySelectorAll(".score-dock text.glyphs:not(.ghost):not(.sel)")].reduce(
  (n, t) => n + [...t.textContent].filter((c) => c === "\u{e0a4}" || c === "\u{e0a3}" || c === "\u{e0a2}").length, 0)`;
const heads = () => page.evaluate(HEADS);
const before = await heads();
await page.click(".score-dock .score-ribbon button[title^='Write']");
// On the middle line of the first staff, a little into the first bar: Write shows a ghost note there.
const lines = await page.locator(".score-dock .score-sys").first().locator("path.staff").boundingBox();
const at = { x: lines.x + lines.width * 0.42, y: lines.y + lines.height / 2 };
await page.mouse.move(at.x, at.y);
await page.waitForSelector(".score-dock text.ghost");
await page.mouse.click(at.x, at.y);
await page.waitForFunction(`${HEADS} > ${before}`);
const after = await heads();
ok(`Write adds a note (${before} → ${after} noteheads)`);
await page.keyboard.press("Control+z");
await page.waitForFunction(`${HEADS} === ${before}`);
ok("Ctrl+Z takes it back");

// Color a passage: drag across the music, pick a color.
await page.click(".score-dock .score-ribbon button[title^='Select']");
const s0 = await page.locator(".score-dock .score-sys").first().boundingBox();
await page.mouse.move(s0.x + s0.width * 0.3, s0.y + s0.height * 0.45);
await page.mouse.down();
await page.mouse.move(s0.x + s0.width * 0.5, s0.y + s0.height * 0.5, { steps: 6 });
await page.mouse.move(s0.x + s0.width * 0.7, s0.y + s0.height * 0.55, { steps: 6 });
await page.mouse.up();
await page.waitForSelector(".score-dock .score-rangebar");
await page.click(".score-dock .score-rangebar .score-swatch >> nth=1");
await page.waitForSelector(".score-dock .score-band");
await page.waitForSelector(".score-dock .score-mark");
ok("dragging across the music colors a passage, listed under Colors");
await page.keyboard.press("Control+z");
await page.waitForFunction(() => document.querySelectorAll(".score-dock .score-band").length === 0);
ok("Ctrl+Z removes the color");

// Download it as a PDF.
const [pdf] = await Promise.all([page.waitForEvent("download"), page.click(".score-dock .score-ribbon button[title^='Download as PDF']")]);
const bytes = await new Promise((resolve, reject) => {
  pdf
    .createReadStream()
    .then((stream) => {
      const chunks = [];
      stream.on("data", (c) => chunks.push(c));
      stream.on("end", () => resolve(Buffer.concat(chunks)));
      stream.on("error", reject);
    })
    .catch(reject);
});
const text = bytes.toString("latin1");
if (!text.startsWith("%PDF-") || !text.trimEnd().endsWith("%%EOF") || !text.includes("/Type /Page ")) throw new Error("the download is not a PDF");
if (!/^[\x20-\x7e]+\.pdf$/.test(pdf.suggestedFilename()) || pdf.suggestedFilename() === "download.pdf")
  throw new Error(`the PDF is not named after the score: ${pdf.suggestedFilename()}`);
ok(`the score downloads as ${pdf.suggestedFilename()} (${Math.round(bytes.length / 1024)} kB)`);

// Hide a part.
const parts = await count(".score-dock .score-part");
if (parts < 1) throw new Error("no parts listed");
await page.click(".score-dock .score-part .score-eye >> nth=0");
await page.waitForSelector(".score-dock .score-empty");
ok("hiding the only part leaves an empty page");
await page.keyboard.press("Control+z");
await page.waitForSelector(".score-dock .score-sys");

// One ink at a time: wet (glossy notes) by default, or dry (faded, no gloss).
await page.waitForSelector(".score-dock path.gloss");
await page.click(".score-dock .score-ribbon button[title^='Ink:']");
await page.waitForFunction(() => document.querySelector(".score-dock path.gloss") === null);
ok("the ink is wet and glossy, or dry and faded");

// Night ink, and the phone layout.
await page.click(".score-dock .score-ribbon button[title^='Night']");
await page.waitForSelector(".score-dock.night");
ok("night ink");
await page.setViewportSize({ width: 390, height: 844 });
await page.waitForSelector(".navbar");
await page.click(".nav-item[aria-label=Score]");
await page.waitForSelector(".score-dock .score-sys");
ok("the phone layout shows the score from its navigation bar");

await browser.close();
if (errors.length > 0) {
  console.error("page errors:", errors);
  process.exit(1);
}
console.log("all score checks passed");
