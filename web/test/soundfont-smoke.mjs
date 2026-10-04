// End-to-end test of soundfont instruments in the browser-only studio, in a
// real browser: the demo's General MIDI instruments load (only the pieces they need), plays
// through the audio worklet, and loading it never stalls the page. Not part
// of `npm test`: it needs Playwright and a served build.
//
//   tools/build-static.sh && python3 -m http.server -d dist 8765 &
//   node web/test/soundfont-smoke.mjs http://localhost:8765/
//
// Set CHROMIUM to use a specific browser binary.

import { chromium } from "playwright";

const base = process.argv[2] || "http://localhost:8765/";
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined, args: ["--autoplay-policy=no-user-gesture-required"] });
const page = await browser.newPage({ viewport: { width: 1500, height: 900 } });
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
const fetched = [];
page.on("requestfinished", (r) => {
  if (r.url().includes("/soundfonts/")) fetched.push(r.url().split("/soundfonts/")[1]);
});
const ok = (s) => console.log("ok  ", s);

await page.goto(base);
await page.waitForFunction(() => document.querySelector(".song-title")?.textContent === "Arietta in J", null, { timeout: 30000 });

// The longest gap between animation frames: how long the page was blocked.
await page.evaluate(() => {
  window.__gap = 0;
  let last = performance.now();
  const tick = (t) => {
    window.__gap = Math.max(window.__gap, t - last);
    last = t;
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
});

// The demo plays the soundfont: tenor sax, grand piano, double bass and the
// jazz kit; the piano plays from the first bar.
await page.click("text=Channel Rack");
const t0 = Date.now();
await page.click('[title="Play / pause (Space)"]');

// Sound: the channel's meter moves.
await page.waitForFunction(
  () => {
    const row = [...document.querySelectorAll(".rack-row")].find((r) => r.textContent.includes("Piano"));
    const fill = row && row.querySelector(".led-fill");
    const m = fill && /scaleX\(([\d.]+)\)/.exec(fill.style.transform);
    return m && Number(m[1]) > 0.3;
  },
  null,
  { timeout: 90000, polling: 100 }
);
ok(`the piano sounds ${((Date.now() - t0) / 1000).toFixed(1)} s after pressing play`);
await page.click('[title="Play / pause (Space)"]');

const pieces = fetched.filter((f) => f.includes("smpl-"));
if (!fetched.includes("gm/index.sf2")) throw new Error(`the index was not fetched: ${fetched}`);
if (pieces.length === 0 || pieces.length > 30) throw new Error(`fetched ${pieces.length} pieces`);
ok(`fetched the index and ${pieces.length} of 38 pieces`);

const gap = await page.evaluate(() => window.__gap);
if (gap > 400) throw new Error(`the page stalled for ${Math.round(gap)} ms`);
ok(`the page kept drawing (longest frame gap ${Math.round(gap)} ms)`);

const toasts = await page.$$eval(".toast.error", (t) => t.map((x) => x.textContent));
if (toasts.length > 0) throw new Error(`error toasts: ${toasts}`);
if (errors.length > 0) throw new Error(`page errors: ${errors}`);
await browser.close();
console.log("soundfont smoke test passed");
