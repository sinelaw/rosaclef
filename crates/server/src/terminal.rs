//! The agent terminal: a PTY session running the user's own coding agent
//! (Claude Code, Codex, Gemini CLI, ...) inside a project folder — one per
//! open project, shared by that project's tabs. The browser renders it with
//! xterm.js over the project's `/s/{key}/ws/term`.

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;

const SCROLLBACK: usize = 512 * 1024;

#[derive(Serialize, Clone)]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub command: Vec<String>,
    pub available: bool,
    pub hint: &'static str,
}

fn on_path(bin: &str) -> bool {
    which::which(bin).is_ok()
}

pub fn presets() -> Vec<Preset> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
    let mk = |id, name, cmd: &[&str], hint| Preset {
        id,
        name,
        command: cmd.iter().map(|s| s.to_string()).collect(),
        available: on_path(cmd[0]),
        hint,
    };
    vec![
        mk(
            "claude",
            "Claude Code",
            &["claude"],
            "npm i -g @anthropic-ai/claude-code",
        ),
        mk("codex", "Codex CLI", &["codex"], "npm i -g @openai/codex"),
        mk(
            "gemini",
            "Gemini CLI",
            &["gemini"],
            "npm i -g @google/gemini-cli",
        ),
        mk("opencode", "OpenCode", &["opencode"], "see opencode.ai"),
        mk(
            "aider",
            "Aider",
            &["aider", "--read", "AGENTS.md"],
            "pip install aider-chat",
        ),
        Preset {
            id: "shell",
            name: "Shell",
            command: vec![shell.clone()],
            available: true,
            hint: "",
        },
    ]
}

#[derive(Clone)]
pub struct AgentEnv {
    pub dir: PathBuf,
    pub url: String,
}

#[derive(Clone)]
enum Event {
    Output(Arc<[u8]>),
    Status(Arc<str>),
}

struct Session {
    /// Identifies the session, so a finished reader thread never tears down
    /// a newer session that replaced its own.
    id: u64,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

/// What the last start asked for, to restart the same agent elsewhere.
#[derive(Clone)]
struct Launch {
    agent: String,
    custom: Option<String>,
    cols: u16,
    rows: u16,
}

pub struct Terminal {
    session: Mutex<Option<Session>>,
    scrollback: Mutex<VecDeque<u8>>,
    status: Mutex<Value>,
    tx: broadcast::Sender<Event>,
    /// Where agents run: the project folder (it changes when the project
    /// is renamed), and the project's own API URL.
    env: Mutex<AgentEnv>,
    last: Mutex<Option<Launch>>,
    next_id: AtomicU64,
}

impl Terminal {
    pub fn new(env: AgentEnv) -> Terminal {
        let (tx, _) = broadcast::channel(1024);
        Terminal {
            session: Mutex::new(None),
            scrollback: Mutex::new(VecDeque::new()),
            status: Mutex::new(json!({"t": "status", "running": false})),
            tx,
            env: Mutex::new(env),
            last: Mutex::new(None),
            next_id: AtomicU64::new(1),
        }
    }

    /// Whether an agent is running.
    pub fn running(&self) -> bool {
        self.session.lock().is_some()
    }

    /// Stop the agent for good (its project closed).
    pub fn shut_down(&self) {
        *self.last.lock() = None;
        self.stop();
    }

    /// Point future agent sessions at another project folder.
    pub fn set_env(&self, env: AgentEnv) {
        *self.env.lock() = env;
    }

