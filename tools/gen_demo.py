#!/usr/bin/env python3
"""Generate the bundled demo song "Velvet Hour".

A four-minute journey in F minor at 92 BPM, told by one harp theme:

  Prologue    bars  1-8   the theme alone, slow, over a breathing pad
  Journey     bars  9-24  the band gathers; the theme is answered and developed
  Rise        bars 25-32  a climbing bass line, strings and a riser
  Summit      bars 33-48  the full band and the theme at its highest
  Fall        bars 49-56  an impact, then ruin: a drone, fragments of the theme
  Struggle    bars 57-68  a half-time grind; the theme tries to climb and falls
                          back, then breaks through on a long ascent
  Redemption  bars 69-84  still F minor, but whole: the theme returns in full,
                          its open question finally cadencing home (C7b9 -> Fm)
  Epilogue    bars 85-92  the theme alone again, one ironic bar in E major,
                          and a quiet F minor to close

Drums are written per bar (not looped) with a drummer's ghost notes, dynamic
hats and phrase-end fills; timing is humanized with a seeded RNG.

Usage:
    cargo build --release -p rosaclef
    python3 tools/gen_demo.py > crates/server/assets/demo/project.json
    ./target/release/rosaclef fmt crates/server/assets/demo/project.json

Instrument settings come from the factory presets (`rosaclef presets NAME`),
so the demo always matches the real preset definitions.
"""
import json
import os
import random
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.environ.get("ROSACLEF", os.path.join(ROOT, "target", "release", "rosaclef"))
BPM = 92
rng = random.Random(1729)


def preset(name, **params):
    """Instrument JSON of a factory preset, with optional overrides."""
    out = subprocess.run([EXE, "presets", name], capture_output=True, text=True, check=True).stdout
    dev = json.loads(out)
    dev.setdefault("params", {}).update(params)
    return dev


def drum(kind, **params):
    return {"type": "drum", "params": params, "options": {"kind": kind}}


GOLD, ROSE, CHAMPAGNE, BURGUNDY = "#d4af37", "#c97b84", "#e8d5b0", "#8e3b46"
EMERALD, SAPPHIRE, AMETHYST, BRONZE, EMBER, TEAL = "#3f8f7a", "#4a6fa5", "#8a6bb0", "#b08d57", "#d98c5f", "#6fa3a0"
ONYX, PEARL = "#5a4e5e", "#d9d4e8"

# ------------------------------------------------------------------ channels
# (id, name, color, instrument, mixer insert, volume, pan)
CHANNELS = [
    ("kick", "Kick", GOLD, drum("kick", tone=0.3, snap=0.45, decay=1.0, tune=-2, gain=0.85), 1, 0.9, 0),
    ("snare", "Snare", ROSE, drum("snare", tone=0.35, snap=0.4, decay=1.05, tune=-1, gain=0.8), 1, 0.72, 0.02),
    ("clap", "Clap", ROSE, drum("clap", tone=0.35, decay=1.1, gain=0.6), 1, 0.4, -0.04),
    ("rim", "Rim", BRONZE, drum("rim", tone=0.45, tune=1, gain=0.6), 1, 0.36, 0.22),
    ("tom", "Toms", BRONZE, drum("tom", tone=0.35, decay=0.9, snap=0.35, gain=0.75), 1, 0.55, -0.12),
    ("hat", "Hi-hat", CHAMPAGNE, drum("hat", tone=0.28, decay=0.8, snap=0.3, gain=0.6), 2, 0.4, 0.18),
    ("openhat", "Open Hat", CHAMPAGNE, drum("openhat", tone=0.25, decay=0.6, gain=0.55), 2, 0.3, 0.2),
    ("shaker", "Shaker", BRONZE, drum("shaker", tone=0.35, decay=0.9, gain=0.6), 2, 0.22, -0.3),
    ("bass", "Bronze Bass", BURGUNDY, preset("Bronze Souverain"), 3, 0.52, 0),
    ("keys", "Opaline Keys", GOLD, preset("Opaline Keys"), 4, 0.5, -0.08),
    ("pad", "Opaline Veil", AMETHYST, preset("Opaline Veil"), 5, 0.3, 0),
    ("choir", "Voile de Chœur", AMETHYST, preset("Voile de Chœur"), 5, 0.4, 0.1),
    ("dawn", "Aurore Spectrale", AMETHYST, preset("Aurore Spectrale"), 5, 0.3, -0.1),
    ("bowls", "Grotte aux Bols", ONYX, preset("Grotte aux Bols"), 5, 0.34, 0),
    ("harp", "Harpe de Saphir", SAPPHIRE, preset("Harpe de Saphir"), 6, 0.74, 0.06),
    ("strings", "Soie Vagabonde", ROSE, preset("Soie Vagabonde"), 9, 0.58, -0.12),
    ("air", "Souffle d'Opale", EMERALD, preset("Souffle d'Opale"), 7, 0.36, 0),
    ("embers", "Cendres d'Ambre", EMBER, preset("Cendres d'Ambre"), 7, 0.34, 0),
    ("stardust", "Poussière d'Astres", PEARL, preset("Poussière d'Astres"), 7, 0.26, 0),
    ("crypt", "Crypte de Grenat", ONYX, preset("Crypte de Grenat"), 10, 0.3, 0.15),
    ("pearls", "Pluie de Perles", TEAL, preset("Pluie de Perles"), 10, 0.22, 0),
    ("sparkle", "Étincelles de Diamant", PEARL, preset("Étincelles de Diamant"), 10, 0.2, 0),
    ("bell", "Cathédrale d'Argent", CHAMPAGNE, preset("Cathédrale d'Argent"), 10, 0.3, 0),
    ("ascent", "Voie Lactée", CHAMPAGNE, preset("Voie Lactée"), 8, 0.36, 0),
    ("riser", "Ascension d'Or", CHAMPAGNE, preset("Ascension d'Or"), 8, 0.32, 0),
    ("impact", "Météore", BRONZE, preset("Météore de Basalte"), 8, 0.5, 0),
    ("subdrop", "Abîme d'Encre", ONYX, preset("Abîme d'Encre"), 8, 0.5, 0),
    ("fall", "Déclin de Lune", CHAMPAGNE, preset("Déclin de Lune"), 8, 0.32, 0),
]

