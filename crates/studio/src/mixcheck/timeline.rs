//! Where things are in the song: bars as the producer counts them (from 1,
//! following the meters), the passes of repeated passages, sections, and
//! the stretches of the performance a range covers.
//!
//! Two clocks: **written** beats (the playlist's, `clip.start`) and
//! **performance** beats (the song as it plays, repeats unrolled). A range is
//! given in written time; where a repeat plays a written bar more than once,
//! each pass is a separate stretch of the performance.

use rosaclef_core::form::{self, Span};
use rosaclef_core::{Project, Transport};

/// A named passage of the song (written beats).
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub name: String,
    pub start: f64,
    pub end: f64,
}

/// A contiguous stretch of the performance to render (performance beats).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub from: f64,
    pub to: f64,
}

pub struct Timeline {
    pub spans: Vec<Span>,
    /// Performance beat at the start of each span.
    pub perf_start: Vec<f64>,
    pub perf_total: f64,
    pub written_end: f64,
    transport: Transport,
}

const EPS: f64 = 1e-6;

impl Timeline {
    pub fn new(p: &Project) -> Timeline {
        let spans = form::performance(p);
        let mut perf_start = vec![];
        let mut t = 0.0;
        for s in &spans {
            perf_start.push(t);
            t += s.end - s.start;
        }
        Timeline {
            spans,
            perf_start,
            perf_total: t,
            written_end: form::written_end(p),
            transport: p.transport.clone(),
        }
    }

    /// The bar (from 1) holding written beat `beat`.
    pub fn bar_of(&self, beat: f64) -> u32 {
        self.transport.bar_at(beat).0 + 1
    }

    /// The written beat where bar `bar` (from 1) starts.
    pub fn bar_start(&self, bar: u32) -> f64 {
        self.transport.bar_start(bar.max(1) - 1)
    }

    /// Beats into its bar of written beat `beat`.
    pub fn beat_in_bar(&self, beat: f64) -> f64 {
        beat - self.transport.bar_at(beat).1
    }

    /// The bar after the last one that sounds.
    pub fn last_bar(&self) -> u32 {
        self.bar_of((self.written_end - EPS).max(0.0))
    }

    /// Which time written beat `beat` plays in span `span`: 1 for the first.
    pub fn pass_of(&self, span: usize, beat: f64) -> u32 {
        1 + self.spans[..span.min(self.spans.len())]
            .iter()
            .filter(|s| beat >= s.start - EPS && beat < s.end - EPS)
            .count() as u32
    }

    /// Whether some written beat in `[a, b)` plays more than once.
    pub fn repeats_in(&self, a: f64, b: f64) -> bool {
        let mut n = 0;
        for s in &self.spans {
            if s.start < b - EPS && s.end > a + EPS {
                n += 1;
            }
        }
        n > 1 || self.spans.is_empty()
    }

    pub fn perf_of(&self, span: usize, beat: f64) -> f64 {
        match self.spans.get(span) {
            Some(s) => self.perf_start[span] + (beat - s.start),
            None => self.perf_total,
        }
    }

    /// The span and written beat of performance beat `perf`.
    pub fn at_perf(&self, perf: f64) -> (usize, f64) {
        if self.spans.is_empty() {
            return (0, perf);
        }
        let i = self.perf_start.partition_point(|s| *s <= perf + EPS).max(1) - 1;
        let s = self.spans[i];
        (i, (s.start + perf - self.perf_start[i]).min(s.end))
    }

    /// The stretches of the performance that play written time `ranges`,
    /// in performance order, joined where they follow each other.
    pub fn segments(&self, ranges: &[(f64, f64)]) -> Vec<Segment> {
        let mut out: Vec<Segment> = vec![];
        let mut pieces: Vec<Segment> = vec![];
        for (i, s) in self.spans.iter().enumerate() {
            for &(a, b) in ranges {
                let (x, y) = (a.max(s.start), b.min(s.end));
                if y > x + EPS {
                    pieces.push(Segment {
                        from: self.perf_of(i, x),
                        to: self.perf_of(i, y),
                    });
                }
            }
        }
        pieces.sort_by(|a, b| a.from.total_cmp(&b.from));
        for p in pieces {
            match out.last_mut() {
                Some(l) if p.from <= l.to + EPS => l.to = l.to.max(p.to),
                _ => out.push(p),
            }
        }
        out
    }
}

