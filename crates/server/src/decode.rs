//! Audio file decoding (wav, flac, mp3, ogg/vorbis, aac, alac) via symphonia.

use anyhow::{anyhow, Context, Result};
use rosaclef_engine::samples::SampleData;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymErr;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub const AUDIO_EXTENSIONS: &[&str] = &["wav", "wave", "flac", "mp3", "ogg", "oga", "m4a", "aac", "aif", "aiff"];

pub fn is_audio_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).map(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false)
}

/// Decode an audio file into de-interleaved f32 channels (at most two).
pub fn decode_file(path: &Path) -> Result<SampleData> {
    // Fast path for our own WAV files.
    if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("wav")).unwrap_or(false) {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        if let Ok(d) = rosaclef_engine::render::decode_wav(&bytes) {
            return Ok(d);
        }
    }
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| anyhow!("unsupported audio file {}: {e}", path.display()))?;
    let mut format = probed.format;
    let track = format.default_track().ok_or_else(|| anyhow!("no audio track in {}", path.display()))?;
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100) as f32;
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    let mut channels: Vec<Vec<f32>> = vec![];
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymErr::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymErr::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymErr::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        let spec = *decoded.spec();
        let n_ch = spec.channels.count().max(1);
        let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buf.copy_interleaved_ref(decoded);
        let used = n_ch.min(2);
        if channels.is_empty() {
            channels = vec![vec![]; used];
        }
        for frame in buf.samples().chunks(n_ch) {
            for (c, ch) in channels.iter_mut().enumerate() {
                ch.push(frame[c]);
            }
        }
    }
    if channels.is_empty() {
        return Err(anyhow!("{} contains no audio", path.display()));
    }
    Ok(SampleData { sample_rate, channels })
}

/// Min/max waveform overview with `buckets` points.
pub fn peaks(data: &SampleData, buckets: usize) -> Vec<[f32; 2]> {
    let len = data.len();
    let buckets = buckets.clamp(1, 20000);
    let mut out = Vec::with_capacity(buckets);
    for b in 0..buckets {
        let a = b * len / buckets;
        let z = ((b + 1) * len / buckets).max(a + 1).min(len);
        let (mut lo, mut hi) = (0f32, 0f32);
        for ch in &data.channels {
            for &x in &ch[a.min(len)..z] {
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
        out.push([lo, hi]);
    }
    out
}
