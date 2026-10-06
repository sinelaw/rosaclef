//! What a fix does to every part's level against the mix, without a render:
//! each part's energy over its stretches (as `bySection` measures them),
//! scaled by the gains the fix's faders, channel volumes and their
//! automation lanes give it, against the mix scaled the same way. It sizes
//! the anchors' fixes, holds them in their ranges when the fixes are taken
//! together, and says what each fix does to the anchors and the lead.

use super::dsp;
use super::report::{Context, Report};
use rosaclef_core::Project;
use serde_json::Value;

/// How much louder a part (its channel, its insert) is in `q` than in `p`
/// (dB): its channel volume's and its insert fader's change — or, where an
/// automation lane drives one, the lane's points' (on average). A part
/// silenced on purpose moves nothing.
pub fn unit_gain(p: &Project, q: &Project, channel: Option<usize>, insert: usize) -> f64 {
    if channel.is_some_and(|c| super::roles::silenced(p, c)) {
        return 0.0;
    }
    let ratio = |a: f64, b: f64| {
        if a <= 1e-6 && b <= 1e-6 {
            0.0
        } else {
            dsp::amp_db(b.max(1e-6) / a.max(1e-6)).clamp(-60.0, 60.0)
        }
    };
    let lane = |target: &str| -> Option<f64> {
        let i = p
            .automation
            .iter()
            .position(|l| !l.mute && l.target == target && !l.points.is_empty())?;
        let (a, b) = (&p.automation[i].points, &q.automation.get(i)?.points);
        let mean = |v: &[rosaclef_core::AutomationPoint]| {
            v.iter().map(|x| x.value).sum::<f64>() / v.len().max(1) as f64
        };
        Some(ratio(mean(a), mean(b)))
    };
    let ch = channel.map_or(0.0, |c| {
        lane(&format!("channel/{}/volume", p.channels[c].id))
            .unwrap_or_else(|| ratio(p.channels[c].volume, q.channels[c].volume))
    });
    let ins = if insert > 0 {
        lane(&format!("insert/{insert}/volume")).unwrap_or_else(|| {
            ratio(
                p.mixer.inserts[insert].volume,
                q.mixer.inserts[insert].volume,
            )
        })
    } else {
        0.0
    };
    ch + ins
}

/// One stretch of an element: each unit's energy there, and the mix's.
struct Stretch {
    units: Vec<f64>,
    mix: f64,
}

pub struct Predict<'a> {
    p: &'a Project,
    units: Vec<(Option<usize>, usize)>,
    /// Per element of the report: its units, and its stretches.
    elems: Vec<(Vec<usize>, Vec<Stretch>)>,
}

impl<'a> Predict<'a> {
    /// The parts `want` picks (the others predict nothing).
    pub fn new(
        report: &Report,
        ctx: &'a Context<'a>,
        want: &dyn Fn(&super::report::ElementOut) -> bool,
    ) -> Predict<'a> {
        let mix = &ctx.mix;
        let units = mix.units.iter().map(|u| (u.channel, u.insert)).collect();
        let elems = report
            .elements
            .iter()
            .zip(&ctx.stretch_blocks)
            .map(|(e, groups)| {
                if !want(e) {
                    return (vec![], vec![]);
                }
                let own: Vec<usize> = mix
                    .elements
                    .iter()
                    .find(|x| x.id == e.id)
                    .map(|x| x.units.clone())
                    .unwrap_or_default();
                let stretches = groups
                    .iter()
                    .map(|blocks| Stretch {
                        units: (0..mix.units.len())
                            .map(|u| blocks.iter().map(|b| mix.unit_kms(u, *b)).sum())
                            .collect(),
                        mix: blocks
                            .iter()
                            .map(|b| mix.a.kms_at(mix.master_pre, *b) as f64)
                            .sum(),
                    })
                    .collect();
                (own, stretches)
            })
            .collect();
        Predict {
            p: ctx.project,
            units,
            elems,
        }
    }

    /// Each unit's gain (dB) under `ops`: its channel volume's and its
    /// insert's fader's change — or, where an automation lane drives one,
    /// the lane's points' (on average).
    pub fn gains(&self, ops: &[Value]) -> Vec<f64> {
        let p = self.p;
        let Ok(q) = super::patched(p, ops, "fix") else {
            return vec![0.0; self.units.len()];
        };
        self.units
            .iter()
            .map(|&(channel, insert)| unit_gain(p, &q, channel, insert))
            .collect()
    }

    /// Element `k`'s level against the mix in each of its stretches: now,
    /// and under unit gains `g` (dB).
    pub fn rel(&self, k: usize, g: &[f64]) -> Vec<(f64, f64)> {
        let Some((own, stretches)) = self.elems.get(k) else {
            return vec![];
        };
        stretches
            .iter()
            .filter(|s| s.mix > 0.0)
            .map(|s| {
                let w = |u: usize| 10f64.powf(g.get(u).copied().unwrap_or(0.0) / 10.0);
                let part: f64 = own.iter().map(|u| s.units[*u]).sum();
                let part2: f64 = own.iter().map(|u| s.units[*u] * w(*u)).sum();
                let all: f64 = s.units.iter().sum();
                let all2: f64 = s.units.iter().enumerate().map(|(u, x)| x * w(u)).sum();
                let mix2 = if all > 0.0 { s.mix * all2 / all } else { s.mix };
                (dsp::db(part / s.mix), dsp::db(part2 / mix2.max(1e-30)))
            })
            .collect()
    }

    /// Element `k`'s lowest and highest level against the mix over its
    /// stretches, under unit gains `g`.
    pub fn spread(&self, k: usize, g: &[f64]) -> Option<(f64, f64)> {
        let v: Vec<f64> = self.rel(k, g).into_iter().map(|x| x.1).collect();
        let lo = v.iter().copied().min_by(|a, b| a.total_cmp(b))?;
        let hi = v.iter().copied().max_by(|a, b| a.total_cmp(b))?;
        Some((lo, hi))
    }

    /// Element `k`'s lowest level against the mix over its stretches, under
    /// unit gains `g`.
    pub fn worst(&self, k: usize, g: &[f64]) -> Option<f64> {
        self.rel(k, g)
            .into_iter()
            .map(|x| x.1)
            .min_by(|a, b| a.total_cmp(b))
    }

    /// How element `k` moves against the mix under `g` (dB, the stretch it
    /// moves most in).
    pub fn moved(&self, k: usize, g: &[f64]) -> f64 {
        self.rel(k, g)
            .into_iter()
            .map(|(a, b)| b - a)
            .max_by(|a, b| a.abs().total_cmp(&b.abs()))
            .unwrap_or(0.0)
    }
}
