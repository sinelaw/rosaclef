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
}

pub const TARGETS: &[Target] = &[
    Target {
        id: "spotify",
        name: "Spotify",
        lufs: -14.0,
        true_peak: -1.0,
        tolerance: None,
    },
    Target {
        id: "apple",
        name: "Apple Music",
        lufs: -16.0,
        true_peak: -1.0,
        tolerance: None,
    },
    Target {
        id: "youtube",
        name: "YouTube",
        lufs: -14.0,
        true_peak: -1.0,
        tolerance: None,
    },
    Target {
        id: "amazon",
        name: "Amazon Music",
        lufs: -14.0,
        true_peak: -2.0,
        tolerance: None,
    },
    Target {
        id: "tidal",
        name: "Tidal",
        lufs: -14.0,
        true_peak: -1.0,
        tolerance: None,
    },
    Target {
        id: "ebu-r128",
        name: "EBU R128 broadcast",
        lufs: -23.0,
        true_peak: -1.0,
        tolerance: Some(0.5),
    },
    Target {
        id: "atsc-a85",
        name: "ATSC A/85 broadcast",
        lufs: -24.0,
        true_peak: -2.0,
        tolerance: Some(2.0),
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
    if let Some(tp) = true_peak {
        if tp > t.true_peak + 0.05 {
            worse(&mut status, "fail");
            notes.push(format!(
                "true peak {:.1} dBTP is over the {:.1} dBTP limit",
                tp, t.true_peak
            ));
        }
    }
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
            None if g > 2.0 => {
                worse(&mut status, "warn");
                notes.push(format!(
                    "{:.1} dB under the {:.0} LUFS target: it plays quieter than other tracks, or is turned up into a limiter",
                    g, t.lufs
                ));
            }
            None => {}
        }
    }
    Verdict {
        id: t.id,
        name: t.name,
        lufs: t.lufs,
        true_peak_dbtp: t.true_peak,
        status,
        playback_gain_db: gain,
        notes,
    }
}
