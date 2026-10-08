// Tests for who has the engine during a preview, run with: node web/test/preview.test.js
// (also type-checked by inty via web/check.sh).
import { claimPreview, releasePreview, previewOwner, previewTakes } from "../src/preview.js";

let failures = 0;
/** function check(name: String, ok: Boolean) => Undefined */
function check(name, ok) {
  if (!ok) failures = failures + 1;
  console.log(`${ok ? "ok  " : "FAIL"} ${name}`);
}

/** What happened, in order. */
/** const log: String[] */
const log = [];

/** A preview that logs its changes and, when stopped, releases the engine as the panels do. */
/** function claim(owner: String) => Undefined */
function claim(owner) {
  claimPreview(
    owner,
    () => {
      log.push(`${owner} change`);
    },
    () => {
      log.push(`${owner} stop`);
      releasePreview(owner);
    }
  );
}

check("nobody has the engine at first", previewOwner() === "" && !previewTakes());

claim("drums");
check("a preview takes the engine", previewOwner() === "drums");
check("a preview takes the song's changes", previewTakes() && log.join(",") === "drums change");

log.length = 0;
claim("mixcheck");
check("the next preview stops the one before", log.join(",") === "drums stop" && previewOwner() === "mixcheck");
check("the stopped preview's release leaves the new one the engine", previewTakes() && log.join(",") === "drums stop,mixcheck change");

log.length = 0;
releasePreview("drums");
check("only the owner releases the engine", previewOwner() === "mixcheck");

claim("mixcheck");
check("claiming again stops nothing", log.length === 0 && previewOwner() === "mixcheck");

releasePreview("mixcheck");
check("released, the song's changes go to the engine", previewOwner() === "" && !previewTakes() && log.length === 0);

if (failures > 0) throw new Error(`${failures} test(s) failed`);
console.log("all preview tests passed");
