//! Automation: lane targets and curve evaluation.
//!
//! A lane's `target` is a short path naming one continuous value of the
//! project:
//!
//! | target | value |
//! |---|---|
//! | `tempo` | BPM, 20..999 |
//! | `swing` | 0..1 |
//! | `channel/<id>/volume` | linear gain, 0..1.5 |
//! | `channel/<id>/pan` | -1..1 |
//! | `channel/<id>/<param>` | an instrument parameter, in its catalog units and range |
//! | `insert/<n>/volume` | linear gain, 0..2 |
//! | `insert/<n>/pan` | -1..1 |
//! | `insert/<n>/effect/<k>/<param>` | parameter of effect `k` of insert `n` |
//!
//! Values between two points are interpolated; the `curve` of the later
//! point bends the segment (see [`shape`]). Before the first point the lane
//! holds the first value, after the last point the last value.

use crate::catalog::{self, Category};
use crate::model::{AutomationPoint, InsertIx, Project};
use std::fmt;
use std::str::FromStr;

/// What an automation lane drives.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AutomationTarget {
    Tempo,
    Swing,
    ChannelVolume(String),
    ChannelPan(String),
    /// Instrument parameter of a channel.
    ChannelParam(String, String),
    InsertVolume(InsertIx),
    InsertPan(InsertIx),
    /// Parameter of effect `k` of an insert.
    EffectParam(InsertIx, usize, String),
}

/// Human description of the target grammar (used in error messages and the schema).
pub const TARGET_GRAMMAR: &str = "tempo | swing | channel/<id>/volume | channel/<id>/pan | channel/<id>/<instrument param> | insert/<n>/volume | insert/<n>/pan | insert/<n>/effect/<k>/<effect param>";

/// JSON-schema pattern for targets.
pub const TARGET_PATTERN: &str = "^(tempo|swing|channel/[A-Za-z0-9_.-]{1,64}/[A-Za-z0-9_.-]+|insert/[0-9]+/(volume|pan|effect/[0-9]+/[A-Za-z0-9_.-]+))$";

impl fmt::Display for AutomationTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AutomationTarget::Tempo => write!(f, "tempo"),
            AutomationTarget::Swing => write!(f, "swing"),
            AutomationTarget::ChannelVolume(c) => write!(f, "channel/{c}/volume"),
            AutomationTarget::ChannelPan(c) => write!(f, "channel/{c}/pan"),
            AutomationTarget::ChannelParam(c, k) => write!(f, "channel/{c}/{k}"),
            AutomationTarget::InsertVolume(i) => write!(f, "insert/{i}/volume"),
            AutomationTarget::InsertPan(i) => write!(f, "insert/{i}/pan"),
            AutomationTarget::EffectParam(i, k, p) => write!(f, "insert/{i}/effect/{k}/{p}"),
        }
    }
}

fn valid_key(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn index(s: &str) -> Option<usize> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

impl FromStr for AutomationTarget {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || format!("invalid automation target {s:?}; expected {TARGET_GRAMMAR}");
        let parts: Vec<&str> = s.split('/').collect();
        match parts.as_slice() {
            ["tempo"] => Ok(AutomationTarget::Tempo),
            ["swing"] => Ok(AutomationTarget::Swing),
            ["channel", id, what] if valid_key(id) && valid_key(what) => Ok(match *what {
                "volume" => AutomationTarget::ChannelVolume(id.to_string()),
                "pan" => AutomationTarget::ChannelPan(id.to_string()),
                key => AutomationTarget::ChannelParam(id.to_string(), key.to_string()),
            }),
            ["insert", n, "volume"] => index(n)
                .map(|n| AutomationTarget::InsertVolume(InsertIx(n as u32)))
                .ok_or_else(bad),
            ["insert", n, "pan"] => index(n)
                .map(|n| AutomationTarget::InsertPan(InsertIx(n as u32)))
                .ok_or_else(bad),
            ["insert", n, "effect", k, key] if valid_key(key) => match (index(n), index(k)) {
                (Some(n), Some(k)) => Ok(AutomationTarget::EffectParam(
                    InsertIx(n as u32),
                    k,
                    key.to_string(),
                )),
                _ => Err(bad()),
            },
            _ => Err(bad()),
        }
    }
}

/// Value range and presentation of a resolved target.
#[derive(Clone, Debug, PartialEq)]
pub struct TargetInfo {
    pub min: f64,
    pub max: f64,
    /// Whole numbers only (the engine rounds automated values).
    pub integer: bool,
    /// Logarithmic travel (frequencies, times): lanes draw on a log scale.
    pub exp: bool,
    pub unit: &'static str,
    /// Short label, e.g. "Pad · Cutoff".
    pub label: String,
}

