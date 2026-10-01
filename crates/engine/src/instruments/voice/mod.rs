//! "Voix": a formant voice that sings the words of its notes.
//!
//! One note at a time. A note-on starts its syllable's consonants, then holds
//! the vowel; the note-off sings the closing consonants. A note that holds
//! the previous syllable (`_`) only moves the pitch. Notes that follow each
//! other glide; a new syllable waits for the previous one's ending.
//!
//! With engine "render", phrases a singing engine rendered play instead
//! (see [`render`]); the formant voice sings the rest.

mod phones;
mod plan;
mod render;
mod tract;

use super::{Instrument, LyricId, Lyrics, NoteEvent, NoteKind, PhraseId};
use crate::dsp::midi_to_hz;
use crate::samples::SampleBank;
use crate::Ctx;
use plan::{Plan, Step};
use render::Renders;
use rosaclef_core::Device;
use std::sync::Arc;
use tract::{Colour, Tract, CONTROL};

/// How long a note-off waits for a note that continues the line (an
/// adjacent note's on comes right after the previous note's off).
const LEGATO_GRACE: f32 = 0.012;
/// Fade after the last sound of a phrase.
const RELEASE: f32 = 0.06;
/// The speaking voice's pitch (MIDI), before `gender`; each spoken syllable
/// falls a little from it.
const SPEAKING: f32 = 52.0;

#[derive(Clone, Copy, Debug)]
struct Params {
    colour: Colour,
    vibrato: f32,
    vibrato_rate: f32,
    glide: f32,
    consonants: f32,
    gain: f32,
}

/// Where the voice is in its syllable.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Idle,
    Onset { step: usize, left: f32 },
    Vowel,
    Coda { step: usize, left: f32 },
    Release { left: f32 },
}

/// A note waiting for the current syllable to end.
#[derive(Clone, Copy, Debug)]
struct Next {
    key: u8,
    velocity: f32,
    plan: Option<LyricId>,
}

pub struct Voice {
    sr: f32,
    p: Params,
    tract: Tract,
    /// The plan of each syllable the engine may send, by lyric id.
    plans: Vec<Plan>,
    vocalise: Plan,
    /// The syllable being sung (`None`: the vocalise) and where in it.
    plan: Option<LyricId>,
    phase: Phase,
    key: u8,
    velocity: f32,
    hz: f32,
    target_hz: f32,
    /// Seconds into the current vowel (vibrato fades in).
    sung: f32,
    lfo: f32,
    /// A note-off waiting out the legato grace: seconds left.
    off_in: Option<f32>,
    next: Option<Next>,
    renders: Renders,
}

impl Voice {
    pub fn new(sr: f32) -> Voice {
        Voice {
            sr,
            p: Params {
                colour: Colour {
                    scale: 1.0,
                    breath: 0.15,
                    bright: 3500.0,
                },
                vibrato: 0.3,
                vibrato_rate: 5.5,
                glide: 0.06,
                consonants: 1.0,
                gain: 0.8,
            },
            tract: Tract::new(sr),
            plans: vec![],
            vocalise: Plan::vocalise(),
            plan: None,
            phase: Phase::Idle,
            key: 60,
            velocity: 0.0,
            hz: 261.6,
            target_hz: 261.6,
            sung: 0.0,
            lfo: 0.0,
            off_in: None,
            next: None,
            renders: Renders::default(),
        }
    }

    fn current(&self) -> &Plan {
        self.plan
            .and_then(|id| self.plans.get(id as usize))
            .unwrap_or(&self.vocalise)
    }

    fn spoken(&self) -> bool {
        matches!(self.current(), Plan::Syllable { spoken: true, .. })
    }

    fn sounding(&self) -> bool {
        matches!(
            self.phase,
            Phase::Onset { .. } | Phase::Vowel | Phase::Coda { .. }
        )
    }

