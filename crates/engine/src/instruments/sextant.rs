//! Placeholder: implementation in progress.

use super::{Instrument, NoteKind};
use crate::Ctx;
use rosaclef_core::Device;

pub struct Sextant;

impl Sextant {
    pub fn new(_sr: f32) -> Sextant {
        Sextant
    }
}

impl Instrument for Sextant {
    fn set_device(&mut self, _dev: &Device, _ctx: &Ctx) {}
    fn handle(&mut self, _ev: NoteKind) {}
    fn render(&mut self, _l: &mut [f32], _r: &mut [f32]) {}
}
