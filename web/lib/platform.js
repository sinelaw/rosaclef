// Rosaclef platform layer — the thin FFI boundary between inty-checked
// application code (web/src) and browser APIs. Types: web/types/platform.d.js.
//
// Keep this file small and free of application logic: it only adapts APIs.

import { Terminal } from "../vendor/xterm/xterm.mjs";
import { FitAddon } from "../vendor/xterm/addon-fit.mjs";
import { WebLinksAddon } from "../vendor/xterm/addon-web-links.mjs";
import { backend, request, localSocket, resolveUrl } from "./backend.js";

// ------------------------------------------------------------------ events

function isTyping(target) {
  if (!target || !target.closest) return false;
  const tag = (target.tagName || "").toLowerCase();
  return tag === "input" || tag === "textarea" || tag === "select" || target.isContentEditable || !!target.closest(".xterm");
}

function enrich(e) {
  if (e.__rc) return e;
  const t = e.currentTarget && e.currentTarget.getBoundingClientRect ? e.currentTarget : null;
  const r = t ? t.getBoundingClientRect() : null;
  const src = e.target || {};
  const extra = {
    typing: isTyping(e.target),
    onControl: !!(src.closest && src.closest("button, select, input, textarea, a, .knob, .fader, .lcd")),
    value: src.value !== undefined ? String(src.value) : "",
    checked: !!src.checked,
    targetLeft: r ? r.left : 0,
    targetTop: r ? r.top : 0,
    targetWidth: r ? r.width : 0,
    targetHeight: r ? r.height : 0,
    scrollLeft: t ? t.scrollLeft : 0,
    scrollTop: t ? t.scrollTop : 0,
  };
  for (const k of Object.keys(extra)) {
    try {
      Object.defineProperty(e, k, { value: extra[k], configurable: true });
    } catch (_) {
      /* ignore */
    }
  }
  try {
    Object.defineProperty(e, "__rc", { value: true });
  } catch (_) {
    /* ignore */
  }
  return e;
}

function wrap(fn) {
  return (e) => fn(enrich(e));
}

export function listen(target, type, fn) {
  const opts = type === "wheel" || type.startsWith("touch") ? { passive: false } : undefined;
  target.addEventListener(type, wrap(fn), opts);
}

export function listenWindow(type, fn) {
  window.addEventListener(type, wrap(fn));
}

export function capturePointer(el, id) {
  try {
    el.setPointerCapture(id);
  } catch (_) {
    /* ignore */
  }
}

export const now = () => performance.now();

// ------------------------------------------------------------------ canvas

/** Size a canvas for the device pixel ratio and return its 2D context. */
export function canvas2d(canvas, width, height) {
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(1, Math.floor(width * dpr));
  const h = Math.max(1, Math.floor(height * dpr));
  if (canvas.width !== w) canvas.width = w;
  if (canvas.height !== h) canvas.height = h;
  canvas.style.width = width + "px";
  canvas.style.height = height + "px";
  const ctx = canvas.getContext("2d");
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  if (!ctx.fillGradient) {
    ctx.fillGradient = (g) => {
      ctx.fillStyle = g;
    };
    ctx.strokeGradient = (g) => {
      ctx.strokeStyle = g;
    };
  }
  if (!ctx.roundRect) {
    ctx.roundRect = (x, y, w2, h2) => ctx.rect(x, y, w2, h2);
  }
  return ctx;
}

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
  return new Date(ms).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
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

export function createTerm(el, onData) {
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
    message: m.message || "",
    sampleRate: m.sampleRate || 0,
  };
}

/** Create the AudioContext + engine worklet. Resolves with the sample rate. */
export async function audioStart(workletUrl, wasmUrl, onMsg) {
  ctx = new AudioContext({ latencyHint: "interactive" });
  await ctx.audioWorklet.addModule(workletUrl);
  node = new AudioWorkletNode(ctx, "rosaclef", { numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [2] });
  node.connect(ctx.destination);
  const wasm = await (await fetch(wasmUrl)).arrayBuffer();
  const ready = new Promise((resolve) => {
    node.port.onmessage = (e) => {
      const m = e.data;
      if (m.t === "rec") {
        if (recording) recChunks.push([m.left, m.right]);
        return;
      }
      if (m.t === "ready") resolve(m.sampleRate);
      onMsg(normalizeMsg(m));
    };
  });
  node.port.postMessage({ t: "init", wasm }, [wasm]);
  return ready;
}

export function audioPost(msg) {
  if (node) node.port.postMessage(msg);
}

