//! The built-in groove and fill library.
//!
//! Every row is one drum role written one character per step: `X` accent,
//! `x` normal, `g` ghost, `f` feathered, `.` rest. Spaces and `|` only group
//! the steps for reading. A 4/4 bar has 16 steps (16th notes) or 12 (8th-note
//! triplets); a 3/4 or 6/8 bar has 12 16ths. A row may span several bars.
//!
//! The grooves are original, written for Rosaclef, and named for what they
//! are rather than after songs or drummers.

/// A groove family: the time-keeping parts a drummer plays for one feel.
#[derive(Debug)]
pub struct Groove {
    pub id: &'static str,
    pub style: &'static str,
    pub name: &'static str,
    /// Bar length in beats (quarter notes): 4 for 4/4, 3 for 3/4 and 6/8.
    pub bar_beats: u32,
    /// Steps per bar.
    pub steps: u32,
    /// Tempo range it is written for, in BPM.
    pub tempo: (u32, u32),
    /// The kit it suggests: a General MIDI kit, or `Ebony`.
    pub kit: &'static str,
    /// Suggested swing (0..1) for straight grooves.
    pub swing: f64,
    /// The meter as it reads ("4/4", "6/8").
    pub meter: &'static str,
    /// Part A: verses, the lighter groove.
    pub a: &'static [(&'static str, &'static str)],
    /// Part B: choruses, the bigger groove.
    pub b: &'static [(&'static str, &'static str)],
}

impl PartialEq for Groove {
    fn eq(&self, other: &Groove) -> bool {
        self.id == other.id
    }
}

impl Groove {
    pub fn steps_per_beat(&self) -> u32 {
        self.steps / self.bar_beats
    }
    pub fn part(&self, b: bool) -> &'static [(&'static str, &'static str)] {
        if b {
            self.b
        } else {
            self.a
        }
    }
    /// A drum machine plays it: no human limits apply.
    pub fn machine(&self) -> bool {
        self.kit == "Ebony"
    }
}

/// A fill: rows over the last `beats` beats of a bar.
#[derive(Debug)]
pub struct Fill {
    pub steps_per_beat: u32,
    pub beats: u32,
    /// 1 = leads into a part of the same size, 2 = into something bigger.
    pub energy: u32,
    pub rows: &'static [(&'static str, &'static str)],
}

macro_rules! groove {
    ($id:literal, $style:literal, $name:literal, $meter:literal, $beats:literal x $steps:literal,
     $lo:literal..$hi:literal, $kit:literal, swing $swing:literal,
     a: [$(($ar:literal, $as:literal)),* $(,)?], b: [$(($br:literal, $bs:literal)),* $(,)?]) => {
        Groove {
            id: $id,
            style: $style,
            name: $name,
            meter: $meter,
            bar_beats: $beats,
            steps: $steps,
            tempo: ($lo, $hi),
            kit: $kit,
            swing: $swing,
            a: &[$(($ar, $as)),*],
            b: &[$(($br, $bs)),*],
        }
    };
}

