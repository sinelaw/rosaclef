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
await page.waitForFunction(() => document.querySelector(".song-title")?.textContent === "Arietta in J", null, { timeout: 30000 });

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

// Repeats: drag across bars of the song, repeat them, play them three times, give them endings.
/** Drag across the first system, from a fraction of its width to another. */
async function dragAcross(f0, f1) {
  const s = await page.locator(".score-top .score-sys").first().boundingBox();
  await page.mouse.move(s.x + s.width * f0, s.y + s.height * 0.4);
  await page.mouse.down();
  await page.mouse.move(s.x + s.width * ((f0 + f1) / 2), s.y + s.height * 0.45, { steps: 6 });
  await page.mouse.move(s.x + s.width * f1, s.y + s.height * 0.5, { steps: 6 });
  await page.mouse.up();
  await page.waitForSelector(".score-top .score-rangebar");
}
await dragAcross(0.3, 0.75);
await page.click(".score-top .score-rangebar button:has-text('Repeat')");
await page.waitForSelector(".score-top .score-rep");
await page.click(".score-top .score-rep button[title^='Play it one more']");
await page.waitForSelector(".score-top text.reptimes");
ok(`a passage repeats, ${await page.textContent(".score-top text.reptimes")}`);
await dragAcross(0.8, 0.88);
await page.click(".score-top .score-rangebar button:has-text('Ending')");
await page.waitForSelector(".score-top text.volta");
ok(`its last bar becomes the ending of passes ${await page.textContent(".score-top text.volta")}`);
await page.click("button.tab:has-text('Playlist')");
await page.waitForSelector(".ruler-repeat .ruler-repeat-times");
await page.waitForSelector(".ruler-ending");
ok("the playlist's ruler shows the repeat and its ending");
await page.click("button.tab:has-text('Score')");
await page.waitForSelector(".score-top .score-rep");
for (let i = 0; i < 3; i++) await page.keyboard.press("Control+z");
await page.waitForFunction(() => document.querySelector(".score-top .score-rep") === null && document.querySelector(".score-top text.volta") === null);
ok("Ctrl+Z takes the repeat back");

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
// Count them once the pattern's notes are drawn (the dock may still be catching up).
await page.waitForFunction(`${HEADS} > 0`);
const before = await heads();
await page.click(".score-dock .score-ribbon button[title^='Write']");
// Notices (the browser studio's welcome) pass in a few seconds; they would cover the staff.
await page.waitForFunction(() => document.querySelector(".toast") === null, null, { timeout: 15000 });
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

// Zoom magnifies the page as it is laid out; the size lays it out again.
const layout = () => page.evaluate(() => document.querySelector(".score-dock .score-sys").getAttribute("viewBox"));
const widthOf = () => page.evaluate(() => document.querySelector(".score-dock .score-paper").getBoundingClientRect().width);
const laid = await layout();
const w100 = await widthOf();
await page.click(".score-dock .score-ribbon button[title^='Zoom in']");
await page.click(".score-dock .score-ribbon button[title^='Zoom in']");
await page.waitForFunction((w) => document.querySelector(".score-dock .score-paper").getBoundingClientRect().width > w * 1.15, w100);
if ((await layout()) !== laid) throw new Error("zooming laid the page out again");
ok(`zoom magnifies the page (${await page.textContent(".score-dock .score-zoom")}), bars stay where they are`);
// Zoomed in, the hand drags the page about.
await page.click(".score-dock .score-ribbon button[title^='Hand']");
const view = await page.locator(".score-dock .score-scroll").boundingBox();
const scrolled = () => page.evaluate(() => document.querySelector(".score-dock .score-scroll").scrollLeft);
const left0 = await scrolled();
await page.mouse.move(view.x + view.width * 0.6, view.y + view.height * 0.5);
await page.mouse.down();
await page.mouse.move(view.x + view.width * 0.3, view.y + view.height * 0.5, { steps: 8 });
await page.mouse.up();
await page.waitForFunction((l) => document.querySelector(".score-dock .score-scroll").scrollLeft > l + 50, left0);
ok("the hand drags the page about");
await page.click(".score-dock .score-ribbon button[title^='Select']");
await page.dblclick(".score-dock .score-zoom");
await page.waitForFunction(() => document.querySelector(".score-dock .score-zoom").textContent === "100%");
await page.click(".score-dock .score-ribbon button[title^='Larger music']");
await page.waitForFunction((v) => document.querySelector(".score-dock .score-sys").getAttribute("viewBox") !== v, laid);
await page.click(".score-dock .score-ribbon button[title^='Smaller music']");
await page.waitForFunction((v) => document.querySelector(".score-dock .score-sys").getAttribute("viewBox") === v, laid);
ok("the music's size lays the page out again");

/** The bytes of a download. */
const read = (dl) =>
  new Promise((resolve, reject) => {
    dl.createReadStream()
      .then((stream) => {
        const chunks = [];
        stream.on("data", (c) => chunks.push(c));
        stream.on("end", () => resolve(Buffer.concat(chunks)));
        stream.on("error", reject);
      })
      .catch(reject);
  });

// Download it as a PDF.
await page.click(".score-dock .score-ribbon button[title^='Download as PDF']");
const [pdf] = await Promise.all([page.waitForEvent("download"), page.click(".score-dock .score-pdfitem:has-text('Vector')")]);
const bytes = await read(pdf);
const text = bytes.toString("latin1");
if (!text.startsWith("%PDF-") || !text.trimEnd().endsWith("%%EOF") || !text.includes("/Type /Page ")) throw new Error("the download is not a PDF");
if (!/^[\x20-\x7e]+\.pdf$/.test(pdf.suggestedFilename()) || pdf.suggestedFilename() === "download.pdf")
  throw new Error(`the PDF is not named after the score: ${pdf.suggestedFilename()}`);
ok(`the score downloads as ${pdf.suggestedFilename()} (${Math.round(bytes.length / 1024)} kB)`);
// Or as on screen: an image a page.
await page.click(".score-dock .score-ribbon button[title^='Download as PDF']");
const [shown] = await Promise.all([page.waitForEvent("download", { timeout: 60000 }), page.click(".score-dock .score-pdfitem:has-text('As on screen')")]);
const img = (await read(shown)).toString("latin1");
if (!img.startsWith("%PDF-") || !img.includes("/Subtype /Image") || !img.includes("/DCTDecode")) throw new Error("the PDF as on screen holds no page images");
ok(`or as on screen, an image a page (${Math.round(img.length / 1024)} kB)`);

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
// Its gloss and shine are knobs on the ribbon, shown while it is wet.
await page.locator(".score-dock input.score-slider").nth(1).fill("0.1");
await page.waitForFunction(() => document.querySelector(".score-dock feSpecularLighting")?.getAttribute("specularExponent") === "84");
ok("the shine of the wet ink tightens to a speck");
await page.click(".score-dock .score-ribbon button[title^='Ink:']");
await page.waitForFunction(() => document.querySelector(".score-dock path.gloss") === null);
if ((await count(".score-dock input.score-slider")) !== 0) throw new Error("the wet ink's knobs show on dry ink");
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
