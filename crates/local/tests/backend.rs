//! The browser back end, driven the way `web/local/worker.js` drives it: a
//! fake IndexedDB (paths → entries, blob ids → contents) that persists every
//! reply's changes, serves the blobs a call needs, and restarts from storage.

use rosaclef_engine::render::{encode_wav, Audio};
use rosaclef_local::{envelope, Backend};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

#[derive(Default, Clone)]
struct Store {
    entries: BTreeMap<String, Value>,
    blobs: HashMap<u64, Vec<u8>>,
}

struct Worker {
    backend: Backend,
    store: Store,
    now: f64,
    /// Messages sent to pages.
    inbox: Vec<Value>,
    loads: usize,
    font_fetches: usize,
}

fn split(reply: &[u8]) -> (Value, Vec<u8>) {
    let n = u32::from_le_bytes([reply[0], reply[1], reply[2], reply[3]]) as usize;
    (
        serde_json::from_slice(&reply[4..4 + n]).unwrap(),
        reply[4 + n..].to_vec(),
    )
}

impl Worker {
    fn boot(store: Store) -> Worker {
        let mut w = Worker {
            backend: Backend::new(),
            store,
            now: 1_790_000_000_000.0,
            inbox: vec![],
            loads: 0,
            font_fetches: 0,
        };
        let entries: Vec<Value> = w.store.entries.values().cloned().collect();
        let (h, _) = w.raw(json!({"op": "boot", "entries": entries}), b"");
        for b in h["wanted"].as_array().unwrap() {
            w.provide(b.as_u64().unwrap());
        }
        let (h, body) = w.call(json!({"op": "start"}), b"");
        assert_eq!(h["status"], 200, "{}", String::from_utf8_lossy(&body));
        w
    }

    fn provide(&mut self, blob: u64) {
        let data = self
            .store
            .blobs
            .get(&blob)
            .cloned()
            .expect("blob in storage");
        self.loads += 1;
        self.raw(json!({"op": "provide", "blob": blob}), &data);
    }

    /// One call, persisting its changes (no retry).
    fn raw(&mut self, mut h: Value, body: &[u8]) -> (Value, Vec<u8>) {
        self.now += 1000.0;
        h["now"] = json!(self.now);
        let reply = self.backend.call(&envelope(&h, body));
        let (h, tail) = split(&reply);
        let body_len = h["bodyLen"].as_u64().unwrap_or(0) as usize;
        for c in h["changes"].as_array().unwrap() {
            let path = c["path"].as_str().unwrap().to_string();
            match c["op"].as_str().unwrap() {
                "file" => {
                    let blob = c["blob"].as_u64().unwrap();
                    if let Some(d) = c["data"].as_array() {
                        let (at, n) = (
                            d[0].as_u64().unwrap() as usize,
                            d[1].as_u64().unwrap() as usize,
                        );
                        self.store.blobs.insert(blob, tail[at..at + n].to_vec());
                    }
                    self.store.entries.insert(path.clone(), json!({"path": path, "dir": false, "blob": blob, "len": c["len"], "modified": c["modified"]}));
                }
                "dir" => {
                    self.store.entries.insert(
                        path.clone(),
                        json!({"path": path, "dir": true, "modified": c["modified"]}),
                    );
                }
                "rm" => {
                    self.store.entries.remove(&path);
                }
                op => panic!("unknown change {op}"),
            }
        }
        for o in h["out"].as_array().unwrap() {
            self.inbox.push(o.clone());
        }
        (h, tail[..body_len].to_vec())
    }

