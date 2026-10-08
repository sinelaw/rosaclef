// Rosaclef platform layer — the thin FFI boundary between inty-checked
// application code (web/src) and browser APIs. Types: web/types/platform.d.js.
//
// Keep this file small and free of application logic: it only adapts APIs.

import { Terminal } from "../vendor/xterm/xterm.mjs";
import { FitAddon } from "../vendor/xterm/addon-fit.mjs";
import { WebLinksAddon } from "../vendor/xterm/addon-web-links.mjs";
import { backend, request, localSocket, resolveUrl, isLocal } from "./backend.js";

// The DOM backend, the event layer, gestures and canvas sizing live with the
// tree UI library (web/tree/dom.js).
export { listen, listenWindow, capturePointer, pixelRatio, canvas2d, drag, pressOrTap, domBackend } from "../tree/dom.js";

export const now = () => performance.now();

// ------------------------------------------------------------------ network

export function wsUrl(path) {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}${path}`;
}

export function connectRaw(url, h) {
  // The back end is chosen asynchronously: until then, sends are dropped
  // (as on a socket that is still connecting).
  let inner = null;
  let closed = false;
  backend.then((m) => {
    if (closed) return;
    if (m === "local") {
      inner = localSocket(new URL(url).pathname.endsWith("/term") ? "term" : "ws", h);
      return;
    }
    const ws = new WebSocket(url);
    ws.binaryType = "arraybuffer";
    ws.onopen = () => h.onOpen();
    ws.onclose = () => h.onClose();
    ws.onmessage = (e) => {
      if (typeof e.data === "string") h.onText(e.data);
      else h.onBinary(new Uint8Array(e.data));
    };
    inner = {
      send: (s) => {
        if (ws.readyState === 1) ws.send(s);
      },
      close: () => ws.close(),
      isOpen: () => ws.readyState === 1,
    };
  });
  return {
    send: (s) => {
      if (inner) inner.send(s);
    },
    close: () => {
      closed = true;
      if (inner) inner.close();
    },
    isOpen: () => !!inner && inner.isOpen(),
  };
}

/** A file of the site itself (a license, a readme), in either back end. */
export async function siteText(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${r.status}`);
  return r.text();
}

export async function getJson(url) {
  const r = await request("GET", url);
  if (!r.ok) throw new Error(`${r.status} ${await r.text()}`);
  return r.json();
}

export async function sendJson(url, method, body) {
  const r = await request(method, url, JSON.stringify(body), { "content-type": "application/json" });
  const text = await r.text();
  if (!r.ok) throw new Error(text || `${r.status}`);
  return text ? JSON.parse(text) : {};
}

export async function uploadFile(url, file) {
  const r = await request("POST", url, file);
  if (!r.ok) throw new Error(await r.text());
  return r.json();
}

/** Run a long request (`kind`: "mixcheck", "render") as a job of the back
 * end: it answers at once with the job's id (POST /api/jobs/<kind>), and the
 * page asks how far the job has come (GET /api/jobs/<id>) until it ends.
 * `step` hears each answer, the first with just the id; resolves with the
 * job's result. */
export async function runJob(kind, body, step) {
  const { job } = await sendJson(`/api/jobs/${kind}`, "POST", body);
  step({ id: job, active: false, what: kind, stage: "", done: 0, render: 0, seconds: 0, total: 0, state: "running" });
  let misses = 0;
  for (;;) {
    await new Promise((r) => setTimeout(r, 200));
    let j;
    try {
      j = await getJson(`/api/jobs/${job}`);
      misses = 0;
    } catch (e) {
      // A busy server can miss an answer; one gone fails the job.
      if (++misses >= 5) throw e;
      continue;
    }
    step(j);
    if (j.state === "done") return j.result;
    if (j.state === "failed") throw new Error(j.error || String(j.status));
  }
}

/** "server" or "local" (the browser-only studio), once known. */
export function backendMode() {
  return backend;
}

