//! Sung lines and the formats made from them.
//!
//! [`line`] performs the song: every note in playing order, timed in beats
//! and seconds, with the syllable and phonemes it sings. [`words`] reads a
//! line as words and lyric lines. [`formats`] writes the song for singing
//! engines (Synthesizer V, OpenUtau, DiffSinger), notation (MusicXML, MIDI),
//! karaoke (UltraStar, LRC, TTML), song generators (tagged lyrics, timed
//! words) and speech (SSML). [`align`] turns what a forced aligner found in
//! a sung take into a lyric line's phoneme timing. [`phrase`] lists the
//! phrases a project's rendering voices need, each as a small song.

pub mod align;
pub mod formats;
pub mod line;
pub mod phrase;
pub mod timing;
pub mod words;

#[cfg(test)]
mod testing;

pub use formats::{Format, FORMATS};
pub use line::{Line, Performed, Song, Syllable, Time};
