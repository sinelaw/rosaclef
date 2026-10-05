// End-to-end smoke test of the Drums tab, in a real browser, against the
// browser-only studio (docs/static.md): one screen that always works on a
// drum pattern at the song cursor — making one from a groove, changing its
// recipe and its notes, copying it into a variation, targeting another from
// the song strip, removing one — with the song drummer folded underneath.
// Not part of `npm test`: it needs Playwright and a served build.
//
//   tools/build-static.sh && python3 -m http.server -d dist 8765 &
//   node web/test/drums-smoke.mjs http://localhost:8765/
//
// Set CHROMIUM to use a specific browser binary.

import { chromium } from "playwright";

const base = process.argv[2] || "http://localhost:8765/";
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined, args: ["--autoplay-policy=no-user-gesture-required"] });
const page = await (await browser.newContext({ viewport: { width: 1500, height: 950 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
const ok = (s) => console.log("ok  ", s);
const until = (fn, arg, what) =>
  page.waitForFunction(fn, arg, { timeout: 15000 }).catch(() => {
    throw new Error(`timed out: ${what}`);
  });
/** The tab's target pattern, and the playlist clips that play it. */
const target = () =>
  page.evaluate(async () => {
    const s = (await import("./src/store.js")).state;
    const d = (await import("./src/ui/drums.js")).drums;
    const pat = s.project.patterns.find((x) => x.id === d.target);
    return {
      id: d.target,
      name: pat?.name ?? "",
      recipe: pat ? pat.drums : null,
      notes: pat ? pat.notes.length : 0,
      clips: s.project.playlist.clips
        .filter((c) => c.pattern === d.target)
        .map((c) => ({ start: c.start, length: c.length, track: s.project.playlist.tracks[c.track]?.name })),
    };
  });

await page.goto(base);
await page.waitForFunction(() => document.querySelector(".song-title")?.textContent === "Arietta in J", null, { timeout: 30000 });
// Start the audio (a gesture), so the song cursor stays where it is put.
await page.click(".kb-keys", { position: { x: 5, y: 40 } });
await page.waitForTimeout(1500);
await page.keyboard.press("F4");
await page.waitForSelector(".drums-screen .drums-target");
await until(() => document.querySelectorAll(".drums-lib-item").length > 10, null, "the groove library loads");

// Bar 3 of the waltz has no drums: the tab says so, and offers the 3/4 grooves first.
await page.evaluate(async () => (await import("./src/audio.js")).seek(6));
await until(() => document.querySelector(".drums-target")?.textContent.includes("Nothing plays drums at bar 3"), null, "no drums at bar 3");
const firstFit = await page.$eval(".drums-lib-item:not(.misfit) .drums-lib-meta", (e) => e.textContent);
if (!firstFit.startsWith("3/4")) throw new Error(`the first groove offered is ${firstFit}`);
ok("an empty bar says it has no drums, and the grooves that fit it come first");

// A click on a groove makes a drum pattern there, on the Drums track.
await page.click(".drums-lib-item:not(.misfit) >> nth=0");
await until(() => document.querySelector(".drums-pattern-grid .drums-row") !== null, null, "a pattern is made");
let t = await target();
if (!t.recipe?.on || t.notes === 0 || t.clips.length !== 1 || t.clips[0].start !== 6 || t.clips[0].track !== "Drums")
  throw new Error(`made: ${JSON.stringify(t)}`);
await until(() => document.querySelector(".drums-where")?.textContent.includes("Drums track, bars 3–"), null, "the tab says where it plays");
ok(`a groove makes ${t.name} at the cursor, on the Drums track (${t.notes} notes)`);

// Its recipe: B with a fill makes it again; a click on the grid edits its notes.
const before = t.notes;
await page.click(".drums-seg button:has-text('B · chorus')");
await until(async () => true, null, "");
await page.click(".drums-seg button:has-text('1 bar')");
await until(() => document.querySelector(".drums-seg button.on")?.textContent.includes("B"), null, "B is lit");
await page.waitForTimeout(800);
t = await target();
if (t.recipe.play !== "b" || t.recipe.fill !== "bar") throw new Error(`recipe: ${JSON.stringify(t.recipe)}`);
ok(`B with a 1-bar fill makes it again (${before} → ${t.notes} notes)`);
await page.click(".drums-pattern-grid .drums-cell.rest >> nth=0");
await page.waitForSelector(".drums-pattern .drums-status.warn");
if ((await target()).notes !== t.notes + 1) throw new Error("a click on an empty step adds no note");
ok("a click on the grid adds a note, and the pattern says it was edited by hand");

// A copy plays at the cursor instead: a variation of its own.
const original = t.id;
await page.click(".drums-target button[title^='Copy']");
t = await target();
if (t.id === original || t.clips.length !== 1) throw new Error(`copy: ${JSON.stringify(t)}`);
ok(`Copy makes ${t.name}, playing here instead`);

// The song strip: a click on another drum clip targets its pattern.
const clips = await page.$$(".drums-map-clip");
if (clips.length < 3) throw new Error(`${clips.length} drum clips on the strip`);
await page.click(".drums-map-clip:not(.on) >> nth=-1");
await until((id) => document.querySelector(".drums-target .select")?.value !== id, t.id, "the strip retargets");
await page.waitForSelector(".drums-pattern .drums-sub:has-text('song drummer')");
ok(`a clip on the song strip targets its pattern (${(await target()).name})`);

// Remove (picked from the tab's list), and undo.
const copy = t.id;
await page.selectOption(".drums-target .select", copy);
await until((id) => document.querySelector(".drums-target .select")?.value === id, copy, "the copy is picked");
await page.click(".drums-target button[title^='Remove']");
await until(() => document.querySelector(".drums-target")?.textContent.includes("Nothing plays drums"), null, "the copy is removed");
await page.keyboard.press("Control+z");
await until(async (id) => (await import("./src/store.js")).state.project.patterns.some((x) => x.id === id), copy, "undo brings the copy back");
ok("a pattern is removed with its clips, and Ctrl+Z brings it back");

// Take over one of the song drummer's patterns: pick a groove for it.
await page.click(".drums-map-clip[title*='Jazz waltz A:'] >> nth=0");
await page.waitForSelector(".drums-pattern .drums-sub:has-text('song drummer')");
const taken = (await target()).id;
const at = (await target()).clips[0].start;
await page.click(".drums-lib-item:not(.misfit):not(.on) >> nth=0");
await until(
  async (id) => {
    const s = (await import("./src/store.js")).state;
    const pat = s.project.patterns.find((x) => x.id === id);
    return pat !== undefined && pat.drums.on && !s.project.drums.written.some((w) => w.id === id);
  },
  taken,
  "the pattern is taken over"
);
ok(`picking a groove for ${taken} takes it over from the song drummer`);

// The song drummer folds out underneath; writing again leaves the taken-over bars to it.
await page.click(".drums-arrange-head");
await page.waitForSelector(".drums-sec");
await page.click(".drums-writebtn");
await page.waitForSelector(".toast:has-text('left to your own drum patterns')");
const playing = await page.evaluate(async (beat) => {
  const s = (await import("./src/store.js")).state;
  return s.project.playlist.clips.filter((c) => c.pattern !== "" && c.start <= beat + 1e-6 && beat < c.start + c.length - 1e-6).map((c) => c.pattern);
}, at);
const drumsThere = playing.filter((id) => id === taken || id.startsWith("drums"));
if (drumsThere.length !== 1 || drumsThere[0] !== taken) throw new Error(`at beat ${at}: ${playing.join(", ")}`);
ok("the song drummer opens underneath, and writing again leaves the taken-over bars to that pattern");

if (errors.length > 0) throw new Error(`page errors: ${errors.join("\n")}`);
console.log("all drums checks passed");
await browser.close();
