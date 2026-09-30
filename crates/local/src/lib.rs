//! The Rosaclef server, compiled to WebAssembly for the browser-only studio.
//!
//! The page's worker (`web/local/worker.js`) owns this module. It restores
//! the file tree from IndexedDB, forwards the studio's requests and socket
//! messages, persists the changes each call reports, and loads file contents
//! the calls ask for. The server logic itself is `rosaclef-studio`, shared
//! with the native server; [`host::Host`] adds the API and socket protocol.
//!
//! # ABI
//!
//! Like the engine module, a minimal C ABI (no wasm-bindgen): the worker
//! writes an *envelope* into memory from `rc_alloc` and calls `rc_call`,
//! which leaves the reply envelope in `rc_result_ptr` / `rc_result_len`.
//!
//! An envelope is `[u32 LE json length][JSON header][bytes]`.
//!
//! Calls (`op` in the header; `now` is the time in ms, `client` a page):
//! - `boot` `{entries: [{path, dir, blob, len, modified}]}` → `{wanted: [blob]}`:
//!   rebuild the tree; the worker then provides the wanted blobs.
//! - `provide` `{blob}` + content.
//! - `start` → open the last project (or create the demo on first use).
//! - `request` `{client, method, url}` + body → an HTTP-like response.
//! - `ws_open`, `ws` `{text}`, `term_open`, `term` `{text}`, `close`.
//!
//! Every reply: `{status, type, bodyLen, blob, need, out, changes}` + the
//! body, then the contents of new blobs:
//! - `need`: blobs to load before calling again (nothing else happened);
//! - `blob`: the response body is this stored blob;
//! - `out`: messages for pages, `{to, exclude, chan, text, raw}`;
//! - `changes`: to persist, in order: `{op: "file", path, blob, len,
//!   modified, data: [offset, length] | null}`, `{op: "dir", path,
//!   modified}`, `{op: "rm", path}` (offsets into the bytes after the body).

pub mod host;
pub mod shell;

use host::Host;
use rosaclef_fs::{Change, MemFs};
use rosaclef_studio::decode::is_audio_file;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

/// The back end: a file tree and, once started, the host.
pub struct Backend {
    mem: Arc<MemFs>,
    host: Option<Host>,
}

impl Default for Backend {
    fn default() -> Self {
        Self::new()
    }
}

fn header(input: &[u8]) -> (Value, &[u8]) {
    if input.len() < 4 {
        return (Value::Null, &[]);
    }
    let n = u32::from_le_bytes([input[0], input[1], input[2], input[3]]) as usize;
    let n = n.min(input.len() - 4);
    (
        serde_json::from_slice(&input[4..4 + n]).unwrap_or(Value::Null),
        &input[4 + n..],
    )
}

/// Build an envelope.
pub fn envelope(h: &Value, bytes: &[u8]) -> Vec<u8> {
    let j = h.to_string();
    let mut out = Vec::with_capacity(4 + j.len() + bytes.len());
    out.extend_from_slice(&(j.len() as u32).to_le_bytes());
    out.extend_from_slice(j.as_bytes());
    out.extend_from_slice(bytes);
    out
}

impl Backend {
    pub fn new() -> Backend {
        Backend {
            mem: Arc::new(MemFs::new()),
            host: None,
        }
    }

    pub fn host(&self) -> Option<&Host> {
        self.host.as_ref()
    }