impl TargetInfo {
    fn new(min: f64, max: f64, unit: &'static str, label: String) -> TargetInfo {
        TargetInfo {
            min,
            max,
            integer: false,
            exp: false,
            unit,
            label,
        }
    }
}

impl AutomationTarget {
    /// Resolve the target against a project: check that what it names
    /// exists and return its range. Plugin parameters are unbounded.
    pub fn resolve(&self, p: &Project) -> Result<TargetInfo, String> {
        let channel = |id: &str| {
            p.channel(id)
                .ok_or_else(|| format!("unknown channel {id:?}"))
        };
        let insert = |i: InsertIx| {
            p.mixer.inserts.get(i.index()).ok_or_else(|| {
                format!(
                    "mixer insert {i} does not exist (there are {})",
                    p.mixer.inserts.len()
                )
            })
        };
        match self {
            AutomationTarget::Tempo => Ok(TargetInfo::new(20.0, 999.0, "BPM", "Tempo".into())),
            AutomationTarget::Swing => Ok(TargetInfo::new(0.0, 1.0, "", "Swing".into())),
            AutomationTarget::ChannelVolume(id) => Ok(TargetInfo::new(
                0.0,
                1.5,
                "",
                format!("{} · Volume", channel(id)?.name),
            )),
            AutomationTarget::ChannelPan(id) => Ok(TargetInfo::new(
                -1.0,
                1.0,
                "",
                format!("{} · Pan", channel(id)?.name),
            )),
            AutomationTarget::ChannelParam(id, key) => {
                let ch = channel(id)?;
                param_info(&ch.instrument.kind, Category::Instrument, key, &ch.name)
            }
            AutomationTarget::InsertVolume(i) => Ok(TargetInfo::new(
                0.0,
                2.0,
                "",
                format!("{} · Volume", insert(*i)?.name),
            )),
            AutomationTarget::InsertPan(i) => Ok(TargetInfo::new(
                -1.0,
                1.0,
                "",
                format!("{} · Pan", insert(*i)?.name),
            )),
            AutomationTarget::EffectParam(i, k, key) => {
                let ins = insert(*i)?;
                let fx = ins.effects.get(*k).ok_or_else(|| {
                    format!(
                        "insert {i} has no effect {k} (it has {})",
                        ins.effects.len()
                    )
                })?;
                param_info(&fx.kind, Category::Effect, key, &ins.name)
            }
        }
    }

    /// The target's value in the project (without automation).
    pub fn base_value(&self, p: &Project) -> Option<f64> {
        match self {
            AutomationTarget::Tempo => Some(p.transport.bpm),
            AutomationTarget::Swing => Some(p.transport.swing),
            AutomationTarget::ChannelVolume(id) => p.channel(id).map(|c| c.volume),
            AutomationTarget::ChannelPan(id) => p.channel(id).map(|c| c.pan),
            AutomationTarget::ChannelParam(id, key) => {
                p.channel(id).map(|c| c.instrument.param(key))
            }
            AutomationTarget::InsertVolume(i) => p.mixer.inserts.get(i.index()).map(|x| x.volume),
            AutomationTarget::InsertPan(i) => p.mixer.inserts.get(i.index()).map(|x| x.pan),
            AutomationTarget::EffectParam(i, k, key) => p
                .mixer
                .inserts
                .get(i.index())
                .and_then(|x| x.effects.get(*k))
                .map(|d| d.param(key)),
        }
    }
}

fn param_info(
    kind: &str,
    category: Category,
    key: &str,
    owner: &str,
) -> Result<TargetInfo, String> {
    let spec = catalog::device_in(kind, category)
        .ok_or_else(|| format!("unknown device type {kind:?}"))?;
    if spec.open_params {
        return Ok(TargetInfo::new(
            f64::NEG_INFINITY,
            f64::INFINITY,
            "",
            format!("{owner} · {key}"),
        ));
    }
    let ps = spec.param(key).ok_or_else(|| {
        let keys: Vec<_> = spec.params.iter().map(|p| p.key).collect();
        format!(
            "{kind:?} has no parameter {key:?}; expected one of {}",
            keys.join(", ")
        )
    })?;
    Ok(TargetInfo {
        min: ps.min,
        max: ps.max,
        integer: ps.integer,
        exp: ps.curve == catalog::Curve::Exp && ps.min > 0.0,
        unit: ps.unit,
        label: format!("{owner} · {}", ps.label),
    })
}