# ------------------------------------------------------------------ patterns & clips

TRACKS = ["Drums", "Bass", "Keys", "Pads", "Strings & Choir", "Harp", "Textures", "Sequences", "FX"]
T_DRUMS, T_BASS, T_KEYS, T_PADS, T_STRINGS, T_HARP, T_TEX, T_SEQ, T_FX = range(len(TRACKS))

patterns = {}  # id -> {name, color, length, notes}
order = []
clips = []


def pattern(pid, name, color, length):
    if pid not in patterns:
        patterns[pid] = {"name": name, "color": color, "length": length, "notes": []}
        order.append(pid)
    return pid


def clip(pid, track, start, length=None):
    clips.append((pid, track, start, patterns[pid]["length"] if length is None else length))


def note(pid, ch, pitch, start, length, vel, human=0.006):
    """Add a note with human timing (never before 0) and velocity jitter."""
    t = max(0.0, start + (rng.uniform(-human, human) if human else 0.0))
    v = min(1.0, max(0.05, vel + (rng.uniform(-0.035, 0.035) if human else 0.0)))
    patterns[pid]["notes"].append(
        {"channel": ch, "pitch": pitch, "start": round(t, 4), "length": round(length, 4), "velocity": round(v, 3)})


SWING = 0.035  # beats: late 16th off-beats, a drummer's lazy feel


def sw(pos):
    """Delay the e / a sixteenths (x.25, x.75)."""
    frac = pos % 1
    return pos + SWING if abs(frac - 0.25) < 1e-6 or abs(frac - 0.75) < 1e-6 else pos


# ------------------------------------------------------------------ harmony
# name -> (keys voicing, bass root)
CHORDS = {
    "Fm9": ([56, 60, 63, 67], 41),
    "Dbmaj9": ([53, 56, 60, 63], 37),
    "Abmaj9": ([55, 58, 60, 63], 44),
    "Eb9sus": ([56, 58, 61, 65], 39),
    "Eb13": ([55, 60, 61, 65], 39),
    "Cm9": ([51, 55, 58, 62], 36),
    "C7b9": ([52, 58, 61, 67], 36),
    "C9sus": ([55, 58, 62, 65], 36),
    "C9": ([55, 58, 62, 64], 36),
    "Gbmaj7/F": ([54, 58, 61, 65], 41),
    "Bbm9": ([56, 60, 61, 65], 34),
    "E": ([52, 56, 59, 64], 40),  # the joke
}

PROLOGUE = [("Fm9", 8), ("Dbmaj9", 8), ("Abmaj9", 8), ("Eb9sus", 4), ("Eb13", 4)]
QUESTION = [("Fm9", 4), ("Dbmaj9", 4), ("Abmaj9", 4), ("Eb9sus", 2), ("Eb13", 2)]
JOURNEY = QUESTION + QUESTION + [("Dbmaj9", 4), ("Eb13", 4), ("Cm9", 4), ("Fm9", 4)] \
    + [("Bbm9", 4), ("Cm9", 4), ("Dbmaj9", 4), ("Eb9sus", 2), ("Eb13", 2)]
RISE = [("Bbm9", 4), ("Cm9", 4), ("Dbmaj9", 4), ("Eb9sus", 4),
        ("Bbm9", 4), ("Cm9", 4), ("Dbmaj9", 2), ("Eb13", 2), ("Eb13", 4)]
SUMMIT_CYCLE = [("Dbmaj9", 4), ("Eb9sus", 4), ("Cm9", 4), ("Fm9", 4),
                ("Dbmaj9", 4), ("Eb13", 4), ("Abmaj9", 4), ("C7b9", 4)]
SUMMIT = SUMMIT_CYCLE + SUMMIT_CYCLE
FALL = [("Fm9", 8), ("Gbmaj7/F", 8), ("Fm9", 8), ("Gbmaj7/F", 4), ("C7b9", 4)]
GRIND = [("Fm9", 4), ("Dbmaj9", 4), ("Bbm9", 4), ("C7b9", 4)]
CLIMB = [("Dbmaj9", 4), ("Bbm9", 4), ("C9sus", 4), ("C9", 4)]
STRUGGLE = GRIND + GRIND + CLIMB
# The question again, but now answered by the dominant: Bbm9 C7b9 -> Fm9.
ANSWER = [("Fm9", 4), ("Dbmaj9", 4), ("Abmaj9", 4), ("Bbm9", 2), ("C7b9", 2)]
REDEMPTION = ANSWER + ANSWER + [("Dbmaj9", 4), ("Eb13", 4), ("Cm9", 4), ("Fm9", 4)] \
    + [("Bbm9", 4), ("C7b9", 4), ("Fm9", 8)]
EPILOGUE = [("Dbmaj9", 8), ("Fm9", 8), ("Dbmaj9", 4), ("E", 4), ("Fm9", 8)]

# Sections: (id, title, start beat, beats, progression)
SECTIONS = []
_bar = 0
for sid, title, prog in [("prologue", "Prologue", PROLOGUE), ("journey", "Journey", JOURNEY),
                         ("rise", "Rise", RISE), ("summit", "Summit", SUMMIT), ("fall", "Fall", FALL),
                         ("struggle", "Struggle", STRUGGLE), ("redemption", "Redemption", REDEMPTION),
                         ("epilogue", "Epilogue", EPILOGUE)]:
    beats = sum(d for _, d in prog)
    SECTIONS.append((sid, title, _bar * 4, beats, prog))
    _bar += beats // 4
