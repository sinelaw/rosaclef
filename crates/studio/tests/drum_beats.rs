//! Drum-beat transcription measured on real drum audio: beats played on the
//! engine's Atelier drum voices (each hit rendered, then mixed at known times
//! and velocities), so every onset and its drum are known. `cargo test -p
//! rosaclef-studio --release --test drum_beats -- --nocapture` prints the
//! scores.

use rosaclef_core::Device;
use rosaclef_engine::render::render_note;
use rosaclef_engine::samples::SampleData;
use rosaclef_studio::transcribe::transcribe;
use std::collections::HashMap;

const SR: f32 = 44100.0;

/// A played hit: seconds, the Atelier drum, velocity 0..1, pitch.
#[derive(Clone, Copy, Debug)]
struct Ev {
    t: f32,
    kind: &'static str,
    vel: f32,
    pitch: u8,
}

struct Kit {
    cache: HashMap<(String, u8), Vec<f32>>,
}

impl Kit {
    fn new() -> Kit {
        Kit {
            cache: HashMap::new(),
        }
    }

    fn sound(&mut self, kind: &str, pitch: u8) -> &Vec<f32> {
        self.cache
            .entry((kind.to_string(), pitch))
            .or_insert_with(|| {
                let mut d = Device::new("drum");
                d.options.insert("kind".into(), kind.into());
                let a = render_note(&d, pitch, 1.0, 1.2, SR);
                a.left
                    .iter()
                    .zip(&a.right)
                    .map(|(l, r)| 0.5 * (l + r))
                    .collect()
            })
    }

    /// Mix the hits (plus a quiet room) into a take.
    fn play(&mut self, evs: &[Ev], seed: u32) -> Vec<f32> {
        let end = evs.iter().map(|e| e.t).fold(0.0, f32::max) + 1.5;
        let mut x = vec![0.0f32; (end * SR) as usize];
        for e in evs {
            let s = self.sound(e.kind, e.pitch).clone();
            let a = (e.t * SR) as usize;
            for (i, v) in s.iter().enumerate() {
                if a + i < x.len() {
                    x[a + i] += 0.45 * e.vel * v;
                }
            }
        }
        let mut r = seed.max(1);
        for v in x.iter_mut() {
            r ^= r << 13;
            r ^= r >> 17;
            r ^= r << 5;
            *v += 0.0006 * (r as f32 / u32::MAX as f32 * 2.0 - 1.0);
        }
        x
    }
}

/// Rows of a drum machine: one character per step ('x' hit, 'g' ghost,
/// 'o' accent-free open, '.' rest), `per_beat` steps a beat.
fn rows(bpm: f32, per_beat: f32, bars: usize, rows: &[(&'static str, &str)], seed: u32) -> Vec<Ev> {
    let step = 60.0 / bpm / per_beat;
    let mut r = seed.max(1);
    let mut jitter = || {
        r ^= r << 13;
        r ^= r >> 17;
        r ^= r << 5;
        (r as f32 / u32::MAX as f32 - 0.5) * 0.008
    };
    let mut out = vec![];
    for bar in 0..bars {
        for &(kind, row) in rows {
            let n = row.len();
            for (i, c) in row.chars().enumerate() {
                let t = 0.3 + (bar * n + i) as f32 * step + jitter();
                let (kind, vel) = match c {
                    'x' => (kind, 0.9),
                    'X' => (kind, 1.0),
                    'g' => (kind, 0.3),
                    'o' => ("openhat", 0.7),
                    _ => continue,
                };
                out.push(Ev {
                    t: t.max(0.0),
                    kind,
                    vel,
                    pitch: 60,
                });
            }
        }
    }
    out.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
    out
}

/// The lane a played drum should come out on.
fn lane(kind: &str) -> &'static str {
    match kind {
        "kick" => "kick",
        "tom" => "tom",
        "snare" | "clap" | "rim" => "snare",
        "openhat" => "openhat",
        _ => "hat",
    }
}

#[derive(Default, Debug, Clone, Copy)]
struct Score {
    tp: usize,
    fp: usize,
    miss: usize,
}

impl Score {
    fn f(&self) -> f32 {
        let d = 2 * self.tp + self.fp + self.miss;
        if d == 0 {
            1.0
        } else {
            2.0 * self.tp as f32 / d as f32
        }
    }
}

/// Match found (time, lane) to played events within 30 ms, lane by lane.
fn score(played: &[Ev], found: &[(f32, String)], strict: bool) -> HashMap<&'static str, Score> {
    let norm = |l: &str| -> String {
        if strict {
            l.to_string()
        } else {
            match l {
                "openhat" => "hat".into(),
                "tom" => "kick".into(),
                o => o.to_string(),
            }
        }
    };
    let mut out: HashMap<&'static str, Score> = HashMap::new();
    for l in ["kick", "tom", "snare", "hat", "openhat"] {
        let want: Vec<f32> = played
            .iter()
            .filter(|e| norm(lane(e.kind)) == norm(l) && lane(e.kind) == l)
            .map(|e| e.t)
            .collect();
        let mut got: Vec<f32> = found
            .iter()
            .filter(|(_, k)| norm(k) == norm(l))
            .map(|(t, _)| *t)
            .collect();
        if !strict && (l == "openhat" || l == "tom") {
            continue;
        }
        let want: Vec<f32> = if strict {
            want
        } else {
            played
                .iter()
                .filter(|e| norm(lane(e.kind)) == l)
                .map(|e| e.t)
                .collect()
        };
        let mut s = Score::default();
        for w in &want {
            if let Some(i) = got.iter().position(|g| (g - w).abs() < 0.03) {
                got.remove(i);
                s.tp += 1;
            } else {
                s.miss += 1;
            }
        }
        s.fp = got.len();
        out.insert(l, s);
    }
    out
}

