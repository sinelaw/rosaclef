// Rosaclef in the browser, without a server: the studio's back end
// (crates/local, compiled to rosaclef-local.wasm) runs in this worker, and
// the files it writes are kept in IndexedDB.
//
// Every open tab talks to one SharedWorker (a plain Worker per tab where
// SharedWorker is missing), just as every tab talks to one server natively:
// an edit in one tab reaches the others. web/lib/backend.js is the page side.
//
// Storage (database "rosaclef"):
//   entries  path → { path, dir, blob, len, modified }   the file tree
//   blobs    blob id → ArrayBuffer                      file contents
// Renames and copies only touch entries; contents are written once. Audio
// is loaded into the module only when something needs it (a render, a
// waveform); files are served to the page straight from storage.
//
// Like the audio worklet, this file is outside inty's reach (worker globals).

const DB_NAME = "rosaclef";
const DB_VERSION = 1;

let db = null;
/** Where contents live when the browser gives no storage (a private window). */
let memory = null;
let storageWarning = "";
let wasm = null;
let bootError = "";
const booted = boot().catch((e) => {
  bootError = String(e && e.message ? e.message : e);
});

/** client id → port */
const ports = new Map();
let nextClient = 1;
/** Calls run one at a time, in order. */
let queue = Promise.resolve();

// ------------------------------------------------------------------ IndexedDB

function openDb() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION);
    req.onupgradeneeded = () => {
      const d = req.result;
      if (!d.objectStoreNames.contains("entries")) d.createObjectStore("entries", { keyPath: "path" });
      if (!d.objectStoreNames.contains("blobs")) d.createObjectStore("blobs");
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
    req.onblocked = () => reject(new Error("the browser's storage is locked by an older version of this page — close other Rosaclef tabs"));
  });
}

function done(tx) {
  return new Promise((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error || new Error("storage transaction aborted"));
  });
}