pub static GROOVES: &[Groove] = &[
    groove!("rock-8ths", "Rock", "Straight 8ths", "4/4", 4 x 16, 70..170, "Standard Kit", swing 0.0,
        a: [("hat",   "X.x. X.x. X.x. X.x."),
            ("snare", ".... X... .... X..."),
            ("kick",  "X... .... X.X. ....")],
        b: [("ride",  "X.x. X.x. X.x. X.x."),
            ("snare", ".... X... .... X..."),
            ("kick",  "X... ..X. X.X. ....")]),
    groove!("rock-halftime", "Rock", "Half-time", "4/4", 4 x 16, 70..170, "Standard Kit", swing 0.0,
        a: [("hat",   "x.x. x.x. x.x. x.x."),
            ("snare", ".... .... X... ...."),
            ("kick",  "X... ..X. .... ....")],
        b: [("ride",  "X.x. X.x. X.x. X.x."),
            ("snare", ".... .... X... ...."),
            ("kick",  "X... ..X. ..X. ....")]),
    groove!("rock-16ths", "Rock", "Driving 16ths", "4/4", 4 x 16, 70..130, "Standard Kit", swing 0.0,
        a: [("hat",   "Xxxx Xxxx Xxxx Xxxx"),
            ("snare", ".... X... .... X..."),
            ("kick",  "X... ...x X.X. ....")],
        b: [("openhat", "X.x. X.x. X.x. X.x."),
            ("snare", ".... X... .... X..."),
            ("kick",  "X.x. ...x X.X. ...x")]),
    groove!("pop-backbeat", "Pop", "Backbeat", "4/4", 4 x 16, 80..140, "Standard Kit", swing 0.0,
        a: [("hat",   "X.x. X.x. X.x. X.x."),
            ("snare", ".... X... .... X..."),
            ("kick",  "X... ...x ..X. ....")],
        b: [("hat",   "X.x. X.x. X.x. X.x."),
            ("tamb",  ".... X... .... X..."),
            ("snare", ".... X... .... X..."),
            ("kick",  "X... ..x. X.x. ....")]),
    groove!("pop-four", "Pop", "Four on the floor", "4/4", 4 x 16, 95..135, "Standard Kit", swing 0.0,
        a: [("hat",   "..x. ..x. ..x. ..x."),
            ("snare", ".... X... .... X..."),
            ("kick",  "X... X... X... X...")],
        b: [("hat",   "x... x... x... x..."),
            ("openhat", "..X. ..X. ..X. ..X."),
            ("snare", ".... X... .... X..."),
            ("kick",  "X... X... X... X...")]),
    groove!("ballad", "Ballad", "Slow ballad", "4/4", 4 x 16, 55..95, "Standard Kit", swing 0.0,
        a: [("hat",   "x.x. x.x. x.x. x.x."),
            ("rim",   ".... X... .... X..."),
            ("kick",  "X... .... X... ..x.")],
        b: [("ride",  "x.x. x.x. x.x. x.x."),
            ("snare", ".... X... .... X..."),
            ("kick",  "X... ..x. X... ....")]),
    groove!("funk-16ths", "Funk", "Tight 16ths", "4/4", 4 x 16, 85..115, "Standard Kit", swing 0.0,
        a: [("hat",   "X.x. X.x. X.x. X.x."),
            ("snare", ".g.. X..g .g.. X..g"),
            ("kick",  "X.X. .... ..X. .x..")],
        b: [("hat",   "XxXx XxXx XxXx Xx.x"),
            ("openhat", ".... .... .... ..X."),
            ("snare", ".g.. X..g .g.. X..."),
            ("kick",  "X.X. .... ..X. .x..")]),
    groove!("funk-linear", "Funk", "Linear", "4/4", 4 x 16, 80..110, "Standard Kit", swing 0.0,
        a: [("kick",  "X... ..X. X... ..X."),
            ("hat",   ".x.x .x.x .x.x .x.x"),
            ("snare", "..g. X... ..g. X...")],
        b: [("kick",  "X... ..X. X... ..X."),
            ("bell",  ".x.X .x.x .x.X .x.x"),
            ("snare", "..g. X... ..g. X...")]),
    groove!("soul-motown", "Soul", "Motown", "4/4", 4 x 16, 100..140, "Standard Kit", swing 0.0,
        a: [("hat",   "x.x. x.x. x.x. x.x."),
            ("snare", "x... X... x... X..."),
            ("kick",  "X... ..X. X... ....")],
        b: [("hat",   "x.x. x.x. x.x. x.x."),
            ("tamb",  ".... X... .... X..."),
            ("snare", "x... X... x... X..."),
            ("kick",  "X... ..X. X.X. ....")]),
    groove!("shuffle-blues", "Shuffle", "Blues shuffle", "4/4", 4 x 12, 80..150, "Standard Kit", swing 0.0,
        a: [("hat",   "X.x X.x X.x X.x"),
            ("snare", "... X.. ... X.."),
            ("kick",  "X.. ... X.. ...")],
        b: [("ride",  "X.x X.x X.x X.x"),
            ("snare", "... X.. ... X.."),
            ("kick",  "X.. ..x X.. ..x")]),
    groove!("shuffle-half", "Shuffle", "Half-time shuffle", "4/4", 4 x 12, 75..110, "Standard Kit", swing 0.0,
        a: [("hat",   "X.x X.x X.x X.x"),
            ("snare", ".g. .g. Xg. .g."),
            ("kick",  "X.. ..x ..x ...")],
        b: [("ride",  "X.x X.x X.x X.x"),
            ("snare", ".g. .g. Xg. .g."),
            ("kick",  "X.. ..x ..x ..x")]),
    groove!("jazz-swing", "Jazz", "Medium swing", "4/4", 4 x 12, 90..220, "Jazz Kit", swing 0.0,
        a: [("ride",  "x.. X.x x.. X.x"),
            ("pedal", "... x.. ... x.."),
            ("snare", "... ... ..g ..."),
            ("kick",  "f.. f.. f.. f..")],
        b: [("ride",  "x.. X.x x.. X.x"),
            ("pedal", "... x.. ... x.."),
            ("snare", "..g ... x.. ..g"),
            ("kick",  "f.. f.. f.. f.x")]),
    groove!("hiphop-boombap", "Hip-hop", "Boom bap", "4/4", 4 x 16, 80..100, "Room Kit", swing 0.3,
        a: [("hat",   "x.x. x.x. x.x. x.x."),
            ("snare", ".... X... .... X..."),
            ("kick",  "X... ...x ..X. ....")],
        b: [("hat",   "x.xx x.x. x.xx x.x."),
            ("snare", ".... X... .... X..g"),
            ("kick",  "X... ...x ..X. .x..")]),
    groove!("hiphop-trap", "Hip-hop", "Trap", "4/4", 4 x 16, 130..160, "TR-808 Kit", swing 0.0,
        a: [("hat",   "x.x. x.x. x.x. x.x."),
            ("snare", ".... .... X... ...."),
            ("kick",  "X... ...x ..X. ....")],
        b: [("hat",   "x.x. x.x. x.xx xxxx"),
            ("snare", ".... .... X... ...."),
            ("kick",  "X... ...x ..X. ..x.")]),
    groove!("house", "House", "House", "4/4", 4 x 16, 118..130, "Ebony", swing 0.0,
        a: [("kick",  "X... X... X... X..."),
            ("openhat", "..X. ..X. ..X. ..X."),
            ("clap",  ".... X... .... X...")],
        b: [("kick",  "X... X... X... X..."),
            ("hat",   "xx.x xx.x xx.x xx.x"),
            ("openhat", "..X. ..X. ..X. ..X."),
            ("clap",  ".... X... .... X..."),
            ("shaker", "gxgx gxgx gxgx gxgx")]),
    groove!("techno", "Techno", "Techno", "4/4", 4 x 16, 124..140, "Ebony", swing 0.0,
        a: [("kick",  "X... X... X... X..."),
            ("hat",   "..x. ..x. ..x. ..x."),
            ("rim",   "...x .... ..x. ....")],
        b: [("kick",  "X... X... X... X..."),
            ("hat",   "xx.x xx.x xx.x xx.x"),
            ("openhat", "..X. ..X. ..X. ..X."),
            ("clap",  ".... X... .... X...")]),
    groove!("disco", "Disco", "Disco", "4/4", 4 x 16, 110..130, "Standard Kit", swing 0.0,
        a: [("hat",   "xx.x xx.x xx.x xx.x"),
            ("openhat", "..X. ..X. ..X. ..X."),
            ("snare", ".... X... .... X..."),
            ("kick",  "X... X... X... X...")],
        b: [("hat",   "X... X... X... X..."),
            ("openhat", "..X. ..X. ..X. ..X."),
            ("tamb",  "x.x. x.x. x.x. x.x."),
            ("snare", ".... X... .... X..."),
            ("kick",  "X... X... X... X...")]),
    groove!("dnb-twostep", "Drum & bass", "Two-step", "4/4", 4 x 16, 160..180, "Room Kit", swing 0.0,
        a: [("hat",   "x.x. x.x. x.x. x.x."),
            ("snare", ".... X... .... X..."),
            ("kick",  "X... .... ..X. ....")],
        b: [("ride",  "x.x. x.x. x.x. x.x."),
            ("snare", ".... X..g ..g. X..."),
            ("kick",  "X.X. .... ..X. ....")]),
    groove!("reggae-onedrop", "Reggae", "One drop", "4/4", 4 x 16, 65..90, "Standard Kit", swing 0.4,
        a: [("hat",   "x.x. x.x. x.x. x.x."),
            ("rim",   ".... .... X... ...."),
            ("kick",  ".... .... X... ....")],
        b: [("hat",   "x.x. x.x. x.x. x.x."),
            ("snare", ".... .... X... ...."),
            ("kick",  "X... X... X... X...")]),
    groove!("latin-bossa", "Latin", "Bossa nova", "4/4", 4 x 16, 110..150, "Standard Kit", swing 0.0,
        a: [("hat",   "x.x. x.x. x.x. x.x."),
            ("rim",   "X..X ..X. ..X. .X.."),
            ("pedal", ".... x... .... x..."),
            ("kick",  "X..x X... X..x X...")],
        b: [("ride",  "x.x. x.x. x.x. x.x."),
            ("rim",   "X..X ..X. ..X. .X.."),
            ("pedal", ".... x... .... x..."),
            ("kick",  "X..x X... X..x X...")]),
    groove!("latin-samba", "Latin", "Samba", "4/4", 4 x 16, 90..110, "Standard Kit", swing 0.0,
        a: [("hat",   "x.xx x.xx x.xx x.xx"),
            ("rim",   "...X ..X. ..X. .X.."),
            ("pedal", "..x. ..x. ..x. ..x."),
            ("kick",  "x..x X..x x..x X..x")],
        b: [("ride",  "X.xx X.xx X.xx X.xx"),
            ("snare", "...X ..X. ..X. .X.."),
            ("pedal", "..x. ..x. ..x. ..x."),
            ("kick",  "x..x X..x x..x X..x")]),
    groove!("country-train", "Country", "Two-beat and train", "4/4", 4 x 16, 100..180, "Standard Kit", swing 0.0,
        a: [("hat",   "x.x. x.x. x.x. x.x."),
            ("rim",   ".... X... .... X..."),
            ("kick",  "X... .... X... ....")],
        b: [("snare", "gggg Xggg gggg Xggg"),
            ("kick",  "X... .... X... ....")]),
    groove!("metal-double", "Metal", "Double kick", "4/4", 4 x 16, 120..200, "Power Kit", swing 0.0,
        a: [("ride",  "X.x. X.x. X.x. X.x."),
            ("snare", ".... X... .... X..."),
            ("kick",  "xxxx xxxx xxxx xxxx")],
        b: [("crash", "X... X... X... X..."),
            ("snare", ".... X... .... X..."),
            ("kick",  "xxxx xxxx xxxx xxxx")]),
    groove!("waltz", "Waltz", "Waltz", "3/4", 3 x 12, 80..180, "Standard Kit", swing 0.0,
        a: [("hat",   "x.x. x.x. x.x."),
            ("snare", ".... x... x..."),
            ("kick",  "X... .... ....")],
        b: [("ride",  "x.x. x.x. x.x."),
            ("snare", ".... X... X..."),
            ("kick",  "X... .... ..x.")]),
    // Jazz waltz: a triplet grid (three steps a beat); the ride's "ding, ding-ga,
    // ding", the hat foot on 2 and 3, a feathered kick, comping on the snare.
    groove!("jazz-waltz", "Jazz", "Jazz waltz", "3/4", 3 x 9, 110..220, "Jazz Kit", swing 0.0,
        a: [("ride",  "x.. X.x x.."),
            ("pedal", "... x.. x.."),
            ("snare", "... ... ..g"),
            ("kick",  "f.. ... ...")],
        b: [("ride",  "x.. X.x X.x"),
            ("pedal", "... x.. x.."),
            ("snare", ".g. ... x.g"),
            ("kick",  "f.. ..x ...")]),
    // Brushes on the snare: triplet strokes with the backbeat on 2 (the sweep
    // is a soft steady stream).
    groove!("jazz-waltz-brushes", "Jazz", "Waltz brushes", "3/4", 3 x 9, 80..200, "Brush Kit", swing 0.0,
        a: [("snare", "gfg xfg gfg"),
            ("pedal", "... x.. x.."),
            ("kick",  "f.. ... ...")],
        b: [("snare", "gfg Xfg xfg"),
            ("ride",  "x.. ... x.x"),
            ("pedal", "... x.. x.."),
            ("kick",  "f.. ... f..")]),
    // A 4-bar drum solo for trading with a soloist: A melodic around the toms,
    // B with space and bombs.
    groove!("jazz-waltz-solo", "Jazz", "Waltz drum solo", "3/4", 3 x 9, 110..220, "Jazz Kit", swing 0.0,
        a: [("snare", "Xgg xg. X.. | X.g .gX .g. | ... ... ... | ... ... ..."),
            ("tom1",  "... ... ... | ... ... ... | x.x ... ... | xxx ... ..."),
            ("tom2",  "... ... ... | ... ... ... | ... x.x ... | ... xxx ..."),
            ("tom3",  "... ... ... | ... ... ... | ... ... xXx | ... ... xxX"),
            ("kick",  "... ..x ..x | ..x ... x.. | .x. .x. ... | ... ... ..."),
            ("pedal", "... x.. x.. | ... x.. x.. | ... ... ... | ... ... ...")],
        b: [("crash", "X.. ... ... | ... ... ... | X.. ... ... | ... ... ..."),
            ("kick",  "X.. ... ..x | x.. ... x.. | X.. ..x ... | ... ... X.."),
            ("snare", "... .g. X.. | ..x X.. ..g | ... ... .gX | gxg xgx ..."),
            ("tom1",  "... ... ... | ... ... ... | ... x.. ... | ... ... ..."),
            ("tom3",  "... ... ... | ... ..x ... | ... ... ... | ... ... ..X"),
            ("pedal", "... x.. x.. | ... x.. x.. | ... x.. x.. | ... ... ...")]),
    groove!("ballad-68", "Ballad", "6/8 ballad", "6/8", 3 x 12, 45..80, "Standard Kit", swing 0.0,
        a: [("hat",   "x.x.x. x.x.x."),
            ("snare", "...... X....."),
            ("kick",  "X..... ....x.")],
        b: [("ride",  "X.x.x. X.x.x."),
            ("snare", "...... X....."),
            ("kick",  "X...x. ....x.")]),
];

