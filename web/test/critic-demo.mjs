// The Critic on the demo song (run with: node web/test/critic-demo.mjs): it
// stays quiet about the things the demo does well, runs fast, and every fix
// it offers makes its own finding go away.
import { readFileSync } from "node:fs";
import { decodeProject, cloneProject } from "../src/model.js";
import { critique } from "../src/critic.js";

const demo = new URL("../../crates/studio/assets/demo/project.json", import.meta.url);
const p = decodeProject(JSON.parse(readFileSync(demo, "utf8")));
let failures = 0;
function check(name, ok) {
  if (!ok) failures++;
  console.log(`${ok ? "ok  " : "FAIL"} ${name}`);
}

const t0 = performance.now();
const found = critique(p, []);
const ms = performance.now() - t0;
check(`the demo is critiqued in ${Math.round(ms)} ms (under 500)`, ms < 500);
check(`the demo has few findings (${found.length})`, found.length > 0 && found.length < 40);
for (const rule of [
  "flat-velocity",
  "loopitis",
  "no-contrast",
  "empty-playlist",
  "not-routed",
  "all-center",
  "no-limiter",
  "master-hot",
  "lowend-panned",
  "sub-too-low",
])
  check(`the demo is not flagged for ${rule}`, !found.some((f) => f.rule === rule));

for (const f of found.filter((x) => x.fix !== "")) {
  const q = cloneProject(p);
  const g = critique(q, []).find((x) => x.key === f.key);
  g.apply(q);
  check(`"${f.fix}" resolves ${f.key}`, !critique(q, []).some((x) => x.key === f.key));
}

if (failures > 0) throw new Error(`${failures} critic demo check(s) failed`);
console.log("all critic demo checks passed");
