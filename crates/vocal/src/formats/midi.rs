//! Standard MIDI Files (format 1, 480 ticks per quarter note) of the whole
//! song: a conductor track (title, tempo, meter), then a track per channel
//! playing its sounding pitches, named after it.
//!
//! On sung channels each syllable is a lyric event (`FF 05`, UTF-8) right
//! before its note-on, as RP-017 karaoke files have them: a trailing space
//! ends a word, CR ends a line and LF a paragraph. Held notes get no lyric
//! (the syllable goes on); breaths are left out. Channels take MIDI
//! channels 1–16 in order, skipping 10 (the drums).

use crate::line::{Performed, Song};
use rosaclef_core::lyrics::{Break, Sung};

/// Ticks per quarter note.
pub const PPQ: u16 = 480;

pub fn write(song: &Song) -> Vec<u8> {
    let mut tracks = vec![conductor(song)];
    for (k, id) in channels(song).iter().enumerate() {
        let notes: Vec<&Performed> = song
            .notes
            .iter()
            .filter(|n| n.channel == *id && !is_breath(n))
            .collect();
        let name = song
            .lines
            .iter()
            .find(|l| l.channel == *id)
            .map_or(id.as_str(), |l| l.name.as_str());
        tracks.push(track(name, &notes, midi_channel(k)));
    }
    file(&tracks)
}

/// Channel ids in order of their first note.
fn channels(song: &Song) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for n in &song.notes {
        if !out.contains(&n.channel) {
            out.push(n.channel.clone());
        }
    }
    out
}

/// The `k`th track's MIDI channel (0-based): 0–8, then 10–15, skipping the
/// drum channel; past 15 they wrap.
fn midi_channel(k: usize) -> u8 {
    let c = (k % 15) as u8;
    if c >= 9 {
        c + 1
    } else {
        c
    }
}

/// A timed event; at one tick note-offs come first, then lyrics, then
/// note-ons.
struct Event {
    tick: u64,
    order: u8,
    bytes: Vec<u8>,
}

fn meta(kind: u8, data: &[u8]) -> Vec<u8> {
    let mut b = vec![0xff, kind];
    b.extend(vlq(data.len() as u64));
    b.extend_from_slice(data);
    b
}

fn conductor(song: &Song) -> Vec<Event> {
    let us = (60_000_000.0 / song.bpm.max(1.0)).round() as u32;
    let beats = song.beats_per_bar.clamp(1, 255) as u8;
    let at_start = |bytes| Event {
        tick: 0,
        order: 0,
        bytes,
    };
    vec![
        at_start(meta(0x03, song.title.as_bytes())),
        at_start(meta(0x51, &us.to_be_bytes()[1..])),
        at_start(meta(0x58, &[beats, 2, 24, 8])),
    ]
}

fn track(name: &str, notes: &[&Performed], channel: u8) -> Vec<Event> {
    let mut events = vec![Event {
        tick: 0,
        order: 0,
        bytes: meta(0x03, name.as_bytes()),
    }];
    for (i, n) in notes.iter().enumerate() {
        let start = ticks(n.start.beat);
        let end = ticks(n.end.beat).max(start + 1);
        let key = n.pitch.clamp(0, 127) as u8;
        let velocity = (n.velocity * 127.0).round().clamp(1.0, 127.0) as u8;
        if let Some(text) = lyric(notes, i) {
            events.push(Event {
                tick: start,
                order: 1,
                bytes: meta(0x05, text.as_bytes()),
            });
        }
        events.push(Event {
            tick: start,
            order: 2,
            bytes: vec![0x90 | channel, key, velocity],
        });
        events.push(Event {
            tick: end,
            order: 0,
            bytes: vec![0x80 | channel, key, 0],
        });
    }
    events
}

fn ticks(beat: f64) -> u64 {
    (beat.max(0.0) * PPQ as f64).round() as u64
}

fn sung(n: &Performed) -> Option<&Sung> {
    n.syllable.as_ref().map(|s| &s.token.sung)
}