pub static FILLS: &[Fill] = &[
    // 16th notes, one beat.
    Fill {
        steps_per_beat: 4,
        beats: 1,
        energy: 1,
        rows: &[("snare", "xxxX")],
    },
    Fill {
        steps_per_beat: 4,
        beats: 1,
        energy: 1,
        rows: &[("snare", "x.xx")],
    },
    Fill {
        steps_per_beat: 4,
        beats: 1,
        energy: 2,
        rows: &[("tom1", "xx.."), ("tom3", "..xX")],
    },
    Fill {
        steps_per_beat: 4,
        beats: 1,
        energy: 2,
        rows: &[("snare", "xx.."), ("tom2", "..x."), ("kick", "...x")],
    },
    // 16th notes, two beats.
    Fill {
        steps_per_beat: 4,
        beats: 2,
        energy: 1,
        rows: &[("snare", "gxgx xxXX")],
    },
    Fill {
        steps_per_beat: 4,
        beats: 2,
        energy: 1,
        rows: &[("snare", "X.x. X.x."), ("kick", "X... X...")],
    },
    Fill {
        steps_per_beat: 4,
        beats: 2,
        energy: 2,
        rows: &[
            ("snare", "xx.. ...."),
            ("tom1", "..xx ...."),
            ("tom2", ".... xx.."),
            ("tom3", ".... ..xX"),
        ],
    },
    Fill {
        steps_per_beat: 4,
        beats: 2,
        energy: 2,
        rows: &[("snare", "xx.x x.xx"), ("kick", "..x. .x..")],
    },
    // 16th notes, three beats (3/4 and 6/8 bars).
    Fill {
        steps_per_beat: 4,
        beats: 3,
        energy: 1,
        rows: &[("snare", "x.x. x.x. xxXX")],
    },
    Fill {
        steps_per_beat: 4,
        beats: 3,
        energy: 2,
        rows: &[
            ("snare", "xxxx .... ...."),
            ("tom1", ".... xxxx ...."),
            ("tom3", ".... .... xxxX"),
        ],
    },
    // 16th notes, a 4/4 bar.
    Fill {
        steps_per_beat: 4,
        beats: 4,
        energy: 1,
        rows: &[("snare", "x.x. x.x. xxxx XXXX")],
    },
    Fill {
        steps_per_beat: 4,
        beats: 4,
        energy: 2,
        rows: &[
            ("snare", "xxxx .... .... ...."),
            ("tom1", ".... xxxx .... ...."),
            ("tom2", ".... .... xxxx ...."),
            ("tom3", ".... .... .... xxxX"),
        ],
    },
    Fill {
        steps_per_beat: 4,
        beats: 4,
        energy: 2,
        rows: &[
            ("snare", "xx.x x.xx .xx. xx.."),
            ("kick", "..x. .x.. x..x ..x."),
            ("tom3", ".... .... .... ...X"),
        ],
    },
    Fill {
        steps_per_beat: 4,
        beats: 4,
        energy: 2,
        rows: &[
            ("tom1", "xxxx xxxx .... ...."),
            ("tom2", ".... .... xx.. xx.."),
            ("tom3", ".... .... ..xx ..xX"),
        ],
    },
    // Triplets, one beat.
    Fill {
        steps_per_beat: 3,
        beats: 1,
        energy: 1,
        rows: &[("snare", "xxX")],
    },
    Fill {
        steps_per_beat: 3,
        beats: 1,
        energy: 1,
        rows: &[("snare", "x.x")],
    },
    Fill {
        steps_per_beat: 3,
        beats: 1,
        energy: 2,
        rows: &[("tom1", "x.."), ("tom2", ".x."), ("tom3", "..X")],
    },
    // Triplets, two beats.
    Fill {
        steps_per_beat: 3,
        beats: 2,
        energy: 1,
        rows: &[("snare", "gxx xxX")],
    },
    Fill {
        steps_per_beat: 3,
        beats: 2,
        energy: 2,
        rows: &[
            ("snare", "xx. ..."),
            ("tom1", "..x x.."),
            ("tom2", "... .x."),
            ("tom3", "... ..X"),
        ],
    },
    Fill {
        steps_per_beat: 3,
        beats: 2,
        energy: 2,
        rows: &[("snare", "x.x x.x"), ("kick", ".x. .x.")],
    },
    // Triplets, a 3/4 bar.
    Fill {
        steps_per_beat: 3,
        beats: 3,
        energy: 1,
        rows: &[("snare", "x.x xxx xxX")],
    },
    Fill {
        steps_per_beat: 3,
        beats: 3,
        energy: 2,
        rows: &[
            ("snare", "xx. ... ..."),
            ("tom1", "..x x.. ..."),
            ("tom2", "... .xx ..."),
            ("tom3", "... ... xxX"),
        ],
    },
    Fill {
        steps_per_beat: 3,
        beats: 3,
        energy: 2,
        rows: &[
            ("snare", "x.x .x. x.."),
            ("kick", ".x. x.x .x."),
            ("tom3", "... ... ..X"),
        ],
    },
    // Triplets, a 4/4 bar.
    Fill {
        steps_per_beat: 3,
        beats: 4,
        energy: 1,
        rows: &[("snare", "x.x x.x xxx xxX")],
    },
    Fill {
        steps_per_beat: 3,
        beats: 4,
        energy: 2,
        rows: &[
            ("snare", "xxx ... ... ..."),
            ("tom1", "... xxx ... ..."),
            ("tom2", "... ... xxx ..."),
            ("tom3", "... ... ... xxX"),
        ],
    },
    Fill {
        steps_per_beat: 3,
        beats: 4,
        energy: 2,
        rows: &[("snare", "xx. xx. xx. xx."), ("kick", "..x ..x ..x ..x")],
    },
];

pub fn groove(id: &str) -> Option<&'static Groove> {
    GROOVES.iter().find(|g| g.id == id)
}