SEC = {sid: (start, beats, title) for sid, title, start, beats, _prog in SECTIONS}


def at(sid, bar=0):
    return SEC[sid][0] + bar * 4


def section_pattern(kind, sid, color, track):
    """A pattern spanning a whole section, placed on the playlist."""
    start, beats, title = SEC[sid]
    pid = pattern(f"{kind.lower()}-{sid}", f"{kind} · {title}", color, beats)
    clip(pid, track, start)
    return pid


# ------------------------------------------------------------------ parts: keys, bass, held chords

COMP = [  # (start, length, velocity) rhythms for one bar of a chord
    [(0, 1.4, .6), (1.75, .6, .46), (2.5, 1.2, .56)],
    [(0, 2.6, .58), (3.0, .7, .42)],
    [(0.5, 1.0, .53), (2.0, .5, .48), (2.75, 1.0, .54)],
    [(0, 1.0, .58), (1.5, .45, .44), (3.25, .7, .5)],
    [(0, 1.6, .56), (2.5, .5, .46), (3.0, .9, .5)],
]


def strum(pid, voicing, t, length, vel, spread=0.012):
    for k, pitch in enumerate(voicing):
        note(pid, "keys", pitch, t + k * spread, length, vel - k * 0.03)


def keys(pid, prog, style, vel=1.0, t0=0.0):
    t = t0
    total = sum(d for _, d in prog)
    for name, dur in prog:
        voicing = CHORDS[name][0]
        if style == "sparse":
            strum(pid, voicing, t + rng.choice((0, 0.5)), dur * 0.85, 0.36 * vel, spread=0.03)
        elif style == "pulse":  # eighths that grow through the section
            for s in range(int(dur * 2)):
                pos = t + s * 0.5
                grow = (pos - t0) / total
                strum(pid, voicing, pos, 0.35, (0.34 + 0.24 * grow + (0.1 if s % 2 == 0 else 0)) * vel, spread=0.006)
        else:  # "comp"
            if dur >= 4:
                for bar in range(int(dur // 4)):
                    for start, length, v in rng.choice(COMP):
                        strum(pid, voicing, sw(t + bar * 4 + start), length, v * vel)
            else:
                for start, length, v in ((0, 1.2, .56), (1.25, .6, .44)):
                    strum(pid, voicing, sw(t + start), length, v * vel)
        t += dur


BASS_FIGURES = [  # (start, length, role, velocity); roles: R root, O octave, F fifth, A approach
    [(0, .7, "R", .9), (0.75, .2, "O", .42), (1.5, .45, "R", .72), (2.5, .45, "F", .74), (3.25, .2, "O", .46), (3.75, .25, "A", .62)],
    [(0, 1.4, "R", .9), (1.5, .4, "F", .64), (2, .4, "O", .5), (2.5, .9, "R", .78), (3.5, .25, "F", .55), (3.75, .25, "A", .6)],
    [(0, .45, "R", .9), (0.5, .2, "R", .38), (1, .9, "F", .7), (2.25, .5, "R", .76), (3, .45, "O", .55), (3.5, .4, "A", .6)],
]


def bass(pid, prog, style, vel=1.0, t0=0.0):
    t = t0
    for i, (name, dur) in enumerate(prog):
        root = CHORDS[name][1]
        nxt = CHORDS[prog[(i + 1) % len(prog)][0]][1]
        appr = nxt - 1 if nxt > root else (nxt + 1 if nxt < root else root + 7)
        roles = {"R": root, "O": root + 12, "F": root + 7, "A": appr}
        if style == "sustain":
            note(pid, "bass", root, t, dur - 0.1, 0.7 * vel)
        elif style == "pulse":  # half-time grind: 3-3-2 eighths
            for s in range(int(dur * 2)):
                accent = s % 8 in (0, 3, 6)
                note(pid, "bass", root + (12 if s % 8 == 7 else 0), sw(t + s * 0.5), 0.35, (0.84 if accent else 0.5) * vel)
        elif style == "climb":  # quarter notes that grow toward the next chord
            for s in range(int(dur)):
                note(pid, "bass", [root, root + 7, root + 12, root + 7][s % 4], t + s, 0.8, (0.72 + 0.04 * s) * vel)
        elif style == "eighths":  # straight eighths, octave pops, into the summit
            for s in range(int(dur * 2)):
                note(pid, "bass", root + (12 if s % 2 else 0), t + s * 0.5, 0.35, (0.62 + 0.25 * s / (dur * 2)) * vel)
        else:  # "walk"
            if dur >= 4:
                bars = int(dur // 4)
                for bar in range(bars):
                    for start, length, role, v in rng.choice(BASS_FIGURES):
                        pitch = roles[role] if (role != "A" or bar == bars - 1) else root + 7
                        note(pid, "bass", pitch, sw(t + bar * 4 + start), length, v * vel)
            else:
                for start, length, role, v in ((0, .6, "R", .88), (1.0, .4, "F", .66), (1.75, .25, "A", .6)):
                    note(pid, "bass", roles[role], sw(t + start), length, v * vel)
        t += dur


def hold(pid, prog, ch, octave=12, vel=0.55, top=True, t0=0.0):
    """Held chord tones (pads, choir, strings)."""
    t = t0
    for name, dur in prog:
        voicing = CHORDS[name][0]
        for pitch in (voicing[1:] if top else voicing):
            note(pid, ch, pitch + octave, t, dur, vel, human=0)
        t += dur


def line(pid, ch, notes, t0=0.0, stretch=1.0, transpose=0, vel=1.0):
    """A melodic line of (start, pitch, length, velocity) tuples."""
    for start, pitch, length, v in notes:
        pos = t0 + start * stretch
        note(pid, ch, pitch + transpose, sw(pos) if stretch == 1 else pos, length * stretch, v * vel)


# ------------------------------------------------------------------ the theme
# The question, over Fm9 | Dbmaj9 | Abmaj9 | Eb9sus-Eb13: it ends open, on the 5th of Eb.
THEME = [(0, 72, 1, .62), (1, 75, .5, .55), (1.5, 77, 1.5, .68), (3.5, 75, .5, .5),
         (4, 72, 1.5, .6), (5.5, 68, .5, .5), (6, 70, 2, .55),
         (8, 72, .5, .58), (8.5, 75, .5, .58), (9, 79, 1.5, .7), (10.5, 77, .5, .55), (11, 75, 1, .58),
         (12, 77, 1.5, .62), (13.5, 73, .5, .5), (14, 70, 2, .55)]
# The same question, reaching upward (hope).
THEME_HOPE = THEME[:-3] + [(12, 77, 1, .62), (13, 80, 1, .64), (14, 79, 2, .66)]
# The theme answered, over ... Bbm9 C7b9: the leading tone E pulls home to the next F minor.
THEME_ANSWER = THEME[:-3] + [(12, 77, 1.5, .68), (13.5, 73, .5, .56), (14, 76, 1, .66), (15, 79, 1, .62)]
# A new phrase over Dbmaj9 | Eb13 | Cm9 | Fm9: the summit, foreshadowed.
JOURNEY_C3 = [(0, 77, 1.5, .64), (1.5, 75, .5, .5), (2, 72, 2, .58),
              (4, 79, 1, .64), (5, 77, 1, .56), (6, 75, 2, .58),
              (8, 74, 1, .56), (9, 75, 1, .58), (10, 79, 2, .64),
              (12, 77, 1, .6), (13, 75, 1, .54), (14, 72, 2, .56)]
# The motif in rising sequence over the climbing bass Bb C Db Eb.
JOURNEY_C4 = [(0, 72, .5, .58), (0.5, 73, .5, .58), (1, 77, 2, .66),
              (4, 74, .5, .6), (4.5, 75, .5, .6), (5, 79, 2, .68),
              (8, 75, .5, .62), (8.5, 77, .5, .62), (9, 80, 2, .72),
              (12, 80, 1, .7), (13, 82, 1, .72), (14, 79, 2, .7)]
SUMMIT_A = [(0, 77, 1.5, .7), (1.5, 75, .5, .56), (2, 77, .5, .6), (2.5, 80, 1.5, .74),
            (4, 82, 2, .76), (6, 80, .5, .6), (6.5, 77, 1.5, .66),
            (8, 75, 1, .64), (9, 79, 1, .7), (10, 75, .5, .56), (10.5, 74, 1.5, .62),
            (12, 72, 2.5, .64), (14.5, 68, .5, .5), (15, 72, 1, .58),
            (16, 77, .5, .68), (16.5, 80, .5, .7), (17, 84, 2, .8), (19, 82, .5, .62), (19.5, 80, .5, .6),
            (20, 79, 1.5, .7), (21.5, 77, .5, .58), (22, 75, 1, .62), (23, 72, 1, .58),
            (24, 75, 1, .64), (25, 79, .5, .66), (25.5, 80, 2.5, .72),
            (28, 79, 1, .66), (29, 76, 1, .62), (30, 73, 1, .6), (31, 70, 1, .58)]
# Second pass: the same climb, then higher, then a fall from the peak.
SUMMIT_B = SUMMIT_A[:14] + [
    (16, 80, .5, .72), (16.5, 84, .5, .76), (17, 87, 2, .86), (19, 84, 1, .7),
    (20, 85, 1.5, .8), (21.5, 84, .5, .66), (22, 82, 2, .72),
    (24, 84, 1, .74), (25, 82, .5, .64), (25.5, 79, 2.5, .7),
    (28, 76, 1, .62), (29, 73, 1, .56), (30, 70, 1, .5), (31, 67, 1, .44)]
# The fall: the theme broken into sighs.
FALL_LINE = [(4, 72, 3, .46), (8, 73, 3, .42), (12, 72, 3, .38),
             (18, 68, 2, .4), (20, 65, 4, .36), (25, 70, 2, .34), (28, 67, 2, .34), (30, 64, 2, .32)]
# The struggle: the theme tries to rise and falls back; tries higher; then breaks through.
STRUGGLE_LINE = [(0, 60, 1, .5), (1, 63, .5, .46), (1.5, 65, 2.5, .52),
                 (4, 63, 1, .46), (5, 60, 3, .44),
                 (8, 61, 1, .48), (9, 65, 1, .5), (10, 68, 2, .54),
                 (12, 67, 1, .5), (13, 64, 1, .46), (14, 61, 2, .44),
                 (16, 72, 1, .58), (17, 75, .5, .56), (17.5, 77, 1.5, .62), (19, 79, 1, .64),
                 (20, 80, 2, .68), (22, 77, 2, .58),
                 (24, 73, 1, .6), (25, 77, 1, .64), (26, 80, 1, .68), (27, 84, 1, .72),
                 (28, 82, 2, .7), (30, 79, 1, .62), (31, 76, 1, .6),
                 (32, 77, 1, .64), (33, 80, 1, .68), (34, 84, 2, .74),
                 (36, 85, 2, .76), (38, 84, 2, .72),
                 (40, 82, 1, .74), (41, 84, 1, .78), (42, 86, 2, .82),
                 (44, 88, 4, .86)]
# Redemption, over Dbmaj9 | Eb13 | Cm9 | Fm9: the summit's height, without the fall.
REDEMPTION_C3 = [(0, 80, 1, .72), (1, 84, 1, .76), (2, 87, 2, .82),
                 (4, 85, 1.5, .76), (5.5, 84, .5, .64), (6, 82, 2, .72),
                 (8, 79, 1, .68), (9, 82, 1, .72), (10, 86, 2, .8),
                 (12, 84, 1.5, .76), (13.5, 80, .5, .62), (14, 79, 1, .68), (15, 77, 1, .66)]
# Over Bbm9 | C7b9 | Fm9: the cadence the whole song has been waiting for.
REDEMPTION_C4 = [(0, 77, 1, .7), (1, 80, 1, .72), (2, 85, 1.5, .78), (3.5, 84, .5, .62),
                 (4, 82, 1.5, .74), (5.5, 79, .5, .62), (6, 76, 2, .7),
                 (8, 77, 4, .76),
                 (12, 72, 1, .56), (13, 75, 1, .56), (14, 79, 2, .58)]
# Epilogue: Dbmaj9 | Fm9 | Dbmaj9 | E (!) | Fm9.
EPILOGUE_LINE = [(0, 72, 2, .5), (2, 75, 1, .44), (3, 77, 5, .5),
                 (8, 75, 2, .46), (10, 72, 2, .42), (12, 68, 4, .44),
                 (16, 73, 2, .42), (18, 72, 2, .4),
                 # One bar, deadpan, in the wrong key: the theme's opening in E major.
                 (20, 71, .5, .62), (20.5, 76, .5, .6), (21, 80, .5, .64), (21.5, 76, .5, .56), (22, 71, .75, .58),
                 # ...and back, as if nothing happened.
                 (24, 79, 2, .4), (26, 77, 6, .42)]

# ------------------------------------------------------------------ drums

KICKS = [
    [(0, .92), (1.75, .55), (2.5, .84)],
    [(0, .9), (0.75, .48), (2.5, .8), (3.25, .58)],
    [(0, .92), (2.25, .6), (2.5, .8)],
    [(0, .94), (1.5, .6), (2.5, .86), (2.75, .5)],
    [(0, .92), (1.75, .55), (2.5, .84), (3.5, .55)],
]
GHOST_SPOTS = [0.75, 1.75, 2.25, 2.75, 3.25, 3.5, 3.75]


def drum_bar(pid, t, style, e, fill=None):
    """One bar of drums at pattern beat `t` with energy `e` (0..1).
    Styles: brush, groove, big, halftime, build. Fills: small, tom, roll, stop."""
    ev = []  # (pos, channel, pitch, length, velocity)

    def hats(busy, open_on=()):
        for beat in range(4):
            for sub, base in ((0.0, 0.46), (0.25, 0.18), (0.5, 0.34), (0.75, 0.22)):
                pos = beat + sub
                if pos in open_on:
                    ev.append((pos, "openhat", 60, 0.4, 0.4 + 0.1 * e))
                elif sub in (0.25, 0.75) and rng.random() > busy:
                    continue
                else:
                    accent = 0.08 if beat % 2 == 0 and sub == 0 else 0.0
                    ev.append((pos, "hat", 60, 0.1, base + accent + 0.06 * e))

    if style == "brush":
        for pos in (1, 3):
            ev.append((pos, "rim", 60, 0.1, 0.34 + 0.12 * e))
        if rng.random() < 0.5:
            ev.append((rng.choice((1.75, 2.75, 3.25)), "rim", 60, 0.1, 0.22))
        for s in range(16):
            if s % 2 == 1 or rng.random() < 0.3:
                ev.append((s * 0.25, "shaker", 60, 0.1, 0.1 + 0.06 * e + (0.05 if s % 4 == 2 else 0)))
        hats(busy=0.15 + 0.3 * e)
        if e > 0.35:
            ev.append((0, "kick", 60, 0.25, 0.5 + 0.2 * e))
            if rng.random() < 0.6:
                ev.append((2.5, "kick", 60, 0.25, 0.42 + 0.2 * e))
    elif style in ("groove", "big"):
        for pos, v in rng.choice(KICKS):
            ev.append((pos, "kick", 60, 0.25, v * (0.85 + 0.15 * e)))
        ev.append((1, "snare", 60, 0.25, 0.8 + 0.12 * e))
        ev.append((3, "snare", 60, 0.25, 0.84 + 0.12 * e))
        for pos in rng.sample(GHOST_SPOTS, rng.choice((1, 2, 3))):
            ev.append((pos, "snare", 60, 0.1, rng.uniform(0.12, 0.24)))
        if style == "big":
            ev.append((3.01, "clap", 60, 0.25, 0.5 + 0.15 * e))
            ev.append((1.01, "clap", 60, 0.25, 0.3))
            hats(busy=0.5 + 0.4 * e, open_on=(1.5, 3.5) if rng.random() < 0.5 else (3.5,))
            for s in range(16):
                if s % 4 in (1, 3) or rng.random() < 0.25:
                    ev.append((s * 0.25, "shaker", 60, 0.1, 0.14 + (0.08 if s % 2 else 0)))
        else:
            hats(busy=0.3 + 0.5 * e, open_on=(3.5,) if rng.random() < 0.4 else ())
    elif style == "halftime":
        ev.append((0, "kick", 60, 0.25, 0.95))
        for pos, v in rng.sample([(1.5, .55), (2.75, .5), (3.5, .6), (0.75, .45)], 2 if e > 0.6 else 1):
            ev.append((pos, "kick", 60, 0.25, v))
        ev.append((2, "snare", 60, 0.3, 0.95))
        if e > 0.6:
            ev.append((2.01, "clap", 60, 0.25, 0.46))
        ev.append((rng.choice((1.25, 3.25, 3.75)), "snare", 60, 0.1, 0.18))
        for s in range(8):
            ev.append((s * 0.5, "hat", 60, 0.1, (0.36 if s % 2 == 0 else 0.22) + 0.05 * e))
            if e > 0.6 and rng.random() < 0.4:
                ev.append((s * 0.5 + 0.25, "hat", 60, 0.1, 0.16))
        if rng.random() < 0.35:
            ev.append((3.5, "tom", 55, 0.25, 0.5))
            ev.append((3.75, "tom", 52, 0.25, 0.45))
    elif style == "build":
        for q in range(4):
            ev.append((q, "kick", 60, 0.25, 0.8 + 0.04 * q))
            ev.append((q, "hat", 60, 0.1, 0.34))
        for s in range(8):
            ev.append((s * 0.5, "snare", 60, 0.1, 0.25 + 0.5 * e * s / 7))

    limit = {"small": 3.0, "tom": 2.0, "roll": 0.0, "stop": 2.0}.get(fill, 4.0)
    ev = [x for x in ev if x[0] < limit]
    if fill == "small":
        for i, pos in enumerate(rng.choice([(3.25, 3.5, 3.75), (3.0, 3.5, 3.75), (3.5, 3.75)])):
            ev.append((pos, "snare", 60, 0.1, 0.3 + 0.13 * i))
        ev.append((3.0, "kick", 60, 0.25, 0.6))
    elif fill == "tom":
        ev.append((2.0, "kick", 60, 0.25, 0.86))
        for pos, pitch, v in [(2.0, 67, .7), (2.25, 67, .5), (2.5, 64, .72), (2.75, 64, .5),
                              (3.0, 60, .75), (3.25, 60, .55), (3.5, 55, .8), (3.75, 55, .62)]:
            ev.append((pos, "tom", pitch, 0.25, v))
        ev.append((3.5, "openhat", 60, 0.5, 0.34))
    elif fill == "roll":
        for q in range(4):
            ev.append((q, "kick", 60, 0.25, 0.8))
        for s in range(15):  # leaves the last sixteenth empty: a breath before the downbeat
            ev.append((s * 0.25, "snare", 60, 0.1, 0.2 + 0.7 * s / 14))
    elif fill == "stop":
        ev.append((2.0, "kick", 60, 0.25, 0.9))
        ev.append((2.0, "snare", 60, 0.3, 0.9))
    for pos, ch, pitch, length, v in ev:
        note(pid, ch, pitch, t + sw(pos), length, v)


def drums(sid, plan):
    """plan: one (style, energy, fill) per bar; style None = silence."""
    pid = section_pattern("Drums", sid, GOLD, T_DRUMS)
    for bar, (style, energy, fill) in enumerate(plan):
        if style is not None:
            drum_bar(pid, bar * 4, style, energy, fill)


def ramp(style, bars, e0, e1, fills=None):
    """`bars` bars of one style with energy rising e0 -> e1; fills by 1-based bar."""
    fills = fills or {}
    return [(style, e0 + (e1 - e0) * i / max(1, bars - 1), fills.get(i + 1)) for i in range(bars)]


# ------------------------------------------------------------------ one-shot FX (reused clips)

for pid, name, ch, length in [("impact", "Impact", "impact", 8), ("subdrop", "Sub Drop", "subdrop", 4),
                              ("fall", "Downlifter", "fall", 4), ("riser", "Riser", "riser", 8),
                              ("ascent", "Long Ascent", "ascent", 16)]:
    pattern(pid, name, ONYX if ch == "subdrop" else CHAMPAGNE, length)
    note(pid, ch, 60, 0, 0.5, 0.85, human=0)


def texture(pid, name, color, ch, pitch, beats, track, *starts, vel=0.6):
    """A generative or drone channel driven by one held note."""
    pattern(pid, name, color, beats)
    note(pid, ch, pitch, 0, beats, vel, human=0)
    for s in starts:
        clip(pid, track, s)


# ------------------------------------------------------------------ the story

# Prologue: the theme alone, stretched, over breath and a veil of pad.
p = section_pattern("Harp", "prologue", SAPPHIRE, T_HARP)
line(p, "harp", THEME, stretch=2.0, vel=0.82)
p = section_pattern("Pad", "prologue", AMETHYST, T_PADS)
hold(p, PROLOGUE, "pad", vel=0.38)
texture("air", "Breath", EMERALD, "air", 60, 32, T_TEX, at("prologue"))

# Journey: the band gathers around the theme.
drums("journey", ramp("brush", 4, 0.2, 0.3) + ramp("brush", 4, 0.4, 0.5, {4: "small"})
      + ramp("groove", 4, 0.35, 0.45, {4: "small"}) + ramp("groove", 4, 0.45, 0.55, {4: "tom"}))
p = section_pattern("Bass", "journey", BURGUNDY, T_BASS)
bass(p, JOURNEY[5:], "walk", vel=0.85, t0=16)  # enters with the second statement of the theme
p = section_pattern("Keys", "journey", ROSE, T_KEYS)
keys(p, JOURNEY, "comp", vel=0.72)
p = section_pattern("Pad", "journey", AMETHYST, T_PADS)
hold(p, JOURNEY, "pad", vel=0.34)
p = section_pattern("Harp", "journey", SAPPHIRE, T_HARP)
line(p, "harp", THEME)
line(p, "harp", THEME_HOPE, t0=16)
line(p, "harp", JOURNEY_C3, t0=32, vel=0.95)
line(p, "harp", JOURNEY_C4, t0=48)

# Rise: the bass climbs Bb C Db Eb; strings swell; a riser into the summit.
drums("rise", ramp("groove", 6, 0.6, 0.72, {4: "small"}) + [("build", 0.8, None), ("build", 1.0, "roll")])
p = section_pattern("Bass", "rise", BURGUNDY, T_BASS)
bass(p, RISE[:6], "walk")
bass(p, RISE[6:], "eighths", t0=24)
p = section_pattern("Keys", "rise", ROSE, T_KEYS)
keys(p, RISE, "pulse", vel=0.72)
p = section_pattern("Strings", "rise", ROSE, T_STRINGS)
line(p, "strings", [(0, 65, 4, .5), (4, 67, 4, .54), (8, 68, 4, .58), (12, 70, 4, .62),
                    (16, 73, 4, .66), (20, 75, 4, .7), (24, 77, 2, .74), (26, 79, 6, .8)])
hold(p, RISE, "strings", octave=0, vel=0.4)
p = section_pattern("Pad", "rise", AMETHYST, T_PADS)
hold(p, RISE, "pad", vel=0.42)
clip("riser", T_FX, at("rise", 6))

# Summit: everything; the theme at its height, then a fall from the peak.
drums("summit", ramp("big", 8, 0.75, 0.85, {4: "small", 8: "tom"})
      + ramp("big", 8, 0.85, 0.95, {4: "small", 8: "stop"}))
p = section_pattern("Bass", "summit", BURGUNDY, T_BASS)
bass(p, SUMMIT, "walk", vel=1.02)
p = section_pattern("Keys", "summit", ROSE, T_KEYS)
keys(p, SUMMIT, "comp")
p = section_pattern("Pad", "summit", AMETHYST, T_PADS)
hold(p, SUMMIT, "pad", vel=0.45)
p = section_pattern("Choir", "summit", AMETHYST, T_STRINGS)
hold(p, SUMMIT_CYCLE, "choir", octave=0, vel=0.42)
hold(p, SUMMIT_CYCLE, "choir", octave=0, vel=0.52, t0=32)
line(p, "strings", SUMMIT_B, t0=32, transpose=-12, vel=0.8)
p = section_pattern("Harp", "summit", SAPPHIRE, T_HARP)
line(p, "harp", SUMMIT_A)
line(p, "harp", SUMMIT_B, t0=32)
# Ab major pentatonic = F minor pentatonic.
texture("pearls", "Pearl Rain", TEAL, "pearls", 80, 32, T_SEQ, at("summit", 8))
clip("impact", T_FX, at("summit"))
clip("fall", T_FX, at("summit", 15))

# Fall: an impact, then a drone and the broken theme.
clip("impact", T_FX, at("fall"))
clip("subdrop", T_FX, at("fall"))
p = section_pattern("Harp", "fall", SAPPHIRE, T_HARP)
line(p, "harp", FALL_LINE)
p = section_pattern("Keys", "fall", ROSE, T_KEYS)
keys(p, FALL, "sparse")
p = section_pattern("Bass", "fall", BURGUNDY, T_BASS)
bass(p, FALL[1:], "sustain", vel=0.42, t0=8)  # enters after the dust settles
p = section_pattern("Bowls", "fall", ONYX, T_PADS)
hold(p, FALL, "bowls", octave=0, vel=0.5)
texture("embers", "Embers", EMBER, "embers", 53, 48, T_TEX, at("fall"), vel=0.55)

# Struggle: half-time grind, a dark sequence, the theme trying to climb.
drums("struggle", ramp("halftime", 4, 0.45, 0.55) + ramp("halftime", 4, 0.65, 0.75, {4: "small"})
      + [("build", 0.5, None), ("build", 0.7, None), ("build", 0.9, None), ("build", 1.0, "roll")])
p = section_pattern("Bass", "struggle", BURGUNDY, T_BASS)
bass(p, GRIND + GRIND, "pulse", vel=0.82)
bass(p, CLIMB, "climb", t0=32)
p = section_pattern("Keys", "struggle", ROSE, T_KEYS)
keys(p, STRUGGLE, "sparse", vel=1.15)
p = section_pattern("Harp", "struggle", SAPPHIRE, T_HARP)
line(p, "harp", STRUGGLE_LINE)
p = section_pattern("Strings", "struggle", ROSE, T_STRINGS)
hold(p, CLIMB, "strings", octave=0, vel=0.5, top=False, t0=32)
texture("crypt", "Crypt Sequence", ONYX, "crypt", 53, 32, T_SEQ, at("struggle"), vel=0.62)
clip("ascent", T_FX, at("struggle", 8))
clip("riser", T_FX, at("struggle", 10))

# Redemption: still F minor, but whole. The question is finally answered.
clip("impact", T_FX, at("redemption"))
drums("redemption", ramp("big", 8, 0.8, 0.88, {4: "small", 8: "tom"})
      + ramp("big", 8, 0.88, 0.95, {4: "small", 8: "tom"}))
p = section_pattern("Bass", "redemption", BURGUNDY, T_BASS)
bass(p, REDEMPTION, "walk", vel=1.02)
p = section_pattern("Keys", "redemption", ROSE, T_KEYS)
keys(p, REDEMPTION, "comp")
p = section_pattern("Dawn", "redemption", AMETHYST, T_PADS)
hold(p, REDEMPTION, "dawn", vel=0.5)
p = section_pattern("Choir", "redemption", AMETHYST, T_STRINGS)
hold(p, REDEMPTION, "choir", octave=0, vel=0.5)
line(p, "strings", [(0, 77, 3, .8)])  # the arrival, on F
line(p, "strings", THEME_ANSWER, t0=16, vel=0.75)
line(p, "strings", REDEMPTION_C3, t0=32, transpose=-12, vel=0.75)
line(p, "strings", REDEMPTION_C4, t0=48, transpose=-12, vel=0.75)
p = section_pattern("Harp", "redemption", SAPPHIRE, T_HARP)
line(p, "harp", THEME_ANSWER)
line(p, "harp", THEME_ANSWER, t0=16, transpose=12, vel=0.9)
line(p, "harp", REDEMPTION_C3, t0=32)
line(p, "harp", REDEMPTION_C4, t0=48)
# Ab major scale = F natural minor.
texture("sparkle", "Diamond Sparkle", PEARL, "sparkle", 80, 44, T_SEQ, at("redemption", 4), vel=0.55)
texture("bell", "Cathedral Bell", CHAMPAGNE, "bell", 65, 4, T_SEQ, at("redemption"), at("redemption", 8))
patterns["bell"]["notes"][0]["length"] = 2

# Epilogue: the theme alone again; one bar of mischief; F minor to close.
drums("epilogue", ramp("brush", 4, 0.3, 0.1) + [(None, 0, None)] * 4)
p = section_pattern("Harp", "epilogue", SAPPHIRE, T_HARP)
line(p, "harp", EPILOGUE_LINE)
p = section_pattern("Pad", "epilogue", AMETHYST, T_PADS)
hold(p, EPILOGUE[:3], "pad", vel=0.4)
hold(p, EPILOGUE[4:], "pad", vel=0.4, t0=24)
p = section_pattern("Keys", "epilogue", ROSE, T_KEYS)
keys(p, EPILOGUE[:3], "sparse", vel=0.9)
for s, v in ((0, .5), (1.5, .42), (2.5, .46)):  # the joke, played straight and staccato
    strum(p, CHORDS["E"][0], 20 + s, 0.22, v, spread=0.004)
keys(p, EPILOGUE[4:], "sparse", vel=0.9, t0=24)
p = section_pattern("Bass", "epilogue", BURGUNDY, T_BASS)
bass(p, EPILOGUE, "sustain", vel=0.38)
texture("stardust", "Stardust", PEARL, "stardust", 77, 20, T_TEX, at("epilogue"), vel=0.5)
clip("air", T_TEX, at("epilogue"))
clip("bell", T_SEQ, at("epilogue", 6))

# ------------------------------------------------------------------ mixer


def fx(kind, params=None, options=None):
    return {"type": kind, "params": params or {}, "options": options or {}}


def ins(name, volume=1.0, effects=None):
    return {"name": name, "volume": volume, "pan": 0, "mute": False, "solo": False, "effects": effects or []}


MIXER = [
    ins("Master", 0.92, [
        fx("eq", {"low": 0.5, "lowFreq": 70, "high": -1.5, "highFreq": 9000}),
        fx("compressor", {"threshold": -12, "ratio": 1.6, "attack": 30, "release": 300, "makeup": 1}),
        fx("limiter", {"gain": 1, "ceiling": -0.8}),
    ]),
    ins("Drums", 0.9, [
        fx("compressor", {"threshold": -18, "ratio": 3, "attack": 12, "release": 120, "makeup": 2.5}),
        fx("eq", {"high": -2.5, "highFreq": 7000, "mid": 1, "midFreq": 180}),
        fx("reverb", {"size": 0.35, "damping": 0.6, "mix": 0.08}),
    ]),
    ins("Hats", 0.62, [
        fx("eq", {"low": -12, "lowFreq": 500, "high": -4, "highFreq": 8000}),
        fx("reverb", {"size": 0.4, "damping": 0.6, "mix": 0.1}),
    ]),
    ins("Bass", 0.85, [
        fx("eq", {"low": 1.5, "lowFreq": 80, "mid": -2.5, "midFreq": 300, "midQ": 1.2}),
        fx("drive", {"amount": 0.12, "tone": 2500, "mix": 0.4, "output": 0.95}),
    ]),
    ins("Keys", 0.8, [
        fx("chorus", {"rate": 0.35, "depth": 0.45, "mix": 0.35}),
        fx("delay", {"time": 0.75, "feedback": 0.25, "tone": 3000, "mix": 0.12}, {"mode": "pingpong"}),
        fx("reverb", {"size": 0.7, "damping": 0.5, "mix": 0.22}),
    ]),
    ins("Pads", 0.75, [
        fx("eq", {"low": -8, "lowFreq": 250}),
        fx("reverb", {"size": 0.9, "damping": 0.45, "mix": 0.35}),
    ]),
    ins("Harp", 0.8, [
        fx("delay", {"time": 0.5, "feedback": 0.3, "tone": 3500, "mix": 0.2}, {"mode": "pingpong"}),
        fx("reverb", {"size": 0.8, "damping": 0.45, "mix": 0.3}),
    ]),
    ins("Textures", 0.7, [
        fx("filter", {"cutoff": 7000, "resonance": 0.1}, {"mode": "lowpass"}),
        fx("reverb", {"size": 0.9, "damping": 0.4, "mix": 0.4}),
    ]),
    ins("FX", 0.7, [fx("reverb", {"size": 0.8, "damping": 0.5, "mix": 0.25})]),
    ins("Strings", 0.75, [
        fx("eq", {"low": -6, "lowFreq": 200, "high": -2, "highFreq": 8000}),
        fx("reverb", {"size": 0.85, "damping": 0.5, "mix": 0.3}),
    ]),
    ins("Bells & Sequences", 0.7, [
        fx("eq", {"low": -6, "lowFreq": 300, "high": -3, "highFreq": 9000}),
        fx("delay", {"time": 0.75, "feedback": 0.3, "tone": 4000, "mix": 0.15}, {"mode": "pingpong"}),
        fx("reverb", {"size": 0.9, "damping": 0.45, "mix": 0.35}),
    ]),
]

project = {
    "$schema": "./project.schema.json",
    "format": "rosaclef/1",
    "meta": {"title": "Velvet Hour", "author": "Rosaclef",
             "description": "A four-minute journey in F minor: a harp theme sets out, climbs, falls, struggles "
                            "and returns whole. A demo of the Rosaclef studio and its factory presets."},
    "transport": {"bpm": BPM, "beatsPerBar": 4, "swing": 0},
    "channels": [
        {"id": i, "name": n, "color": c, "instrument": dev, "volume": vol, "pan": pan, "mute": False, "mixer": m}
        for (i, n, c, dev, m, vol, pan) in CHANNELS
    ],
    "patterns": [
        {"id": pid, "name": patterns[pid]["name"], "color": patterns[pid]["color"], "length": patterns[pid]["length"],
         "notes": sorted(patterns[pid]["notes"], key=lambda n: (n["start"], n["channel"], n["pitch"]))}
        for pid in order
    ],
    "playlist": {
        "tracks": [{"name": t, "mute": False} for t in TRACKS],
        "clips": [{"pattern": p, "track": tr, "start": s, "length": l}
                  for (p, tr, s, l) in sorted(clips, key=lambda c: (c[2], c[1]))],
    },
    "mixer": {"inserts": MIXER},
}
json.dump(project, sys.stdout, indent=2, ensure_ascii=False)
print()
