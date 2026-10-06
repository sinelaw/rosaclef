//! A/B against a reference recording: measured like the master's output,
//! then level-matched (the reference moved to the mix's loudness) so the
//! comparison of tone, dynamics and width is not fooled by loudness.

use super::analyze;
use super::dsp::r1;
use super::options::{Options, CHECKS};
use super::report::{master_numbers, MasterOut};
use crate::folder::Folder;
use serde_json::{json, Value};

/// The largest recording measured (a decoded hour of stereo is ~1.4 GB).
const MAX_BYTES: u64 = 512 << 20;

/// Measure the recording at `path`: in the project folder, or (`any_file`,
/// the command line) anywhere.
pub fn measure(folder: &Folder, path: &str, any_file: bool) -> Result<(MasterOut, f64), String> {
    let p = folder
        .resolve(path)
        .filter(|p| folder.fs.is_file(p))
        .or_else(|| {
            let abs = std::path::PathBuf::from(path);
            (any_file && folder.fs.is_file(&abs)).then_some(abs)
        })
        .ok_or_else(|| {
            if any_file {
                format!("reference: no file {path:?}")
            } else {
                format!("reference: no file {path:?} in the project folder")
            }
        })?;
    let size = folder.fs.metadata(&p).map(|m| m.len).unwrap_or(0);
    if size > MAX_BYTES {
        return Err(format!(
            "reference: {path:?} is too large ({} MB; at most {} MB)",
            size >> 20,
            MAX_BYTES >> 20
        ));
    }
    let data = crate::decode::decode_file(folder.fs.as_ref(), &p)
        .map_err(|e| format!("reference: {e}"))?;
    let l = data.channels.first().cloned().unwrap_or_default();
    let r = data.channels.get(1).cloned().unwrap_or_else(|| l.clone());
    let a = analyze::measure_audio(&l, &r, data.sample_rate);
    let hops: Vec<usize> = (0..a.hops.len()).collect();
    let blocks: Vec<usize> = (0..a.lblocks.len()).collect();
    let all = Options {
        checks: CHECKS.iter().map(|c| c.1).collect(),
        ..Options::default()
    };
    Ok((
        master_numbers(&a, 0, &hops, &blocks, &all),
        l.len() as f64 / data.sample_rate as f64,
    ))
}

/// The comparison with the mix's master numbers.
pub fn compare(path: &str, mix: &MasterOut, reference: &MasterOut, seconds: f64) -> Value {
    let li = mix.integrated_lufs.flatten();
    let ri = reference.integrated_lufs.flatten();
    let matched = match (li, ri) {
        (Some(a), Some(b)) => Some(r1(a - b)),
        _ => None,
    };
    let g = matched.unwrap_or(0.0);
    let spectrum = match (&mix.spectrum, &reference.spectrum) {
        (Some(a), Some(b)) => Some(
            a.db.iter()
                .zip(&b.db)
                .map(|(x, y)| r1(x - (y + g)))
                .collect::<Vec<f64>>(),
        ),
        _ => None,
    };
    let opt = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(x), Some(y)) => json!(r1(x - y)),
        _ => Value::Null,
    };
    let mut notes = vec![];
    if let Some(m) = matched {
        notes.push(format!("the mix is {m:+.1} LU against the reference"));
    }
    if let Some(sp) = &spectrum {
        let names = super::dsp::BAND_NAMES;
        if let Some((i, d)) = sp
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        {
            if d.abs() >= 1.5 {
                notes.push(format!(
                    "level-matched, {} is {:+.1} dB against it",
                    names[i], d
                ));
            }
        }
    }
    let plr = opt(mix.plr_db.flatten(), reference.plr_db.flatten());
    if let Some(p) = plr.as_f64() {
        if p.abs() >= 1.5 {
            notes.push(format!(
                "{:.1} dB {} peak-to-loudness",
                p.abs(),
                if p < 0.0 { "less" } else { "more" }
            ));
        }
    }
    json!({
        "file": path,
        "seconds": r1(seconds),
        "master": reference,
        "levelMatchDb": matched,
        "delta": {
            "integratedLufs": opt(li, ri),
            "truePeakDbtp": opt(mix.true_peak_dbtp, reference.true_peak_dbtp),
            "plrDb": plr,
            "lra": opt(mix.lra.flatten(), reference.lra.flatten()),
            "spectrumDbLevelMatched": spectrum,
        },
        "summary": notes.join("; "),
    })
}
