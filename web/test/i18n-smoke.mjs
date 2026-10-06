// End-to-end smoke test of the interface's languages (docs/i18n.md), in a real
// browser, against the browser-only studio (docs/static.md): the switcher at
// the top right, every language loading and showing, the choice kept across a
// reload, and the browser's language picked at first. Not part of `npm test`:
// it needs Playwright and a served build.
//
//   tools/build-static.sh && python3 -m http.server -d dist 8765 &
//   node web/test/i18n-smoke.mjs http://localhost:8765/
//
// Set CHROMIUM to use a specific browser binary.

import { chromium } from "playwright";

const base = process.argv[2] || "http://localhost:8765/";
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined, args: ["--autoplay-policy=no-user-gesture-required"] });
const errors = [];
const ok = (s) => console.log("ok  ", s);

/** A page of the studio in a browser that asks for a language, once its song is in. */
async function open(locale) {
  const context = await browser.newContext({ viewport: { width: 1500, height: 900 }, locale });
  const page = await context.newPage();
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(base);
  await page.waitForSelector(".topbar .lang-switch", { timeout: 30000 });
  await page.waitForFunction(() => document.querySelector(".song-title")?.textContent !== "", null, { timeout: 30000 });
  return { context, page };
}

const exportText = (page) => page.textContent(".topbar .btn.gold");
const langIs = (page, code) => page.waitForFunction((c) => document.documentElement.lang === c, code, { timeout: 15000 });

// No key shows as it is: every text the page shows comes from en.json (or the language).
const keyLike = (page) =>
  page.evaluate(() => {
    const re = /^[a-z][a-zA-Z0-9]*(\.[a-z0-9][a-zA-Z0-9]*)+$/;
    const texts = [...document.querySelectorAll("#app *")].flatMap((e) => [
      ...[...e.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent.trim()),
      e.getAttribute("title") ?? "",
      e.getAttribute("aria-label") ?? "",
      e.getAttribute("placeholder") ?? "",
    ]);
    return texts.filter((s) => re.test(s) && /\.(label|title|hint|aria|placeholder|empty|body|option)\b|^(common|term|panel|format|drum)\./.test(s));
  });

// English by default, with the switcher at the right end of the top bar.
const { context, page } = await open("en-US");
await langIs(page, "en");
if ((await exportText(page)).trim() !== "Export") throw new Error("the studio does not start in English");
const box = await page.locator(".topbar .lang-switch").boundingBox();
const bar = await page.locator(".topbar").boundingBox();
if (!box || !bar || bar.x + bar.width - (box.x + box.width) > 40 || box.y > bar.y + bar.height)
  throw new Error("the language switcher is not at the top right");
if ((await page.textContent(".lang-switch .lang-code")).trim() !== "EN") throw new Error("the switcher does not say EN");
const codes = await page.$$eval(".lang-select option", (os) => os.map((o) => o.value));
if (codes.length !== 10) throw new Error(`expected 10 languages, got ${codes.length}: ${codes.join(", ")}`);
ok(`English first, the switcher at the top right offers ${codes.join(", ")}`);
const raw = await keyLike(page);
if (raw.length > 0) throw new Error(`keys shown instead of texts: ${raw.slice(0, 5).join(", ")}`);
ok("no key shows instead of its text");

// Every language loads and shows its words.
for (const code of codes) {
  if (code === "en") continue;
  const words = await page.evaluate(async (c) => (await fetch(`locales/${c}.json`)).json(), code);
  await page.selectOption(".lang-select", code);
  await langIs(page, code);
  const want = words["topbar.export.label"];
  await page
    .waitForFunction((w) => document.querySelector(".topbar .btn.gold")?.textContent.trim() === w, want, { timeout: 15000 })
    .catch(() => {
      throw new Error(`${code}: the export button does not say ${want}`);
    });
  if ((await page.textContent(".lang-switch .lang-code")).trim() !== code.split("-")[0].toUpperCase()) throw new Error(`${code}: the switcher's code`);
  const title = await page.getAttribute(".topbar .btn.icon.play, .topbar .btn.icon.play.on", "title");
  if (title === "Play / pause (Space)") throw new Error(`${code}: the play button's tip is still English`);
  ok(`${code}: ${Object.keys(words).length} texts, the export button says “${want}”`);
}

// The choice stays across a reload.
await page.selectOption(".lang-select", "de");
await langIs(page, "de");
await page.reload();
await page.waitForSelector(".topbar .lang-switch", { timeout: 30000 });
await langIs(page, "de");
ok("the language chosen stays after a reload");

// Back to English: everything English again.
await page.selectOption(".lang-select", "en");
await langIs(page, "en");
// The page says its language before it draws in it: wait for the drawing.
await page
  .waitForFunction(() => document.querySelector(".topbar .btn.gold")?.textContent.trim() === "Export", null, { timeout: 15000 })
  .catch(() => {
    throw new Error("English does not come back");
  });
ok("back to English");
await context.close();

// A browser that asks for Japanese gets Japanese at first; one asking for a language the studio does not speak, English.
const ja = await open("ja-JP");
await langIs(ja.page, "ja");
ok("a Japanese browser starts in Japanese");
await ja.context.close();
const pt = await open("pt-PT");
await langIs(pt.page, "pt-BR");
ok("a Portuguese (Portugal) browser starts in Portuguese (Brazil)");
await pt.context.close();
const nl = await open("nl-NL");
await langIs(nl.page, "en");
ok("a Dutch browser starts in English");
await nl.context.close();

await browser.close();
if (errors.length > 0) {
  console.error(errors.join("\n"));
  process.exit(1);
}
console.log("i18n smoke: all good");
