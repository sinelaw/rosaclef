//! A progress line for long commands (`rosaclef render`): redrawn in place on
//! stderr, and only when stderr is a terminal, so pipes and logs stay clean.

use rosaclef_engine::render::RenderProgress;
use rosaclef_studio::render::Progress;
use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};

/// Redraws at most this often.
const EVERY: Duration = Duration::from_millis(100);
const WIDTH: usize = 24;

pub struct Bar {
    on: bool,
    shown: bool,
    last: Option<Instant>,
    /// When the audio itself started rendering (for the speed and the ETA).
    music_start: Option<Instant>,
}

impl Bar {
    /// A bar on stderr, silent when stderr is not a terminal.
    pub fn new() -> Bar {
        Bar {
            on: std::io::stderr().is_terminal(),
            shown: false,
            last: None,
            music_start: None,
        }
    }

    /// Show a render's progress.
    pub fn render(&mut self, p: Progress) {
        if !self.on {
            return;
        }
        if let Progress::Render(RenderProgress::Music { .. }) = p {
            self.music_start.get_or_insert_with(Instant::now);
        }
        if !self.due() {
            return;
        }
        let line = match p {
            Progress::Samples { done, total } => counted("loading samples", done, total),
            Progress::Presets { done, total } => counted("loading instruments", done, total),
            Progress::Render(RenderProgress::Music { done, total }) => {
                let f = if total > 0.0 { done / total } else { 1.0 };
                let elapsed = self.music_start.map_or(0.0, |t| t.elapsed().as_secs_f64());
                let mut s = format!(
                    "rendering {} {:>3.0}%  {} / {}",
                    bar(f),
                    f * 100.0,
                    clock(done),
                    clock(total)
                );
                if elapsed > 0.5 && done > 0.0 {
                    let speed = done / elapsed;
                    let left = (total - done) / speed;
                    s += &format!("  {speed:.1}x real time, {} left", clock(left));
                }
                s
            }
            Progress::Render(RenderProgress::Tail { done, .. }) => {
                format!("rendering {} 100%  ringing out ({done:.1}s)", bar(1.0))
            }
        };
        self.draw(&line);
    }

    /// Erase the line (before printing the result).
    pub fn clear(&mut self) {
        if self.shown {
            eprint!("\r\x1b[K");
            let _ = std::io::stderr().flush();
            self.shown = false;
        }
    }

    fn due(&mut self) -> bool {
        let now = Instant::now();
        if self.last.is_some_and(|t| now - t < EVERY) {
            return false;
        }
        self.last = Some(now);
        true
    }

    fn draw(&mut self, line: &str) {
        eprint!("\r{line}\x1b[K");
        let _ = std::io::stderr().flush();
        self.shown = true;
    }
}

impl Drop for Bar {
    fn drop(&mut self) {
        self.clear();
    }
}

fn counted(what: &str, done: usize, total: usize) -> String {
    let f = if total > 0 {
        done as f64 / total as f64
    } else {
        1.0
    };
    format!("{what} {} {done}/{total}", bar(f))
}

/// `[#########...............]`
fn bar(f: f64) -> String {
    let n = ((f.clamp(0.0, 1.0) * WIDTH as f64).round() as usize).min(WIDTH);
    format!("[{}{}]", "#".repeat(n), ".".repeat(WIDTH - n))
}

/// Seconds as m:ss.
fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_and_clock() {
        assert_eq!(bar(0.0), format!("[{}]", ".".repeat(WIDTH)));
        assert_eq!(bar(1.5), format!("[{}]", "#".repeat(WIDTH)));
        assert_eq!(bar(0.5).matches('#').count(), WIDTH / 2);
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(169.6), "2:50");
        assert_eq!(clock(-3.0), "0:00");
    }
}
