//! The report: the stable JSON a mix check returns (documented by
//! `mixcheck.schema.json`), and how it is computed from the measurements.
//! Numbers are rounded to 0.1 dB (percentages to whole numbers); silent
//! elements are left out.

use super::analyze::{Analysis, GrSeries, F_LL, F_LR, F_PEAK, F_RR, F_SIX, F_TP};
use super::dsp::{self, r1, BANDS, BAND_NAMES, BARKS};
use super::model::{self, Audibility, Change, Ear, Element, Loudness, Mix};
use super::options::{By, Check, Options, Threshold};
use super::timeline::{self, Resolved, Timeline};
use rosaclef_core::Project;
use serde::Serialize;
use serde_json::{json, Value};

pub const VERSION: u32 = 1;

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub version: u32,
    pub range: RangeOut,
    pub master: MasterOut,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<super::targets::Verdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<Value>,
    pub per_bar: Vec<Row>,
    pub elements: Vec<ElementOut>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gain_reduction: Vec<GrOut>,
    pub findings: Vec<FindingOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<HistoryOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub what_if: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compare: Option<Value>,
    pub render: RenderOut,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Asked with `--focus`: the summary lists its parts even when ok.
    #[serde(skip)]
    pub focused: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct RangeOut {
    pub from_bar: u32,
    pub to_bar: u32,
    pub from_beat: f64,
    pub to_beat: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    pub by: String,
    /// Seconds of music measured (every pass).
    pub seconds: f64,
    /// Some bar in the range plays more than once (rows carry `pass`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub repeats: bool,
    pub threshold: &'static str,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GrStat {
    pub max: f64,
    pub mean: f64,
    #[serde(rename = "pctTimeAbove3")]
    pub pct_time_above3: f64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Spectrum {
    pub bands: Vec<&'static str>,
    pub db: Vec<f64>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct MasterOut {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrated_lufs: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_term_lufs_max: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub momentary_lufs_max: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub true_peak_dbtp: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_peak_dbfs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rms_dbfs: Option<f64>,
    /// > 0: the limiter is fighting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_limiter_peak_dbfs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_effects_peak_dbfs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limiter_gain_reduction_db: Option<GrStat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compressor_gain_reduction_db: Option<GrStat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plr_db: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crest_factor_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lra: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spectrum: Option<Spectrum>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Contributor {
    pub id: String,
    pub share_of_energy_pct: f64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bar: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pass: Option<u32>,
    pub from_beat: f64,
    pub to_beat: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_bar: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lufs: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lufs_momentary_max: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lufs_short_term_max: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rms_dbfs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_dbfs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub true_peak_dbtp: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_limiter_peak_dbfs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limiter_gr_max_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gain_reduction_db: Option<serde_json::Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spectrum_db: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_contributors: Option<Vec<Contributor>>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct MaskedBy {
    pub id: String,
    pub band_hz: [f64; 2],
    pub masking_db: f64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AudibilityOut {
    pub audible_fraction_pct: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub masked_by: Option<Vec<MaskedBy>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dominant_band_hz: Option<[f64; 2]>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct LevelOut {
    pub fader_db: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_volume: Option<f64>,
    pub automated: Vec<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub why: String,
    pub patch: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_relative_to_mix_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_audible_fraction_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<Value>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ElementOut {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<&'static str>,
    /// The song's lead (one per song): judged by its level in the mix too.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub lead: bool,
    pub insert: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rms_dbfs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_dbfs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lufs: Option<Option<f64>>,
    /// Its loudness (at the master) minus the mix's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative_to_mix_db: Option<Option<f64>>,
    pub share_of_energy_pct: f64,
    /// Share of the range it plays in.
    pub active_pct: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audibility: Option<AudibilityOut>,
    /// Its own spectrum at the master where it plays: the six bands of
    /// `master.spectrum` (dB).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spectrum_db: Option<Vec<f64>>,
    pub level: LevelOut,
    /// Its level against the mix per section (per 4 bars without
    /// sections), each pass apart: where it plays 2 s or more.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub by_section: Vec<UnderMix>,
    /// The stretches of `bySection` where it is under: the lead under its
    /// floor when the whole range's level hides it; another part playing
    /// 10 dB or more under its own level in its loudest stretch, and 15 dB
    /// under the mix (a dropout).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub buried_in: Vec<UnderMix>,
    /// inaudible | buried | ok | dominant | overloading
    pub verdict: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<Suggestion>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct UnderMix {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    pub from_bar: u32,
    pub to_bar: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pass: Option<u32>,
    pub relative_to_mix_db: f64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GrOut {
    pub id: String,
    pub effect: usize,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub stat: GrStat,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FindingOut {
    /// warn | info
    pub severity: &'static str,
    pub rule: &'static str,
    /// Stable: for `rosaclef critic --audio --fix KEY` and suppressing.
    pub key: String,
    #[serde(rename = "where")]
    pub at: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_bar: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_bar: Option<u32>,
    /// The written beat `fromBar` starts on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_beat: Option<f64>,
    pub fix: Vec<Value>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub fix_label: String,
    /// `--verify`: the check again with `fix` applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<Value>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct HistoryOut {
    pub step_seconds: f64,
    pub t: Vec<f64>,
    pub momentary: Vec<Option<f64>>,
    pub short_term: Vec<Option<f64>>,
    pub true_peak: Vec<Option<f64>>,
    pub limiter_gr: Vec<f64>,
    pub bars: Vec<Value>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct RenderOut {
    pub cached: bool,
    pub ms: u64,
    pub preroll_beats: f64,
    pub renders: u32,
    pub sample_rate: f64,
}

// ---------------------------------------------------------------- helpers

fn opt_db(p: f64) -> Option<f64> {
    (p > 1e-12).then(|| r1(dsp::db(p)))
}

fn amp(a: f64) -> f64 {
    r1(dsp::amp_db(a))
}

/// Correlations are rounded to 0.01.
fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

pub fn pct(x: f64) -> f64 {
    (x * 100.0).round()
}

/// Loudness numbers of one stream over the inside region.
pub struct StreamLoudness {
    pub integrated: Option<f64>,
    pub momentary_max: Option<f64>,
    pub short_max: Option<f64>,
    pub lra: Option<f64>,
    /// Per inside block: momentary and short-term energies.
    pub momentary: Vec<f64>,
    pub short: Vec<f64>,
}

pub fn loudness_of(a: &Analysis, blocks: &[usize], kms: &dyn Fn(usize) -> f64) -> StreamLoudness {
    let l = Loudness { a };
    let momentary: Vec<f64> = blocks.iter().map(|b| l.window(kms, *b, 4)).collect();
    let short: Vec<f64> = blocks.iter().map(|b| l.window(kms, *b, 30)).collect();
    let max = |v: &[f64]| {
        let m = v.iter().copied().fold(0.0, f64::max);
        (m > 0.0).then(|| dsp::lufs(m))
    };
    StreamLoudness {
        integrated: model::integrated(&momentary),
        momentary_max: max(&momentary),
        short_max: max(&short),
        lra: model::loudness_range(&short),
        momentary,
        short,
    }
}

/// The master-style numbers of stream `s` (the output, or a reference).
pub fn master_numbers(
    a: &Analysis,
    s: usize,
    hops: &[usize],
    blocks: &[usize],
    o: &Options,
) -> MasterOut {
    let kms = |b: usize| a.kms_at(s, b) as f64;
    let l = loudness_of(a, blocks, &kms);
    let mut peak = 0f64;
    let mut tp = 0f64;
    let (mut ll, mut rr, mut lr) = (0.0, 0.0, 0.0);
    let mut six = [0f64; BANDS];
    let mut corr_min: Option<f64> = None;
    let loudest = hops
        .iter()
        .map(|h| {
            let f = a.frame(s, *h);
            (f[F_LL] + f[F_RR]) as f64
        })
        .fold(0.0, f64::max);
    for &h in hops {
        let f = a.frame(s, h);
        peak = peak.max(f[F_PEAK] as f64);
        tp = tp.max(f[F_TP] as f64);
        let (x, y, z) = (f[F_LL] as f64, f[F_RR] as f64, f[F_LR] as f64);
        ll += x;
        rr += y;
        lr += z;
        if x + y > loudest * 1e-3 {
            if let Some(c) = model::correlation(x, y, z) {
                corr_min = Some(corr_min.map_or(c, |m: f64| m.min(c)));
            }
        }
        for (b, v) in six.iter_mut().enumerate() {
            *v += f[F_SIX + b] as f64;
        }
    }
    let n = hops.len().max(1) as f64;
    let rms = (ll + rr) / (2.0 * n);
    let mut m = MasterOut::default();
    if o.has(Check::Levels) {
        m.integrated_lufs = Some(l.integrated.map(r1));
        m.short_term_lufs_max = Some(l.short_max.map(r1));
        m.momentary_lufs_max = Some(l.momentary_max.map(r1));
        m.true_peak_dbtp = Some(amp(tp));
        m.sample_peak_dbfs = Some(amp(peak));
        m.rms_dbfs = Some(r1(dsp::db(rms)));
    }
    if o.has(Check::Dynamics) {
        m.plr_db = Some(l.integrated.map(|i| r1(dsp::amp_db(tp) - i)));
        m.crest_factor_db = Some(r1(dsp::amp_db(peak) - dsp::db(rms)));
        m.lra = Some(l.lra.map(r1));
    }
    if o.has(Check::Stereo) {
        m.correlation = Some(json!({
            "mean": model::correlation(ll, rr, lr).map(r2),
            "min": corr_min.map(r2),
            "monoLossDb": model::mono_loss(ll, rr, lr).map(r1),
        }));
    }
    if o.has(Check::Spectrum) {
        m.spectrum = Some(Spectrum {
            bands: BAND_NAMES.to_vec(),
            db: six.iter().map(|v| r1(dsp::db(*v / n))).collect(),
        });
    }
    m
}

fn gr_stat(series: &[&GrSeries], hops: &[usize]) -> Option<GrStat> {
    if series.is_empty() || hops.is_empty() {
        return None;
    }
    let (mut max, mut sum, mut above) = (0f64, 0f64, 0usize);
    for &h in hops {
        let m: f64 = series.iter().map(|g| g.max[h] as f64).fold(0.0, f64::max);
        let mean: f64 = series.iter().map(|g| g.mean[h] as f64).sum();
        max = max.max(m);
        sum += mean;
        if mean >= 3.0 {
            above += 1;
        }
    }
    Some(GrStat {
        max: r1(max),
        mean: r1(sum / hops.len() as f64),
        pct_time_above3: pct(above as f64 / hops.len() as f64),
    })
}

// ------------------------------------------------------------------ build

/// Everything the findings and the text summary read besides the report.
pub struct Context<'a> {
    pub mix: Mix<'a>,
    pub timeline: &'a Timeline,
    pub project: &'a Project,
    pub roles: Vec<&'static str>,
    pub audibility: Vec<(String, Audibility)>,
    pub include_master: bool,
    /// The inside hops of each row of `perBar`.
    pub rows_hops: Vec<Vec<usize>>,
}

/// Elements named by `--focus` (all of them when it is empty) and whether
/// the master is in focus.
fn focused(mix: &Mix, p: &Project, focus: &[String]) -> Result<(Vec<Element>, bool), String> {
    if focus.is_empty() {
        return Ok((mix.elements.clone(), true));
    }
    let mut out: Vec<Element> = vec![];
    let mut master = false;
    for f in focus {
        let f = f.trim();
        if f.eq_ignore_ascii_case("master") || f == "insert:0" || f == "0" {
            master = true;
            continue;
        }
        let ch = f.strip_prefix("channel:").unwrap_or(f);
        if let Some(c) = p.channels.iter().position(|x| x.id == ch) {
            if let Some(e) = mix.elements.iter().find(|e| e.channel == Some(c)) {
                out.push(e.clone());
                continue;
            }
        }
        let ins = f.strip_prefix("insert:").unwrap_or(f);
        let ins = ins.split('/').next().unwrap_or(ins);
        let index = ins.parse::<usize>().ok().or_else(|| {
            p.mixer
                .inserts
                .iter()
                .position(|x| x.name.eq_ignore_ascii_case(f))
        });
        match index {
            Some(0) => master = true,
            Some(i) if i < p.mixer.inserts.len() => {
                let e = mix
                    .elements
                    .iter()
                    .find(|e| e.channel.is_none() && e.insert == i)
                    .cloned()
                    .or_else(|| mix.insert_element(i));
                match e {
                    Some(e) => out.push(e),
                    None => return Err(format!("focus: insert {i} has nothing to measure")),
                }
            }
            _ => {
                return Err(format!(
                    "focus: no channel or insert {f:?} (channel ids, insert indices or names, or master)"
                ))
            }
        }
    }
    let mut seen: Vec<String> = vec![];
    out.retain(|e| {
        let fresh = !seen.contains(&e.id);
        seen.push(e.id.clone());
        fresh
    });
    Ok((out, master))
}

/// Where a part sits under the mix stretch by stretch.
struct Under {
    stretches: Vec<UnderMix>,
    /// The same, in written beats.
    beats: Vec<(f64, f64)>,
    /// The blocks of those stretches, and its level against the mix there.
    blocks: Vec<usize>,
    rel: f64,
    /// Their share of the blocks where it plays.
    share: f64,
}

/// A stretch (a section, or 4 bars) and pass: its blocks where the part
/// plays, the part's energy there and the mix's.
struct Stretch {
    key: (i64, u32),
    blocks: Vec<usize>,
    part: f64,
    mix: f64,
}

impl Stretch {
    /// Its level against the mix (dB).
    fn rel(&self) -> f64 {
        dsp::db(self.part / self.mix)
    }
    /// Its own level (dB, K-weighted mean square).
    fn own(&self) -> f64 {
        dsp::db(self.part / self.blocks.len().max(1) as f64)
    }
}

/// A part's stretches (2 s of it or more), and how many blocks it plays in.
fn stretches_of(
    mix: &Mix,
    t: &Timeline,
    sections: &[timeline::Section],
    e: &Element,
    blocks: &[usize],
) -> (Vec<Stretch>, usize) {
    let a = mix.a;
    let own = |b: usize| -> f64 { e.units.iter().map(|u| mix.unit_kms(*u, b)).sum() };
    let loudest = blocks.iter().map(|b| own(*b)).fold(0.0, f64::max);
    let mut groups: Vec<Stretch> = vec![];
    if loudest <= 0.0 {
        return (groups, 0);
    }
    let mut playing = 0usize;
    for &b in blocks {
        let x = own(b);
        if x <= loudest * 1e-4 {
            continue;
        }
        playing += 1;
        let pos = a.lblocks[b];
        let bar = t.bar_of(pos.beat) as i64;
        let k = sections
            .iter()
            .position(|s| pos.beat >= s.start - 1e-6 && pos.beat < s.end - 1e-6)
            .map(|i| i as i64)
            .unwrap_or(1000 + (bar - 1) / 4);
        let key = (k, t.pass_of(pos.span as usize, pos.beat));
        let m = a.kms_at(mix.master_pre, b) as f64;
        match groups.last_mut().filter(|g| g.key == key) {
            Some(g) => {
                g.blocks.push(b);
                g.part += x;
                g.mix += m;
            }
            None => groups.push(Stretch {
                key,
                blocks: vec![b],
                part: x,
                mix: m,
            }),
        }
    }
    // At least 2 s of it in a stretch to judge it.
    groups.retain(|g| g.blocks.len() >= 20 && g.mix > 0.0 && g.part > 0.0);
    (groups, playing)
}

/// A stretch for the report: its bars, pass and section.
fn stretch_out(mix: &Mix, t: &Timeline, sections: &[timeline::Section], g: &Stretch) -> UnderMix {
    let bar = |b: usize| t.bar_of(mix.a.lblocks[b].beat);
    UnderMix {
        section: usize::try_from(g.key.0)
            .ok()
            .and_then(|i| sections.get(i))
            .map(|s| s.name.clone()),
        from_bar: bar(g.blocks[0]),
        to_bar: bar(*g.blocks.last().unwrap_or(&g.blocks[0])),
        pass: (g.key.1 > 1).then_some(g.key.1),
        relative_to_mix_db: r1(g.rel()),
    }
}

/// The stretches of a part that are `under`, merged.
fn under_mix(
    mix: &Mix,
    t: &Timeline,
    sections: &[timeline::Section],
    groups: &[Stretch],
    playing: usize,
    under: &dyn Fn(&Stretch) -> bool,
) -> Option<Under> {
    let under: Vec<&Stretch> = groups.iter().filter(|g| under(g)).collect();
    if under.is_empty() || playing == 0 {
        return None;
    }
    let mut stretches: Vec<UnderMix> = vec![];
    for g in &under {
        let o = stretch_out(mix, t, sections, g);
        match stretches.last_mut() {
            Some(s)
                if s.to_bar + 1 >= o.from_bar && s.pass == o.pass && o.from_bar >= s.from_bar =>
            {
                s.to_bar = o.to_bar;
                s.relative_to_mix_db = s.relative_to_mix_db.min(o.relative_to_mix_db);
                if s.section != o.section {
                    s.section = None;
                }
            }
            _ => stretches.push(o),
        }
    }
    let blocks: Vec<usize> = under
        .iter()
        .flat_map(|g| g.blocks.iter().copied())
        .collect();
    let (x, m) = under
        .iter()
        .fold((0.0, 0.0), |(x, m), g| (x + g.part, m + g.mix));
    let beats = stretches
        .iter()
        .map(|s| (t.bar_start(s.from_bar), t.bar_start(s.to_bar + 1)))
        .collect();
    Some(Under {
        beats,
        stretches,
        share: blocks.len() as f64 / playing as f64,
        blocks,
        rel: dsp::db(x / m),
    })
}

/// The song's lead: the part named like one (lead, vocal, melody, topline,
/// solo), else the loudest the Critic reads as a lead. One song, one lead;
/// other melodic parts are judged as parts.
fn lead_of(mix: &Mix, roles: &[&'static str], hops: &[usize]) -> Option<String> {
    let energy = |e: &Element| -> f64 {
        hops.iter()
            .map(|h| e.units.iter().map(|u| mix.unit_ms(*u, *h)).sum::<f64>())
            .sum()
    };
    let named = |e: &Element| {
        let n = format!("{} {}", e.id, e.name).to_ascii_lowercase();
        ["lead", "vocal", "vox", "melody", "topline", "solo"]
            .iter()
            .any(|w| n.contains(w))
    };
    let loudest = |pick: &dyn Fn(&Element) -> bool| {
        mix.elements
            .iter()
            .filter(|e| e.channel.is_some() && pick(e))
            .map(|e| (e, energy(e)))
            .filter(|x| x.1 > 0.0)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|x| x.0.id.clone())
    };
    loudest(&named).or_else(|| {
        loudest(&|e: &Element| e.channel.and_then(|c| roles.get(c).copied()) == Some("lead"))
    })
}

/// The written beat where row key `k` ends (for `--by`).
fn row_key(
    t: &Timeline,
    by: By,
    sections: &[timeline::Section],
    span: u32,
    beat: f64,
) -> (i64, u32) {
    let pass = t.pass_of(span as usize, beat);
    let k = match by {
        By::Bar => t.bar_of(beat) as i64,
        By::Section => sections
            .iter()
            .position(|s| beat >= s.start - 1e-6 && beat < s.end - 1e-6)
            .map(|i| i as i64)
            .unwrap_or(-1),
        By::Beats(n) => (beat / n + 1e-9).floor() as i64,
    };
    (k, pass)
}

/// A row of `perBar` as it is gathered.
struct RowAcc {
    key: (i64, u32),
    hops: Vec<usize>,
    blocks: Vec<usize>,
    /// The first and last written beats of its blocks.
    first: f64,
    last: f64,
}

fn row_of(rows: &mut Vec<RowAcc>, key: (i64, u32)) -> &mut RowAcc {
    let i = rows.iter().rposition(|r| r.key == key).unwrap_or_else(|| {
        rows.push(RowAcc {
            key,
            hops: vec![],
            blocks: vec![],
            first: f64::INFINITY,
            last: 0.0,
        });
        rows.len() - 1
    });
    &mut rows[i]
}

pub fn build<'a>(
    a: &'a Analysis,
    p: &'a Project,
    t: &'a Timeline,
    res: &Resolved,
    o: &Options,
) -> Result<(Report, Context<'a>), String> {
    let mix = Mix::new(a, p);
    let sections = timeline::sections(p);
    let roles = rosaclef_core::critic::channel_roles(p);
    let (elements, include_master) = focused(&mix, p, &o.focus)?;
    let hops = mix.inside.clone();
    let blocks = mix.inside_blocks.clone();
    let mut r = Report {
        version: VERSION,
        focused: !o.focus.is_empty(),
        ..Default::default()
    };
    let repeats = res.ranges.iter().any(|(x, y)| t.repeats_in(*x, *y));
    r.range = RangeOut {
        from_bar: res.from_bar,
        to_bar: res.to_bar,
        from_beat: res.from_beat,
        to_beat: res.to_beat,
        section: res.section.clone(),
        by: match o.by {
            By::Bar => "bar".into(),
            By::Section => "section".into(),
            By::Beats(n) => format!("{}-beats", rosaclef_core::format::format_f64(n)),
        },
        seconds: r1(blocks.len() as f64 * a.lblock as f64 / a.sr as f64),
        repeats,
        threshold: o.threshold.name(),
    };

    // ---- the master
    r.master = master_numbers(a, mix.master_out, &hops, &blocks, o);
    let peak_of = |s: usize, hs: &[usize]| hs.iter().map(|h| mix.peak(s, *h)).fold(0.0, f64::max);
    if o.has(Check::Levels) {
        r.master.pre_limiter_peak_dbfs = Some(amp(peak_of(mix.master_lim, &hops)));
        r.master.pre_effects_peak_dbfs = Some(amp(peak_of(mix.master_pre, &hops)));
    }
    let master_lims = a.master_gr("limiter");
    if o.has(Check::GainReduction) {
        r.master.limiter_gain_reduction_db = gr_stat(&master_lims, &hops);
        r.master.compressor_gain_reduction_db = gr_stat(&a.master_gr("compressor"), &hops);
        for g in &a.gr {
            r.gain_reduction.push(GrOut {
                id: model::insert_id(p, g.insert),
                effect: g.fx,
                kind: g.kind.clone(),
                stat: gr_stat(&[g], &hops).unwrap_or_default(),
            });
        }
    }
    if let Some(tg) = o.target.as_deref().and_then(super::targets::find) {
        let out_kms = |b: usize| a.kms_at(mix.master_out, b) as f64;
        let l = loudness_of(a, &blocks, &out_kms);
        let tp = hops.iter().map(|h| mix.true_peak(*h)).fold(0.0, f64::max);
        r.target = Some(super::targets::judge(
            tg,
            l.integrated.map(r1),
            (tp > 0.0).then(|| amp(tp)),
        ));
    }

    // ---- rows
    let out_kms = |b: usize| a.kms_at(mix.master_out, b) as f64;
    // One row per key, in playing order.
    let mut rows: Vec<RowAcc> = vec![];
    for &h in &hops {
        let pos = a.hops[h];
        row_of(&mut rows, row_key(t, o.by, &sections, pos.span, pos.beat))
            .hops
            .push(h);
    }
    for &b in &blocks {
        let pos = a.lblocks[b];
        let row = row_of(&mut rows, row_key(t, o.by, &sections, pos.span, pos.beat));
        row.blocks.push(b);
        row.first = row.first.min(pos.beat);
        row.last = row.last.max(pos.beat);
    }
    let other_dyn: Vec<&GrSeries> = a.gr.iter().filter(|g| g.insert != 0).collect();
    for acc in &rows {
        let (key, hs, bs) = (acc.key, &acc.hops, &acc.blocks);
        let (b0, b1) = (acc.first, acc.last);
        let mut row = Row::default();
        let first = if b0.is_finite() { b0 } else { 0.0 };
        match o.by {
            By::Bar => {
                let bar = key.0 as u32;
                row.bar = Some(bar);
                row.from_beat = t.bar_start(bar);
                row.to_beat = t.bar_start(bar + 1);
            }
            By::Section => {
                match usize::try_from(key.0).ok().and_then(|k| sections.get(k)) {
                    Some(s) => {
                        row.section = Some(s.name.clone());
                        row.from_beat = s.start;
                        row.to_beat = s.end;
                    }
                    None => {
                        row.section = Some("(no section)".into());
                        row.from_beat = t.bar_start(t.bar_of(first));
                        row.to_beat = t.bar_start(t.bar_of(b1) + 1);
                    }
                }
                row.bar = Some(t.bar_of(row.from_beat));
                row.to_bar = Some(t.bar_of((row.to_beat - 1e-6).max(row.from_beat)));
            }
            By::Beats(n) => {
                row.from_beat = key.0 as f64 * n;
                row.to_beat = row.from_beat + n;
                row.bar = Some(t.bar_of(row.from_beat));
            }
        }
        if repeats || key.1 > 1 {
            row.pass = Some(key.1);
        }
        if o.has(Check::Levels) {
            // Windows reach back (400 ms, 3 s): a row longer than one
            // counts only those that start inside it, not the row before.
            let l = loudness_of(a, bs, &out_kms);
            let inside = |v: &[f64], n: usize| -> Vec<f64> {
                if v.len() >= n {
                    v[n - 1..].to_vec()
                } else {
                    v.to_vec()
                }
            };
            let (mom, short) = (inside(&l.momentary, 4), inside(&l.short, 30));
            let max = |v: &[f64]| {
                let m = v.iter().copied().fold(0.0, f64::max);
                (m > 0.0).then(|| r1(dsp::lufs(m)))
            };
            row.lufs = Some(model::integrated(&mom).map(r1));
            row.lufs_momentary_max = Some(max(&mom));
            row.lufs_short_term_max = Some(max(&short));
            let ms: f64 =
                hs.iter().map(|h| mix.ms(mix.master_out, *h)).sum::<f64>() / hs.len().max(1) as f64;
            row.rms_dbfs = opt_db(ms).or(Some(-120.0));
            row.peak_dbfs = Some(amp(peak_of(mix.master_out, hs)));
            row.true_peak_dbtp = Some(amp(hs
                .iter()
                .map(|h| mix.true_peak(*h))
                .fold(0.0, f64::max)));
            row.pre_limiter_peak_dbfs = Some(amp(peak_of(mix.master_lim, hs)));
        }
        if o.has(Check::GainReduction) {
            if let Some(g) = gr_stat(&master_lims, hs) {
                row.limiter_gr_max_db = Some(g.max);
            }
            let mut map = serde_json::Map::new();
            for g in &other_dyn {
                let m = hs.iter().map(|h| g.max[*h] as f64).fold(0.0, f64::max);
                if m >= 0.5 {
                    map.insert(
                        format!("{}#{}", model::insert_id(p, g.insert), g.fx),
                        json!(r1(m)),
                    );
                }
            }
            if !map.is_empty() {
                row.gain_reduction_db = Some(map);
            }
        }
        if o.has(Check::Stereo) {
            row.correlation = Some(mix.correlation(mix.master_out, hs).map(r2));
        }
        if o.has(Check::Spectrum) {
            let mut six = [0f64; BANDS];
            for &h in hs {
                let s = mix.six(mix.master_out, h);
                for b in 0..BANDS {
                    six[b] += s[b];
                }
            }
            let n = hs.len().max(1) as f64;
            row.spectrum_db = Some(six.iter().map(|v| r1(dsp::db(v / n))).collect());
        }
        if o.has(Check::Levels) {
            row.top_contributors = Some(
                mix.ranked(hs, &|u, h| mix.unit_ms(u, h))
                    .iter()
                    .filter(|x| x.1 >= 0.05)
                    .take(3)
                    .map(|x| Contributor {
                        id: mix.units[x.0].id.clone(),
                        share_of_energy_pct: pct(x.1),
                    })
                    .collect(),
            );
        }
        r.per_bar.push(row);
    }

    // ---- elements
    let pre_kms = |b: usize| a.kms_at(mix.master_pre, b) as f64;
    let mix_lufs = loudness_of(a, &blocks, &pre_kms).integrated;
    let pre_total: f64 = hops.iter().map(|h| mix.ms(mix.master_pre, *h)).sum();
    let want_aud = o.has(Check::Audibility) || o.has(Check::Masking);
    let ear = Ear::default();
    let (aud, _) = if want_aud {
        model::audibility(&mix, &ear, model::AUDIBLE_SHARE, &elements, &[])
    } else {
        (vec![Audibility::default(); elements.len()], vec![])
    };
    // The master overloads where its limiter's input goes over 0 dBFS and
    // the limiter works for it (3 dB or more), not where it catches a peak.
    let overloaded = r.per_bar.iter().any(|x| {
        x.pre_limiter_peak_dbfs.unwrap_or(-99.0) > 0.0
            && x.limiter_gr_max_db.is_none_or(|g| g >= 3.0)
    });
    let (inaudible_at, buried_at) = match o.threshold {
        Threshold::Strict => (0.35, 0.7),
        Threshold::Normal => (0.25, 0.6),
        Threshold::Loose => (0.15, 0.45),
    };
    let lead = lead_of(&mix, &roles, &hops);
    let lead_floor = o.threshold.lead_floor_db();
    let mut outs: Vec<(ElementOut, usize)> = vec![];
    let mut unders: Vec<Option<Under>> = vec![];
    let mut kept_aud = vec![];
    for (i, e) in elements.iter().enumerate() {
        let levels: Vec<f64> = hops
            .iter()
            .map(|h| e.units.iter().map(|u| mix.unit_ms(*u, *h)).sum())
            .collect();
        let loudest = levels.iter().copied().fold(0.0, f64::max);
        let active: Vec<usize> = hops
            .iter()
            .zip(&levels)
            .filter(|(_, l)| **l > 1e-8 && **l > loudest * 1e-4)
            .map(|(h, _)| *h)
            .collect();
        if active.is_empty() {
            continue;
        }
        let own_ms: f64 =
            active.iter().map(|h| mix.ms(e.stream, *h)).sum::<f64>() / active.len() as f64;
        let own_peak = peak_of(e.stream, &hops);
        let ekms = |b: usize| -> f64 { e.units.iter().map(|u| mix.unit_kms(*u, b)).sum() };
        let el = loudness_of(a, &blocks, &ekms);
        let share = if pre_total > 0.0 {
            levels.iter().sum::<f64>() / pre_total
        } else {
            0.0
        };
        let rel = match (el.integrated, mix_lufs) {
            (Some(x), Some(m)) => Some(r1(x - m)),
            _ => None,
        };
        let au = &aud[i];
        let frac = au.fraction();
        let is_lead = lead.as_deref() == Some(e.id.as_str());
        // A lead under the mix only in its choruses (say) is hidden by the
        // whole range's level: it is judged stretch by stretch too.
        let (groups, playing) = stretches_of(&mix, t, &sections, e, &blocks);
        // Another part: where it drops out — its own level 10 dB or more
        // under its loudest stretch's, and 15 dB under the mix.
        let top = groups
            .iter()
            .map(|g| g.own())
            .fold(f64::NEG_INFINITY, f64::max);
        let drop = o.threshold.dropout_db();
        let dropped = |g: &Stretch| g.own() < top - drop && g.rel() < -15.0;
        let floored = |g: &Stretch| g.rel() < lead_floor;
        let under = under_mix(
            &mix,
            t,
            &sections,
            &groups,
            playing,
            if is_lead { &floored } else { &dropped },
        );
        let verdict = if want_aud && frac < inaudible_at {
            "inaudible"
        } else if want_aud && frac < buried_at {
            "buried"
        } else if is_lead
            && (rel.is_some_and(|r| r < lead_floor)
                || under.as_ref().is_some_and(|u| u.share >= 0.2))
        {
            // Heard, but far under the mix: a lead the song leans on.
            "buried"
        } else if own_peak > 1.0 || (overloaded && share >= 0.4) {
            "overloading"
        } else if share >= 0.45 && rel.is_some_and(|r| r >= -3.0) {
            "dominant"
        } else {
            "ok"
        };
        let ins = &p.mixer.inserts[e.insert];
        let automated: Vec<String> = p
            .automation
            .iter()
            .filter(|l| {
                !l.mute
                    && (l.target == format!("insert/{}/volume", e.insert)
                        || e.channel.is_some_and(|c| {
                            l.target == format!("channel/{}/volume", p.channels[c].id)
                        }))
            })
            .map(|l| l.id.clone())
            .collect();
        let mut out = ElementOut {
            id: e.id.clone(),
            name: e.name.clone(),
            kind: if e.channel.is_some() {
                "channel"
            } else {
                "insert"
            },
            // One lead per song: another part the Critic reads as a lead
            // is a melody here.
            role: e.channel.and_then(|c| roles.get(c).copied()).map(|r| {
                if r == "lead" && !is_lead {
                    "melody"
                } else {
                    r
                }
            }),
            lead: is_lead,
            insert: e.insert,
            share_of_energy_pct: pct(share),
            active_pct: pct(active.len() as f64 / hops.len().max(1) as f64),
            level: LevelOut {
                fader_db: r1(dsp::amp_db(ins.volume)),
                channel_volume: e.channel.map(|c| p.channels[c].volume),
                automated,
            },
            verdict,
            ..Default::default()
        };
        if o.has(Check::Levels) {
            out.rms_dbfs = opt_db(own_ms);
            out.peak_dbfs = Some(amp(own_peak));
            out.lufs = Some(el.integrated.map(r1));
            out.relative_to_mix_db = Some(rel);
        }
        if o.has(Check::Stereo) {
            out.correlation = Some(mix.correlation(e.stream, &active).map(r2));
        }
        if o.has(Check::Spectrum) {
            let mut six = [0f64; BANDS];
            for &h in &active {
                for u in &e.units {
                    for (a, b) in six.iter_mut().zip(mix.unit_six(*u, h)) {
                        *a += b;
                    }
                }
            }
            let n = active.len().max(1) as f64;
            out.spectrum_db = Some(six.iter().map(|v| r1(dsp::db(v / n))).collect());
        }
        if want_aud {
            out.audibility = Some(audibility_out(&mix, au, o));
        }
        if o.has(Check::Levels) && groups.len() > 1 {
            out.by_section = groups
                .iter()
                .map(|g| stretch_out(&mix, t, &sections, g))
                .collect();
        }
        // The stretches tell what the whole range's level hides.
        if let Some(u) = under
            .as_ref()
            .filter(|_| !(is_lead && rel.is_some_and(|r| r < lead_floor)))
        {
            out.buried_in = u.stretches.clone();
        }
        unders.push(under);
        kept_aud.push((e.id.clone(), au.clone()));
        outs.push((out, i));
    }

    // ---- suggestions (predicted by the model)
    let flagged: Vec<usize> = outs
        .iter()
        .enumerate()
        .filter(|(_, (x, _))| matches!(x.verdict, "inaudible" | "buried"))
        .map(|(k, _)| k)
        .collect();
    if want_aud && !flagged.is_empty() {
        let gains = [
            1.5, 3.0, 4.5, 6.0, 7.5, 9.0, 10.5, 12.0, 15.0, 18.0, 21.0, 24.0, 30.0,
        ];
        let mut trial_elems = vec![];
        let mut trials = vec![];
        let mut cuts = vec![];
        let mut bals = vec![];
        for &k in &flagged {
            let ei = outs[k].1;
            let au = &aud[ei];
            let mut t: Vec<Change> = gains
                .iter()
                .map(|g| Change {
                    gain_db: *g,
                    masker: None,
                })
                .collect();
            let cut = super::findings::masker_cut(&mix, au, &elements[ei]);
            if let Some((u, bands, _, _)) = &cut {
                t.push(Change {
                    gain_db: 0.0,
                    masker: Some((*u, *bands)),
                });
            }
            cuts.push(cut);
            // A lead under the mix: its balance, as one change.
            // Over the whole range, or over the stretches where it is under.
            let rel = outs[k].0.relative_to_mix_db.flatten();
            let bal = match (rel, &unders[k]) {
                (Some(r), _) if outs[k].0.lead && r < lead_floor => {
                    super::findings::balance(p, &mix, &blocks, &[], &elements[ei], au, r)
                }
                (_, Some(u)) if outs[k].0.lead && u.share >= 0.2 => {
                    super::findings::balance(p, &mix, &u.blocks, &u.beats, &elements[ei], au, u.rel)
                }
                _ => None,
            };
            if let Some(b) = &bal {
                t.push(b.change.clone());
            }
            bals.push(bal);
            trial_elems.push(elements[ei].clone());
            trials.push(t);
        }
        let (_, fr) = model::audibility(&mix, &ear, model::AUDIBLE_SHARE, &trial_elems, &trials);
        for (n, &k) in flagged.iter().enumerate() {
            let e = &trial_elems[n];
            let (out, _) = &mut outs[k];
            out.suggestions = super::findings::suggestions(
                p,
                &mix,
                e,
                out,
                &gains,
                &fr[n],
                cuts[n].as_ref(),
                bals[n].as_ref(),
                buried_at,
            );
        }
    }
    r.elements = outs.into_iter().map(|x| x.0).collect();

    // ---- history
    if o.history {
        r.history = Some(history(&mix, t, &blocks, &hops, &master_lims));
    }
    r.warnings = a.warnings.clone();
    r.warnings.extend(res.note.clone());
    r.render.preroll_beats = r1(a.preroll);
    r.render.sample_rate = a.sr as f64;
    let ctx = Context {
        mix,
        timeline: t,
        project: p,
        roles,
        audibility: kept_aud,
        include_master,
        rows_hops: rows.into_iter().map(|r| r.hops).collect(),
    };
    Ok((r, ctx))
}

fn audibility_out(mix: &Mix, au: &Audibility, o: &Options) -> AudibilityOut {
    let mut out = AudibilityOut {
        audible_fraction_pct: pct(au.fraction()),
        ..Default::default()
    };
    if !o.has(Check::Masking) {
        return out;
    }
    // Who covers it, when it is covered more than now and then.
    let total: f64 = au.maskers.iter().map(|m| m.weight).sum();
    let covered = au.fraction() < 0.95;
    out.masked_by = Some(
        au.maskers
            .iter()
            .filter(|m| covered && total > 0.0 && m.weight / total >= 0.1)
            .take(3)
            .map(|m| {
                let (lo, hi) = band_span(&m.bands, 0.15);
                MaskedBy {
                    id: mix.units[m.unit].id.clone(),
                    band_hz: model::band_hz(lo, hi),
                    masking_db: r1(m.db_sum / m.n.max(1) as f64),
                }
            })
            .collect(),
    );
    let sp = &au.spectrum;
    let peak = (0..BARKS)
        .max_by(|x, y| sp[*x].total_cmp(&sp[*y]))
        .unwrap_or(0);
    if sp[peak] > 0.0 {
        let floor = sp[peak] / 4.0; // within 6 dB
        let (mut lo, mut hi) = (peak, peak);
        while lo > 0 && sp[lo - 1] >= floor {
            lo -= 1;
        }
        while hi + 1 < BARKS && sp[hi + 1] >= floor {
            hi += 1;
        }
        out.dominant_band_hz = Some(model::band_hz(lo, hi));
    }
    out
}

/// The run of bands holding at least `share` of the heaviest band's weight.
pub fn band_span(w: &[f64; BARKS], share: f64) -> (usize, usize) {
    let max = w.iter().copied().fold(0.0, f64::max);
    let on: Vec<usize> = (0..BARKS)
        .filter(|z| w[*z] > 0.0 && w[*z] >= max * share)
        .collect();
    (
        on.first().copied().unwrap_or(0),
        on.last().copied().unwrap_or(0),
    )
}

fn history(
    mix: &Mix,
    t: &Timeline,
    blocks: &[usize],
    hops: &[usize],
    lims: &[&GrSeries],
) -> HistoryOut {
    let a = mix.a;
    let kms = |b: usize| a.kms_at(mix.master_out, b) as f64;
    let lo = Loudness { a };
    let step = 2;
    let dt = a.lblock as f64 / a.sr as f64;
    let mut h = HistoryOut {
        step_seconds: r1(dt * step as f64),
        ..Default::default()
    };
    let hop_dt = super::analyze::HOP as f64 / a.sr as f64;
    let mut last_bar = None;
    for (k, chunk) in blocks.chunks(step).enumerate() {
        let t0 = k as f64 * step as f64 * dt;
        h.t.push(r1(t0));
        let m = chunk
            .iter()
            .map(|b| lo.window(&kms, *b, 4))
            .fold(0.0, f64::max);
        let s = lo.window(&kms, *chunk.last().unwrap_or(&0), 30);
        h.momentary.push((m > 0.0).then(|| r1(dsp::lufs(m))));
        h.short_term.push((s > 0.0).then(|| r1(dsp::lufs(s))));
        let i0 = (t0 / hop_dt) as usize;
        let i1 = (((t0 + step as f64 * dt) / hop_dt) as usize).min(hops.len());
        let tp = hops[i0.min(hops.len())..i1]
            .iter()
            .map(|x| mix.true_peak(*x))
            .fold(0.0, f64::max);
        h.true_peak.push((tp > 0.0).then(|| amp(tp)));
        let gr = hops[i0.min(hops.len())..i1]
            .iter()
            .map(|x| lims.iter().map(|g| g.max[*x] as f64).fold(0.0, f64::max))
            .fold(0.0, f64::max);
        h.limiter_gr.push(r1(gr));
        if let Some(b) = chunk.first() {
            let pos = a.lblocks[*b];
            let bar = (t.bar_of(pos.beat), t.pass_of(pos.span as usize, pos.beat));
            if last_bar != Some(bar) {
                h.bars.push(
                    json!({"t": r1(t0), "bar": bar.0, "pass": bar.1, "beat": t.bar_start(bar.0)}),
                );
                last_bar = Some(bar);
            }
        }
    }
    h
}

#[cfg(test)]
mod tests {
    //! Loudness against signals of known loudness (after EBU Tech 3341 and
    //! 3342): a 1 kHz stereo sine at -23 dBFS reads -23 LUFS.
    use super::*;
    use crate::mixcheck::analyze::measure_audio;

    const SR: f32 = 48000.0;

    fn sine(db: f64, seconds: f64) -> Vec<f32> {
        let a = 10f64.powf(db / 20.0);
        (0..(seconds * SR as f64) as usize)
            .map(|i| (a * (std::f64::consts::TAU * 1000.0 * i as f64 / SR as f64).sin()) as f32)
            .collect()
    }

    fn numbers(x: &[f32]) -> MasterOut {
        let a = measure_audio(x, x, SR);
        let hops: Vec<usize> = (0..a.hops.len()).collect();
        let blocks: Vec<usize> = (0..a.lblocks.len()).collect();
        master_numbers(&a, 0, &hops, &blocks, &Options::default())
    }

    fn near(x: Option<f64>, want: f64, tol: f64) {
        let v = x.expect("a value");
        assert!((v - want).abs() <= tol, "{v} is not {want} ± {tol}");
    }

    #[test]
    fn a_steady_tone_reads_its_loudness() {
        let m = numbers(&sine(-23.0, 20.0));
        near(m.integrated_lufs.flatten(), -23.0, 0.1);
        near(m.momentary_lufs_max.flatten(), -23.0, 0.1);
        near(m.short_term_lufs_max.flatten(), -23.0, 0.1);
        near(m.true_peak_dbtp, -23.0, 0.1);
    }

    #[test]
    fn windows_are_whole() {
        // A 100 ms burst at the very start: the 400 ms window around it is a
        // quarter full of it (-6 dB), not the burst alone.
        let mut x = sine(-23.0, 0.1);
        x.extend(vec![0.0; (3.0 * SR) as usize]);
        let m = numbers(&x);
        near(m.momentary_lufs_max.flatten(), -23.0 - 6.02, 0.2);
    }

    #[test]
    fn loudness_range_of_two_levels() {
        // EBU Tech 3342 case 1: 20 s at -20 LUFS, then 20 s at -30: LRA 10.
        let mut x = sine(-20.0, 20.0);
        x.extend(sine(-30.0, 20.0));
        let m = numbers(&x);
        near(m.lra.flatten(), 10.0, 1.0);
    }
}
