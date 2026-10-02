// Rosaclef soundfont loader: a worker that turns General MIDI programs into
// decoded presets for the audio engine, away from the page and the audio
// thread (a grand piano is ~15 MB of Ogg Vorbis and ~100 MB decoded).
//
// It runs its own instance of the engine module (rosaclef.wasm, the
// `rc_font_*` functions): it fetches a soundfont's index and the pieces of
// sample data a preset needs (soundfonts/<font>/, see
// tools/split_soundfont.py), decodes the preset and replies with its header
// and samples (transferred, not copied). The page hands them to the worklet
// in small steps. The page ends this worker when it is idle, which returns
// its memory.
//
// Like the worklet, this file is outside inty's reach (worker globals).
//
// Request: { id, font, bank, program }
// Reply:   { id, ok: true, header: ArrayBuffer, samples: Int16Array[] }
//        | { id, ok: false, message }

let wasm = null;
const indexes = new Set();
let queue = Promise.resolve();
const dec = new TextDecoder();
const enc = new TextEncoder();

async function init() {
  if (wasm) return wasm;
  const url = new URL("rosaclef.wasm", self.location.href);
  const { instance } = await WebAssembly.instantiate(await (await fetch(url, { cache: "no-cache" })).arrayBuffer(), {});
  wasm = instance.exports;
  return wasm;
}

/** Copy bytes into the module; returns [ptr, len] (free with rc_free). */
function put(bytes) {
  const ptr = wasm.rc_alloc(bytes.length);
  new Uint8Array(wasm.memory.buffer, ptr, bytes.length).set(bytes);
  return [ptr, bytes.length];
}

function withBytes(bytes, fn) {
  const [ptr, len] = put(bytes);
  try {
    return fn(ptr, len);
  } finally {
    wasm.rc_free(ptr, len);
  }
}

function result() {
  return new Uint8Array(wasm.memory.buffer, wasm.rc_font_result_ptr(), wasm.rc_font_result_len()).slice();
}

async function fetchBytes(path) {
  const res = await fetch(new URL(`../soundfonts/${path}`, self.location.href));
  if (!res.ok) throw new Error(`could not load soundfonts/${path} (${res.status})`);
  return new Uint8Array(await res.arrayBuffer());
}

async function load(font, bank, program) {
  await init();
  const name = enc.encode(font);
  if (!indexes.has(font)) {
    const index = await fetchBytes(`${font}/index.sf2`);
    const ok = withBytes(name, (np, nl) => withBytes(index, (p, l) => wasm.rc_font_index(np, nl, p, l)));
    if (ok !== 0) throw new Error(dec.decode(result()));
    indexes.add(font);
  }
  withBytes(name, (np, nl) => wasm.rc_font_pieces(np, nl, bank, program));
  const pieces = JSON.parse(dec.decode(result()) || "[]");
  const data = await Promise.all(pieces.map((k) => fetchBytes(`${font}/smpl-${String(k).padStart(3, "0")}.bin`)));
  pieces.forEach((k, i) => withBytes(name, (np, nl) => withBytes(data[i], (p, l) => wasm.rc_font_piece(np, nl, k, p, l))));
  data.length = 0;
  const status = withBytes(name, (np, nl) => wasm.rc_font_load(np, nl, bank, program));
  if (status !== 0) {
    const message = dec.decode(result());
    wasm.rc_font_clear();
    throw new Error(message);
  }
  const header = result().buffer;
  const samples = [];
  const n = wasm.rc_font_sample_count();
  for (let i = 0; i < n; i++) {
    const len = wasm.rc_font_sample_len(i);
    samples.push(new Int16Array(wasm.memory.buffer, wasm.rc_font_sample_ptr(i), len).slice());
  }
  wasm.rc_font_clear();
  return { header, samples };
}

self.onmessage = (e) => {
  const { id, font, bank, program } = e.data;
  queue = queue.then(async () => {
    try {
      const { header, samples } = await load(font, bank, program);
      self.postMessage({ id, ok: true, header, samples }, [header, ...samples.map((s) => s.buffer)]);
    } catch (err) {
      self.postMessage({ id, ok: false, message: String(err && err.message ? err.message : err) });
    }
  });
};
