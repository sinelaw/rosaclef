//! Sung lines and the formats made from them.
//!
//! [`line`] performs the song: every note in playing order, timed in beats
//! and seconds, with the syllable and phonemes it sings. Exporters write
//! that out for singing engines (Synthesizer V, OpenUtau, DiffSinger),
//! notation (MusicXML), karaoke (UltraStar, LRC, TTML), song generators
//! (tagged lyrics, timed words) and speech (SSML).

pub mod line;

pub use line::{Line, Performed, Song, Syllable, Time};
