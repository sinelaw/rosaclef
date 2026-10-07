//! Prints the drum hits found in a take, and plays them back on the drum
//! machine: `cargo run -p rosaclef-studio --release --example beatbox --
//! take.mp3 [--render drums.wav]`. With `--render` it writes the hits the
//! studio keeps at its default sensitivity, at the times and velocities they
//! were found; `--compare` puts the take on the left and the drums on the
//! right instead.

use rosaclef_core::Device;
use rosaclef_engine::render::{encode_wav, render_note, Audio};
use rosaclef_studio::decode::decode_bytes;
use rosaclef_studio::transcribe::transcribe;

/// The studio's default sensitivity (web/src/voice.js strengthNeeded(0.5)).
const DEFAULT_FLOOR: f32 = 0.12;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("a take (wav, mp3, …)");
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    let data = decode_bytes(std::fs::read(path).expect("read"), &ext).expect("decode");
    let t = transcribe(&data, "drums");
    for h in &t.hits {
        println!(
            "{:>8.0} ms  {:<7} strength {:.2} velocity {:.2}",
            h.time * 1000.0,
            h.kind,
            h.strength,
            h.velocity
        );
    }
    let Some(out) = args
        .iter()
        .position(|a| a == "--render")
        .and_then(|i| args.get(i + 1))
    else {
        return;
    };
    let sr = data.sample_rate;
    let len = data.len() + sr as usize;
    let mut drums = vec![0f32; len];
    for h in t.hits.iter().filter(|h| h.strength >= DEFAULT_FLOOR) {
        let mut d = Device::new("drum");
        d.options.insert("kind".into(), h.kind.into());
        let a = render_note(&d, 60, h.velocity, 1.0, sr);
        let at = (h.time * sr) as usize;
        for (i, (l, r)) in a.left.iter().zip(&a.right).enumerate() {
            if at + i < len {
                drums[at + i] += 0.5 * (l + r) * 0.5;
            }
        }
    }
    let take: Vec<f32> = (0..len)
        .map(|i| {
            if i < data.len() {
                data.channels.iter().map(|c| c[i]).sum::<f32>() / data.channels.len() as f32
            } else {
                0.0
            }
        })
        .collect();
    // Both at the same peak, a little under full scale.
    let norm = |x: &[f32]| {
        let p = x.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
        x.iter().map(|v| v * 0.8 / p).collect::<Vec<f32>>()
    };
    let drums = norm(&drums);
    let audio = if args.iter().any(|a| a == "--compare") {
        Audio {
            sample_rate: sr,
            left: norm(&take),
            right: drums,
        }
    } else {
        Audio {
            sample_rate: sr,
            left: drums.clone(),
            right: drums,
        }
    };
    std::fs::write(out, encode_wav(&audio, 16)).expect("write");
}