fn beats() -> Vec<(&'static str, Vec<Ev>)> {
    let mut out = vec![];
    out.push((
        "rock 100",
        rows(
            100.0,
            4.0,
            2,
            &[
                ("kick", "x.......x.x....."),
                ("snare", "....x.......x..."),
                ("hat", "x.x.x.x.x.x.x.x."),
            ],
            1,
        ),
    ));
    out.push((
        "hip-hop 90, ghosts",
        rows(
            90.0,
            4.0,
            2,
            &[
                ("kick", "x..x......x..x.."),
                ("snare", "....x..g....x..g"),
                ("hat", "x.x.x.x.x.x.x.o."),
            ],
            2,
        ),
    ));
    out.push((
        "house 124",
        rows(
            124.0,
            4.0,
            2,
            &[
                ("kick", "x...x...x...x..."),
                ("clap", "....x.......x..."),
                ("hat", "..o...o...o...o."),
                ("shaker", "g.g.g.g.g.g.g.g."),
            ],
            3,
        ),
    ));
    out.push((
        "sixteenth hats 110",
        rows(
            110.0,
            4.0,
            2,
            &[
                ("kick", "x.....x...x....."),
                ("snare", "....x.......x..."),
                ("hat", "xgxgxgxgxgxgxgxg"),
            ],
            4,
        ),
    ));
    // A snare roll: 32nds for a beat, getting louder, into a kick.
    let mut roll = rows(
        110.0,
        4.0,
        1,
        &[
            ("kick", "x..............."),
            ("snare", "....x.......x..."),
            ("hat", "x.x.x.x.x.x....."),
        ],
        5,
    );
    let step = 60.0 / 110.0 / 8.0;
    for i in 0..8 {
        roll.push(Ev {
            t: 0.3 + 60.0 / 110.0 * 3.0 + i as f32 * step,
            kind: "snare",
            vel: 0.35 + 0.08 * i as f32,
            pitch: 60,
        });
    }
    roll.push(Ev {
        t: 0.3 + 60.0 / 110.0 * 4.0,
        kind: "kick",
        vel: 1.0,
        pitch: 60,
    });
    roll.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
    out.push(("snare roll", roll));
    // A tom fill.
    let beat = 60.0 / 100.0;
    let mut fill = rows(
        100.0,
        4.0,
        1,
        &[
            ("kick", "x.......x......."),
            ("snare", "....x..........."),
            ("hat", "x.x.x.x........."),
        ],
        6,
    );
    for (i, p) in [67u8, 67, 60, 60, 53, 53].iter().enumerate() {
        fill.push(Ev {
            t: 0.3 + beat * 2.0 + i as f32 * beat / 4.0 + beat / 2.0,
            kind: "tom",
            vel: 0.85,
            pitch: *p,
        });
    }
    fill.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
    out.push(("tom fill", fill));
    out.push((
        "rim and shaker 96",
        rows(
            96.0,
            4.0,
            2,
            &[
                ("kick", "x.....x...x....."),
                ("rim", "....x..x....x..."),
                ("shaker", "xgxgxgxgxgxgxgxg"),
            ],
            7,
        ),
    ));
    out
}

