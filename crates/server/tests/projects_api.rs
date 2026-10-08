//! `rosaclef serve` with several projects open at once: each is reached only
//! through its own key (`/s/{key}/...`), and nothing a request carries — a
//! job id, a file path, a project name or folder — reaches another project.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Reply {
    status: u16,
    body: String,
}

impl Reply {
    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body)
            .unwrap_or_else(|e| panic!("not JSON ({e}): {:?}", self.body))
    }
}

impl Server {
    fn request(&self, method: &str, path: &str, body: &str, origin: Option<&str>) -> Reply {
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
        let origin = origin
            .map(|o| format!("Origin: {o}\r\n"))
            .unwrap_or_default();
        write!(
            s,
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{origin}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.port,
            body.len()
        )
        .unwrap();
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).unwrap();
        let text = String::from_utf8_lossy(&raw).to_string();
        let (head, rest) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        let body = if head
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked")
        {
            unchunk(rest)
        } else {
            rest.to_string()
        };
        Reply { status, body }
    }

    fn get(&self, path: &str) -> Reply {
        self.request("GET", path, "", None)
    }

    fn post(&self, path: &str, body: serde_json::Value) -> Reply {
        self.request("POST", path, &body.to_string(), None)
    }

    /// Open a project for a tab: its key.
    fn open(&self, body: serde_json::Value) -> String {
        let r = self.post("/api/projects/open", body);
        assert_eq!(r.status, 200, "{}", r.body);
        r.json()["session"].as_str().unwrap().to_string()
    }
}

fn unchunk(mut s: &str) -> String {
    let mut out = String::new();
    while let Some((len, rest)) = s.split_once("\r\n") {
        let n = usize::from_str_radix(len.trim(), 16).unwrap_or(0);
        if n == 0 {
            break;
        }
        out.push_str(&rest[..n]);
        s = rest[n..].trim_start_matches("\r\n");
    }
    out
}

fn rosaclef() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rosaclef"))
}

fn new_project(dir: &Path) {
    let st = rosaclef().arg("new").arg(dir).output().unwrap();
    assert!(
        st.status.success(),
        "{}",
        String::from_utf8_lossy(&st.stderr)
    );
}

fn title(dir: &Path) -> String {
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("project.json")).unwrap()).unwrap();
    v["meta"]["title"].as_str().unwrap().to_string()
}

fn start(home: &Path, library: &Path) -> Server {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let child = rosaclef()
        .arg("serve")
        .arg(home)
        .arg("--port")
        .arg(port.to_string())
        .arg("--library")
        .arg(library)
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let server = Server { child, port };
    let t0 = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "the server did not start"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    server
}