    fn note_on(&mut self, key: u8, velocity: f32, lyric: Option<LyricId>) {
        if self.renders.note_on(lyric, self.sr) {
            // A rendered phrase sings this note; the formant voice rests.
            if self.sounding() {
                self.end_syllable();
            }
            return;
        }
        let hold = lyric
            .and_then(|id| self.plans.get(id as usize))
            .is_some_and(|p| *p == Plan::Hold);
        let continuing = self.off_in.take().is_some() || self.sounding();
        if hold && continuing {
            // A melisma: the same vowel, a new pitch.
            self.key = key;
            self.target_hz = midi_to_hz(key as f32);
            if !matches!(self.phase, Phase::Onset { .. }) {
                self.phase = Phase::Vowel;
            }
            return;
        }
        let next = Next {
            key,
            velocity,
            plan: lyric.filter(|_| !hold),
        };
        if continuing {
            self.next = Some(next);
            self.end_syllable();
        } else {
            self.start(next, false);
        }
    }

    fn note_off(&mut self, key: u8) {
        if key == self.key && self.sounding() && self.off_in.is_none() {
            self.off_in = Some(LEGATO_GRACE);
        }
    }

    /// Begin a syllable; `legato` glides into its pitch.
    fn start(&mut self, n: Next, legato: bool) {
        self.plan = n.plan;
        self.key = n.key;
        self.velocity = n.velocity;
        self.target_hz = midi_to_hz(n.key as f32);
        if !legato {
            self.hz = self.target_hz;
        }
        self.sung = 0.0;
        self.phase = self.after_onset(0);
    }

    /// The phase at onset step `step`, or the vowel when the onset is done.
    fn after_onset(&self, step: usize) -> Phase {
        match self.current() {
            Plan::Syllable { onset, .. } if step < onset.len() => Phase::Onset {
                step,
                left: onset[step].secs * self.p.consonants,
            },
            _ => Phase::Vowel,
        }
    }

    /// The phase at coda step `step`, or the release (or the next note).
    fn after_coda(&mut self, step: usize) -> Phase {
        if let Plan::Syllable { coda, .. } = self.current() {
            if step < coda.len() {
                return Phase::Coda {
                    step,
                    left: coda[step].secs * self.p.consonants,
                };
            }
        }
        match self.next.take() {
            Some(n) => {
                self.start(n, true);
                self.phase
            }
            None => Phase::Release { left: RELEASE },
        }
    }

    fn end_syllable(&mut self) {
        self.off_in = None;
        if !matches!(self.phase, Phase::Coda { .. }) {
            self.phase = self.after_coda(0);
        }
    }

    /// Advance the phase by `dt` seconds.
    fn advance(&mut self, dt: f32) {
        if let Some(t) = self.off_in.as_mut() {
            *t -= dt;
            if *t <= 0.0 {
                self.end_syllable();
            }
        }
        self.phase = match self.phase {
            Phase::Onset { step, left } if left <= dt => self.after_onset(step + 1),
            Phase::Onset { step, left } => Phase::Onset {
                step,
                left: left - dt,
            },
            Phase::Coda { step, left } if left <= dt => self.after_coda(step + 1),
            Phase::Coda { step, left } => Phase::Coda {
                step,
                left: left - dt,
            },
            Phase::Release { left } if left <= dt => Phase::Idle,
            Phase::Release { left } => Phase::Release { left: left - dt },
            Phase::Vowel => {
                self.sung += dt;
                Phase::Vowel
            }
            Phase::Idle => Phase::Idle,
        };
    }

    /// What the tract aims for now, and at what level.
    fn aim(&self) -> (phones::Target, f32) {
        let Plan::Syllable {
            onset, vowel, coda, ..
        } = self.current()
        else {
            return (phones::SCHWA, 0.0);
        };
        let step = |s: &Step| (s.target, s.gate);
        match self.phase {
            Phase::Onset { step: i, .. } => step(&onset[i]),
            Phase::Coda { step: i, .. } => step(&coda[i]),
            Phase::Vowel => (*vowel, 1.0),
            Phase::Release { .. } | Phase::Idle => (*vowel, 0.0),
        }
    }

