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
// Waits for a condition in the page (no fixed sleeps), then reports it.
const until = async (page, fn, arg, m) => {
  await page.waitForFunction(fn, arg, { timeout: 10000 }).catch(() => {
    throw new Error(`timed out: ${m}`);
  });
  console.log("ok  ", m);
};
// Waits until the playhead (beats, negative during a count-in) reaches `beat`.
const untilBeat = (page, beat) => page.waitForFunction((b) => window.__audio.livePosition() >= b, beat, { timeout: 60000 });
const frame = (page) => page.evaluate(() => new Promise((r) => requestAnimationFrame(() => r(true))));
// Every finite CSS transition and animation has finished (panels in place); endless ones (a pulsing dot) do not count.
const settled = (page) =>
  page.waitForFunction(() => document.getAnimations().every((a) => a.playState !== "running" || a.effect.getComputedTiming().endTime === Infinity), null, {
    timeout: 10000,
  });
async function open(opts) {
  const ctx = await browser.newContext(opts);
  const page = await ctx.newPage();
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(base);
  await page.waitForFunction(() => document.querySelector(".song-title")?.textContent === "Demo", null, { timeout: 30000 });
  // The store module, to wait on the app's state.
  await page.evaluate(async () => {
    window.__store = await import("/src/store.js");
    window.__audio = await import("/src/audio.js");
  });
  return page;
}
const clips = (page) => page.evaluate(() => document.querySelectorAll(".clip").length);
const keysDown = (sel, n) => [(a) => document.querySelectorAll(a.sel).length === a.n, { sel: sel, n: n }];

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
  await until(page, ...keysDown(".kb-white.down", 2), "two fingers hold two keys");
  await touch("touchEnd", [{ x: kb.x + w.width * 2.5, y, id: 2 }]);
  await until(page, ...keysDown(".kb-white.down", 1), "lifting one finger keeps the other key");
  const before = await page.evaluate(() => [...document.querySelectorAll(".kb-white")].findIndex((e) => e.classList.contains("down")));
  for (let i = 1; i <= 8; i++) await touch("touchMove", [{ x: kb.x + w.width * (0.5 + i * 0.5), y, id: 1 }]);
  assert(before === 0, "the first finger holds the first key");
  await until(
    page,
    () =>
      document.querySelectorAll(".kb-white.down").length === 1 &&
      [...document.querySelectorAll(".kb-white")].findIndex((e) => e.classList.contains("down")) === 4,
    null,
    "sliding moves the note (glissando)"
  );
  await touch("touchEnd", []);
  await until(page, ...keysDown(".kb-white.down, .kb-black.down", 0), "all keys released");

  // swipe on the playlist scrolls, adds nothing
  const grid = await page.locator(".pl .scroller").last().boundingBox();
  const n0 = await clips(page);
  const sx = grid.x + grid.width * 0.8,
    sy = grid.y + 30;
  await touch("touchStart", [{ x: sx, y: sy, id: 3 }]);
  for (let i = 1; i <= 10; i++) {
    await touch("touchMove", [{ x: sx - i * 20, y: sy + i * 3, id: 3 }]);
    await frame(page);
  }
  await touch("touchEnd", []);
  await until(page, () => [...document.querySelectorAll(".pl .scroller")].some((e) => e.scrollLeft > 0), null, "a swipe scrolls the playlist");
  await frame(page);
  assert((await clips(page)) === n0, `a swipe adds no clip (${n0})`);
  // a tap on an empty cell paints a clip
  await touch("touchStart", [{ x: grid.x + grid.width * 0.5, y: grid.y + 30, id: 4 }]);
  await touch("touchEnd", []);
  await until(page, (n) => document.querySelectorAll(".clip").length === n, n0 + 1, "a tap paints a clip");
  await page.keyboard.press("Control+z");
  await until(page, (n) => document.querySelectorAll(".clip").length === n, n0, "Ctrl+Z takes it back");

  // nav: browser pattern double-click reveals the piano roll
  await page.click(".nav-item[aria-label=Browser]");
  await page
    .locator(".b-item, .b-row")
    .filter({ hasText: "Piano · Intro" })
    .first()
    .dblclick()
    .catch(() => {});
  await until(page, () => document.querySelector(".studio").className.includes("v-dock"), null, "double-clicking a pattern switches to the dock");
  await page.click(".nav-item[aria-label=Maestro]");
  await until(
    page,
    () => document.querySelector(".studio").className.includes("v-agent") && !document.querySelector(".keyboard"),
    null,
    "no keys over the terminal"
  );
  // The keys make room where they do not help, and come back where they do.
  for (const view of ["Mixer", "Voice"]) {
    await page.click(`.nav-item[aria-label=${view}]`);
    await until(
      page,
      (v) => document.querySelector(`.nav-item[aria-label=${v}]`).classList.contains("on") && !document.querySelector(".keyboard"),
      view,
      `no keys under the ${view.toLowerCase()}`
    );
  }
  await page.click(".nav-item[aria-label=Rack]");
  await until(page, () => document.querySelectorAll(".keyboard").length === 1, null, "the keys are back under the rack");
  // Everything in the Voice panel can be scrolled to, uncovered, on a phone.
  await page.click(".nav-item[aria-label=Voice]");
  await page.waitForSelector(".voice");
  // A notice (the first visit's welcome) would cover the bottom of the panel: dismiss it.
  for (const t of await page.locator(".toast").all()) await t.click();
  await page.waitForFunction(() => document.querySelector(".toast") === null, null, { timeout: 10000 });
  await settled(page);
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
  await until(page, (n) => document.querySelectorAll(".clip").length === n, n0 + 1, "desktop: a click paints a clip");
  const k = await page.locator(".kb-white").nth(3).boundingBox();
  await page.mouse.move(k.x + k.width / 2, k.y + k.height * 0.8);
  await page.mouse.down();
  await until(page, ...keysDown(".kb-white.down", 1), "desktop: mouse plays a key");
  await page.mouse.up();
  await until(page, ...keysDown(".kb-white.down", 0), "desktop: mouse releases it");
  await page.mouse.click(700, 300);
  await page.keyboard.down("z");
  await until(page, ...keysDown(".kb-white.down", 1), "desktop: the Z key lights C4");
  await page.keyboard.up("z");
  await until(page, ...keysDown(".kb-white.down", 0), "desktop: and goes out");
  // The Q row plays too; letter shortcuts take Shift; Shift+M the metronome.
  await page.keyboard.down("q");
  await until(page, ...keysDown(".kb-white.down", 1), "desktop: Q plays a key");
  await page.keyboard.up("q");
  const mode0 = await page.evaluate(() => window.__store.state.mode);
  await page.keyboard.press("Shift+L");
  await until(page, (m) => window.__store.state.mode !== m, mode0, "desktop: Shift+L switches pattern/song");
  await page.keyboard.press("Shift+M");
  await until(page, ...keysDown(".btn.metro.on", 1), "desktop: Shift+M turns the metronome on");
  await page.click(".btn.metro");
  await until(page, ...keysDown(".btn.metro.on", 0), "desktop: its button turns it off");
  const notes = () => page.evaluate(() => window.__store.currentPattern()?.notes.length ?? -1);
  const pos = () => page.evaluate(() => window.__audio.livePosition());
  // Recording counts in a bar, then writes what is played in real time: each
  // key where it went down, as long as it was held, both ends on the grid.
  const before = await notes();
  await page.click(".kb-rec");
  await until(page, () => window.__audio.livePosition() < 0, null, "desktop: recording starts with a count-in");
  await until(page, () => document.querySelector(".lcd").textContent.includes("Count-in"), null, "the position shows the count-in");
  await page.keyboard.press("z");
  await frame(page);
  assert((await notes()) === before, "a key early in the count-in is not recorded");
  await untilBeat(page, 0.3);
  // A chord: both keys struck at once (one page event, however busy the machine), held a beat.
  const chord = (type) =>
    page.evaluate((type) => {
      for (const code of ["KeyZ", "KeyC"]) window.dispatchEvent(new KeyboardEvent(type, { code: code, key: code[3].toLowerCase() }));
    }, type);
  await chord("keydown");
  await untilBeat(page, (await pos()) + 1);
  await chord("keyup");
  // Half a beat's rest, then a short note.
  await untilBeat(page, (await pos()) + 0.5);
  await page.keyboard.press("x");
  await until(
    page,
    (n) => (window.__store.currentPattern()?.notes.length ?? -1) === n,
    before + 3,
    "desktop: armed, keys played in time are written, a chord together"
  );
  const took = await page.evaluate(() => {
    const s = window.__store;
    const g = s.state.snap;
    const n = s.currentPattern().notes.slice(-3);
    const onGrid = (x) => Math.abs(x / g - Math.round(x / g)) < 1e-6;
    return {
      grid: n.every((x) => onGrid(x.start) && onGrid(x.length)),
      chord: n[0].start === n[1].start && Math.abs(n[0].length - n[1].length) <= g + 1e-6,
      rest: n[2].start > n[0].start + n[0].length,
      held: n[0].length > n[2].length,
    };
  });
  assert(took.grid, "recorded notes start and end on the grid");
  assert(took.chord, "keys held together make a chord");
  assert(took.rest && took.held, "a key held longer is longer, and the gap before the next is a rest");
  await page.keyboard.press("Escape");
  await until(page, () => document.querySelectorAll(".kb-rec.armed").length === 0, null, "Esc stops recording notes");
  // Recording does not loop: a one-bar pattern grows as the playhead goes on,
  // then ends with the bar of the last note played.
  const one = await page.evaluate(() => {
    const s = window.__store;
    const p = s.currentPattern();
    s.commit(() => {
      p.notes = [];
      p.length = s.state.project.transport.beatsPerBar;
    });
    return s.state.project.transport.beatsPerBar;
  });
  await page.click(".kb-rec");
  await untilBeat(page, one + 1.5);
  await page.keyboard.press("v");
  await until(
    page,
    (one) => {
      const p = window.__store.currentPattern();
      return p.length >= 2 * one && p.notes.length > 0 && p.notes[p.notes.length - 1].start > one;
    },
    one,
    "the pattern grows while recording, with the note played in its second bar"
  );
  await untilBeat(page, 3 * one + 1.5);
  await until(page, (one) => window.__store.currentPattern().length >= 4 * one, one, "and keeps growing");
  await page.keyboard.press("Escape");
  await until(page, (one) => window.__store.currentPattern().length === 2 * one, one, "stopping trims it after the last note");
  await page.click(".kb-toggle");
  await until(page, () => document.querySelectorAll(".keyboard").length === 0, null, "the toggle hides the keys");
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