/// A job's answer, once it ends.
fn finished(s: &Server, path: &str) -> serde_json::Value {
    let t0 = Instant::now();
    loop {
        let r = s.get(path);
        assert_eq!(r.status, 200, "{}", r.body);
        let v = r.json();
        if v["state"] != "running" {
            return v;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(60),
            "the job never ended"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn projects_are_isolated_from_each_other() {
    let root: PathBuf =
        std::env::temp_dir().join(format!("rosaclef-projects-api-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let lib = root.join("lib");
    let (alpha, beta) = (lib.join("Alpha"), lib.join("Beta"));
    new_project(&alpha);
    new_project(&beta);
    // A project that is not in the library: nothing may open it.
    let outside = root.join("Outside");
    new_project(&outside);
    let lib = lib.canonicalize().unwrap();
    let s = start(&alpha, &lib);

    // The home project answers unscoped requests (scripts, the CLI).
    let info = s.get("/api/info").json();
    assert!(info["folder"].as_str().unwrap().ends_with("Alpha"));

    // A tab opens Beta: a key of 128 random bits, the same while it is open.
    let b = s.open(serde_json::json!({"name": "Beta"}));
    assert_eq!(b.len(), 32);
    assert!(b.chars().all(|c| c.is_ascii_hexdigit()));
    let by_path = s.open(serde_json::json!({"path": lib.join("Beta").display().to_string()}));
    assert_eq!(b, by_path);
    let a =
        s.open(serde_json::json!({"path": alpha.canonicalize().unwrap().display().to_string()}));
    assert_ne!(a, b);
    assert!(s.get(&format!("/s/{b}/api/info")).json()["folder"]
        .as_str()
        .unwrap()
        .ends_with("Beta"));

    // An edit through Beta's key changes Beta, and only Beta.
    let mut p = s.get(&format!("/s/{b}/api/project")).json();
    p["meta"]["title"] = "Beta edited".into();
    let r = s.request("PUT", &format!("/s/{b}/api/project"), &p.to_string(), None);
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(title(&lib.join("Beta")), "Beta edited");
    assert_ne!(title(&alpha), "Beta edited");
    assert_ne!(s.get("/api/project").json()["meta"]["title"], "Beta edited");

    // A key that names no open project reaches nothing (never another project).
    let r = s.get(&format!("/s/{}/api/project", "0".repeat(32)));
    assert_eq!(r.status, 404);
    let r = s.get("/s//api/project");
    assert_eq!(r.status, 404);

    // Only projects of the library open: not by path elsewhere, not by a
    // name that climbs out of it, not through `..`.
    for body in [
        serde_json::json!({"path": outside.display().to_string()}),
        serde_json::json!({"path": lib.join("Beta/../../Outside").display().to_string()}),
        serde_json::json!({"path": "Beta"}),
        serde_json::json!({"name": "../Outside"}),
        serde_json::json!({"name": "Nope"}),
        serde_json::json!({}),
    ] {
        let r = s.post("/api/projects/open", body.clone());
        assert_eq!(r.status, 400, "{body} opened: {}", r.body);
    }
    // Not from another site either.
    let r = s.request(
        "POST",
        "/api/projects/open",
        r#"{"name":"Beta"}"#,
        Some("http://evil.example"),
    );
    assert_eq!(r.status, 403);

    // A job is found only through the project it was started for.
    let r = s.post(&format!("/s/{b}/api/jobs/render"), serde_json::json!({}));
    assert_eq!(r.status, 202, "{}", r.body);
    let job = r.json()["job"].as_u64().unwrap();
    assert_eq!(s.get(&format!("/api/jobs/{job}")).status, 404);
    assert_eq!(s.get(&format!("/s/{a}/api/jobs/{job}")).status, 404);
    let done = finished(&s, &format!("/s/{b}/api/jobs/{job}"));
    assert_eq!(done["state"], "done", "{done}");
    let url = done["result"]["url"].as_str().unwrap().to_string();
    assert!(url.starts_with("/files/renders/"));

    // So are its files: Beta's render is not a file of Alpha.
    assert_eq!(s.get(&format!("/s/{b}{url}")).status, 200);
    assert_eq!(s.get(&url).status, 404);
    assert_eq!(s.get(&format!("/s/{a}{url}")).status, 404);
    assert_eq!(
        s.get(&format!("/s/{b}/files/../Alpha/project.json")).status,
        404
    );
    // And each project lists its own files.
    let files = s.get(&format!("/s/{b}/api/files")).json();
    assert!(files["folder"].as_str().unwrap().ends_with("Beta"));
    let files = s.get(&format!("/s/{a}/api/files")).json();
    assert!(files["folder"].as_str().unwrap().ends_with("Alpha"));

    // An open project cannot be deleted from under its tabs.
    let r = s.request("DELETE", "/api/projects/Beta", "", None);
    assert_eq!(r.status, 409, "{}", r.body);

    // Renamed while open, it keeps its key and follows its folder.
    let r = s.post(
        "/api/projects/rename",
        serde_json::json!({"name": "Beta", "to": "Gamma"}),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(s.get(&format!("/s/{b}/api/info")).json()["folder"]
        .as_str()
        .unwrap()
        .ends_with("Gamma"));
    assert_eq!(s.get(&format!("/s/{b}{url}")).status, 200);

    drop(s);
    let _ = std::fs::remove_dir_all(&root);
}