function result(req) {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function getBlob(id) {
  if (memory) return memory.get(id);
  const tx = db.transaction("blobs", "readonly");
  return result(tx.objectStore("blobs").get(id));
}

/** Persist a call's changes (one transaction, in order). */
async function persist(changes, tail) {
  if (changes.length === 0) return;
  if (memory) {
    for (const c of changes) if (c.op === "file" && c.data) memory.set(c.blob, tail.slice(c.data[0], c.data[0] + c.data[1]).buffer);
    return;
  }
  const tx = db.transaction(["entries", "blobs"], "readwrite");
  const entries = tx.objectStore("entries");
  const blobs = tx.objectStore("blobs");
  for (const c of changes) {
    if (c.op === "file") {
      if (c.data) blobs.put(tail.slice(c.data[0], c.data[0] + c.data[1]).buffer, c.blob);
      entries.put({ path: c.path, dir: false, blob: c.blob, len: c.len, modified: c.modified });
    } else if (c.op === "dir") {
      entries.put({ path: c.path, dir: true, modified: c.modified });
    } else if (c.op === "rm") {
      entries.delete(c.path);
    }
  }
  await done(tx);
}

/** Delete contents no file refers to any more (deleted, or overwritten). */
async function collectGarbage(entries) {
  const used = new Set(entries.filter((e) => !e.dir).map((e) => e.blob));
  const tx = db.transaction("blobs", "readwrite");
  const store = tx.objectStore("blobs");
  const keys = await result(store.getAllKeys());
  for (const k of keys) if (!used.has(k)) store.delete(k);
  await done(tx);
}

// ------------------------------------------------------------------ the module

const enc = new TextEncoder();
const dec = new TextDecoder();

/** One synchronous call into the module: envelope in, envelope out. */
function call(header, body) {
  const h = enc.encode(JSON.stringify({ ...header, now: Date.now() }));
  const b = body ? new Uint8Array(body) : new Uint8Array(0);
  const len = 4 + h.length + b.length;
  const ptr = wasm.rc_alloc(len);
  const mem = new Uint8Array(wasm.memory.buffer, ptr, len);
  new DataView(wasm.memory.buffer, ptr, 4).setUint32(0, h.length, true);
  mem.set(h, 4);
  mem.set(b, 4 + h.length);
  const n = wasm.rc_call(ptr, len);
  wasm.rc_free(ptr, len);
  const out = new Uint8Array(wasm.memory.buffer, wasm.rc_result_ptr(), n).slice();
  wasm.rc_result_free();
  const hl = new DataView(out.buffer).getUint32(0, true);
  const reply = JSON.parse(dec.decode(out.subarray(4, 4 + hl)));
  const tail = out.subarray(4 + hl);
  return { h: reply, body: tail.subarray(0, reply.bodyLen || 0), tail };
}

/** Call, persist what changed, load what is needed and retry. */
async function run(header, body) {
  for (let attempt = 0; attempt < 6; attempt++) {
    const r = call(header, body);
    await persist(r.h.changes || [], r.tail);
    const need = r.h.need || [];
    if (need.length === 0) {
      deliver(r.h.out || []);
      return r;
    }
    for (const id of need) {
      const data = await getBlob(id);
      if (data === undefined) throw new Error(`a file's content is missing from the browser's storage (blob ${id})`);
      call({ op: "provide", blob: id }, data);
    }
  }
  throw new Error("file contents did not load");
}

async function boot() {
  let entries = [];
  try {
    db = await openDb();
    const tx = db.transaction("entries", "readonly");
    entries = await result(tx.objectStore("entries").getAll());
    await collectGarbage(entries);
  } catch (e) {
    // Keep working, in memory only.
    db = null;
    memory = new Map();
    storageWarning = `This browser gives the studio no storage (${e && e.message ? e.message : e}) — a private window? Your work lasts until the last studio tab closes: download projects (Projects → .zip) to keep them.`;
  }
  const url = new URL("rosaclef-local.wasm", self.location.href);
  const { instance } = await WebAssembly.instantiate(await (await fetch(url)).arrayBuffer(), {});
  wasm = instance.exports;
  const r = call({ op: "boot", entries }, null);
  for (const id of r.h.wanted || []) {
    const data = await getBlob(id);
    if (data !== undefined) call({ op: "provide", blob: id }, data);
  }
  const s = await run({ op: "start" }, null);
  if (s.h.status !== 200) throw new Error(dec.decode(s.body));
}

// ------------------------------------------------------------------ pages

function deliver(out) {
  for (const o of out) {
    const msg = { t: "msg", chan: o.chan, text: o.text, raw: o.raw };
    if (o.to !== null && o.to !== undefined) {
      const p = ports.get(o.to);
      if (p) p.postMessage(msg);
    } else {
      for (const [id, p] of ports) if (id !== o.exclude) p.postMessage(msg);
    }
  }
}

function enqueue(fn) {
  const p = queue.then(fn);
  queue = p.catch(() => undefined);
  return p;
}

async function onRequest(client, port, m) {
  try {
    await booted;
    if (bootError) throw new Error(bootError);
    const r = await enqueue(() => run({ op: "request", client, method: m.method, url: m.url }, m.body));
    let body = r.body.slice().buffer;
    if (r.h.blob !== null && r.h.blob !== undefined) {
      const data = await getBlob(r.h.blob);
      if (data === undefined) throw new Error("the file's content is missing from the browser's storage");
      body = data;
    }
    port.postMessage({ t: "res", id: m.id, status: r.h.status, type: r.h.type, body }, [body]);
  } catch (e) {
    const body = enc.encode(String(e && e.message ? e.message : e)).buffer;
    port.postMessage({ t: "res", id: m.id, status: 500, type: "text/plain", body }, [body]);
  }
}

async function onChannel(client, port, m) {
  try {
    await booted;
    if (bootError) throw new Error(bootError);
    if (m.t === "open") {
      await enqueue(() => run({ op: m.chan === "term" ? "term_open" : "ws_open", client }, null));
      if (m.chan === "ws" && storageWarning) port.postMessage({ t: "msg", chan: "ws", text: JSON.stringify({ t: "notice", title: "Projects are not saved", message: storageWarning }), raw: false });
    }
    else if (m.t === "send") await enqueue(() => run({ op: m.chan === "term" ? "term" : "ws", client, text: m.text }, null));
  } catch (e) {
    port.postMessage({ t: "fatal", message: String(e && e.message ? e.message : e) });
  }
}

function attach(port) {
  const client = nextClient++;
  ports.set(client, port);
  port.onmessage = (e) => {
    const m = e.data;
    if (m.t === "req") onRequest(client, port, m);
    else if (m.t === "open" || m.t === "send") onChannel(client, port, m);
    else if (m.t === "bye") {
      ports.delete(client);
      booted.then(() => (wasm ? enqueue(() => run({ op: "close", client }, null)) : undefined)).catch(() => undefined);
    }
  };
  port.postMessage({ t: "hello", client });
}

if (typeof SharedWorkerGlobalScope !== "undefined" && self instanceof SharedWorkerGlobalScope) {
  self.onconnect = (e) => attach(e.ports[0]);
} else {
  attach(self);
}