/** How much of the browser's storage the studio uses: { usage, quota } in bytes (0 when unknown). */
export async function storageEstimate() {
  try {
    const e = await navigator.storage.estimate();
    return { usage: e.usage || 0, quota: e.quota || 0 };
  } catch (_) {
    return { usage: 0, quota: 0 };
  }
}

export function onFileDrop(el, fn) {
  el.addEventListener("dragover", (e) => {
    if (e.dataTransfer && e.dataTransfer.types.includes("Files")) e.preventDefault();
  });
  el.addEventListener("drop", (e) => {
    if (!e.dataTransfer || e.dataTransfer.files.length === 0) return;
    e.preventDefault();
    const rect = el.getBoundingClientRect();
    fn(Array.from(e.dataTransfer.files), e.clientX - rect.left, e.clientY - rect.top);
  });
}

export function pickFiles(accept, fn) {
  const input = document.createElement("input");
  input.type = "file";
  input.accept = accept;
  input.multiple = true;
  input.onchange = () => fn(Array.from(input.files || []));
  input.click();
}

// One <audio> element for previewing files (the Projects window).
let preview = null;

export function previewAudio(url, onEnd) {
  stopPreview();
  const a = document.createElement("audio");
  a.preload = "auto";
  preview = a;
  let src = "";
  const done = () => {
    if (preview === a) preview = null;
    if (src.startsWith("blob:")) URL.revokeObjectURL(src);
    onEnd();
  };
  a.onended = done;
  a.onerror = done;
  resolveUrl(url)
    .then((u) => {
      src = u;
      if (preview !== a) return done();
      a.src = u;
      return a.play();
    })
    .catch(done);
}

export function stopPreview() {
  if (preview) {
    const a = preview;
    preview = null;
    a.pause();
    a.removeAttribute("src");
  }
}

/** The current time as an ISO 8601 string (inty trips over `new Date()`). */
export function nowIso() {
  return new Date().toISOString();
}

export function fmtDate(ms) {
  return new Date(ms).toLocaleString(uiLocale || undefined, { dateStyle: "medium", timeStyle: "short" });
}

// ------------------------------------------------------------------ translations

/** The interface's texts by key: the language shown, the English under it, and the locale ("" = the browser's). */
let messages = new Map();
let baseMessages = new Map();
let uiLocale = "";

