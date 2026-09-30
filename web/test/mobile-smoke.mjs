// End-to-end checks of the phone layout, touch input and the on-screen
// piano, in a real browser. Like static-smoke.mjs, not part of `npm test`: it
// needs Playwright and a served build.
//
//   tools/build-static.sh && python3 -m http.server -d dist 8765 &
//   node web/test/mobile-smoke.mjs http://localhost:8765/
//
// Set CHROMIUM to use a specific browser binary.

import { chromium } from "playwright";

const base = process.argv[2] || "http://localhost:8765/";
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined, args: ["--autoplay-policy=no-user-gesture-required"] });
const errors = [];
const assert = (c, m) => {
  if (!c) throw new Error(m);
  console.log("ok  ", m);
};
async function open(opts) {
  const ctx = await browser.newContext(opts);
  const page = await ctx.newPage();
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(base);
  await page.waitForFunction(() => document.querySelector(".song-title")?.textContent === "Demo", null, { timeout: 30000 });
  await page.waitForTimeout(500);
  return page;
}
const clips = (page) => page.evaluate(() => document.querySelectorAll(".clip").length);

// ---- phone
{
  const page = await open({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  assert(await page.evaluate(() => document.querySelector(".studio").classList.contains("compact")), "a phone gets the compact layout");
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), "nothing overflows the phone's width");
  const cdp = await page.context().newCDPSession(page);
  const touch = (type, pts) => cdp.send("Input.dispatchTouchEvent", { type, touchPoints: pts });
  // multi-touch chord on the keyboard
  const kb = await page.locator(".kb-keys").boundingBox();
  const y = kb.y + kb.height * 0.85;
  const w = await page.locator(".kb-white").first().boundingBox();
  await touch("touchStart", [{ x: kb.x + w.width * 0.5, y, id: 1 }]);
  await touch("touchStart", [
    { x: kb.x + w.width * 0.5, y, id: 1 },
    { x: kb.x + w.width * 2.5, y, id: 2 },
  ]);
  await page.waitForTimeout(100);
  assert((await page.locator(".kb-white.down").count()) === 2, "two fingers hold two keys");
  await touch("touchEnd", [{ x: kb.x + w.width * 2.5, y, id: 2 }]);
  await page.waitForTimeout(100);
  assert((await page.locator(".kb-white.down").count()) === 1, "lifting one finger keeps the other key");
  const before = await page.evaluate(() => [...document.querySelectorAll(".kb-white")].findIndex((e) => e.classList.contains("down")));
  for (let i = 1; i <= 8; i++) await touch("touchMove", [{ x: kb.x + w.width * (0.5 + i * 0.5), y, id: 1 }]);
  await page.waitForTimeout(100);
  const after = await page.evaluate(() => [...document.querySelectorAll(".kb-white")].findIndex((e) => e.classList.contains("down")));
  assert(before === 0 && after === 4 && (await page.locator(".kb-white.down").count()) === 1, `sliding moves the note (glissando) ${before}->${after}`);
  await touch("touchEnd", []);
  await page.waitForTimeout(100);
  assert((await page.locator(".kb-white.down, .kb-black.down").count()) === 0, "all keys released");

  // swipe on the playlist scrolls, adds nothing
  const grid = await page.locator(".pl .scroller").last().boundingBox();
  const n0 = await clips(page);
  const sx = grid.x + grid.width * 0.8,
    sy = grid.y + 30;
  await touch("touchStart", [{ x: sx, y: sy, id: 3 }]);
  for (let i = 1; i <= 10; i++) {
    await touch("touchMove", [{ x: sx - i * 20, y: sy + i * 3, id: 3 }]);
    await page.waitForTimeout(16);
  }
  await touch("touchEnd", []);
  await page.waitForTimeout(400);
  const sl = await page.evaluate(() => [...document.querySelectorAll(".pl .scroller")].map((e) => e.scrollLeft));
  assert((await clips(page)) === n0, `a swipe adds no clip (${n0})`);
  assert(
    sl.some((v) => v > 0),
    `a swipe scrolls the playlist (${sl})`
  );
  // a tap on an empty cell paints a clip
  await touch("touchStart", [{ x: grid.x + grid.width * 0.5, y: grid.y + 30, id: 4 }]);
  await touch("touchEnd", []);
  await page.waitForTimeout(400);
  assert((await clips(page)) === n0 + 1, "a tap paints a clip");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);

  // nav: browser pattern double-click reveals the piano roll
  await page.click(".nav-item[aria-label=Browser]");
  await page
    .locator(".b-item, .b-row")
    .filter({ hasText: "Pad · Prologue" })
    .first()
    .dblclick()
    .catch(() => {});
  await page.waitForTimeout(300);
  assert(await page.evaluate(() => document.querySelector(".studio").className.includes("v-dock")), "double-clicking a pattern switches to the dock");
  await page.click(".nav-item[aria-label=Maestro]");
  await page.waitForTimeout(300);
  assert((await page.locator(".keyboard").count()) === 0, "no keys over the terminal");
  // The keys make room where they do not help, and come back where they do.
  for (const view of ["Mixer", "Voice"]) {
    await page.click(`.nav-item[aria-label=${view}]`);
    await page.waitForTimeout(300);
    assert((await page.locator(".keyboard").count()) === 0, `no keys under the ${view.toLowerCase()}`);
  }
  await page.click(".nav-item[aria-label=Rack]");
  await page.waitForTimeout(300);
  assert((await page.locator(".keyboard").count()) === 1, "the keys are back under the rack");
  // Everything in the Voice panel can be scrolled to, uncovered, on a phone.
  await page.click(".nav-item[aria-label=Voice]");
  await page.waitForTimeout(300);
  const hidden = await page.evaluate(() => {
    const out = [];
    for (const el of document.querySelectorAll(".voice button, .voice select, .voice .knob")) {
      el.scrollIntoView({ block: "center" });
      const r = el.getBoundingClientRect();
      const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      if (r.right > innerWidth + 1 || !top || !(el === top || el.contains(top))) out.push(el.getAttribute("title") || el.textContent);
    }
    return out;
  });
  assert(hidden.length === 0, `every Voice control is reachable on a phone (not: ${hidden.join(", ")})`);
  assert(
    await page.evaluate(() => {
      const v = document.querySelector(".voice");
      return v.scrollWidth <= v.clientWidth;
    }),
    "the Voice panel does not scroll sideways on a phone"
  );
  await page.close();
}