fn found(x: Vec<f32>, sensitivity_floor: f32) -> Vec<(f32, String)> {
    let t = transcribe(
        &SampleData {
            sample_rate: SR,
            channels: vec![x],
        },
        "drums",
    );
    t.hits
        .iter()
        .filter(|h| h.strength >= sensitivity_floor)
        .map(|h| (h.time, h.kind.to_string()))
        .collect()
}

/// The studio's default sensitivity (web/src/voice.js strengthNeeded(0.5)).
const DEFAULT_FLOOR: f32 = 0.12;

/// A phone in a room: the top rolled off above ~7 kHz, the bottom below
/// ~90 Hz, 30 dB quieter and hiss 45 dB down.
fn phone(x: &[f32]) -> Vec<f32> {
    let lp = 1.0 - (-2.0 * std::f32::consts::PI * 7000.0 / SR).exp();
    let hp = (-2.0 * std::f32::consts::PI * 90.0 / SR).exp();
    let (mut a, mut b, mut prev, mut y) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut r = 99u32;
    x.iter()
        .map(|&v| {
            a += (v - a) * lp;
            b += (a - b) * lp;
            y = hp * (y + b - prev);
            prev = b;
            r ^= r << 13;
            r ^= r >> 17;
            r ^= r << 5;
            0.03 * y + 0.00017 * (r as f32 / u32::MAX as f32 * 2.0 - 1.0)
        })
        .collect()
}

fn report(variant: &str, f: &dyn Fn(Vec<f32>) -> Vec<f32>) -> HashMap<&'static str, Score> {
    let mut kit = Kit::new();
    let mut total: HashMap<&'static str, Score> = HashMap::new();
    println!("-- {variant}");
    for (name, evs) in beats() {
        let x = f(kit.play(&evs, 11));
        let got = found(x, DEFAULT_FLOOR);
        let s = score(&evs, &got, true);
        let mut line = format!("{name:<20}");
        for l in ["kick", "tom", "snare", "hat", "openhat"] {
            let v = s[l];
            line += &format!(" {l} {}/{} +{}", v.tp, v.tp + v.miss, v.fp);
            let e = total.entry(l).or_default();
            e.tp += v.tp;
            e.fp += v.fp;
            e.miss += v.miss;
        }
        println!("{line}");
    }
    let mut line = "TOTAL F".to_string();
    for l in ["kick", "tom", "snare", "hat", "openhat"] {
        line += &format!(" {l} {:.2}", total[l].f());
    }
    println!("{line}");
    total
}

#[test]
fn drum_beats_report() {
    // Floors a little under what the detector scores today (F per drum
    // over all the beats), so a change that loses hits shows here.
    let floors = [
        ("studio", [0.95, 0.85, 0.95, 0.85, 0.95]),
        ("phone", [0.95, 0.85, 0.9, 0.75, 0.95]),
    ];
    for (variant, floor) in floors {
        let total = if variant == "studio" {
            report(variant, &|x| x)
        } else {
            report(variant, &|x| phone(&x))
        };
        for (l, want) in ["kick", "tom", "snare", "hat", "openhat"].iter().zip(floor) {
            let f = total[l].f();
            assert!(f >= want, "{variant}: {l} F {f:.2} < {want}");
        }
    }
}

#[test]
#[ignore]
fn drum_beats_detail() {
    let mut kit = Kit::new();
    let which = std::env::var("BEAT").unwrap_or_else(|_| "rock 100".into());
    for (name, evs) in beats() {
        if name != which {
            continue;
        }
        let x = kit.play(&evs, 11);
        let x = if std::env::var("PHONE").is_ok() {
            phone(&x)
        } else {
            x
        };
        let t = transcribe(
            &SampleData {
                sample_rate: SR,
                channels: vec![x],
            },
            "drums",
        );
        for e in &evs {
            println!("played {:.3} {}", e.t, e.kind);
        }
        for h in &t.hits {
            println!(
                "found  {:.3} {:<7} strength {:.2} velocity {:.2}",
                h.time, h.kind, h.strength, h.velocity
            );
        }
    }
}