    /// The pitch now: gliding to the note, with vibrato once it settles; a
    /// spoken syllable falls from the speaking pitch instead.
    fn pitch(&mut self, dt: f32) -> f32 {
        if self.spoken() {
            let speaking = SPEAKING + 12.0 * self.p.colour.scale.log2();
            return midi_to_hz(speaking - (self.sung * 4.0).min(2.0));
        }
        let k = crate::dsp::settle_coef(self.p.glide, 1.0 / dt);
        self.hz = self.target_hz + (self.hz - self.target_hz) * k;
        self.lfo = (self.lfo + self.p.vibrato_rate * dt).fract();
        let depth = self.p.vibrato * ((self.sung - 0.25) / 0.4).clamp(0.0, 1.0);
        let wobble = (self.lfo * std::f32::consts::TAU).sin() * depth;
        self.hz * 2f32.powf(wobble / 12.0)
    }

    /// Render frames `from..to`, starting a cued phrase on its frame.
    fn render_to(&mut self, to: usize, from: usize, left: &mut [f32], right: &mut [f32]) -> usize {
        let mut pos = from;
        if let Some(cue) = self.renders.cued_at().filter(|c| *c < to) {
            let cue = cue.max(pos);
            self.render(&mut left[pos..cue], &mut right[pos..cue]);
            self.renders.start_cued(self.sr);
            pos = cue;
        }
        if to > pos {
            self.render(&mut left[pos..to], &mut right[pos..to]);
        }
        to.max(pos)
    }

    fn render_block(&mut self, out: &mut [f32]) {
        let dt = out.len() as f32 / self.sr;
        let (target, gate) = self.aim();
        self.tract.steer(&target, gate, &self.p.colour);
        let hz = self.pitch(dt);
        let amp = self.p.gain * (0.3 + 0.7 * self.velocity);
        self.tract.render(hz, amp, &self.p.colour, out);
        self.advance(dt);
    }
}

impl Instrument for Voice {
    fn set_device(&mut self, d: &Device, _ctx: &Ctx) {
        self.p = Params {
            colour: Colour {
                scale: 2f32.powf(d.param("gender") as f32 * 0.25),
                breath: d.param("breath") as f32,
                bright: d.param("bright") as f32,
            },
            vibrato: d.param("vibrato") as f32,
            vibrato_rate: d.param("vibratoRate") as f32,
            glide: d.param("glide") as f32,
            consonants: d.param("consonants") as f32,
            gain: d.param("gain") as f32,
        };
        self.renders.on = d.option("engine") == "render";
    }

    fn set_lyrics(&mut self, lyrics: &Arc<Lyrics>) {
        self.plans = lyrics.sung.iter().map(Plan::of).collect();
        self.plan = None;
        self.renders.set_lyrics(lyrics);
    }

    fn set_samples(&mut self, bank: &SampleBank) {
        self.renders.set_samples(bank);
    }

    fn cue(&mut self, offset: usize, phrase: PhraseId) {
        self.renders.cue(offset, phrase);
    }

    fn handle(&mut self, ev: NoteKind) {
        match ev {
            NoteKind::On { key, velocity } => self.note_on(key, velocity, None),
            NoteKind::Off { key } => self.note_off(key),
            NoteKind::AllOff => {
                self.next = None;
                self.off_in = None;
                self.renders.stop();
                if self.phase != Phase::Idle {
                    self.phase = Phase::Release { left: RELEASE };
                }
            }
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.renders.mix(self.p.gain, left, right);
        if self.phase == Phase::Idle && self.tract.silent() {
            return;
        }
        let n = left.len();
        let mut mono = [0.0f32; CONTROL];
        let mut i = 0;
        while i < n {
            let len = CONTROL.min(n - i);
            mono[..len].fill(0.0);
            self.render_block(&mut mono[..len]);
            for k in 0..len {
                left[i + k] += mono[k];
                right[i + k] += mono[k];
            }
            i += len;
        }
    }

    /// Like the default, but note-ons carry their syllable, and a cued
    /// phrase starts at its frame.
    fn process(&mut self, events: &[NoteEvent], left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        let mut pos = 0;
        for ev in events {
            let at = ev.offset.min(n);
            pos = self.render_to(at, pos, left, right);
            match ev.kind {
                NoteKind::On { key, velocity } => self.note_on(key, velocity, ev.lyric),
                kind => self.handle(kind),
            }
        }
        let end = self.render_to(n, pos, left, right);
        debug_assert_eq!(end, n);
    }
}

#[cfg(test)]
mod tests;
