// Tests for the project model's wire format (model.js), run with:
// node web/test/model.test.js (also type-checked by inty via web/check.sh).
import { emptyProject, decodeProject, projectJson } from "../src/model.js";

let failures = 0;
/** function check(name: String, ok: Boolean) => Undefined */
function check(name, ok) {
  if (!ok) failures = failures + 1;
  console.log(`${ok ? "ok  " : "FAIL"} ${name}`);
}

// A project without a drum part writes none.
const bare = emptyProject();
check("no drum part, no drums key", !projectJson(bare).includes('"drums"'));
check("no drum part decodes as off", decodeProject(JSON.parse(projectJson(bare))).drums.on === false);

// A drum part survives a round trip (the studio re-encodes the project on every edit).
const p = emptyProject();
p.drums = {
  on: true,
  groove: "rock-8ths",
  kit: "Ebony",
  feel: "loose",
  swing: 0.3,
  start: 5,
  ending: "none",
  variations: false,
  seed: 7,
  sections: [
    { name: "Verse", bars: 8, play: "a", fill: "beat", crash: false, groove: "" },
    { name: "Bridge", bars: 4, play: "b", fill: "none", crash: true, groove: "rock-halftime" },
  ],
  grooves: [{ groove: "rock-8ths", a: [["kick", "X... ...."]], b: [] }],
  kept: [{ slot: "rock-8ths/a+crash", name: "Straight 8ths A + crash", notes: [{ role: "snare", start: 1, length: 0.25, velocity: 0.5 }] }],
  written: [{ id: "drums-straight-8ths-a", slot: "rock-8ths/a", print: "0badf00d" }],
};
const text = projectJson(p);
const back = decodeProject(JSON.parse(text));
check("drum part round-trips", JSON.stringify(back.drums) === JSON.stringify(p.drums));
const wire = JSON.parse(text).drums;
check("defaults are left out of sections", wire.sections[0].crash === undefined && wire.sections[1].fill === undefined);
check(
  "written maps pattern ids to slot and fingerprint",
  wire.written["drums-straight-8ths-a"].print === "0badf00d" && wire.written["drums-straight-8ths-a"].slot === "rock-8ths/a"
);
check("kept patterns are keyed by slot", wire.kept["rock-8ths/a+crash"].notes[0].role === "snare");
check("groove edits are keyed by groove", wire.grooves["rock-8ths"].a[0][1] === "X... ....");

// The server's defaults fill a sparse part.
const sparse = decodeProject(JSON.parse(projectJson(emptyProject())));
const raw = JSON.parse(projectJson(sparse));
raw.drums = { groove: "house", sections: [{ bars: 4 }] };
const d = decodeProject(raw).drums;
check(
  "sparse part gets defaults",
  d.on &&
    d.feel === "natural" &&
    d.start === 1 &&
    d.ending === "hit" &&
    d.variations &&
    d.seed === 1 &&
    d.sections[0].play === "a" &&
    d.sections[0].fill === "none"
);

if (failures > 0) {
  console.log(`${failures} model test(s) failed`);
  throw new Error("model tests failed");
}
console.log("all model tests passed");