async function fetchMessages(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${r.status}`);
  const o = await r.json();
  const m = new Map();
  for (const k of Object.keys(o)) {
    if (typeof o[k] === "string" && o[k] !== "") m.set(k, o[k]);
  }
  return m;
}

export async function loadMessages(url) {
  messages = await fetchMessages(url);
  return messages.size;
}

export async function loadBaseMessages(url) {
  baseMessages = await fetchMessages(url);
  return baseMessages.size;
}

export function clearMessages() {
  messages = new Map();
}

export function message(key) {
  return messages.get(key) ?? baseMessages.get(key) ?? key;
}

export function setUiLocale(code) {
  uiLocale = code;
  document.documentElement.lang = code;
}

export function browserLanguages() {
  const l = navigator.languages && navigator.languages.length > 0 ? navigator.languages : [navigator.language || "en"];
  return l.map((x) => String(x));
}

export function download(url, name) {
  resolveUrl(url)
    .then((u) => {
      const a = document.createElement("a");
      a.href = u;
      a.download = name;
      document.body.appendChild(a);
      a.click();
      a.remove();
      if (u.startsWith("blob:")) setTimeout(() => URL.revokeObjectURL(u), 60000);
    })
    .catch((e) => console.error("download failed", e));
}

// ------------------------------------------------------------------ terminal

/** A terminal in the element with this id. */
export function createTerm(id, onData) {
  const el = document.getElementById(id);
  const term = new Terminal({
    fontFamily: '"JetBrains Mono", "SF Mono", Menlo, Consolas, monospace',
    fontSize: 12.5,
    lineHeight: 1.2,
    cursorBlink: true,
    cursorStyle: "bar",
    allowProposedApi: true,
    scrollback: 5000,
    theme: {
      background: "#0b0a0d",
      foreground: "#e9dfcb",
      cursor: "#e3c47a",
      cursorAccent: "#0b0a0d",
      selectionBackground: "rgba(212,175,55,0.28)",
      black: "#1a1720",
      red: "#d9707e",
      green: "#8fbf9a",
      yellow: "#e3c47a",
      blue: "#7f9fd6",
      magenta: "#c98bc4",
      cyan: "#7cc3c1",
      white: "#e9dfcb",
      brightBlack: "#5d5566",
      brightRed: "#f08c99",
      brightGreen: "#a9d8b3",
      brightYellow: "#f3d894",
      brightBlue: "#9fbaf0",
      brightMagenta: "#e2a8dd",
      brightCyan: "#9fe0dd",
      brightWhite: "#fff8ea",
    },
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.loadAddon(new WebLinksAddon());
  term.open(el);
  term.onData((d) => onData(d));
  return {
    write: (b) => term.write(b),
    writeText: (s) => term.write(s),
    fit: () => {
      try {
        fit.fit();
      } catch (_) {
        /* hidden */
      }
    },
    cols: () => term.cols,
    rows: () => term.rows,
    focus: () => term.focus(),
    clear: () => term.clear(),
    reset: () => term.reset(),
  };
}

// ------------------------------------------------------------------ audio

let ctx = null;
let node = null;
let mic = null;
let micSource = null;
let recChunks = [];
let recording = false;

function normalizeMsg(m) {
  return {
    t: m.t || "",
    position: m.position || 0,
    playing: !!m.playing,
    loopLength: m.loopLength || 0,
    meters: m.meters ? Array.from(m.meters) : [],
    missing: m.missing || [],
    presets: m.presets || [],
    message: m.message || "",
    sampleRate: m.sampleRate || 0,
    // (An engine older than the page reports no `transport`: its statuses are taken as they come.)
    stale: m.t === "status" && m.transport !== undefined && m.transport < transport.sent,
  };
}

/** Create the AudioContext + engine worklet. Resolves with the sample rate. */
export async function audioStart(workletUrl, wasmUrl, onMsg) {
  // "playback" gives the browser a larger output buffer than "interactive":
  // a render call that runs late (GC, a busy core) no longer drops out.
  // Notes played live respond a little later; the native output is the
  // low-latency path.
  ctx = new AudioContext({ latencyHint: "playback" });
  await ctx.audioWorklet.addModule(workletUrl);
  node = new AudioWorkletNode(ctx, "rosaclef", { numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [2] });
  node.connect(ctx.destination);
  // Revalidate: an engine cached from an older deploy rejects what a newer
  // importer writes (an effect type it doesn't know yet).
  const wasm = await (await fetch(wasmUrl, { cache: "no-cache" })).arrayBuffer();
  const ready = new Promise((resolve) => {
    node.port.onmessage = (e) => {
      const m = e.data;
      if (m.t === "rec") {
        if (recording) recChunks.push([m.left, m.right]);
        return;
      }
      if (m.t === "presetAck") {
        const ack = presetAcks.shift();
        if (ack) ack(m.ok);
        return;
      }
      if (m.t === "ready") resolve(m.sampleRate);
      onMsg(normalizeMsg(m));
    };
  });
  node.port.postMessage({ t: "init", wasm }, [wasm]);
  return ready;
}

// Transport commands (play, pause, stop, seek) are numbered; the engine reports
// the last one it applied with each status. A status sent before the engine
// applied the latest command is stale: where it plays, and whether, are from
// before it.
const transport = { sent: 0 };

export function audioPost(msg) {
  if (!node) return;
  if (msg.t === "play" || msg.t === "pause" || msg.t === "stop" || msg.t === "seek") {
    transport.sent = transport.sent + 1;
    node.port.postMessage({ ...msg, transport: transport.sent });
  } else node.port.postMessage(msg);
}

export function audioPostSample(path, decoded) {
  if (!node) return;
  const channels = decoded.channels.map((c) => new Float32Array(c));
  node.port.postMessage(
    { t: "sample", path, sampleRate: decoded.sampleRate, channels },
    channels.map((c) => c.buffer)
  );
}

// ------------------------------------------------------------------ soundfonts
// Presets of `soundfont` instruments are fetched and decoded by a worker
// (engine/fonts.js), then streamed to the worklet in acknowledged chunks:
// neither the page nor the audio thread does the heavy work.

let fontWorker = null;
let fontIdle = 0;
let fontSeq = 0;
const fontWaiting = new Map();
const presetAcks = [];
let presetChain = Promise.resolve(true);
/** Frames per chunk sent to the worklet (512 kB). */
const PRESET_CHUNK = 1 << 18;

function fontsWorker() {
  clearTimeout(fontIdle);
  if (!fontWorker) {
    fontWorker = new Worker(new URL("engine/fonts.js", document.baseURI));
    fontWorker.onmessage = (e) => {
      const done = fontWaiting.get(e.data.id);
      fontWaiting.delete(e.data.id);
      if (done) done(e.data);
      // Idle for a while: end the worker, which returns its memory.
      if (fontWaiting.size === 0) {
        fontIdle = setTimeout(() => {
          if (fontWaiting.size === 0 && fontWorker) {
            fontWorker.terminate();
            fontWorker = null;
          }
        }, 20000);
      }
    };
  }
  return fontWorker;
}

function decodePreset(font, bank, program) {
  return new Promise((resolve) => {
    const id = ++fontSeq;
    fontWaiting.set(id, resolve);
    fontsWorker().postMessage({ id, font, bank, program });
  });
}

function toWorklet(msg, transfer) {
  return new Promise((resolve) => {
    presetAcks.push(resolve);
    node.port.postMessage(msg, transfer || []);
  });
}

async function streamPreset(font, bank, program, header, samples) {
  if (!(await toWorklet({ t: "presetBegin", font, bank, program, header }))) throw new Error("the engine rejected the preset");
  for (let i = 0; i < samples.length; i++) {
    const s = samples[i];
    for (let at = 0; at < s.length; at += PRESET_CHUNK) {
      // A copy of just this chunk (posting a view would clone its whole buffer).
      const data = s.slice(at, at + PRESET_CHUNK);
      await toWorklet({ t: "presetData", index: i, offset: at, data }, [data.buffer]);
    }
  }
  await toWorklet({ t: "presetEnd" });
  return true;
}

/** Load a soundfont preset into the browser engine (one at a time). */
export function audioLoadPreset(font, bank, program) {
  const run = async () => {
    if (!node) return false;
    const r = await decodePreset(font, bank, program);
    if (!r.ok) throw new Error(r.message);
    return streamPreset(font, bank, program, r.header, r.samples);
  };
  const p = presetChain.then(run, run);
  presetChain = p.catch(() => false);
  return p;
}

export async function audioResume() {
  if (ctx && ctx.state !== "running") await ctx.resume();
  return !!ctx && ctx.state === "running";
}

export function audioRunning() {
  return !!ctx && ctx.state === "running";
}

export async function decodeAudioUrl(url) {
  const r = await request("GET", url);
  if (!r.ok) throw new Error(`${r.status}`);
  const buf = await r.arrayBuffer();
  const ac = ctx || new OfflineAudioContext(2, 1, 48000);
  const audio = await ac.decodeAudioData(buf);
  const channels = [];
  for (let c = 0; c < Math.min(2, audio.numberOfChannels); c++) channels.push(audio.getChannelData(c).slice());
  return { sampleRate: audio.sampleRate, channels, duration: audio.duration };
}

// ------------------------------------------------------------------ recording

export async function recStart() {
  if (!ctx || !node) return false;
  if (!mic) {
    mic = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false } });
    micSource = ctx.createMediaStreamSource(mic);
    micSource.connect(node);
  }
  recChunks = [];
  recording = true;
  node.port.postMessage({ t: "record", on: true });
  return true;
}

function encodeWav(chunks, sampleRate) {
  const frames = chunks.reduce((n, c) => n + c[0].length, 0);
  const buf = new ArrayBuffer(44 + frames * 4);
  const v = new DataView(buf);
  const str = (o, s) => {
    for (let i = 0; i < s.length; i++) v.setUint8(o + i, s.charCodeAt(i));
  };
  str(0, "RIFF");
  v.setUint32(4, 36 + frames * 4, true);
  str(8, "WAVE");
  str(12, "fmt ");
  v.setUint32(16, 16, true);
  v.setUint16(20, 1, true);
  v.setUint16(22, 2, true);
  v.setUint32(24, sampleRate, true);
  v.setUint32(28, sampleRate * 4, true);
  v.setUint16(32, 4, true);
  v.setUint16(34, 16, true);
  str(36, "data");
  v.setUint32(40, frames * 4, true);
  let o = 44;
  for (const [l, r] of chunks) {
    for (let i = 0; i < l.length; i++) {
      v.setInt16(o, Math.max(-1, Math.min(1, l[i])) * 32767, true);
      v.setInt16(o + 2, Math.max(-1, Math.min(1, r[i])) * 32767, true);
      o += 4;
    }
  }
  return new Blob([buf], { type: "audio/wav" });
}

/** Stop recording, upload the take as a WAV, resolve with its project path ("" if empty). */
export async function recStop(name) {
  recording = false;
  if (node) node.port.postMessage({ t: "record", on: false });
  if (recChunks.length === 0 || !ctx) return "";
  const blob = encodeWav(recChunks, ctx.sampleRate);
  recChunks = [];
  const j = await uploadFile(`/api/samples?name=${encodeURIComponent(name)}`, blob);
  return j.path || "";
}

// ------------------------------------------------------------------ misc

export function setTitle(t) {
  document.title = t;
}

/** The address's fragment, without the "#" ("" when none). */
export function urlFragment() {
  return location.hash.replace(/^#/, "");
}

/** Show `frag` as the address's fragment without reloading or a hashchange:
 * a new history entry when `push` (Back returns to the last one), else in place. */
export function setUrlFragment(frag, push) {
  const url = location.pathname + location.search + (frag === "" ? "" : "#" + frag);
  if (url === location.pathname + location.search + location.hash) return;
  try {
    if (push) history.pushState(null, "", url);
    else history.replaceState(null, "", url);
  } catch (_) {
    /* a sandboxed frame: the address stays */
  }
}

export function confirmBox(msg) {
  return window.confirm(msg);
}

export function promptBox(msg, def) {
  const r = window.prompt(msg, def);
  return r === null ? "" : r;
}

/** A small persisted UI preference; "" when absent or storage is unavailable. */
export function loadPref(key) {
  try {
    return localStorage.getItem(key) ?? "";
  } catch (_) {
    return "";
  }
}

/** A stored preference, or `fallback` when none is stored (a stored "" is kept). */
export function loadPrefOr(key, fallback) {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : v;
  } catch (_) {
    return fallback;
  }
}

export function savePref(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch (_) {
    /* private mode, quota: not persisted */
  }
}

export function fmt(n, digits) {
  return Number(n).toFixed(digits);
}

export function debounce(ms, fn) {
  let t = 0;
  return () => {
    clearTimeout(t);
    t = setTimeout(fn, ms);
  };
}

// ------------------------------------------------------------------ PDF

/** Deflate a string (zlib format: PDF's FlateDecode). */
async function deflate(text) {
  const stream = new Blob([new TextEncoder().encode(text)]).stream().pipeThrough(new CompressionStream("deflate"));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

export async function downloadPdf(name, objs) {
  const enc = new TextEncoder();
  const parts = [enc.encode("%PDF-1.5\n%\u00e2\u00e3\u00cf\u00d3\n")];
  const offsets = [];
  let at = parts[0].byteLength;
  for (let i = 0; i < objs.length; i++) {
    const o = objs[i];
    offsets.push(at);
    let chunk;
    if (o.bin) {
      // Bytes already encoded (an image): written as they are.
      const dict = o.head.replace(/>>\s*$/, `/Length ${o.bin.byteLength} >>`);
      chunk = [enc.encode(`${i + 1} 0 obj\n${dict}\nstream\n`), o.bin, enc.encode("\nendstream\nendobj\n")];
    } else if (o.stream !== "") {
      const data = await deflate(o.stream);
      const dict = o.head.replace(/>>\s*$/, `/Length ${data.byteLength} /Filter /FlateDecode >>`);
      chunk = [enc.encode(`${i + 1} 0 obj\n${dict}\nstream\n`), data, enc.encode("\nendstream\nendobj\n")];
    } else chunk = [enc.encode(`${i + 1} 0 obj\n${o.head}\nendobj\n`)];
    for (const c of chunk) {
      parts.push(c);
      at += c.byteLength;
    }
  }
  const xref = [`xref\n0 ${objs.length + 1}\n0000000000 65535 f \n`];
  for (const off of offsets) xref.push(`${String(off).padStart(10, "0")} 00000 n \n`);
  xref.push(`trailer\n<< /Size ${objs.length + 1} /Root 1 0 R /Info ${objs.length} 0 R >>\nstartxref\n${at}\n%%EOF\n`);
  parts.push(enc.encode(xref.join("")));
  const url = URL.createObjectURL(new Blob(parts, { type: "application/pdf" }));
  download(url, name);
  return true;
}

let measureCtx = null;
/** An SVG document drawn into a canvas of `pw` by `ph` pixels, as JPEG bytes. */
async function svgJpeg(svg, pw, ph) {
  const url = URL.createObjectURL(new Blob([svg], { type: "image/svg+xml" }));
  try {
    const img = new Image();
    img.src = url;
    await img.decode();
    const canvas = document.createElement("canvas");
    canvas.width = pw;
    canvas.height = ph;
    const ctx = canvas.getContext("2d");
    ctx.fillStyle = "#fff";
    ctx.fillRect(0, 0, pw, ph);
    ctx.drawImage(img, 0, 0, pw, ph);
    const blob = await new Promise((done) => canvas.toBlob(done, "image/jpeg", 0.9));
    if (!blob) throw new Error("the page could not be drawn");
    return new Uint8Array(await blob.arrayBuffer());
  } finally {
    URL.revokeObjectURL(url);
  }
}

/** A PDF of pages drawn as images: each SVG (`w` by `h` points, drawn at `scale` pixels a point)
 * becomes a JPEG filling its page. `info` is the document info dictionary. */
export async function downloadImagePdf(name, info, svgs, w, h, scale) {
  const pw = Math.round(w * scale);
  const ph = Math.round(h * scale);
  const objs = [
    { head: "<< /Type /Catalog /Pages 2 0 R >>", stream: "" },
    { head: "", stream: "" },
  ];
  const kids = [];
  for (const svg of svgs) {
    const jpeg = await svgJpeg(svg, pw, ph);
    objs.push({
      head: `<< /Type /XObject /Subtype /Image /Width ${pw} /Height ${ph} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode >>`,
      stream: "",
      bin: jpeg,
    });
    const image = objs.length;
    objs.push({ head: "<< >>", stream: `q ${w} 0 0 ${h} 0 0 cm /Im Do Q` });
    const contents = objs.length;
    objs.push({
      head: `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${w} ${h}] /Resources << /XObject << /Im ${image} 0 R >> >> /Contents ${contents} 0 R >>`,
      stream: "",
    });
    kids.push(`${objs.length} 0 R`);
  }
  objs[1] = { head: `<< /Type /Pages /Kids [${kids.join(" ")}] /Count ${kids.length} >>`, stream: "" };
  objs.push({ head: info, stream: "" });
  return downloadPdf(name, objs);
}

let softwareGl = null;

/** Whether the browser's graphics are emulated on the CPU (SwiftShader, llvmpipe): no GPU to draw on. */
function softwareGraphics() {
  if (softwareGl === null) {
    softwareGl = false;
    try {
      const gl = document.createElement("canvas").getContext("webgl");
      const info = gl && gl.getExtension("WEBGL_debug_renderer_info");
      if (info) softwareGl = /swiftshader|llvmpipe|softpipe|software/i.test(String(gl.getParameter(info.UNMASKED_RENDERER_WEBGL)));
      gl?.getExtension("WEBGL_lose_context")?.loseContext();
    } catch (e) {}
  }
  return softwareGl;
}

/** Whether the film draws smoothly here as it is: Chrome (or another Chromium browser) on a GPU. */
export function fastGraphics() {
  if (typeof navigator === "undefined") return false;
  const brands = navigator.userAgentData && navigator.userAgentData.brands;
  const chromium = Array.isArray(brands)
    ? brands.some((b) => b.brand === "Chromium")
    : /Chrome\//.test(navigator.userAgent) && !/Firefox\//.test(navigator.userAgent);
  return chromium && !softwareGraphics();
}

/** Draw an SVG document into a bitmap of `w` by `h` pixels: an object URL of an image of `type` (image/jpeg, image/png). */
export async function rasterSvg(svg, w, h, type) {
  const url = URL.createObjectURL(new Blob([svg], { type: "image/svg+xml" }));
  try {
    const img = new Image();
    img.src = url;
    await img.decode();
    const canvas = document.createElement("canvas");
    canvas.width = w;
    canvas.height = h;
    // Without a GPU, the canvas's own software drawing is several times faster than an emulated GPU's.
    canvas.getContext("2d", softwareGraphics() ? { willReadFrequently: true } : undefined).drawImage(img, 0, 0, w, h);
    const blob = await new Promise((done) => canvas.toBlob(done, type || "image/jpeg", 0.92));
    if (!blob) throw new Error("the image could not be drawn");
    return URL.createObjectURL(blob);
  } finally {
    URL.revokeObjectURL(url);
  }
}

export { filmDraw, filmLive, filmForget, encodeFilm, renderStill } from "./filmgl.js";

export function dropUrl(url) {
  URL.revokeObjectURL(url);
}

export function toggleFullscreen(selector) {
  if (document.fullscreenElement) {
    document.exitFullscreen().catch(() => {});
    return;
  }
  const el = document.querySelector(selector);
  if (el && el.requestFullscreen) el.requestFullscreen().catch(() => {});
}

// A press on the music (not on a control) takes the keyboard back from a
// text field, a menu or the agent's terminal, so the studio's keys (Space
// plays) work at once.
export function releaseFocus() {
  const el = document.activeElement;
  if (el && el !== document.body && typeof el.blur === "function") el.blur();
}

export function textWidth(face, text) {
  if (!measureCtx) measureCtx = document.createElement("canvas").getContext("2d");
  const style = face.includes("Italic") ? "italic " : "";
  const weight = face.includes("Bold") ? "bold " : "";
  // Liberation Serif and Times New Roman share Times' widths.
  measureCtx.font = `${style}${weight}100px "Times New Roman", "Liberation Serif", Tinos, Times, serif`;
  return measureCtx.measureText(text).width / 100;
}

/** The window's inner size, in CSS pixels. */
export function windowSize() {
  return { width: window.innerWidth, height: window.innerHeight };
}

export function paperSize() {
  const lang = (navigator.language || "").toLowerCase();
  return lang === "en-us" || lang === "en-ca" || lang.endsWith("-us") ? "letter" : "a4";
}
