//! Rendered phrases: their audio, resampled to the engine's rate, started
//! by a cue ahead of the phrase (or late, at a note, when the cue was
//! missed).

use crate::dsp::hermite;
use crate::instruments::{LyricId, Lyrics, PhraseId, PhraseNote};
use crate::samples::{SampleBank, SampleRef};

struct Playback {
    sample: SampleRef,
    /// Position in the sample, in its frames.
    pos: f64,
    /// Sample frames per output frame.
    step: f64,
}

impl Playback {
    /// Start `sample` `secs` into it, played at `sr`.
    fn new(sample: SampleRef, secs: f64, sr: f32) -> Playback {
        let rate = sample.sample_rate as f64;
        Playback {
            pos: secs.max(0.0) * rate,
            step: rate / sr as f64,
            sample,
        }
    }

    /// Add the next `left.len()` frames at `gain`; false once it has ended.
    fn mix(&mut self, gain: f32, left: &mut [f32], right: &mut [f32]) -> bool {
        let chans = &self.sample.channels;
        let len = self.sample.len() as f64;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if self.pos >= len - 1.0 {
                return false;
            }
            let a = hermite(&chans[0], self.pos);
            let b = chans.get(1).map_or(a, |c| hermite(c, self.pos));
            *l += a * gain;
            *r += b * gain;
            self.pos += self.step;
        }
        true
    }
}

/// The rendered phrases a voice plays instead of singing them itself
/// (engine "render"); phrases not rendered yet are sung by the formant voice.
#[derive(Default)]
pub struct Renders {
    pub on: bool,
    paths: Vec<String>,
    audio: Vec<Option<SampleRef>>,
    /// Each syllable's place in a phrase, by lyric id.
    notes: Vec<Option<PhraseNote>>,
    playing: Option<(PhraseId, Playback)>,
    /// A phrase cued in the next block: (frame offset, phrase).
    cued: Option<(usize, PhraseId)>,
}

impl Renders {
    pub fn set_lyrics(&mut self, lyrics: &Lyrics) {
        self.paths = lyrics.phrases.clone();
        self.notes = lyrics.sung.iter().map(|s| s.phrase).collect();
        self.audio = vec![None; self.paths.len()];
        self.playing = None;
    }

    pub fn set_samples(&mut self, bank: &SampleBank) {
        self.audio = self.paths.iter().map(|p| bank.get(p)).collect();
    }

    fn audio(&self, id: PhraseId) -> Option<&SampleRef> {
        self.audio.get(id as usize)?.as_ref()
    }

    pub fn cue(&mut self, offset: usize, id: PhraseId) {
        if self.on && self.audio(id).is_some() {
            self.cued = Some((offset, id));
        }
    }

    /// A note-on: whether its phrase is rendered (and now playing, from the
    /// note's place when it was not cued).
    pub fn note_on(&mut self, lyric: Option<LyricId>, sr: f32) -> bool {
        let Some(n) = lyric.and_then(|id| self.notes.get(id as usize).copied().flatten()) else {
            return false;
        };
        let Some(sample) = self.audio(n.id).filter(|_| self.on).cloned() else {
            return false;
        };
        if self.playing.as_ref().map(|(id, _)| *id) != Some(n.id) {
            self.playing = Some((n.id, Playback::new(sample, n.at as f64, sr)));
        }
        true
    }

    /// The frame of this block where a cued phrase starts.
    pub fn cued_at(&self) -> Option<usize> {
        self.cued.map(|(at, _)| at)
    }

    /// Start the cued phrase from its beginning.
    pub fn start_cued(&mut self, sr: f32) {
        if let Some((_, id)) = self.cued.take() {
            if let Some(sample) = self.audio(id).cloned() {
                self.playing = Some((id, Playback::new(sample, 0.0, sr)));
            }
        }
    }

    pub fn stop(&mut self) {
        self.playing = None;
        self.cued = None;
    }

    pub fn mix(&mut self, gain: f32, left: &mut [f32], right: &mut [f32]) {
        if let Some((_, p)) = &mut self.playing {
            if !p.mix(gain, left, right) {
                self.playing = None;
            }
        }
    }
}
