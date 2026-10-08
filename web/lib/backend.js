// Where the studio's requests go: the Rosaclef server, or — in the static,
// browser-only build — the studio back end running in a worker
// (web/local/worker.js, the server's logic compiled to WebAssembly).
//
// Part of the platform layer (unchecked): web/lib/platform.js routes its
// network calls through here, so the application code is the same in both.
//
// The back end is chosen by, in order:
//   ?backend=local|server in the page URL,
//   <meta name="rosaclef-backend" content="local"> (the static build),
//   whether GET api/info answers like the server.

let mode = "";
let port = null;
let seq = 0;
const pending = new Map();
/** chan → handlers of the page's (single) socket on that channel */
const sockets = new Map();

function choose() {
  const param = new URLSearchParams(location.search).get("backend");
  if (param === "local" || param === "server") return Promise.resolve(param);
  const meta = document.querySelector('meta[name="rosaclef-backend"]');
  if (meta && (meta.content === "local" || meta.content === "server")) return Promise.resolve(meta.content);
  return fetch("api/info", { cache: "no-store" })
    .then((r) => (r.ok ? r.json() : null))
    .then((j) => (j && j.version ? "server" : "local"))
    .catch(() => "local");
}

/** Resolves with "server" or "local" once the back end is known. */
export const backend = choose().then((m) => {
  mode = m;
  if (m === "local") startWorker();
  return m;
});

export function isLocal() {
  return mode === "local";
}

/** The jobs this page started on the browser back end (`/api/jobs`), by id:
 * how far each has come (the worker says as it goes), then how it ended. */
const jobs = new Map();

/** POST /api/jobs/<kind>, served here: the worker runs POST /api/<kind> (busy
 * until it ends) and tells how far it has come, and GET /api/jobs/<id> is
 * answered from what it said — the same exchange as with the server. */
function startLocalJob(kind, body) {
  // Random: the tabs share the worker, and it tells every tab of every job.
  const id = 1 + Math.floor(Math.random() * 4294967294);
  jobs.set(id, { id, active: false, what: kind, stage: "", done: 0, render: 0, seconds: 0, total: 0, state: "running" });
  // Forget the oldest finished ones.
  for (const [k, j] of jobs) if (jobs.size > 32 && j.state !== "running") jobs.delete(k);
  localRequest("POST", `/api/${kind}?job=${id}`, body).then((r) => {
    const j = jobs.get(id);
    if (!j) return;
    const text = new TextDecoder().decode(r.body);
    if (r.status >= 200 && r.status < 300) Object.assign(j, { state: "done", result: text ? JSON.parse(text) : {} });
    else Object.assign(j, { state: "failed", status: r.status, error: text || String(r.status) });
  });
  return { status: 202, type: "application/json", body: JSON.stringify({ job: id }) };
}

/** GET /api/jobs/<id>, served here. */
function localJob(id) {
  const j = jobs.get(id);
  if (!j) return { status: 404, type: "text/plain", body: `no job ${id}` };
  return { status: 200, type: "application/json", body: JSON.stringify(j) };
}

function startWorker() {
  const url = new URL("local/worker.js", document.baseURI);
  if (typeof SharedWorker !== "undefined") {
    const w = new SharedWorker(url, { name: "rosaclef" });
    port = w.port;
    port.start();
  } else {
    port = new Worker(url);
  }
  port.onmessage = (e) => onWorker(e.data);
  window.addEventListener("pagehide", (e) => {
    if (!e.persisted) port.postMessage({ t: "bye" });
  });
  // Ask the browser not to evict the projects under storage pressure.
  if (navigator.storage && navigator.storage.persist) navigator.storage.persist().catch(() => false);
}