/// The song's named passages: labelled score marks in song time, and the
/// drum part's sections (bar by bar from its start), in song order.
pub fn sections(p: &Project) -> Vec<Section> {
    let mut out = vec![];
    for m in &p.score.marks {
        if m.pattern.is_empty() && !m.label.trim().is_empty() && m.end > m.start {
            out.push(Section {
                name: m.label.trim().to_string(),
                start: m.start,
                end: m.end,
            });
        }
    }
    if let Some(d) = &p.drums {
        let mut bar = d.start.max(1);
        for s in &d.sections {
            let (a, b) = (bar, bar + s.bars);
            if !s.name.trim().is_empty() && s.bars > 0 {
                out.push(Section {
                    name: s.name.trim().to_string(),
                    start: p.transport.bar_start(a - 1),
                    end: p.transport.bar_start(b - 1),
                });
            }
            bar = b;
        }
    }
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    // A mark and a drum section naming the same passage count once.
    out.dedup_by(|b, a| {
        a.name.eq_ignore_ascii_case(&b.name)
            && (a.start - b.start).abs() < EPS
            && (a.end - b.end).abs() < EPS
    });
    out
}

/// What a range names, resolved to written time.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub ranges: Vec<(f64, f64)>,
    pub from_bar: u32,
    pub to_bar: u32,
    pub from_beat: f64,
    pub to_beat: f64,
    pub section: Option<String>,
}

fn parse_pair(s: &str, what: &str) -> Result<(f64, f64), String> {
    let s = s.trim();
    let (a, b) = s
        .split_once(':')
        .or_else(|| s.split_once('-'))
        .or_else(|| s.split_once('–'))
        .unwrap_or((s, s));
    let num = |x: &str| {
        x.trim()
            .parse::<f64>()
            .map_err(|_| format!("{what}: {s:?} is not FROM:TO"))
    };
    Ok((num(a)?, num(b)?))
}

/// Resolve `--range BAR:BAR` (bars from 1, both included), `--beats B:B`
/// (written beats, end excluded) or `--section NAME`; none: the whole song.
pub fn resolve(
    p: &Project,
    t: &Timeline,
    bars: Option<&str>,
    beats: Option<&str>,
    section: Option<&str>,
) -> Result<Resolved, String> {
    let end = t.written_end;
    if end <= 0.0 {
        return Err("the song is empty: nothing is on the playlist".into());
    }
    let given = [bars.is_some(), beats.is_some(), section.is_some()];
    if given.iter().filter(|g| **g).count() > 1 {
        return Err("give one of --range, --beats or --section".into());
    }
    let mut name = None;
    let ranges = if let Some(r) = bars {
        let (a, b) = parse_pair(r, "range")?;
        if a < 1.0 || b < a || a.fract() != 0.0 || b.fract() != 0.0 {
            return Err(format!(
                "range: {r:?} — bars are counted from 1, FROM ≤ TO (e.g. 52:59)"
            ));
        }
        let last = t.last_bar();
        if a as u32 > last {
            return Err(format!("range: the song has {last} bars"));
        }
        vec![(t.bar_start(a as u32), t.bar_start(b as u32 + 1).min(end))]
    } else if let Some(r) = beats {
        let (a, b) = parse_pair(r, "beats")?;
        if a < 0.0 || b <= a {
            return Err(format!("beats: {r:?} — FROM < TO, in song beats"));
        }
        if a >= end {
            return Err(format!("beats: the song ends at beat {end}"));
        }
        vec![(a, b.min(end))]
    } else if let Some(s) = section {
        let all = sections(p);
        let found: Vec<(f64, f64)> = all
            .iter()
            .filter(|x| x.name.eq_ignore_ascii_case(s.trim()))
            .map(|x| (x.start, x.end.min(end)))
            .filter(|(a, b)| b > a)
            .collect();
        if found.is_empty() {
            let mut names: Vec<&str> = all.iter().map(|x| x.name.as_str()).collect();
            names.dedup();
            return Err(if names.is_empty() {
                format!("section: no section {s:?} — the song has none (label a score mark, or name the drum part's sections)")
            } else {
                format!(
                    "section: no section {s:?} (the song has: {})",
                    names.join(", ")
                )
            });
        }
        name = Some(
            all.iter()
                .find(|x| x.name.eq_ignore_ascii_case(s.trim()))
                .map(|x| x.name.clone())
                .unwrap_or_default(),
        );
        found
    } else {
        vec![(0.0, end)]
    };
    let from_beat = ranges.iter().map(|r| r.0).fold(f64::INFINITY, f64::min);
    let to_beat = ranges.iter().map(|r| r.1).fold(0.0, f64::max);
    Ok(Resolved {
        from_bar: t.bar_of(from_beat),
        to_bar: t.bar_of((to_beat - EPS).max(from_beat)),
        from_beat,
        to_beat,
        ranges,
        section: name,
    })
}