export function audioPostSample(path, decoded) {
  if (!node) return;
  const channels = decoded.channels.map((c) => new Float32Array(c));
  node.port.postMessage(
    { t: "sample", path, sampleRate: decoded.sampleRate, channels },
    channels.map((c) => c.buffer),
  );
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

/** Track a pointer gesture on the window until the button is released. */
export function drag(start, onMove, onUp) {
  // Follow only the pointer that started the drag: other fingers on a touch
  // screen are drags of their own.
  const mine = (e) => start.pointerId === undefined || e.pointerId === start.pointerId;
  const move = wrap((e) => {
    if (mine(e)) onMove(e);
  });
  const up = wrap((e) => {
    if (!mine(e)) return;
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", up);
    onUp(e);
  });
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", up);
}

// A finger on a scrollable editor scrolls it; only a tap (lifted without
// moving) acts, as a click there would. Mouse and pen act at once.
export function pressOrTap(e, act) {
  if (e.pointerType !== "touch") {
    act(e);
    return;
  }
  const id = e.pointerId;
  const x0 = e.clientX;
  const y0 = e.clientY;
  const done = (u) => {
    if (u.pointerId !== id) return;
    window.removeEventListener("pointerup", done);
    window.removeEventListener("pointercancel", done);
    if (u.type !== "pointerup" || Math.hypot(u.clientX - x0, u.clientY - y0) > 10) return;
    act(e);
    // The finger is already up: end any drag the action started, where it began.
    window.dispatchEvent(new PointerEvent("pointerup", { clientX: x0, clientY: y0, pointerId: id, pointerType: "touch" }));
  };
  window.addEventListener("pointerup", done);
  window.addEventListener("pointercancel", done);
}

export function debounce(ms, fn) {
  let t = 0;
  return () => {
    clearTimeout(t);
    t = setTimeout(fn, ms);
  };
}

// ------------------------------------------------------------------ UI backend

/** The DOM backend for web/src/ui/tree.js: primitive operations on handles. */
export function domBackend(rootId) {
  const nodes = [document.getElementById(rootId)];
  const texts = [];
  const SVG = new Set(["svg", "path", "circle", "g", "line", "rect", "polyline", "defs", "linearGradient", "radialGradient", "stop"]);
  return {
    root: () => 0,
    create: (type) => {
      const el = SVG.has(type) ? document.createElementNS("http://www.w3.org/2000/svg", type) : document.createElement(type);
      nodes.push(el);
      return nodes.length - 1;
    },
    setText: (h, s) => {
      let t = texts[h];
      if (!t) {
        t = document.createTextNode("");
        texts[h] = t;
        nodes[h].insertBefore(t, nodes[h].firstChild);
      }
      t.data = s;
    },
    setClass: (h, c) => {
      nodes[h].setAttribute("class", c);
    },
    setAttr: (h, k, v) => {
      nodes[h].setAttribute(k, v);
    },
    removeAttr: (h, k) => {
      nodes[h].removeAttribute(k);
    },
    setStyle: (h, k, v) => {
      nodes[h].style.setProperty(k, v);
    },
    setProp: (h, k, v) => {
      const el = nodes[h];
      if (k === "checked" || k === "disabled") el[k] = v === "true";
      else if (k === "focus") {
        if (v === "true") setTimeout(() => el.focus(), 0);
      } else if (k === "scrollLeft" || k === "scrollTop") {
        // Scrolling a node that is not in the document yet is ignored.
        if (el.isConnected) el[k] = Number(v);
        else
          requestAnimationFrame(() => {
            el[k] = Number(v);
          });
      } else if (el[k] !== v) el[k] = v;
    },
    append: (p, c) => {
      nodes[p].appendChild(nodes[c]);
    },
    remove: (h) => {
      const el = nodes[h];
      if (el) el.remove();
      nodes[h] = null;
      texts[h] = null;
    },
    listen: (h, event, fn) => {
      const el = nodes[h];
      if (event === "resize") {
        const ro = new ResizeObserver(() => fn(enrich({ currentTarget: el, target: el, preventDefault() {}, stopPropagation() {} })));
        ro.observe(el);
        return;
      }
      if (event === "mount") {
        setTimeout(() => fn(enrich({ currentTarget: el, target: el, preventDefault() {}, stopPropagation() {} })), 0);
        return;
      }
      const passive = event === "wheel" ? { passive: false } : undefined;
      el.addEventListener(event, (e) => fn(enrich(e)), passive);
    },
    paint: (h, fn) => {
      const el = nodes[h];
      const w = el.clientWidth;
      const hh = el.clientHeight;
      if (w === 0 || hh === 0) return;
      const ctx = canvas2d(el, w, hh);
      ctx.clearRect(0, 0, w, hh);
      fn(ctx, w, hh);
    },
    frame: (fn) => {
      requestAnimationFrame(() => fn());
    },
  };
}