/// How strongly `curve = ±1` bends a segment.
pub const CURVE_STRENGTH: f64 = 6.0;

/// Shape a segment position `t` (0..1) by `curve` (-1..1): an exponential
/// ease, `(e^(k t) - 1) / (e^k - 1)` with `k = 6 curve`. `curve > 0` rises
/// slowly first (e.g. a filter sweep that opens late), `curve < 0` rises
/// quickly and settles.
pub fn shape(t: f64, curve: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if curve.abs() < 1e-6 {
        return t;
    }
    let k = curve.clamp(-1.0, 1.0) * CURVE_STRENGTH;
    (k * t).exp_m1() / k.exp_m1()
}

/// Value of a lane at `beat`. `points` must be sorted by beat; an empty lane
/// yields `None`.
pub fn value_at(points: &[AutomationPoint], beat: f64) -> Option<f64> {
    let first = points.first()?;
    let i = points.partition_point(|p| p.beat <= beat);
    if i == 0 {
        return Some(first.value);
    }
    if i == points.len() {
        return Some(points[i - 1].value);
    }
    let (a, b) = (points[i - 1], points[i]);
    let span = b.beat - a.beat;
    if span <= 1e-12 {
        return Some(b.value);
    }
    Some(a.value + (b.value - a.value) * shape((beat - a.beat) / span, b.curve))
}

/// The active (unmuted, non-empty) tempo lane of a project, if any.
pub fn tempo_lane(p: &Project) -> Option<&[AutomationPoint]> {
    p.automation
        .iter()
        .find(|l| {
            !l.mute
                && !l.points.is_empty()
                && matches!(
                    l.target.parse::<AutomationTarget>(),
                    Ok(AutomationTarget::Tempo)
                )
        })
        .map(|l| l.points.as_slice())
}

/// Song time in seconds as a function of beats, following tempo automation.
#[derive(Clone, Debug, Default)]
pub struct TempoMap {
    /// Seconds per beat when there is no tempo lane.
    spb: f64,
    /// Seconds at `k * STEP` beats, up to the last tempo point.
    table: Vec<f64>,
    /// Seconds per beat after the table (the last tempo point's value).
    end_spb: f64,
}

impl TempoMap {
    const STEP: f64 = 1.0 / 16.0;
    const MAX_ENTRIES: usize = 1 << 20;

    pub fn new(p: &Project) -> TempoMap {
        let spb = 60.0 / p.transport.bpm.max(1.0);
        let Some(points) = tempo_lane(p) else {
            return TempoMap {
                spb,
                table: vec![],
                end_spb: spb,
            };
        };
        let spb_at = |b: f64| 60.0 / value_at(points, b).unwrap_or(p.transport.bpm).max(1.0);
        let last = points.last().map(|pt| pt.beat).unwrap_or(0.0).max(0.0);
        let n = ((last / Self::STEP).ceil() as usize + 1).min(Self::MAX_ENTRIES);
        let mut table = Vec::with_capacity(n + 1);
        let mut t = 0.0;
        table.push(t);
        for k in 0..n {
            // Simpson's rule over one step.
            let b0 = k as f64 * Self::STEP;
            t += Self::STEP / 6.0
                * (spb_at(b0) + 4.0 * spb_at(b0 + Self::STEP / 2.0) + spb_at(b0 + Self::STEP));
            table.push(t);
        }
        TempoMap {
            spb,
            table,
            end_spb: spb_at(last),
        }
    }

    /// Whether the tempo changes over the song.
    pub fn is_automated(&self) -> bool {
        !self.table.is_empty()
    }