function onWorker(m) {
  if (m.t === "res") {
    const p = pending.get(m.id);
    pending.delete(m.id);
    if (p) p({ status: m.status, type: m.type, body: m.body });
  } else if (m.t === "msg") {
    const s = sockets.get(m.chan);
    if (!s) return;
    if (m.raw) s.onBinary(new TextEncoder().encode(m.text));
    else s.onText(m.text);
  } else if (m.t === "progress") {
    const j = jobs.get(m.job.id);
    if (j) Object.assign(j, m.job);
  } else if (m.t === "fatal") {
    console.error("Rosaclef back end:", m.message);
    for (const s of sockets.values()) s.onText(JSON.stringify({ t: "error", message: m.message }));
  }
}

/** A request to the browser back end: resolves with { status, type, body: ArrayBuffer }. */
export async function localRequest(method, url, body) {
  await backend;
  let data = null;
  if (body instanceof Blob) data = await body.arrayBuffer();
  else if (typeof body === "string") data = new TextEncoder().encode(body).buffer;
  else if (body instanceof ArrayBuffer) data = body;
  const id = ++seq;
  const res = new Promise((resolve) => pending.set(id, resolve));
  port.postMessage({ t: "req", id, method, url, body: data }, data ? [data] : []);
  return res;
}

// The server can have several projects open, one per tab: a tab's requests
// name its project by the key the server gave it (`/s/<key>/api/...`), and
// reach no other project. "" = the server's home project, until the first
// welcome names it; the browser-only studio has one project and no keys.
let scope = "";

/** Name the project this tab's requests are for (its key from the server). */
export function setScope(key) {
  scope = key;
}

/** `url` for this tab's project: API calls, project files and sockets get its
 * prefix. Opening a project names it by itself (it works whatever the tab had). */
export function scoped(url) {
  if (scope === "" || url.startsWith("/s/") || url === "/api/projects/open") return url;
  if (url.startsWith("/api/") || url.startsWith("/files/") || url.startsWith("/ws")) return `/s/${scope}${url}`;
  return url;
}

/** Like fetch, for either back end: { ok, status, text(), json(), blob() }. */
export async function request(method, url, body, headers) {
  const m = await backend;
  if (m === "server") {
    const r = await fetch(scoped(url), { method, headers, body });
    return r;
  }
  const started = method === "POST" && url.match(/^\/api\/jobs\/([a-z]+)$/);
  const asked = method === "GET" && url.match(/^\/api\/jobs\/(\d+)$/);
  const r = started ? startLocalJob(started[1], body) : asked ? localJob(Number(asked[1])) : await localRequest(method, url, body);
  if (typeof r.body === "string") r.body = new TextEncoder().encode(r.body).buffer;
  return {
    ok: r.status >= 200 && r.status < 300,
    status: r.status,
    text: async () => new TextDecoder().decode(r.body),
    json: async () => JSON.parse(new TextDecoder().decode(r.body)),
    blob: async () => new Blob([r.body], { type: r.type }),
    arrayBuffer: async () => r.body,
  };
}

/** A socket-like channel to the browser back end (`ws` or `term`). */
export function localSocket(chan, h) {
  let open = true;
  sockets.set(chan, h);
  backend.then(() => {
    port.postMessage({ t: "open", chan });
    h.onOpen();
  });
  return {
    send: (s) => {
      if (open) port.postMessage({ t: "send", chan, text: s });
    },
    close: () => {
      open = false;
      if (sockets.get(chan) === h) sockets.delete(chan);
    },
    isOpen: () => open,
  };
}

/** Is `url` a file of the open project or an API call (served by the back end)? */
export function isBackendUrl(url) {
  return url.startsWith("/files/") || url.startsWith("/api/");
}

/** A URL the browser can load for a back-end resource (an object URL when local). */
export async function resolveUrl(url) {
  const m = await backend;
  if (m === "server") return scoped(url);
  if (!isBackendUrl(url)) return url;
  const r = await localRequest("GET", url, null);
  if (r.status !== 200) throw new Error(new TextDecoder().decode(r.body) || `${r.status}`);
  return URL.createObjectURL(new Blob([r.body], { type: r.type }));
}