    /// Restart the running agent (if any) in the current folder: each
    /// project has its own AGENTS.md, so the agent must start over there.
    pub fn restart(self: &Arc<Self>) -> Result<(), String> {
        let running = self.session.lock().is_some();
        let last = self.last.lock().clone();
        match (running, last) {
            (true, Some(l)) => {
                // Clear every attached terminal view (RIS escape).
                let _ = self.tx.send(Event::Output(Arc::from(&b"\x1bc"[..])));
                self.start(&l.agent, l.custom.as_deref(), l.cols, l.rows)
            }
            (true, None) => {
                self.stop();
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn set_status(&self, v: Value) {
        *self.status.lock() = v.clone();
        let _ = self.tx.send(Event::Status(v.to_string().into()));
    }

    fn push_output(&self, bytes: &[u8]) {
        {
            let mut sb = self.scrollback.lock();
            sb.extend(bytes.iter().copied());
            let excess = sb.len().saturating_sub(SCROLLBACK);
            sb.drain(..excess);
        }
        let _ = self.tx.send(Event::Output(bytes.into()));
    }

    fn stop(&self) {
        let taken = self.session.lock().take();
        if let Some(mut s) = taken {
            let _ = s.child.kill();
            let code = s.child.wait().ok().map(|st| st.exit_code());
            let agent = self
                .status
                .lock()
                .get("agent")
                .cloned()
                .unwrap_or(Value::Null);
            self.set_status(
                json!({"t": "status", "running": false, "agent": agent, "exitCode": code}),
            );
        }
    }

    fn start(
        self: &Arc<Self>,
        agent: &str,
        custom: Option<&str>,
        cols: u16,
        rows: u16,
    ) -> Result<(), String> {
        self.stop();
        *self.last.lock() = Some(Launch {
            agent: agent.to_string(),
            custom: custom.map(|c| c.to_string()),
            cols,
            rows,
        });
        let env = self.env.lock().clone();
        let (name, argv): (String, Vec<String>) =
            if let Some(cmd) = custom.filter(|c| !c.trim().is_empty()) {
                (
                    "Custom".into(),
                    vec!["/bin/sh".into(), "-c".into(), cmd.to_string()],
                )
            } else {
                let p = presets()
                    .into_iter()
                    .find(|p| p.id == agent)
                    .ok_or_else(|| format!("unknown agent {agent:?}"))?;
                if !p.available {
                    return Err(format!(
                        "{} is not installed (not found on PATH). Install it with: {}",
                        p.name, p.hint
                    ));
                }
                (p.name.to_string(), p.command)
            };
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: rows.max(5),
                cols: cols.max(20),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(&argv[0]);
        cmd.args(&argv[1..]);
        cmd.cwd(&env.dir);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("ROSACLEF_PROJECT", env.dir.display().to_string());
        cmd.env("ROSACLEF_URL", &env.url);
        // Nested Claude Code sessions refuse to start when this is inherited.
        cmd.env_remove("CLAUDECODE");
        if let Ok(exe) = std::env::current_exe() {
            cmd.env("ROSACLEF_BIN", exe.display().to_string());
            if let Some(dir) = exe.parent() {
                let path = std::env::var("PATH").unwrap_or_default();
                let sep = if cfg!(windows) { ";" } else { ":" };
                cmd.env("PATH", format!("{}{sep}{path}", dir.display()));
            }
        }
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("failed to start {}: {e}", argv[0]))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        self.scrollback.lock().clear();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        *self.session.lock() = Some(Session {
            id,
            master: pair.master,
            writer,
            child,
        });
        self.set_status(json!({"t": "status", "running": true, "agent": agent, "name": name}));

        let me = self.clone();
        let agent = agent.to_string();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => me.push_output(&buf[..n]),
                }
            }
            // Only tear down our own session: a restart may already have
            // replaced it (and `stop` reported that one's exit).
            let mine = {
                let mut guard = me.session.lock();
                if guard.as_ref().map(|s| s.id) == Some(id) {
                    guard.take()
                } else {
                    None
                }
            };
            if let Some(mut s) = mine {
                let code = s.child.wait().ok().map(|st| st.exit_code());
                me.set_status(
                    json!({"t": "status", "running": false, "agent": agent, "exitCode": code}),
                );
            }
        });
        Ok(())
    }

    fn write(&self, data: &[u8]) {
        if let Some(s) = self.session.lock().as_mut() {
            let _ = s.writer.write_all(data);
            let _ = s.writer.flush();
        }
    }

    fn resize(&self, cols: u16, rows: u16) {
        if let Some(l) = self.last.lock().as_mut() {
            l.cols = cols;
            l.rows = rows;
        }
        if let Some(s) = self.session.lock().as_ref() {
            let _ = s.master.resize(PtySize {
                rows: rows.max(5),
                cols: cols.max(20),
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }
}

/// Serve one browser terminal view (several may be attached at once).
pub async fn serve(term: Arc<Terminal>, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let mut rx = term.tx.subscribe();
    let snapshot: Vec<u8> = term.scrollback.lock().iter().copied().collect();
    let status = term.status.lock().to_string();
    if sink.send(Message::Text(status.into())).await.is_err() {
        return;
    }
    if !snapshot.is_empty() && sink.send(Message::Binary(snapshot.into())).await.is_err() {
        return;
    }
    let (err_tx, mut err_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let send = tokio::spawn(async move {
        loop {
            tokio::select! {
                ev = rx.recv() => match ev {
                    Ok(Event::Output(b)) => if sink.send(Message::Binary(b.to_vec().into())).await.is_err() { break },
                    Ok(Event::Status(s)) => if sink.send(Message::Text(s.to_string().into())).await.is_err() { break },
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                },
                e = err_rx.recv() => match e {
                    Some(t) => if sink.send(Message::Text(t.into())).await.is_err() { break },
                    None => break,
                },
            }
        }
    });
    while let Some(Ok(msg)) = stream.next().await {
        let Message::Text(text) = msg else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let num = |k: &str, d: u64| v.get(k).and_then(|x| x.as_u64()).unwrap_or(d) as u16;
        match v.get("t").and_then(|t| t.as_str()).unwrap_or("") {
            "input" => {
                if let Some(d) = v.get("data").and_then(|d| d.as_str()) {
                    term.write(d.as_bytes());
                }
            }
            "resize" => term.resize(num("cols", 80), num("rows", 24)),
            "start" => {
                let agent = v
                    .get("agent")
                    .and_then(|a| a.as_str())
                    .unwrap_or("shell")
                    .to_string();
                let custom = v
                    .get("command")
                    .and_then(|c| c.as_str())
                    .map(|s| s.to_string());
                let (cols, rows) = (num("cols", 80), num("rows", 24));
                let t2 = term.clone();
                let res = tokio::task::spawn_blocking(move || {
                    t2.start(&agent, custom.as_deref(), cols, rows)
                })
                .await;
                let err = match res {
                    Ok(Ok(())) => None,
                    Ok(Err(e)) => Some(e),
                    Err(e) => Some(e.to_string()),
                };
                if let Some(e) = err {
                    let _ = err_tx.send(json!({"t": "error", "message": e}).to_string());
                }
            }
            "stop" => {
                let t2 = term.clone();
                let _ = tokio::task::spawn_blocking(move || t2.stop()).await;
            }
            _ => {}
        }
    }
    send.abort();
}