    /// A call, loading what it needs and retrying, like the worker.
    fn call(&mut self, h: Value, body: &[u8]) -> (Value, Vec<u8>) {
        for _ in 0..6 {
            let (r, b) = self.raw(h.clone(), body);
            let need: Vec<u64> = r["need"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b.as_u64().unwrap())
                .collect();
            let fonts: Vec<String> = r["needFonts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_str().unwrap().to_string())
                .collect();
            if need.is_empty() && fonts.is_empty() {
                return (r, b);
            }
            for blob in need {
                self.provide(blob);
            }
            // The worker fetches soundfont files from the site (web/soundfonts).
            for name in fonts {
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../web/soundfonts")
                    .join(&name);
                let data = std::fs::read(&path).expect("soundfont file");
                self.font_fetches += 1;
                self.raw(json!({"op": "font", "name": name}), &data);
            }
        }
        panic!("still needs content after 6 tries");
    }

    fn req(&mut self, method: &str, url: &str, body: &[u8]) -> (u64, Vec<u8>, Value) {
        let (h, b) = self.call(
            json!({"op": "request", "client": 1, "method": method, "url": url}),
            body,
        );
        (h["status"].as_u64().unwrap(), b, h)
    }

    fn json(&mut self, method: &str, url: &str, body: Value) -> Value {
        let (status, b, _) = self.req(method, url, body.to_string().as_bytes());
        assert_eq!(
            status,
            200,
            "{method} {url}: {}",
            String::from_utf8_lossy(&b)
        );
        serde_json::from_slice(&b).unwrap()
    }

    /// The contents of a file of the open project, as the worker serves it.
    fn file(&mut self, rel: &str) -> Vec<u8> {
        let (status, b, h) = self.req("GET", &format!("/files/{rel}"), b"");
        assert_eq!(status, 200);
        match h["blob"].as_u64() {
            Some(blob) => self.store.blobs[&blob].clone(),
            None => b,
        }
    }

    fn messages(&mut self, chan: &str) -> Vec<Value> {
        let (mine, rest) = std::mem::take(&mut self.inbox)
            .into_iter()
            .partition(|o| o["chan"] == chan);
        self.inbox = rest;
        mine
    }

    fn restart(&self) -> Worker {
        Worker::boot(self.store.clone())
    }
}

fn wav(seconds: f32) -> Vec<u8> {
    let n = (48000.0 * seconds) as usize;
    let left: Vec<f32> = (0..n).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    encode_wav(
        &Audio {
            sample_rate: 48000.0,
            left: left.clone(),
            right: left,
        },
        16,
    )
}

#[test]
fn first_run_sync_and_persistence() {
    let mut w = Worker::boot(Store::default());
    let list = w.json("GET", "/api/projects", json!(null));
    let projects = list["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 1, "the first run starts from the demo song");
    assert_eq!(projects[0]["name"], "Demo");
    assert!(projects[0]["current"].as_bool().unwrap());

    // A page connects and edits the song.
    w.call(json!({"op": "ws_open", "client": 1}), b"");
    let welcome: Value =
        serde_json::from_str(w.messages("ws")[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(welcome["t"], "welcome");
    assert_eq!(welcome["native"]["available"], false);
    let mut project = welcome["project"].clone();
    project["transport"]["bpm"] = json!(97);
    w.call(json!({"op": "ws_open", "client": 2}), b"");
    w.messages("ws");
    w.call(json!({"op": "ws", "client": 1, "text": json!({"t": "put", "folder": welcome["folder"], "project": project}).to_string()}), b"");
    let msgs = w.messages("ws");
    assert!(msgs
        .iter()
        .any(|m| m["to"] == 1 && m["text"].as_str().unwrap().contains("\"ack\"")));
    let broadcast = msgs
        .iter()
        .find(|m| m["to"].is_null())
        .expect("the other page hears of it");
    assert_eq!(broadcast["exclude"], 1);

    // An invalid edit is rejected.
    project["transport"]["bpm"] = json!(-5);
    w.call(json!({"op": "ws", "client": 1, "text": json!({"t": "put", "project": project}).to_string()}), b"");
    assert!(w.messages("ws")[0]["text"]
        .as_str()
        .unwrap()
        .contains("rejected"));

    // Everything survives a reload.
    let mut w2 = w.restart();
    let p: Value = serde_json::from_slice(&w2.req("GET", "/api/project", b"").1).unwrap();
    assert_eq!(p["transport"]["bpm"], 97);
}

#[test]
fn samples_peaks_render_and_lazy_contents() {
    let mut w = Worker::boot(Store::default());
    // A small song renders fast in debug builds.
    w.json("POST", "/api/projects", json!({"name": "Small"}));
    w.json("POST", "/api/projects/open", json!({"name": "Small"}));
    let (status, body, _) = w.req("POST", "/api/samples?name=hit.wav", &wav(0.5));
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["path"],
        "samples/hit.wav"
    );
    assert!(w
        .messages("ws")
        .iter()
        .any(|m| m["text"].as_str().unwrap().contains("\"samples\"")));

    // After a reload, audio is not in memory until something needs it.
    let mut w = w.restart();
    let loads = w.loads;
    let peaks = w.json("GET", "/api/peaks?path=samples/hit.wav&n=16", json!(null));
    assert_eq!(peaks["peaks"].as_array().unwrap().len(), 16);
    assert!(w.loads > loads, "the sample was loaded on demand");
    let again = w.loads;
    w.json("GET", "/api/peaks?path=samples/hit.wav&n=16", json!(null));
    assert_eq!(w.loads, again, "peaks are cached");
    assert_eq!(w.file("samples/hit.wav"), wav(0.5));

    // Voice to notes: the take is one steady tone (382 Hz).
    let t = w.json(
        "GET",
        "/api/transcribe?path=samples/hit.wav&mode=melody",
        json!(null),
    );
    assert_eq!(t["mode"], "melody");
    let notes = t["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 1, "{t}");
    assert!(
        (notes[0]["pitch"].as_f64().unwrap() - 66.55).abs() < 0.1,
        "{t}"
    );
    let t = w.json(
        "GET",
        "/api/transcribe?path=samples/hit.wav&mode=drums",
        json!(null),
    );
    assert_eq!(t["mode"], "drums");
    assert!(t["hits"].as_array().unwrap().len() <= 1, "{t}");
    let (status, _, _) = w.req(
        "GET",
        "/api/transcribe?path=samples/none.wav&mode=melody",
        b"",
    );
    assert_eq!(status, 404);

    // Use the sample in the song, then render it.
    let mut p: Value = serde_json::from_slice(&w.req("GET", "/api/project", b"").1).unwrap();
    p["playlist"]["tracks"] = json!([{"name": "Audio"}]);
    p["playlist"]["clips"] =
        json!([{"sample": "samples/hit.wav", "track": 0, "start": 0, "length": 1}]);
    let (status, body, _) = w.req("PUT", "/api/project", p.to_string().as_bytes());
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let mut w = w.restart();
    let r = w.json("POST", "/api/render", json!({"bits": 16, "pattern": ""}));
    assert!(
        r["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|x| !x.as_str().unwrap().contains("hit.wav")),
        "{r}"
    );
    let rendered = w.file(r["path"].as_str().unwrap());
    assert_eq!(&rendered[..4], b"RIFF");
    let files = w.json("GET", "/api/files", json!(null));
    assert!(files["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["path"] == r["path"]));
}

#[test]
fn library_zip_and_trash() {
    let mut w = Worker::boot(Store::default());
    w.json(
        "POST",
        "/api/projects",
        json!({"name": "Second", "demo": false}),
    );
    w.json(
        "POST",
        "/api/projects/duplicate",
        json!({"name": "Second", "to": "Third"}),
    );
    assert!(
        w.req(
            "POST",
            "/api/projects",
            json!({"name": "Second"}).to_string().as_bytes()
        )
        .0 == 409
    );
    w.call(json!({"op": "ws_open", "client": 1}), b"");
    w.messages("ws");
    let r = w.json("POST", "/api/projects/open", json!({"name": "Second"}));
    assert_eq!(r["switched"], true);
    let switched: Value =
        serde_json::from_str(w.messages("ws")[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(switched["t"], "switched");
    assert_eq!(switched["name"], "Second");

    // The open project can be renamed; its song title follows.
    w.json(
        "POST",
        "/api/projects/rename",
        json!({"name": "Second", "to": "Deuxième"}),
    );
    let list = w.json("GET", "/api/projects", json!(null));
    let cur = list["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["current"] == true)
        .unwrap()
        .clone();
    assert_eq!(cur["name"], "Deuxième");
    assert_eq!(cur["title"], "Deuxième");
    assert_eq!(
        w.req("DELETE", "/api/projects/Deuxi%C3%A8me", b"").0,
        409,
        "the open project cannot be deleted"
    );

    // Zip round trip (the export needs every file's contents).
    w.req("POST", "/api/samples?name=a.wav", &wav(0.1));
    let mut w = w.restart();
    let (status, zip, h) = w.req("GET", "/api/projects/export", b"");
    assert_eq!(status, 200);
    assert_eq!(h["type"], "application/zip");
    let (status, body, _) = w.req("POST", "/api/projects/import-zip", &zip);
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let imported: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(imported["name"], "Deuxième 2");

    // Delete, then empty the trash.
    w.json("DELETE", "/api/projects/Third", json!(null));
    assert_eq!(
        w.json("POST", "/api/trash/empty", json!({"scope": "library"}))["removed"],
        1
    );
    let names: Vec<String> = w.json("GET", "/api/projects", json!(null))["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect();
    assert!(!names.contains(&"Third".to_string()));
    assert!(
        !w.store.entries.keys().any(|k| k.contains("/Third")),
        "removed from storage too"
    );

    // A reload opens the project that was open.
    let mut w = w.restart();
    w.call(json!({"op": "ws_open", "client": 9}), b"");
    let welcome: Value =
        serde_json::from_str(w.messages("ws")[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(welcome["name"], "Deuxième");
}

fn term(w: &mut Worker, input: &str) -> String {
    w.call(json!({"op": "term", "client": 1, "text": json!({"t": "input", "data": input}).to_string()}), b"");
    w.messages("term")
        .iter()
        .filter(|m| m["raw"] == true)
        .map(|m| m["text"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn shell() {
    let mut w = Worker::boot(Store::default());
    w.call(json!({"op": "ws_open", "client": 1}), b"");
    w.call(json!({"op": "term_open", "client": 1}), b"");
    let status: Value =
        serde_json::from_str(w.messages("term")[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(status["running"], false);
    w.call(json!({"op": "term", "client": 1, "text": json!({"t": "start", "agent": "shell"}).to_string()}), b"");
    let started = w.messages("term");
    assert!(started
        .iter()
        .any(|m| m["text"].as_str().unwrap().contains("\"running\":true")));

    assert!(term(&mut w, "summary\r").contains("BPM"));
    assert!(term(&mut w, "rosaclef validate\r").contains("ok"));
    w.messages("ws");
    let out = term(&mut w, "set /transport/bpm 131\r");
    assert!(out.contains("ok"), "{out}");
    let ws = w.messages("ws");
    assert!(
        ws.iter()
            .any(|m| m["text"].as_str().unwrap().contains("\"origin\":\"disk\"")),
        "the studio hears of the edit"
    );
    assert!(term(&mut w, "get /transport/bpm\r").contains("131"));
    assert!(term(&mut w, "set /transport/bpm -1\r").contains("not applied"));
    // Typing, backspace, history.
    assert!(term(&mut w, "sumx\x7fmary\r").contains("BPM"));
    let up = term(&mut w, "\x1b[A");
    assert!(up.contains("summary"), "{up:?}");
    term(&mut w, "\x03");
    // Rendering writes into renders/ (a small song renders fast in debug builds).
    term(&mut w, "set /playlist/clips []\r");
    let out = term(&mut w, "render --bits 16\r");
    assert!(out.contains("rendered renders/"), "{out}");
    let out = term(&mut w, "note --channel nope --out samples/x.wav\r");
    assert!(out.contains("no channel"), "{out}");
    assert!(term(&mut w, "ls\r").contains("project.json"));
    assert!(term(&mut w, "bogus\r").contains("unknown command"));
    w.call(json!({"op": "term", "client": 1, "text": json!({"t": "start", "agent": "claude"}).to_string()}), b"");
    assert!(w.messages("term")[0]["text"]
        .as_str()
        .unwrap()
        .contains("native studio"));
}

#[test]
fn renders_soundfont_instruments_with_files_from_the_site() {
    let mut w = Worker::boot(Store::default());
    w.json("POST", "/api/projects", json!({"name": "Bass"}));
    w.json("POST", "/api/projects/open", json!({"name": "Bass"}));
    let mut p: Value = serde_json::from_slice(&w.req("GET", "/api/project", b"").1).unwrap();
    p["channels"] = json!([{"id": "bass", "name": "Bass", "instrument": {"type": "soundfont", "options": {"program": "Acoustic Bass"}}}]);
    p["patterns"] = json!([{"id": "a", "name": "A", "length": 2, "notes": [{"channel": "bass", "pitch": 40, "start": 0, "length": 1}]}]);
    p["playlist"] = json!({"tracks": [{"name": "Bass"}], "clips": [{"pattern": "a", "track": 0, "start": 0, "length": 2}]});
    let (status, body, _) = w.req("PUT", "/api/project", p.to_string().as_bytes());
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));

    let r = w.json("POST", "/api/render", json!({"bits": 16, "pattern": ""}));
    assert_eq!(r["warnings"], json!([]), "{r}");
    assert!(
        r["peakDb"].as_f64().unwrap() > -30.0,
        "the bass sounds: {r}"
    );
    // The index and the pieces holding the bass, not the whole soundfont.
    assert!((2..6).contains(&w.font_fetches), "{} files", w.font_fetches);

    // The shell's note command plays soundfont instruments too.
    w.call(json!({"op": "term_open", "client": 1}), b"");
    w.call(json!({"op": "term", "client": 1, "text": json!({"t": "start", "agent": "shell"}).to_string()}), b"");
    w.messages("term");
    let out = term(
        &mut w,
        "note --channel bass --pitch 40 --seconds 0.5 --out samples/b.wav\r",
    );
    assert!(out.contains("wrote samples/b.wav"), "{out}");
}

#[test]
fn writes_a_drum_part() {
    let mut w = Worker::boot(Store::default());
    let grooves = w.json("GET", "/api/grooves", json!(null));
    assert!(grooves["grooves"].as_array().unwrap().len() > 10);
    let mut p: Value = serde_json::from_slice(&w.req("GET", "/api/project", b"").1).unwrap();
    // The demo is a jazz waltz with a drum part: write it again from scratch.
    assert_eq!(p["drums"]["groove"], json!("jazz-waltz"));
    p["drums"] = json!({"groove": "jazz-waltz-brushes"});
    let g = w.json("POST", "/api/drums?guess=true&write=false", p.clone());
    assert!(g["report"].is_null());
    assert!(g["project"]["drums"]["written"].is_null());
    let r = w.json("POST", "/api/drums?guess=true", p.clone());
    let sections = r["project"]["drums"]["sections"].as_array().unwrap();
    assert!(!sections.is_empty(), "{r}");
    assert!(r["report"]["patterns"].as_u64().unwrap() > 0);
    assert_eq!(r["edited"], json!([]));
    // The written project is a valid edit.
    let (status, body, _) = w.req("PUT", "/api/project", r["project"].to_string().as_bytes());
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    // A part that cannot be written says why.
    p["drums"] = json!({"groove": "rock-8ths", "sections": [{"bars": 4}]});
    let (status, body, _) = w.req("POST", "/api/drums", p.to_string().as_bytes());
    assert_eq!(status, 422);
    assert!(
        String::from_utf8_lossy(&body).contains("4/4"),
        "{}",
        String::from_utf8_lossy(&body)
    );
}
