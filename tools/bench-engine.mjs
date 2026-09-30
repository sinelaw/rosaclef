#!/usr/bin/env node
// Measure how the WebAssembly engine keeps up with real time: renders a
// project's song in 128-frame calls (the AudioWorklet's block) and times
// each one against its deadline (128 / sample rate).
//
//   node tools/bench-engine.mjs [PROJECT.json] [ENGINE.wasm] [SAMPLE_RATE]
//
// Defaults: the demo song, web/engine/rosaclef.wasm, 48000 Hz. For a
// per-function profile, build without stripping and run under the profiler:
//   CARGO_PROFILE_WASM_STRIP=false tools/build-wasm.sh
//   node --cpu-prof tools/bench-engine.mjs   (open the .cpuprofile in DevTools)
// Projects that use audio files play them silent (samples are not loaded).
import fs from "node:fs";
import path from "node:path";

const root = path.join(path.dirname(new URL(import.meta.url).pathname), "..");
const [projPath = path.join(root, "crates/studio/assets/demo/project.json"), wasmPath = path.join(root, "web/engine/rosaclef.wasm"), rate = "48000"] =
  process.argv.slice(2);
const SR = Number(rate);
const N = 128;

const { instance } = await WebAssembly.instantiate(fs.readFileSync(wasmPath), {});
const w = instance.exports;
w.rc_init(SR);
const json = fs.readFileSync(projPath);
const ptr = w.rc_alloc(json.length);
new Uint8Array(w.memory.buffer, ptr, json.length).set(json);
if (w.rc_set_project(ptr, json.length) !== 0) {
  console.error("The engine rejected the project.");
  process.exit(1);
}
w.rc_set_mode(0, 0);
w.rc_play();

const bpm = JSON.parse(json.toString()).transport.bpm;
const seconds = (w.rc_loop_length() * 60) / bpm;
const calls = Math.ceil((seconds * SR) / N);
const times = new Float64Array(calls);
for (let i = 0; i < calls; i++) {
  const t = performance.now();
  w.rc_process(N);
  times[i] = performance.now() - t;
}

const budget = (N / SR) * 1000;
const total = times.reduce((a, b) => a + b, 0);
const sorted = Float64Array.from(times).sort();
const at = (q) => sorted[Math.min(calls - 1, Math.floor(q * calls))].toFixed(2);
const late = times.filter((t) => t > budget).length;
// The busiest second decides whether a slower machine keeps up.
const win = Math.round(SR / N);
let busiest = 0;
for (let i = 0; i + win <= calls; i += win) {
  let s = 0;
  for (let j = i; j < i + win; j++) s += times[j];
  busiest = Math.max(busiest, s);
}
console.log(`${seconds.toFixed(1)} s of song, ${calls} calls of ${N} frames at ${SR} Hz (deadline ${budget.toFixed(2)} ms each)`);
console.log(`load: ${((100 * total) / (seconds * 1000)).toFixed(1)}% of one core on average, ${(busiest / 10).toFixed(1)}% in the busiest second`);
console.log(`per call (ms): median ${at(0.5)}, p99 ${at(0.99)}, p99.9 ${at(0.999)}, max ${sorted[calls - 1].toFixed(2)}`);
console.log(`late calls (each one is an audible dropout): ${late} (${((100 * late) / calls).toFixed(2)}%)`);