// ---- desktop
{
  const page = await open({ viewport: { width: 1500, height: 900 } });
  assert(await page.evaluate(() => !document.querySelector(".studio").classList.contains("compact")), "a desktop keeps the three columns");
  const grid = await page.locator(".pl .scroller").last().boundingBox();
  const n0 = await clips(page);
  await page.mouse.click(grid.x + 30, grid.y + 30);
  await page.waitForTimeout(300);
  assert((await clips(page)) === n0 + 1, "desktop: a click paints a clip");
  const k = await page.locator(".kb-white").nth(3).boundingBox();
  await page.mouse.move(k.x + k.width / 2, k.y + k.height * 0.8);
  await page.mouse.down();
  await page.waitForTimeout(80);
  assert((await page.locator(".kb-white.down").count()) === 1, "desktop: mouse plays a key");
  await page.mouse.up();
  await page.waitForTimeout(80);
  assert((await page.locator(".kb-white.down").count()) === 0, "desktop: mouse releases it");
  await page.mouse.click(700, 300);
  await page.keyboard.down("z");
  await page.waitForTimeout(80);
  assert((await page.locator(".kb-white.down").count()) === 1, "desktop: the Z key lights C4");
  await page.keyboard.up("z");
  // The Q row plays too; letter shortcuts take Shift; Shift+M the metronome.
  await page.keyboard.down("q");
  await page.waitForTimeout(80);
  assert((await page.locator(".kb-white.down").count()) === 1, "desktop: Q plays a key");
  await page.keyboard.up("q");
  const mode = () => page.evaluate(async () => (await import("/src/store.js")).state.mode);
  const mode0 = await mode();
  await page.keyboard.press("Shift+L");
  await page.waitForTimeout(80);
  assert((await mode()) !== mode0, "desktop: Shift+L switches pattern/song");
  await page.keyboard.press("Shift+M");
  await page.waitForTimeout(80);
  assert((await page.locator(".btn.metro.on").count()) === 1, "desktop: Shift+M turns the metronome on");
  await page.click(".btn.metro");
  await page.waitForTimeout(80);
  assert((await page.locator(".btn.metro.on").count()) === 0, "desktop: its button turns it off");
  const notes = () =>
    page.evaluate(async () => {
      const s = await import("/src/store.js");
      const p = s.currentPattern();
      return p ? p.notes.length : -1;
    });
  // Recording counts in a bar, then writes what is played in real time: each
  // key where it went down, as long as it was held, both ends on the grid.
  const before = await notes();
  const pos = () => page.evaluate(async () => (await import("/src/audio.js")).livePosition());
  await page.click(".kb-rec");
  await page.waitForTimeout(150);
  assert((await pos()) < 0, "desktop: recording starts with a count-in");
  assert((await page.locator(".lcd").first().textContent()).includes("Count-in"), "the position shows the count-in");
  await page.keyboard.press("z");
  await page.waitForTimeout(50);
  assert((await notes()) === before, "a key early in the count-in is not recorded");
  for (let i = 0; i < 100 && (await pos()) < 0.3; i++) await page.waitForTimeout(50);
  await page.keyboard.down("z");
  await page.keyboard.down("c");
  await page.waitForTimeout(600);
  await page.keyboard.up("z");
  await page.keyboard.up("c");
  await page.waitForTimeout(300);
  await page.keyboard.press("x");
  await page.waitForTimeout(500);
  assert((await notes()) === before + 3, "desktop: armed, keys played in time are written, a chord together");
  const took = await page.evaluate(async () => {
    const s = await import("/src/store.js");
    const g = s.state.snap;
    const n = s.currentPattern().notes.slice(-3);
    const onGrid = (x) => Math.abs(x / g - Math.round(x / g)) < 1e-6;
    return {
      grid: n.every((x) => onGrid(x.start) && onGrid(x.length)),
      chord: n[0].start === n[1].start && n[0].length === n[1].length,
      rest: n[2].start > n[0].start + n[0].length,
      held: n[0].length > n[2].length,
    };
  });
  assert(took.grid, "recorded notes start and end on the grid");
  assert(took.chord, "keys held together make a chord");
  assert(took.rest && took.held, "a key held longer is longer, and the gap before the next is a rest");
  await page.keyboard.press("Escape");
  await page.waitForTimeout(80);
  assert((await page.locator(".kb-rec.armed").count()) === 0, "Esc stops recording notes");
  await page.click(".kb-toggle");
  await page.waitForTimeout(100);
  assert((await page.locator(".keyboard").count()) === 0, "the toggle hides the keys");
  await page.reload();
  await page.waitForFunction(() => document.querySelector(".song-title")?.textContent === "Demo", null, { timeout: 30000 });
  assert((await page.locator(".keyboard").count()) === 0, "hidden keys stay hidden after a reload");
  await page.click(".kb-toggle");
  await page.close();
}
await browser.close();
if (errors.length > 0) {
  console.error("page errors:", errors);
  process.exit(1);
}
console.log("all phone layout and keyboard checks passed");
