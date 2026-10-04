//! Built-in instruments and the common instrument interface.

mod additive;
mod analog;
mod drum;
mod fm;
mod generative;
mod granular;
mod sampler;
mod soundfont;
mod transition;
mod wavetable;

pub use additive::Additive;
pub use analog::Analog;
pub use drum::Drum;
pub use fm::Fm;
pub use generative::Generative;
pub use granular::Granular;
pub use sampler::Sampler;
pub use soundfont::SoundFontInst;
pub use transition::Transition;
pub use wavetable::Wavetable;

use crate::samples::SampleBank;
use crate::Ctx;
use rosaclef_core::Device;

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
        "drum" => Box::new(Drum::new(ctx.sr)),
        "sampler" => Box::new(Sampler::new(ctx.sr)),
        "additive" => Box::new(Additive::new(ctx.sr)),
        "fm" => Box::new(Fm::new(ctx.sr)),
        "wavetable" => Box::new(Wavetable::new(ctx.sr)),
        "analog" => Box::new(Analog::new(ctx.sr)),
        "granular" => Box::new(Granular::new(ctx.sr)),
        "generative" => Box::new(Generative::new(ctx.sr)),
        "transition" => Box::new(Transition::new(ctx.sr)),
        "soundfont" => Box::new(SoundFontInst::new(ctx.sr)),
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
