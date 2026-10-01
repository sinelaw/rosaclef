//! The voice instrument sings the words of its channel's lyrics, verse by
//! verse, through the whole engine.

use rosaclef_core::{validate, Project};
use rosaclef_engine::render::{render, RenderScope};
use rosaclef_engine::Engine;
use serde_json::json;

/// "Hel-lo" / "Good-bye" on two half notes, verse 1 then verse 2.
fn song(second_verse: Option<u32>) -> Project {
    let mut clip2 = json!({"pattern": "v", "track": 0, "start": 4, "length": 4});
    if let Some(v) = second_verse {
        clip2["verse"] = json!(v);
    }
    let text = json!({
        "format": "rosaclef/1",
        "meta": {"title": "t"},
        "transport": {"bpm": 120},
        "channels": [{"id": "vox", "name": "Vox", "instrument": {"type": "voice"}}],
        "patterns": [{"id": "v", "name": "V", "length": 4,
            "notes": [
                {"channel": "vox", "pitch": 57, "start": 0, "length": 2},
                {"channel": "vox", "pitch": 60, "start": 2, "length": 2}
            ],
            "lyrics": [{"channel": "vox", "verses": {"1": "Hel-lo", "2": "Good-bye"}}]}],
        "playlist": {"tracks": [{"name": "T"}], "clips": [
            {"pattern": "v", "track": 0, "start": 0, "length": 4}, clip2]},
        "mixer": {"inserts": [{"name": "Master"}]}
    })
    .to_string();
    let checked = validate::parse_and_validate(&text);
    assert!(checked.is_ok(), "{:?}", checked.issues);
    checked.project.unwrap()
}

fn song_audio(p: Project) -> Vec<f32> {
    let mut e = Engine::new(48000.0);
    e.set_project(p);
    render(&mut e, &RenderScope::Song).left
}

#[test]
fn the_voice_sings_each_clips_verse() {
    let same = song_audio(song(Some(1)));
    let other = song_audio(song(Some(2)));
    let half = 4 * 24000; // the second clip starts at 2 s
    assert!(same.iter().all(|x| x.is_finite()));
    let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
    assert!(rms(&same[..half]) > 0.02, "it sings");
    assert_eq!(same[..half], other[..half], "verse 1 both times");
    assert_ne!(same[half..], other[half..], "the second clip sings verse 2");
    // Without a clip verse, the second clip (no repeat) sings verse 1 too.
    assert_eq!(song_audio(song(None)), same);
}

/// One phrase at beat 2 (1 s) on a voice that plays renders.
fn rendering() -> Project {
    let text = json!({
        "format": "rosaclef/1",
        "meta": {"title": "t"},
        "transport": {"bpm": 120},
        "channels": [{"id": "vox", "name": "Vox",
            "instrument": {"type": "voice", "options": {"engine": "render", "voice": "test"}}}],
        "patterns": [{"id": "v", "name": "V", "length": 8,
            "notes": [{"channel": "vox", "pitch": 57, "start": 2, "length": 1}],
            "lyrics": [{"channel": "vox", "verses": {"1": "la"}}]}],
        "playlist": {"tracks": [{"name": "T"}], "clips": [
            {"pattern": "v", "track": 0, "start": 0, "length": 8}]},
        "mixer": {"inserts": [{"name": "Master"}]}
    })
    .to_string();
    validate::parse_and_validate(&text).project.unwrap()
}

fn first_sound(x: &[f32]) -> f32 {
    x.iter().position(|v| v.abs() > 1e-4).unwrap_or(x.len()) as f32 / 48000.0
}

#[test]
fn a_rendered_phrase_plays_ahead_of_its_note() {
    let mut e = Engine::new(48000.0);
    e.set_project(rendering());
    let wanted = e.wanted_renders();
    assert_eq!(wanted.len(), 1);
    assert!(wanted[0].starts_with("renders/voice/test/"), "{wanted:?}");
    // Not rendered yet: the formant voice sings, on the note.
    let sung = render(&mut e, &RenderScope::Song).left;
    assert!(
        (first_sound(&sung) - 1.0).abs() < 0.01,
        "{}",
        first_sound(&sung)
    );
    // Rendered: the phrase's audio starts LEAD (0.3 s) before the note.
    let sine = (0..24000)
        .map(|i| 0.25 * (i as f32 * 200.0 / 24000.0 * std::f32::consts::TAU).sin())
        .collect();
    let audio = rosaclef_engine::samples::SampleData {
        sample_rate: 24000.0,
        channels: vec![sine],
    };
    e.set_sample(&wanted[0], audio);
    let played = render(&mut e, &RenderScope::Song).left;
    assert!(
        (first_sound(&played) - 0.7).abs() < 0.01,
        "{}",
        first_sound(&played)
    );
    let window = &played[(1.1 * 48000.0) as usize..(1.3 * 48000.0) as usize];
    let rms = (window.iter().map(|v| v * v).sum::<f32>() / window.len() as f32).sqrt();
    let want = 0.25 * 0.8 * 0.8 / std::f32::consts::SQRT_2;
    assert!(
        (rms - want).abs() < 0.01,
        "the render alone, at the voice's gain: {rms}"
    );
    assert!(
        played[(1.8 * 48000.0) as usize].abs() < 1e-4,
        "it ends with its audio"
    );
}