    /// Seconds from the start of the song to `beat`.
    pub fn seconds_at(&self, beat: f64) -> f64 {
        if self.table.is_empty() {
            return beat * self.spb;
        }
        let beat = beat.max(0.0);
        let x = beat / Self::STEP;
        let k = x.floor() as usize;
        if k + 1 >= self.table.len() {
            let end = (self.table.len() - 1) as f64 * Self::STEP;
            return self.table[self.table.len() - 1] + (beat - end) * self.end_spb;
        }
        let f = x - k as f64;
        self.table[k] + (self.table[k + 1] - self.table[k]) * f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_round_trip() {
        for s in [
            "tempo",
            "swing",
            "channel/pad/volume",
            "channel/pad/pan",
            "channel/pad/cutoff",
            "insert/3/volume",
            "insert/0/pan",
            "insert/2/effect/1/mix",
        ] {
            let t: AutomationTarget = s.parse().unwrap();
            assert_eq!(t.to_string(), s);
        }
        assert_eq!(
            "channel/pad/cutoff".parse::<AutomationTarget>().unwrap(),
            AutomationTarget::ChannelParam("pad".into(), "cutoff".into())
        );
        for bad in [
            "",
            "bpm",
            "channel/pad",
            "channel//cutoff",
            "insert/x/volume",
            "insert/1/effect/a/mix",
            "insert/1/gain",
            "tempo/1",
        ] {
            assert!(bad.parse::<AutomationTarget>().is_err(), "{bad}");
        }
    }

    #[test]
    fn interpolation_and_curves() {
        let pts = |c: f64| {
            vec![
                AutomationPoint {
                    beat: 0.0,
                    value: 0.0,
                    curve: 0.0,
                },
                AutomationPoint {
                    beat: 4.0,
                    value: 8.0,
                    curve: c,
                },
            ]
        };
        let lin = pts(0.0);
        assert_eq!(value_at(&lin, -1.0), Some(0.0));
        assert_eq!(value_at(&lin, 2.0), Some(4.0));
        assert_eq!(value_at(&lin, 9.0), Some(8.0));
        assert!(value_at(&pts(0.5), 2.0).unwrap() < 4.0);
        assert!(value_at(&pts(-0.5), 2.0).unwrap() > 4.0);
        assert!((value_at(&pts(1.0), 4.0).unwrap() - 8.0).abs() < 1e-9);
        assert_eq!(value_at(&[], 1.0), None);
        // A vertical step: two points on the same beat.
        let step = vec![
            AutomationPoint {
                beat: 0.0,
                value: 1.0,
                curve: 0.0,
            },
            AutomationPoint {
                beat: 2.0,
                value: 1.0,
                curve: 0.0,
            },
            AutomationPoint {
                beat: 2.0,
                value: 5.0,
                curve: 0.0,
            },
        ];
        assert_eq!(value_at(&step, 1.999), Some(1.0));
        assert_eq!(value_at(&step, 2.0), Some(5.0));
    }

    #[test]
    fn tempo_map_integrates_a_ramp() {
        let mut p = Project::empty("t");
        assert!((TempoMap::new(&p).seconds_at(8.0) - 4.0).abs() < 1e-12);
        p.automation.push(crate::model::AutomationLane {
            id: "t".into(),
            name: String::new(),
            target: "tempo".into(),
            color: "#ffffff".into(),
            mute: false,
            points: vec![
                AutomationPoint {
                    beat: 0.0,
                    value: 120.0,
                    curve: 0.0,
                },
                AutomationPoint {
                    beat: 8.0,
                    value: 240.0,
                    curve: 0.0,
                },
            ],
        });
        let m = TempoMap::new(&p);
        // bpm(b) = 120 + 15 b  =>  t(b) = 4 ln(1 + b / 8)
        for b in [1.0, 3.3, 8.0] {
            let expected = 4.0 * (1.0f64 + b / 8.0).ln();
            assert!(
                (m.seconds_at(b) - expected).abs() < 5e-5,
                "{b}: {} vs {expected}",
                m.seconds_at(b)
            );
        }
        assert!((m.seconds_at(12.0) - (4.0 * 2f64.ln() + 1.0)).abs() < 1e-6);
    }

    #[test]
    fn resolve_ranges() {
        let mut p = Project::empty("t");
        p.channels.push(crate::model::Channel {
            id: "pad".into(),
            name: "Pad".into(),
            color: "#ffffff".into(),
            instrument: crate::model::Device::new("synth"),
            volume: 0.8,
            pan: 0.0,
            mute: false,
            mixer: InsertIx(1),
            arp: None,
        });
        let t: AutomationTarget = "channel/pad/cutoff".parse().unwrap();
        let info = t.resolve(&p).unwrap();
        assert_eq!((info.min, info.max, info.exp), (20.0, 20000.0, true));
        assert_eq!(info.label, "Pad · Cutoff");
        assert_eq!(t.base_value(&p), Some(2400.0));
        assert!("channel/pad/nope"
            .parse::<AutomationTarget>()
            .unwrap()
            .resolve(&p)
            .is_err());
        assert!("channel/lead/volume"
            .parse::<AutomationTarget>()
            .unwrap()
            .resolve(&p)
            .is_err());
        assert!("insert/0/effect/0/ceiling"
            .parse::<AutomationTarget>()
            .unwrap()
            .resolve(&p)
            .is_ok());
        assert!("insert/0/effect/1/ceiling"
            .parse::<AutomationTarget>()
            .unwrap()
            .resolve(&p)
            .is_err());
        assert!("insert/42/volume"
            .parse::<AutomationTarget>()
            .unwrap()
            .resolve(&p)
            .is_err());
    }
}
