//! Offline (faster than real time) rendering.

use crate::{Engine, PlayMode, MAX_BLOCK};
use rosaclef_core::{Device, Project};

/// What to render.
#[derive(Clone, Debug)]
pub enum RenderScope {
    /// The whole playlist arrangement.
    Song,
    /// A pattern, repeated `loops` times.
    Pattern { id: String, loops: u32 },
}

/// Stereo audio.
#[derive(Clone, Debug, Default)]
pub struct Audio {
    pub sample_rate: f32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

impl Audio {
    pub fn peak(&self) -> f32 {
        self.left.iter().chain(self.right.iter()).fold(0.0f32, |m, x| m.max(x.abs()))
    }
    pub fn rms(&self) -> f32 {
        let n = (self.left.len() * 2).max(1) as f32;
        (self.left.iter().chain(self.right.iter()).map(|x| x * x).sum::<f32>() / n).sqrt()
    }
    pub fn duration(&self) -> f64 {
        self.left.len() as f64 / self.sample_rate as f64
    }
}

/// Maximum reverb/delay tail rendered after the music ends.
const MAX_TAIL_SECONDS: f32 = 8.0;

/// Render with an engine that already has the project, samples and plugin host.
pub fn render(engine: &mut Engine, scope: &RenderScope) -> Audio {
    let sr = engine.sample_rate();
    let beats = match scope {
        RenderScope::Song => {
            engine.set_mode(PlayMode::Song);
            engine.project().song_length()
        }
        RenderScope::Pattern { id, loops } => {
            engine.set_mode(PlayMode::Pattern(id.clone()));
            engine.project().pattern(id).map(|p| p.length).unwrap_or(0.0) * (*loops).max(1) as f64
        }
    };
    engine.stop();
    engine.play();
    let spb = engine.project().seconds_per_beat();
    let music_frames = (beats * spb * sr as f64).round() as usize;
    let mut out = Audio { sample_rate: sr, left: Vec::with_capacity(music_frames), right: Vec::with_capacity(music_frames) };
    let mut bl = [0f32; MAX_BLOCK];
    let mut br = [0f32; MAX_BLOCK];
    let mut done = 0;
    while done < music_frames {
        let n = (music_frames - done).min(MAX_BLOCK);
        engine.process(&mut bl[..n], &mut br[..n]);
        out.left.extend_from_slice(&bl[..n]);
        out.right.extend_from_slice(&br[..n]);
        done += n;
    }
    // Let notes release and effects ring out.
    engine.pause();
    let max_tail = (MAX_TAIL_SECONDS * sr) as usize;
    let quiet_needed = (0.25 * sr) as usize;
    let mut quiet = 0;
    let mut tail = 0;
    while tail < max_tail && quiet < quiet_needed {
        engine.process(&mut bl, &mut br);
        let peak = bl.iter().chain(br.iter()).fold(0.0f32, |m, x| m.max(x.abs()));
        if peak < 1e-4 {
            quiet += MAX_BLOCK;
        } else {
            quiet = 0;
        }
        out.left.extend_from_slice(&bl);
        out.right.extend_from_slice(&br);
        tail += MAX_BLOCK;
    }
    // Trim trailing near-silence.
    let mut end = out.left.len();
    while end > music_frames && out.left[end - 1].abs() < 1e-5 && out.right[end - 1].abs() < 1e-5 {
        end -= 1;
    }
    out.left.truncate(end);
    out.right.truncate(end);
    engine.stop();
    out
}

/// Render a single note of an instrument (used to generate samples).
pub fn render_note(device: &Device, pitch: u8, velocity: f32, seconds: f32, sample_rate: f32) -> Audio {
    let mut project = Project::empty("note");
    project.channels.push(rosaclef_core::Channel {
        id: "x".into(),
        name: "x".into(),
        color: "#ffffff".into(),
        instrument: device.clone(),
        volume: 1.0,
        pan: 0.0,
        mute: false,
        mixer: 0,
    });
    project.mixer.inserts[0].effects.clear();
    let mut engine = Engine::new(sample_rate);
    engine.set_project(project);
    engine.note_on("x", pitch, velocity);
    let frames = (seconds * sample_rate) as usize;
    let mut out = Audio { sample_rate, ..Default::default() };
    let mut bl = [0f32; MAX_BLOCK];
    let mut br = [0f32; MAX_BLOCK];
    let mut done = 0;
    let mut released = false;
    let hold = (seconds * 0.6 * sample_rate) as usize;
    while done < frames {
        if !released && done >= hold {
            engine.note_off("x", pitch);
            released = true;
        }
        let n = (frames - done).min(MAX_BLOCK);
        engine.process(&mut bl[..n], &mut br[..n]);
        out.left.extend_from_slice(&bl[..n]);
        out.right.extend_from_slice(&br[..n]);
        done += n;
    }
    out
}

/// Interleave to 16-bit/24-bit/32-bit-float WAV bytes.
pub fn encode_wav(audio: &Audio, bits: u16) -> Vec<u8> {
    let channels = 2u16;
    let float = bits == 32;
    let bytes_per = (bits / 8) as u32;
    let frames = audio.left.len() as u32;
    let data_len = frames * channels as u32 * bytes_per;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    let sr = audio.sample_rate as u32;
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&(if float { 3u16 } else { 1u16 }).to_le_bytes());
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&sr.to_le_bytes());
    v.extend_from_slice(&(sr * channels as u32 * bytes_per).to_le_bytes());
    v.extend_from_slice(&((channels as u32 * bytes_per) as u16).to_le_bytes());
    v.extend_from_slice(&bits.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames as usize {
        for s in [audio.left[i], audio.right[i]] {
            let s = s.clamp(-1.0, 1.0);
            match bits {
                16 => v.extend_from_slice(&((s * 32767.0).round() as i16).to_le_bytes()),
                24 => {
                    let x = (s * 8_388_607.0).round() as i32;
                    v.extend_from_slice(&x.to_le_bytes()[..3]);
                }
                _ => v.extend_from_slice(&s.to_le_bytes()),
            }
        }
    }
    v
}

