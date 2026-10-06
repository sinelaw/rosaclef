//! The parts' jobs in a mix beyond the lead: the anchors — the kick and the
//! bass — that a groove stands on. An anchor is judged by its level against
//! the mix, not only by whether it is heard: a kick 16 dB under a techno mix
//! is audible and weak. Its range follows the style: dance music (a kick on
//! every beat) keeps the kick about 6–9 dB and the bass 10–13 dB under the
//! mix; other music leaves them more room. The fixes hold the anchors where
//! they are unless a finding is about them, and never cut one to make room
//! for a supporting part.

use rosaclef_core::Project;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Anchor {
    Kick,
    Bass,
}

impl Anchor {
    pub fn name(self) -> &'static str {
        match self {
            Anchor::Kick => "kick",
            Anchor::Bass => "bass",
        }
    }
}

/// Each channel's anchor, in channel order: a drum channel named for a kick
/// (kick, bd, bass drum), and the bass — of the channels the Critic reads as
/// a bass (not a layer), the one named for a sub, else the one playing
/// lowest (an acid line or a bass riff above it is a part, not the floor).
pub fn anchors(p: &Project, roles: &[&str]) -> Vec<Option<Anchor>> {
    let name = |c: &rosaclef_core::Channel| format!("{} {}", c.id, c.name).to_ascii_lowercase();
    let basses: Vec<usize> = p
        .channels
        .iter()
        .enumerate()
        .filter(|(i, c)| c.layer_of.is_none() && roles.get(*i).copied() == Some("bass"))
        .map(|(i, _)| i)
        .collect();
    let bass = basses
        .iter()
        .copied()
        .find(|c| name(&p.channels[*c]).contains("sub"))
        .or_else(|| {
            basses
                .iter()
                .copied()
                .min_by(|a, b| median_pitch(p, *a).total_cmp(&median_pitch(p, *b)))
        });
    p.channels
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if c.layer_of.is_some() {
                return None;
            }
            let role = roles.get(i).copied().unwrap_or("");
            if role == "drums" && kickish(&name(c)) {
                Some(Anchor::Kick)
            } else if bass == Some(i) {
                Some(Anchor::Bass)
            } else {
                None
            }
        })
        .collect()
}

/// The median pitch of a channel's notes (in every pattern; 128 with none).
fn median_pitch(p: &Project, channel: usize) -> f64 {
    let id = &p.channels[channel].id;
    let mut v: Vec<f64> = p
        .patterns
        .iter()
        .flat_map(|pat| pat.notes.iter())
        .filter(|n| &n.channel == id)
        .map(|n| n.pitch as f64)
        .collect();
    if v.is_empty() {
        return 128.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

fn kickish(name: &str) -> bool {
    name.contains("kick")
        || name.contains("bass drum")
        || name.contains("bassdrum")
        || name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|w| w == "bd")
}

/// Whether the song is dance music: a kick channel playing on most beats of
/// the bars it plays in (four on the floor).
pub fn dance(p: &Project, anchors: &[Option<Anchor>]) -> bool {
    anchors
        .iter()
        .enumerate()
        .filter(|(_, a)| **a == Some(Anchor::Kick))
        .any(|(c, _)| on_the_beat(p, c) >= 0.75)
}

/// The share of the beats of its bars a channel plays on (a note within a
/// sixteenth of the beat).
fn on_the_beat(p: &Project, channel: usize) -> f64 {
    let id = &p.channels[channel].id;
    let mut beats: Vec<i64> = vec![];
    let mut bars: Vec<i64> = vec![];
    let per_bar = p.transport.beats_per_bar.max(1) as f64;
    for c in &p.playlist.clips {
        let Some(pat) = p.pattern(&c.pattern) else {
            continue;
        };
        if pat.length <= 0.0 || !pat.notes.iter().any(|n| &n.channel == id) {
            continue;
        }
        let mut k = 0.0;
        while c.start - c.offset + k * pat.length < c.start + c.length {
            let origin = c.start - c.offset + k * pat.length;
            for n in pat.notes.iter().filter(|n| &n.channel == id) {
                let at = origin + n.start;
                if at < c.start || at >= c.start + c.length {
                    continue;
                }
                if (at - at.round()).abs() <= 0.25 {
                    beats.push(at.round() as i64);
                }
                bars.push((at / per_bar).floor() as i64);
            }
            k += 1.0;
        }
    }
    beats.sort_unstable();
    beats.dedup();
    bars.sort_unstable();
    bars.dedup();
    if bars.is_empty() {
        return 0.0;
    }
    beats.len() as f64 / (bars.len() as f64 * per_bar)
}

/// Where an anchor belongs against the mix (dB, low and high).
pub fn range(anchor: Anchor, dance: bool) -> [f64; 2] {
    match (anchor, dance) {
        (Anchor::Kick, true) => [-9.0, -6.0],
        (Anchor::Bass, true) => [-13.0, -10.0],
        (_, false) => [-15.0, -6.0],
    }
}

/// A channel the producer silenced on purpose: muted, or at volume 0 (under
/// -60 dB) with no automation lane bringing it up — no finding is about it
/// and no fix touches it.
pub fn silenced(p: &Project, channel: usize) -> bool {
    let c = &p.channels[channel];
    if c.mute || p.mixer.inserts.get(c.mixer.index()).is_some_and(|i| i.mute) {
        return true;
    }
    let target = format!("channel/{}/volume", c.id);
    let lifted = p
        .automation
        .iter()
        .any(|l| !l.mute && l.target == target && l.points.iter().any(|q| q.value > 1e-3));
    c.volume <= 1e-3 && !lifted
}

/// An insert silenced on purpose: muted, or its fader at 0 with no lane
/// bringing it up.
pub fn silenced_insert(p: &Project, insert: usize) -> bool {
    let Some(ins) = p.mixer.inserts.get(insert) else {
        return false;
    };
    let target = format!("insert/{insert}/volume");
    let lifted = p
        .automation
        .iter()
        .any(|l| !l.mute && l.target == target && l.points.iter().any(|q| q.value > 1e-3));
    insert > 0 && (ins.mute || (ins.volume <= 1e-3 && !lifted))
}

/// Whether a fix's op touches something silenced on purpose.
pub fn touches_silenced(p: &Project, op: &serde_json::Value) -> bool {
    let path = op["path"].as_str().unwrap_or("");
    let num = |s: &str| s.split('/').next().and_then(|x| x.parse::<usize>().ok());
    if let Some(rest) = path.strip_prefix("/channels/") {
        return num(rest).is_some_and(|c| c < p.channels.len() && silenced(p, c));
    }
    if let Some(rest) = path.strip_prefix("/mixer/inserts/") {
        return num(rest).is_some_and(|i| silenced_insert(p, i));
    }
    if let Some(rest) = path.strip_prefix("/automation/") {
        let Some(l) = num(rest).and_then(|i| p.automation.get(i)) else {
            return false;
        };
        let parts: Vec<&str> = l.target.split('/').collect();
        return match parts.as_slice() {
            ["channel", id, ..] => p
                .channels
                .iter()
                .position(|c| c.id == *id)
                .is_some_and(|c| silenced(p, c)),
            ["insert", i, ..] => i.parse::<usize>().is_ok_and(|i| silenced_insert(p, i)),
            _ => false,
        };
    }
    false
}
