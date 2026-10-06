//! Delivery targets: the loudness and true-peak a destination expects, and
//! what it will do to the mix (streaming services turn loud masters down).

use serde::Serialize;

#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub id: &'static str,
    pub name: &'static str,
    pub lufs: f64,
    pub true_peak: f64,
    /// Broadcast: the loudness must be met within this many LU (streaming:
    /// none; louder is turned down).
    pub tolerance: Option<f64>,
    /// A stricter true-peak limit for masters louder than `lufs` (Spotify
    /// asks for -2 dBTP there: its encoder overshoots more).
    pub loud_true_peak: Option<f64>,
    /// How far a quiet master is turned up.
    pub boost: Boost,
}

/// What a destination does with a master under its loudness.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Boost {
    /// Nothing: it plays quieter than the rest.
    None,
    /// Up to where its true peak reaches this (dBTP).
    PeakLimited(f64),
    /// Broadcast: the loudness must be met (a tolerance), not turned up.
    Delivered,
}

impl Target {
    /// The true-peak limit for a master of `integrated` LUFS.
    pub fn true_peak_for(&self, integrated: Option<f64>) -> f64 {
        match (self.loud_true_peak, integrated) {
            (Some(tp), Some(i)) if i > self.lufs => tp,
            _ => self.true_peak,
        }
    }
}

pub const TARGETS: &[Target] = &[
    Target {
        id: "spotify",
        name: "Spotify",
        lufs: -14.0,
        true_peak: -1.0,
        tolerance: None,
        loud_true_peak: Some(-2.0),
        boost: Boost::PeakLimited(-1.0),
    },
    Target {
        id: "apple",
        name: "Apple Music",
        lufs: -16.0,
        true_peak: -1.0,
        tolerance: None,
        loud_true_peak: None,
        boost: Boost::PeakLimited(-1.0),
    },
    Target {
        id: "youtube",
        name: "YouTube",
        lufs: -14.0,
        true_peak: -1.0,
        tolerance: None,
        loud_true_peak: None,
        boost: Boost::None,
    },
    Target {
        id: "amazon",
        name: "Amazon Music",
        lufs: -14.0,
        true_peak: -2.0,
        tolerance: None,
        loud_true_peak: None,
        boost: Boost::None,
    },
    Target {
        id: "tidal",
        name: "Tidal",
        lufs: -14.0,
        true_peak: -1.0,
        tolerance: None,
        loud_true_peak: None,
        boost: Boost::None,
    },
    Target {
        id: "ebu-r128",
        name: "EBU R128 broadcast",
        lufs: -23.0,
        true_peak: -1.0,
        tolerance: Some(0.5),
        loud_true_peak: None,
        boost: Boost::Delivered,
    },
    Target {
        id: "atsc-a85",
        name: "ATSC A/85 broadcast",
        lufs: -24.0,
        true_peak: -2.0,
        tolerance: Some(2.0),
        loud_true_peak: None,
        boost: Boost::Delivered,
    },
];

pub fn find(id: &str) -> Option<&'static Target> {
    TARGETS
        .iter()
        .find(|t| t.id.eq_ignore_ascii_case(id.trim()))
}

pub fn names() -> Vec<&'static str> {
    TARGETS.iter().map(|t| t.id).collect()
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    pub id: &'static str,
    pub name: &'static str,
    pub lufs: f64,
    pub true_peak_dbtp: f64,
    /// pass | warn | fail
    pub status: &'static str,
    /// The gain the destination applies (negative: turned down).
    pub playback_gain_db: Option<f64>,
    pub notes: Vec<String>,
}

pub fn judge(t: &Target, integrated: Option<f64>, true_peak: Option<f64>) -> Verdict {
    let mut status = "pass";
    let mut notes = vec![];
    let worse = |s: &mut &'static str, to: &'static str| {
        if to == "fail" || *s == "pass" {
            *s = to;
        }
    };
    let gain = integrated.map(|i| super::dsp::r1(t.lufs - i));
    let limit = t.true_peak_for(integrated);
    if let Some(tp) = true_peak {
        if tp > limit + 0.05 {
            worse(&mut status, "fail");
            notes.push(format!(
                "true peak {:.1} dBTP is over the {:.1} dBTP limit{}",
                tp,
                limit,
                if limit < t.true_peak {
                    format!(" (for a master louder than {:.0} LUFS)", t.lufs)
                } else {
                    String::new()
                }
            ));
        }
    }
    // What the destination really applies: it turns loud masters down;
    // quiet ones up only as far as it allows.
    let applied = match (gain, t.boost) {
        (Some(g), _) if g <= 0.0 => Some(g),
        (Some(_), Boost::None) => Some(0.0),
        (Some(g), Boost::PeakLimited(l)) => Some(super::dsp::r1(
            g.min(true_peak.map(|tp| l - tp).unwrap_or(g)).max(0.0),
        )),
        (g, _) => g,
    };
    if let (Some(i), Some(g)) = (integrated, gain) {
        match t.tolerance {
            Some(tol) if (i - t.lufs).abs() > tol => {
                worse(&mut status, "fail");
                notes.push(format!(
                    "{:.1} LUFS is outside {:.1} ± {tol} LUFS",
                    i, t.lufs
                ));
            }
            Some(_) => {}
            None if g < -0.5 => notes.push(format!(
                "played {:.1} dB quieter (normalized to {:.0} LUFS): the extra loudness only costs dynamics",
                -g, t.lufs
            )),
            None if g > 1.0 => {
                let up = applied.unwrap_or(0.0);
                let left = g - up;
                let how = match t.boost {
                    Boost::None => format!("{} does not turn quiet tracks up", t.name),
                    _ => format!(
                        "{} turns it up {up:.1} dB, as far as its {:.0} dBTP peak limit allows",
                        t.name,
                        match t.boost {
                            Boost::PeakLimited(l) => l,
                            _ => 0.0,
                        }
                    ),
                };
                if left > 1.0 {
                    // A quiet, dynamic master (jazz, classical) is a choice:
                    // a warning only when it ends up far under the rest.
                    if left > 6.0 {
                        worse(&mut status, "warn");
                    }
                    notes.push(format!(
                        "{g:.1} dB under {:.0} LUFS: {how}, so it plays {left:.1} dB quieter than other tracks",
                        t.lufs
                    ));
                } else {
                    notes.push(format!("{g:.1} dB under {:.0} LUFS: {how}", t.lufs));
                }
            }
            None => {}
        }
    }
    if t.id == "apple" {
        notes.push(
            "Apple's Sound Check measures loudness its own way: expect a dB or so of difference"
                .into(),
        );
    }
    Verdict {
        id: t.id,
        name: t.name,
        lufs: t.lufs,
        true_peak_dbtp: limit,
        status,
        playback_gain_db: applied,
        notes,
    }
}
