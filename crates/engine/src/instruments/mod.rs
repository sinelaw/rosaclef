//! Built-in instruments and the common instrument interface.

mod comete;
mod cuivre;
mod dedale;
mod drum;
mod fm;
mod nebula;
mod prisme;
mod sampler;
mod sextant;
mod soundfont;
mod synth;
mod tessera;
mod voice;

pub use comete::Comete;
pub use cuivre::Cuivre;
pub use dedale::Dedale;
pub use drum::Drum;
pub use fm::Fm;
pub use nebula::Nebula;
pub use prisme::Prisme;
pub use sampler::Sampler;
pub use sextant::Sextant;
pub use soundfont::SoundFontInst;
pub use synth::Synth;
pub use tessera::Tessera;
pub use voice::Voice;

use crate::samples::SampleBank;
use crate::Ctx;
use rosaclef_core::lyrics::Token;
use rosaclef_core::{Device, LyricMode};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoteKind {
    On {
        key: u8,
        velocity: f32,
    },
    Off {
        key: u8,
    },
    /// Release every sounding note.
    AllOff,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteEvent {
    /// Frame offset inside the current block.
    pub offset: usize,
    pub kind: NoteKind,
    /// A note-on's syllable (see [`Instrument::set_lyrics`]).
    pub lyric: Option<LyricId>,
}

/// Index of a syllable in [`Lyrics::sung`].
pub type LyricId = u32;

/// Index of a rendered phrase in [`Lyrics::phrases`].
pub type PhraseId = u32;

/// What singing instruments need to know about the song's words (see
/// [`Instrument::set_lyrics`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lyrics {
    /// The syllables note-ons refer to.
    pub sung: Vec<Sung>,
    /// Where each rendered phrase's audio is (a sample path).
    pub phrases: Vec<String>,
}

/// What a note sings, for a singing instrument.
#[derive(Clone, Debug, PartialEq)]
pub struct Sung {
    /// The lyric token; `None` for a note without words.
    pub token: Option<Token>,
    pub lang: String,
    pub mode: LyricMode,
    /// IPA; empty for holds, breaths and notes without words.
    pub phonemes: Vec<String>,
    /// The rendered phrase the note belongs to, if its voice renders.
    pub phrase: Option<PhraseNote>,
}

/// A note's place in a rendered phrase.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhraseNote {
    pub id: PhraseId,
    /// Seconds from the start of the phrase's audio to the note's start.
    pub at: f32,
}

pub trait Instrument: Send {
    /// Apply (new) settings from the project document.
    fn set_device(&mut self, dev: &Device, ctx: &Ctx);
    /// Handle a note event at the current position.
    fn handle(&mut self, ev: NoteKind);
    /// Render (adding) into the output buffers.
    fn render(&mut self, left: &mut [f32], right: &mut [f32]);
    /// Resolve sample references (samplers only).
    fn set_samples(&mut self, _bank: &SampleBank) {}
    /// The syllables note-ons refer to and the rendered phrases (singing
    /// instruments only).
    fn set_lyrics(&mut self, _lyrics: &Arc<Lyrics>) {}
    /// A rendered phrase is about to start: its audio begins at `offset` in
    /// the next block, ahead of its first note (singing instruments only).
    fn cue(&mut self, _offset: usize, _phrase: PhraseId) {}
    /// Render a block with sample-accurate events. `events` are sorted by offset.
    fn process(&mut self, events: &[NoteEvent], left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        let mut pos = 0;
        for ev in events {
            let at = ev.offset.min(n);
            if at > pos {
                self.render(&mut left[pos..at], &mut right[pos..at]);
                pos = at;
            }
            self.handle(ev.kind);
        }
        if pos < n {
            self.render(&mut left[pos..n], &mut right[pos..n]);
        }
    }
}

/// Maximum simultaneous voices per built-in instrument.
pub const MAX_VOICES: usize = 24;

/// Pick a voice slot: a free one, otherwise the oldest released one,
/// otherwise the oldest.
pub(crate) fn pick_voice<V>(
    voices: &[V],
    active: impl Fn(&V) -> bool,
    released: impl Fn(&V) -> bool,
    age: impl Fn(&V) -> u64,
) -> usize {
    if let Some(i) = voices.iter().position(|v| !active(v)) {
        return i;
    }
    let mut best = 0;
    let mut best_key = (false, u64::MAX);
    for (i, v) in voices.iter().enumerate() {
        // Prefer released voices, then the oldest.
        let key = (!released(v), age(v));
        if key < best_key {
            best_key = key;
            best = i;
        }
    }
    best
}

/// Create a built-in instrument for a device type.
pub fn create(dev: &Device, ctx: &Ctx) -> Option<Box<dyn Instrument>> {
    let mut inst: Box<dyn Instrument> = match dev.kind.as_str() {
        "synth" => Box::new(Synth::new(ctx.sr)),
        "fm" => Box::new(Fm::new(ctx.sr)),
        "drum" => Box::new(Drum::new(ctx.sr)),
        "sampler" => Box::new(Sampler::new(ctx.sr)),
        "prisme" => Box::new(Prisme::new(ctx.sr)),
        "sextant" => Box::new(Sextant::new(ctx.sr)),
        "tessera" => Box::new(Tessera::new(ctx.sr)),
        "cuivre" => Box::new(Cuivre::new(ctx.sr)),
        "nebula" => Box::new(Nebula::new(ctx.sr)),
        "dedale" => Box::new(Dedale::new(ctx.sr)),
        "comete" => Box::new(Comete::new(ctx.sr)),
        "soundfont" => Box::new(SoundFontInst::new(ctx.sr)),
        "voice" => Box::new(Voice::new(ctx.sr)),
        _ => return None,
    };
    inst.set_device(dev, ctx);
    Some(inst)
}

/// An instrument that renders nothing (unknown or unavailable devices).
pub struct Silent;

impl Instrument for Silent {
    fn set_device(&mut self, _dev: &Device, _ctx: &Ctx) {}
    fn handle(&mut self, _ev: NoteKind) {}
    fn render(&mut self, _l: &mut [f32], _r: &mut [f32]) {}
}
