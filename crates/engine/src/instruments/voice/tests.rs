use super::*;
use crate::instruments::Sung;
use rosaclef_core::lyrics::parse;
use rosaclef_core::LyricMode;

const SR: f32 = 48000.0;

fn sung(text: &str, phonemes: &[&str]) -> Vec<Sung> {
    parse(text)
        .unwrap()
        .into_iter()
        .zip(phonemes)
        .map(|(token, ph)| Sung {
            token: Some(token),
            lang: "en".into(),
            mode: LyricMode::Sing,
            phonemes: ph.split_whitespace().map(str::to_string).collect(),
            phrase: None,
        })
        .collect()
}

fn lyrics(sung: Vec<Sung>) -> Arc<Lyrics> {
    Arc::new(Lyrics {
        sung,
        phrases: vec![],
    })
}

/// Play notes (key, start s, end s, lyric id) and return the mono output.
fn sing(voice: &mut Voice, notes: &[(u8, f32, f32, Option<u32>)], secs: f32) -> Vec<f32> {
    let n = (secs * SR) as usize;
    let mut out = vec![0.0; n];
    let mut right = vec![0.0; n];
    let mut events: Vec<(usize, NoteEvent)> = vec![];
    for &(key, a, b, lyric) in notes {
        let on = NoteKind::On { key, velocity: 0.9 };
        events.push((
            (a * SR) as usize,
            NoteEvent {
                offset: 0,
                kind: on,
                lyric,
            },
        ));
        let off = NoteKind::Off { key };
        events.push((
            (b * SR) as usize,
            NoteEvent {
                offset: 0,
                kind: off,
                lyric: None,
            },
        ));
    }
    // Offs before ons at the same moment, as the engine sends them.
    events.sort_by_key(|(t, e)| (*t, matches!(e.kind, NoteKind::On { .. })));
    for start in (0..n).step_by(128) {
        let end = (start + 128).min(n);
        let block: Vec<NoteEvent> = events
            .iter()
            .filter(|(t, _)| *t >= start && *t < end)
            .map(|(t, e)| NoteEvent {
                offset: t - start,
                ..*e
            })
            .collect();
        voice.process(&block, &mut out[start..end], &mut right[start..end]);
    }
    out
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn at(x: &[f32], a: f32, b: f32) -> &[f32] {
    &x[(a * SR) as usize..(b * SR) as usize]
}

/// Zero crossings per second: high for hiss, low for voiced sound.
fn crossings(x: &[f32]) -> f32 {
    let n = x
        .windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count();
    n as f32 / (x.len() as f32 / SR)
}

#[test]
fn it_sings_words_and_stops() {
    let mut v = Voice::new(SR);
    v.set_lyrics(&lyrics(sung("Hel-lo", &["h ə", "l oʊ"])));
    let out = sing(
        &mut v,
        &[(60, 0.0, 0.5, Some(0)), (62, 0.5, 1.0, Some(1))],
        1.6,
    );
    assert!(out.iter().all(|x| x.is_finite()));
    let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(peak > 0.05 && peak < 1.0, "peak {peak}");
    assert!(rms(at(&out, 0.2, 0.4)) > 0.02, "the first vowel sounds");
    assert!(rms(at(&out, 0.7, 0.9)) > 0.02, "the second vowel sounds");
    assert!(rms(at(&out, 1.4, 1.6)) < 1e-3, "silence after the release");
}

#[test]
fn a_hiss_is_noisier_than_a_vowel() {
    let mut v = Voice::new(SR);
    v.set_lyrics(&lyrics(sung("sss ah", &["s", "ɑ"])));
    let hiss = sing(&mut v, &[(60, 0.0, 0.08, Some(0))], 0.08);
    let mut v = Voice::new(SR);
    v.set_lyrics(&lyrics(sung("sss ah", &["s", "ɑ"])));
    let vowel = sing(&mut v, &[(48, 0.0, 0.5, Some(1))], 0.5);
    assert!(
        crossings(at(&hiss, 0.02, 0.07)) > 3.0 * crossings(at(&vowel, 0.2, 0.45)),
        "{} vs {}",
        crossings(at(&hiss, 0.02, 0.07)),
        crossings(at(&vowel, 0.2, 0.45))
    );
}

#[test]
fn a_hold_carries_the_vowel_over_the_next_note() {
    let mut v = Voice::new(SR);
    v.set_lyrics(&lyrics(sung("go _", &["g oʊ", ""])));
    let out = sing(
        &mut v,
        &[(60, 0.0, 0.5, Some(0)), (64, 0.5, 1.0, Some(1))],
        1.0,
    );
    // No gap where the notes meet.
    let quietest = (45..55)
        .map(|k| rms(at(&out, k as f32 / 100.0, (k + 1) as f32 / 100.0)))
        .fold(f32::MAX, f32::min);
    assert!(quietest > 0.02, "{quietest}");
}

#[test]
fn notes_without_words_sing_ah() {
    let mut v = Voice::new(SR);
    let out = sing(&mut v, &[(57, 0.0, 0.4, None)], 0.4);
    assert!(rms(at(&out, 0.1, 0.35)) > 0.02);
}

#[test]
fn spoken_words_ignore_the_notes_pitch() {
    let spoken = |key: u8| {
        let mut words = sung("hey", &["h eɪ"]);
        words[0].mode = LyricMode::Speak;
        let mut v = Voice::new(SR);
        v.set_lyrics(&lyrics(words));
        sing(&mut v, &[(key, 0.0, 0.4, Some(0))], 0.5)
    };
    assert_eq!(spoken(48), spoken(72));
}
