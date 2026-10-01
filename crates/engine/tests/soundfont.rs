//! The built-in General MIDI soundfont (web/soundfonts/gm): loading presets
//! from the split files and playing them.

use rosaclef_core::Device;
use rosaclef_engine::instruments::{create, Instrument, NoteEvent, NoteKind};
use rosaclef_engine::samples::{PresetKey, SampleBank};
use rosaclef_engine::soundfont::{LoadedPreset, SoundFont};
use rosaclef_engine::Ctx;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

const SR: f32 = 48000.0;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/soundfonts/gm")
}

fn load(bank: u16, program: u16) -> LoadedPreset {
    let sf = SoundFont::parse(&std::fs::read(dir().join("index.sf2")).unwrap()).unwrap();
    assert!(sf.is_split());
    let p = sf.find(bank, program).unwrap();
    let mut pieces: HashMap<usize, Vec<u8>> = HashMap::new();
    for k in sf.pieces(p) {
        pieces.insert(
            k,
            std::fs::read(dir().join(format!("smpl-{k:03}.bin"))).unwrap(),
        );
    }
    sf.load(p, &pieces, &mut HashMap::new()).unwrap()
}

/// Play one note for `hold` seconds, then `tail` seconds of release; returns (left, right).
fn play(
    program: &str,
    preset: LoadedPreset,
    key: u8,
    vel: f32,
    hold: f32,
    tail: f32,
) -> (Vec<f32>, Vec<f32>) {
    let mut dev = Device::new("soundfont");
    dev.options.insert("program".into(), program.into());
    let mut inst: Box<dyn Instrument> = create(&dev, &Ctx { sr: SR, bpm: 120.0 }).unwrap();
    let (bank, prog) = rosaclef_core::gm::lookup(program).unwrap();
    let mut samples = SampleBank::default();
    samples.insert_preset(PresetKey::gm(bank, prog), Arc::new(preset));
    inst.set_samples(&samples);
    let n = ((hold + tail) * SR) as usize;
    let off = (hold * SR) as usize;
    let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
    let mut pos = 0;
    while pos < n {
        let m = (n - pos).min(128);
        let mut ev = vec![];
        if pos == 0 {
            ev.push(NoteEvent {
                offset: 0,
                kind: NoteKind::On { key, velocity: vel },
                lyric: None,
            });
        }
        if off >= pos && off < pos + m {
            ev.push(NoteEvent {
                offset: off - pos,
                kind: NoteKind::Off { key },
                lyric: None,
            });
        }
        inst.process(&ev, &mut l[pos..pos + m], &mut r[pos..pos + m]);
        pos += m;
    }
    (l, r)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// Frequency from the autocorrelation peak (between 50 and 2000 Hz).
fn pitch(x: &[f32]) -> f32 {
    let (lo, hi) = ((SR / 2000.0) as usize, (SR / 50.0) as usize);
    let n = x.len() - hi;
    let ac = |lag: usize| (0..n).map(|i| x[i] * x[i + lag]).sum::<f32>();
    let best = (lo..hi).max_by(|a, b| ac(*a).total_cmp(&ac(*b))).unwrap();
    SR / best as f32
}

#[test]
fn the_piano_loads_and_plays_in_tune() {
    let t = std::time::Instant::now();
    let piano = load(0, 0);
    eprintln!(
        "piano: {} regions, {} samples, {:.1} MB decoded in {:?}",
        piano.regions.len(),
        piano.samples.len(),
        piano
            .samples
            .iter()
            .map(|s| s.data.len() * 2)
            .sum::<usize>() as f64
            / 1e6,
        t.elapsed()
    );
    assert!(piano.regions.len() > 50);
    for s in &piano.samples {
        assert!(s.loop_end as usize <= s.data.len());
    }
    let (l, r) = play("Acoustic Grand Piano", piano, 69, 0.8, 1.0, 1.0);
    let body = &l[2400..24000];
    assert!(rms(body) > 0.02, "audible: {}", rms(body));
    let peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak < 1.0, "no clipping: {peak}");
    let f = pitch(&l[4800..4800 + 9600]);
    assert!((f - 440.0).abs() < 8.0, "A4 plays at 440 Hz, got {f}");
    assert!(
        rms(&l[l.len() - 4800..]) < rms(body) * 0.05,
        "the note dies after release"
    );
}

#[test]
fn looped_strings_sustain_and_velocity_matters() {
    let (loud, _) = play("String Ensemble 1", load(0, 48), 60, 1.0, 3.0, 0.5);
    let (soft, _) = play("String Ensemble 1", load(0, 48), 60, 0.3, 3.0, 0.5);
    let late = |x: &[f32]| rms(&x[(2.5 * SR) as usize..(2.9 * SR) as usize]);
    assert!(
        late(&loud) > 0.01,
        "a held string note keeps sounding: {}",
        late(&loud)
    );
    assert!(late(&soft) < late(&loud) * 0.7, "soft is quieter");
    let f = pitch(&loud[(1.0 * SR) as usize..(1.2 * SR) as usize]);
    assert!((f - 261.6).abs() < 6.0, "C4, got {f}");
}

#[test]
fn drum_kit_plays_the_gm_map() {
    let (kick, _) = play("Standard Kit", load(128, 0), 36, 1.0, 0.1, 0.5);
    let (none, _) = play("Standard Kit", load(128, 0), 12, 1.0, 0.1, 0.5);
    assert!(rms(&kick) > 0.01);
    assert!(rms(&none) < 1e-4, "no drum on key 12");
}

#[test]
#[ignore]
fn dump_piano_a4() {
    use rosaclef_engine::soundfont::gen;
    let sf = SoundFont::parse(&std::fs::read(dir().join("index.sf2")).unwrap()).unwrap();
    let p = sf.find(0, 0).unwrap();
    for r in sf.regions(p) {
        if r.keys.0 <= 69 && 69 <= r.keys.1 && r.vels.0 <= 102 && 102 <= r.vels.1 {
            let h = &sf.samples[r.sample as usize];
            eprintln!("{:?} {:?} {} att={} pan={} fc={} q={} mode={} root={} vol env d{} a{} h{} dc{} s{} r{} start={} end={} loop={}..{}",
                r.keys, r.vels, h.name, r.get(gen::ATTENUATION), r.get(gen::PAN), r.get(gen::FILTER_FC), r.get(gen::FILTER_Q), r.get(gen::SAMPLE_MODES), r.get(gen::ROOT_KEY),
                r.get(gen::VOL_ENV_DELAY), r.get(gen::VOL_ENV_ATTACK), r.get(gen::VOL_ENV_HOLD), r.get(gen::VOL_ENV_DECAY), r.get(gen::VOL_ENV_SUSTAIN), r.get(gen::VOL_ENV_RELEASE),
                h.start, h.end, h.loop_start, h.loop_end);
        }
    }
}