/// Decode a PCM/float WAV file (the formats written by [`encode_wav`] and most
/// recorders). Other formats are decoded by the host.
pub fn decode_wav(bytes: &[u8]) -> Result<crate::samples::SampleData, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".into());
    }
    let mut i = 12;
    let (mut fmt, mut channels, mut sr, mut bits) = (0u16, 0u16, 0u32, 0u16);
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let len = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().unwrap()) as usize;
        let body = &bytes[i + 8..(i + 8 + len).min(bytes.len())];
        if id == b"fmt " && body.len() >= 16 {
            fmt = u16::from_le_bytes([body[0], body[1]]);
            channels = u16::from_le_bytes([body[2], body[3]]);
            sr = u32::from_le_bytes(body[4..8].try_into().unwrap());
            bits = u16::from_le_bytes([body[14], body[15]]);
            if fmt == 0xFFFE && body.len() >= 26 {
                fmt = u16::from_le_bytes([body[24], body[25]]);
            }
        } else if id == b"data" {
            if channels == 0 {
                return Err("missing fmt chunk".into());
            }
            let bps = (bits / 8) as usize;
            let frame = bps * channels as usize;
            let frames = body.len() / frame.max(1);
            let used = channels.min(2) as usize;
            let mut out = vec![Vec::with_capacity(frames); used];
            for f in 0..frames {
                for (c, ch) in out.iter_mut().enumerate() {
                    let o = f * frame + c * bps;
                    let s = &body[o..o + bps];
                    let x = match (fmt, bits) {
                        (1, 8) => (s[0] as f32 - 128.0) / 128.0,
                        (1, 16) => i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0,
                        (1, 24) => (i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8) as f32 / 8_388_608.0,
                        (1, 32) => i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32 / 2_147_483_648.0,
                        (3, 32) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                        (3, 64) => f64::from_le_bytes(s[..8].try_into().unwrap()) as f32,
                        _ => return Err(format!("unsupported WAV encoding (format {fmt}, {bits} bits)")),
                    };
                    ch.push(x);
                }
            }
            return Ok(crate::samples::SampleData { sample_rate: sr as f32, channels: out });
        }
        i += 8 + len + (len & 1);
    }
    Err("no data chunk".into())
}