    /// Handle one envelope; returns the reply envelope.
    pub fn call(&mut self, input: &[u8]) -> Vec<u8> {
        let (h, body) = header(input);
        if let Some(now) = h.get("now").and_then(|n| n.as_f64()) {
            self.mem.set_now(now);
        }
        let op = h.get("op").and_then(|o| o.as_str()).unwrap_or("");
        let client = h.get("client").and_then(|c| c.as_u64()).unwrap_or(0);
        let text = h.get("text").and_then(|t| t.as_str()).unwrap_or("");
        let mut reply = json!({"status": 200, "type": "application/json"});
        let mut resp_body: Vec<u8> = vec![];

        // Terminal state to restore if the call must be retried.
        let shell_before = self
            .host
            .as_ref()
            .and_then(|h| h.shells.get(&client).cloned());

        match (op, self.host.as_mut()) {
            ("boot", _) => {
                for e in h
                    .get("entries")
                    .and_then(|e| e.as_array())
                    .into_iter()
                    .flatten()
                {
                    let path = e.get("path").and_then(|p| p.as_str()).unwrap_or("");
                    let modified = e.get("modified").and_then(|m| m.as_f64()).unwrap_or(0.0);
                    if e.get("dir").and_then(|d| d.as_bool()).unwrap_or(false) {
                        self.mem.restore_dir(path, modified);
                    } else {
                        let blob = e.get("blob").and_then(|b| b.as_u64()).unwrap_or(0);
                        let len = e.get("len").and_then(|l| l.as_u64()).unwrap_or(0);
                        self.mem.restore_file(path, blob, len, modified);
                    }
                }
                // Everything but audio stays in memory.
                reply["wanted"] = json!(self.mem.unloaded(|p| !is_audio_file(Path::new(p))));
            }
            ("provide", _) => {
                if let Some(b) = h.get("blob").and_then(|b| b.as_u64()) {
                    self.mem.provide(b, body.to_vec());
                }
            }
            ("start", _) => match Host::start(self.mem.clone()) {
                Ok(host) => self.host = Some(host),
                Err(e) => {
                    reply["status"] = json!(500);
                    resp_body = format!("{e:#}").into_bytes();
                }
            },
            ("request", Some(host)) => {
                let method = h.get("method").and_then(|m| m.as_str()).unwrap_or("GET");
                let url = h.get("url").and_then(|u| u.as_str()).unwrap_or("/");
                let r = host.request(client, method, url, body);
                reply["status"] = json!(r.status);
                reply["type"] = json!(r.content_type);
                reply["blob"] = json!(r.blob);
                resp_body = r.body;
            }
            ("ws_open", Some(host)) => host.ws_open(client),
            ("ws", Some(host)) => host.ws_message(client, text),
            ("term_open", Some(host)) => host.term_open(client),
            ("term", Some(host)) => host.term_message(client, text),
            ("close", Some(host)) => host.close(client),
            (_, None) => {
                reply["status"] = json!(503);
                resp_body = b"the studio is not started".to_vec();
            }
            (op, Some(_)) => {
                reply["status"] = json!(400);
                resp_body = format!("unknown call {op:?}").into_bytes();
            }
        }

        let mut out = self
            .host
            .as_mut()
            .map(|h| std::mem::take(&mut h.out))
            .unwrap_or_default();
        let need = self.mem.take_missing();
        if !need.is_empty() {
            // Retry once the contents are loaded: nothing of this call counts.
            out.clear();
            resp_body.clear();
            reply = json!({"status": 200, "type": "application/json"});
            if let Some(host) = self.host.as_mut() {
                match shell_before {
                    Some(s) => {
                        host.shells.insert(client, s);
                    }
                    None => {
                        host.shells.remove(&client);
                    }
                }
            }
        }
        reply["need"] = json!(need);
        reply["out"] = Value::Array(
            out.into_iter()
                .map(|o| json!({"to": o.to, "exclude": o.exclude, "chan": o.chan, "text": o.text, "raw": o.raw}))
                .collect(),
        );

        // Changes to persist, with the contents of new blobs after the body.
        let mut tail = resp_body;
        reply["bodyLen"] = json!(tail.len());
        let changes: Vec<Value> = self
            .mem
            .take_changes()
            .into_iter()
            .map(|c| match c {
                Change::File { path, blob, len, modified, data } => {
                    let data = data.map(|d| {
                        let at = tail.len();
                        tail.extend_from_slice(&d);
                        json!([at, d.len()])
                    });
                    json!({"op": "file", "path": path, "blob": blob, "len": len, "modified": modified, "data": data})
                }
                Change::Dir { path, modified } => json!({"op": "dir", "path": path, "modified": modified}),
                Change::Remove { path } => json!({"op": "rm", "path": path}),
            })
            .collect();
        reply["changes"] = Value::Array(changes);
        // Audio is loaded on demand; do not keep it around (once used).
        if op != "provide" && op != "boot" {
            self.mem.evict(|p| is_audio_file(Path::new(p)));
        }
        envelope(&reply, &tail)
    }
}

// ------------------------------------------------------------------ C ABI

thread_local! {
    static BACKEND: RefCell<Backend> = RefCell::new(Backend::new());
    static RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

#[no_mangle]
pub extern "C" fn rc_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # Safety
/// `ptr` must come from `rc_alloc(len)`.
#[no_mangle]
pub unsafe extern "C" fn rc_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(Vec::from_raw_parts(ptr, 0, len.max(1)));
    }
}

/// Handle the envelope at `ptr`; the reply is at `rc_result_ptr`.
///
/// # Safety
/// `ptr`/`len` must describe readable memory.
#[no_mangle]
pub unsafe extern "C" fn rc_call(ptr: *const u8, len: usize) -> usize {
    let input = if ptr.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(ptr, len)
    };
    let reply = BACKEND.with(|b| b.borrow_mut().call(input));
    let n = reply.len();
    RESULT.with(|r| *r.borrow_mut() = reply);
    n
}

#[no_mangle]
pub extern "C" fn rc_result_ptr() -> *const u8 {
    RESULT.with(|r| r.borrow().as_ptr())
}

#[no_mangle]
pub extern "C" fn rc_result_len() -> usize {
    RESULT.with(|r| r.borrow().len())
}

/// Release the last reply (it can be large: a render, a zip).
#[no_mangle]
pub extern "C" fn rc_result_free() {
    RESULT.with(|r| *r.borrow_mut() = Vec::new());
}
