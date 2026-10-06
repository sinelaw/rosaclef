//! How far a long job (an export, a mix check) has come, for a studio's
//! progress bar. A page picks a job id and sends it with the request
//! (`?job=ID`), so it can ask about that job while it runs — several jobs at
//! once included: the native server answers `GET /api/progress?job=ID`; the
//! browser back end, busy in the job, has a listener pass each step to its
//! page. A job runs on one thread: its steps go to that thread's job.

use crate::render::Progress;
use rosaclef_engine::render::RenderProgress;
use serde::Serialize;
use std::cell::Cell;
use std::sync::Mutex;

/// A job under way (or finished, `active: false`).
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: u32,
    pub active: bool,
    /// "export" or "mixcheck".
    pub what: String,
    /// What it is doing: "samples", "instruments", "render", "tail",
    /// "measure".
    pub stage: String,
    /// How far the stage has come (0 … 1).
    pub done: f64,
    /// Seconds of music rendered, of `total` (0 before the render).
    pub seconds: f64,
    pub total: f64,
    /// Which render of the job (a what-if or a verify renders more than once).
    pub render: u32,
    /// How far it had come when the listener last heard (it hears of a
    /// move of half a percent or more).
    #[serde(skip)]
    told: f64,
}

/// The jobs under way and the last few finished, by id.
static JOBS: Mutex<Vec<Job>> = Mutex::new(Vec::new());

/// Finished jobs kept for a late question.
const KEEP: usize = 16;

thread_local! {
    /// The job this thread works for (0: none).
    static CURRENT: Cell<u32> = const { Cell::new(0) };
}

/// Told of every step (the browser back end passes it to its page).
static LISTENER: Mutex<Option<fn(&Job)>> = Mutex::new(None);

pub fn set_listener(f: fn(&Job)) {
    *LISTENER.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
}

/// Job `id` (an unknown one reads as not active).
pub fn get(id: u32) -> Job {
    JOBS.lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|j| j.id == id)
        .cloned()
        .unwrap_or(Job {
            id,
            ..Job::default()
        })
}

/// A job under way on this thread until dropped.
pub struct Running {
    id: u32,
    before: u32,
}

impl Drop for Running {
    fn drop(&mut self) {
        if self.id != 0 {
            change(|j| j.active = false, true);
        }
        CURRENT.with(|c| c.set(self.before));
    }
}

/// Start job `id` ("export", "mixcheck") on this thread; 0: nobody asks.
pub fn start(id: u32, what: &str) -> Running {
    let before = CURRENT.with(|c| c.replace(id));
    if id != 0 {
        {
            let mut jobs = JOBS.lock().unwrap_or_else(|e| e.into_inner());
            jobs.retain(|j| j.id != id);
            jobs.push(Job {
                id,
                active: true,
                what: what.into(),
                ..Job::default()
            });
            // Forget the oldest finished ones.
            while jobs.iter().filter(|j| !j.active).count() > KEEP {
                if let Some(k) = jobs.iter().position(|j| !j.active) {
                    jobs.remove(k);
                }
            }
        }
        change(|_| {}, true);
    }
    Running { id, before }
}

/// Change this thread's job; the listener hears of it when it moved by half
/// a percent or more (or `always`).
fn change(f: impl FnOnce(&mut Job), always: bool) {
    let id = CURRENT.with(|c| c.get());
    if id == 0 {
        return;
    }
    let (job, moved) = {
        let mut jobs = JOBS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(j) = jobs.iter_mut().find(|j| j.id == id) else {
            return;
        };
        let before = j.clone();
        f(j);
        let moved = always
            || j.stage != before.stage
            || j.render != before.render
            || (j.done - j.told).abs() >= 0.005;
        if moved {
            j.told = j.done;
        }
        (j.clone(), moved)
    };
    if moved {
        let l = *LISTENER.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(l) = l {
            l(&job);
        }
    }
}

/// A new render of the job begins.
pub fn next_render() {
    change(
        |j| {
            j.render += 1;
            j.stage = "render".into();
            j.done = 0.0;
        },
        true,
    );
}

/// A mix check's render: `f` of it done (0 … 1).
pub fn rendered(f: f64) {
    change(
        |j| {
            j.stage = "render".into();
            j.done = f.clamp(0.0, 1.0);
        },
        false,
    );
}

/// The render is done; the job measures it.
pub fn measuring() {
    change(
        |j| {
            j.stage = "measure".into();
            j.done = 1.0;
        },
        true,
    );
}

/// An export's step.
pub fn exported(p: Progress) {
    change(|j| step(j, p), false);
}

/// What an export's step says of the job.
fn step(j: &mut Job, p: Progress) {
    match p {
        Progress::Samples { done, total } => {
            j.stage = "samples".into();
            j.done = done as f64 / total.max(1) as f64;
        }
        Progress::Presets { done, total } => {
            j.stage = "instruments".into();
            j.done = done as f64 / total.max(1) as f64;
        }
        Progress::Render(RenderProgress::Music { done, total }) => {
            j.stage = "render".into();
            j.render = j.render.max(1);
            j.done = if total > 0.0 { done / total } else { 1.0 };
            j.seconds = done;
            j.total = total;
        }
        Progress::Render(RenderProgress::Tail { .. }) => {
            j.stage = "tail".into();
            j.done = 1.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_export_step_says_where_it_is() {
        let mut j = Job::default();
        step(&mut j, Progress::Samples { done: 1, total: 4 });
        assert!(j.stage == "samples" && (j.done - 0.25).abs() < 1e-9);
        step(
            &mut j,
            Progress::Render(RenderProgress::Music {
                done: 30.0,
                total: 120.0,
            }),
        );
        assert!(j.stage == "render" && (j.done - 0.25).abs() < 1e-9);
        assert!(j.seconds == 30.0 && j.total == 120.0 && j.render == 1);
        step(
            &mut j,
            Progress::Render(RenderProgress::Tail {
                done: 1.0,
                max: 8.0,
            }),
        );
        assert!(j.stage == "tail" && j.done == 1.0);
    }

    #[test]
    fn jobs_at_once_keep_their_own_progress() {
        let (a, b) = (0x7a11_0001, 0x7a11_0002);
        let t = std::thread::spawn(move || {
            let _job = start(b, "mixcheck");
            rendered(0.75);
            get(b)
        });
        {
            let _job = start(a, "export");
            exported(Progress::Render(RenderProgress::Music {
                done: 10.0,
                total: 100.0,
            }));
            let mid = t.join().unwrap();
            assert!(mid.active && mid.what == "mixcheck" && (mid.done - 0.75).abs() < 1e-9);
            let ja = get(a);
            assert!(ja.active && ja.what == "export" && (ja.done - 0.1).abs() < 1e-9);
        }
        assert!(!get(a).active && !get(b).active);
        assert!(!get(0x7a11_0003).active);
    }
}
