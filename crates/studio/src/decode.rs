//! Audio file decoding (wav, flac, mp3, ogg/vorbis, aac, alac) via symphonia.

use anyhow::{anyhow, Result};
use rosaclef_engine::samples::SampleData;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymErr;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "wav", "wave", "flac", "mp3", "ogg", "oga", "m4a", "aac", "aif", "aiff",
];

pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Decode a file of `fs` into de-interleaved f32 channels (at most two).
pub fn decode_file(fs: &dyn rosaclef_fs::Fs, path: &Path) -> Result<SampleData> {
    let bytes = fs.read(path)?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    decode_bytes(bytes, ext).map_err(|e| anyhow!("{}: {e}", path.display()))
}

/// Decode an audio file's bytes; `ext` (e.g. "mp3") helps guess the format.
pub fn decode_bytes(bytes: Vec<u8>, ext: &str) -> Result<SampleData> {
    // Fast path for our own WAV files.
    if ext.eq_ignore_ascii_case("wav") {
        if let Ok(d) = rosaclef_engine::render::decode_wav(&bytes) {
            return Ok(d);
        }
    }
    let mss = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    if !ext.is_empty() {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| anyhow!("unsupported audio file: {e}"))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| anyhow!("no audio track"))?;
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100) as f32;
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
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
        return Err(anyhow!("the file contains no audio"));
    }
    Ok(SampleData {
        sample_rate,
        channels,
    })
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