fn is_breath(n: &Performed) -> bool {
    matches!(sung(n), Some(Sung::Breath))
}

/// The lyric event of note `i`: its syllable and what ends after it (a
/// line, a paragraph, or the word), counting the notes that hold it.
fn lyric(notes: &[&Performed], i: usize) -> Option<String> {
    let Sung::Syllable { text, pos, .. } = sung(notes[i])? else {
        return None;
    };
    let held = notes[i + 1..]
        .iter()
        .copied()
        .take_while(|n| matches!(sung(n), Some(Sung::Hold)));
    let breaks = std::iter::once(notes[i]).chain(held);
    let brk = breaks
        .filter_map(|n| n.syllable.as_ref())
        .map(|s| s.token.brk)
        .fold(Break::None, |a, b| if b == Break::None { a } else { b });
    let end = match brk {
        Break::Paragraph => "\n",
        Break::Line => "\r",
        Break::None if pos.ends_word() => " ",
        Break::None => "",
    };
    Some(format!("{text}{end}"))
}

/// The file: header, then each track's events as delta-timed messages.
fn file(tracks: &[Vec<Event>]) -> Vec<u8> {
    let mut out = b"MThd".to_vec();
    out.extend(6u32.to_be_bytes());
    out.extend(1u16.to_be_bytes());
    out.extend((tracks.len() as u16).to_be_bytes());
    out.extend(PPQ.to_be_bytes());
    for t in tracks {
        out.extend(chunk(t));
    }
    out
}

fn chunk(events: &[Event]) -> Vec<u8> {
    let mut order: Vec<&Event> = events.iter().collect();
    order.sort_by_key(|e| (e.tick, e.order));
    let mut data = vec![];
    let mut tick = 0;
    for e in order {
        data.extend(vlq(e.tick - tick));
        data.extend(&e.bytes);
        tick = e.tick;
    }
    data.extend([0x00, 0xff, 0x2f, 0x00]);
    let mut out = b"MTrk".to_vec();
    out.extend((data.len() as u32).to_be_bytes());
    out.extend(data);
    out
}

/// A variable-length quantity.
fn vlq(mut v: u64) -> Vec<u8> {
    let mut out = vec![(v & 0x7f) as u8];
    v >>= 7;
    while v > 0 {
        out.insert(0, 0x80 | (v & 0x7f) as u8);
        v >>= 7;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::song;

    fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).position(|w| w == needle)
    }

    #[test]
    fn tracks_with_lyrics_before_their_notes() {
        let bytes = write(&song());
        assert_eq!(&bytes[..4], b"MThd");
        assert_eq!(
            &bytes[8..14],
            &[0, 1, 0, 3, 0x01, 0xe0],
            "format 1, 3 tracks, 480"
        );
        let tempo = find(&bytes, &[0xff, 0x51, 3]).unwrap();
        assert_eq!(&bytes[tempo + 3..tempo + 6], &[0x07, 0xa1, 0x20]);
        // "Hel" (the word goes on), then its note-on: C4 + transpose 0.
        let hel = find(&bytes, b"\xff\x05\x03Hel").unwrap();
        assert_eq!(&bytes[hel + 6..hel + 10], &[0x00, 0x90, 60, 102]);
        assert!(find(&bytes, b"\xff\x05\x03lo ").is_some());
        assert!(
            find(&bytes, b"\xff\x05\x07friend\r").is_some(),
            "the hold's line end"
        );
        assert!(find(&bytes, b"(br)").is_none());
        let lyrics = bytes.windows(2).filter(|w| w == &[0xff, 0x05]).count();
        assert_eq!(lyrics, 13, "a lyric per syllable, none on the hold");
        assert!(find(&bytes, b"\xff\x03\x04Lead").is_some());
        assert!(find(&bytes, b"\xff\x03\x03kit").is_some());
        assert_eq!(vlq(0x4000), [0x81, 0x80, 0x00]);
        assert_eq!(midi_channel(9), 10);
    }
}
