#!/usr/bin/env python3
"""Generate the bundled demo song "Velvet Hour".

Neo-soul / downtempo in F minor at 92 BPM: humanized, drummer-style groove
(ghost notes, dynamic hats, fills), extended-chord harmony, and a selection
of the factory presets of the built-in engines.

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

# ------------------------------------------------------------------ channels
# (id, name, color, instrument, mixer insert, volume, pan)
CHANNELS = [
    ("kick", "Kick", GOLD, drum("kick", tone=0.3, snap=0.45, decay=1.0, tune=-2, gain=0.85), 1, 0.9, 0),
    ("snare", "Snare", ROSE, drum("snare", tone=0.35, snap=0.4, decay=1.05, tune=-1, gain=0.8), 1, 0.72, 0.02),
    ("clap", "Clap", ROSE, drum("clap", tone=0.35, decay=1.1, gain=0.6), 1, 0.42, -0.04),
    ("rim", "Rim", BRONZE, drum("rim", tone=0.45, tune=1, gain=0.6), 1, 0.36, 0.22),
    ("tom", "Toms", BRONZE, drum("tom", tone=0.35, decay=0.9, snap=0.35, gain=0.75), 1, 0.55, -0.12),
    ("hat", "Hi-hat", CHAMPAGNE, drum("hat", tone=0.28, decay=0.8, snap=0.3, gain=0.6), 2, 0.42, 0.18),
    ("openhat", "Open Hat", CHAMPAGNE, drum("openhat", tone=0.25, decay=0.6, gain=0.55), 2, 0.3, 0.2),
    ("shaker", "Shaker", BRONZE, drum("shaker", tone=0.35, decay=0.9, gain=0.6), 2, 0.22, -0.3),
    ("bass", "Bronze Bass", BURGUNDY, preset("Bronze Souverain"), 3, 0.62, 0),
    ("keys", "Opaline Keys", GOLD, preset("Opaline Keys"), 4, 0.62, -0.05),
    ("pad", "Opaline Veil", AMETHYST, preset("Opaline Veil"), 5, 0.4, 0),
    ("choir", "Voile de Chœur", AMETHYST, preset("Voile de Chœur"), 5, 0.3, 0.1),
    ("harp", "Harpe de Saphir", SAPPHIRE, preset("Harpe de Saphir"), 6, 0.46, 0.08),
    ("pearls", "Pluie de Perles", TEAL, preset("Pluie de Perles"), 7, 0.24, 0),
    ("air", "Souffle d'Opale", EMERALD, preset("Souffle d'Opale"), 7, 0.36, 0),
    ("embers", "Cendres d'Ambre", EMBER, preset("Cendres d'Ambre"), 7, 0.3, 0),
    ("ascent", "Voie Lactée", CHAMPAGNE, preset("Voie Lactée"), 8, 0.38, 0),
    ("riser", "Ascension d'Or", CHAMPAGNE, preset("Ascension d'Or"), 8, 0.34, 0),
    ("impact", "Météore", BRONZE, preset("Météore de Basalte"), 8, 0.5, 0),
    ("fall", "Déclin de Lune", CHAMPAGNE, preset("Déclin de Lune"), 8, 0.34, 0),
]

# ------------------------------------------------------------------ helpers

notes_of = {}


def note(pattern, ch, pitch, start, length, vel, human=0.006):
    """Add a note with human timing (never before 0) and velocity jitter."""
    t = max(0.0, start + rng.uniform(-human, human))
    v = min(1.0, max(0.05, vel + rng.uniform(-0.04, 0.04)))
    notes_of.setdefault(pattern, []).append(
        {"channel": ch, "pitch": pitch, "start": round(t, 4), "length": round(length, 4), "velocity": round(v, 3)})


SWING = 0.035  # beats: late 16th off-beats, a drummer's lazy feel


def sw(pos):
    """Delay the e / a sixteenths (x.25, x.75)."""
    frac = pos % 1
    return pos + SWING if abs(frac - 0.25) < 1e-6 or abs(frac - 0.75) < 1e-6 else pos


# ------------------------------------------------------------------ drums


def hats(pat, bar, busy, open_on=(), skip=()):
    """Closed hats: accented 8ths, soft sixteenths that come and go."""
    for beat in range(4):
        for sub, base in ((0.0, 0.5), (0.25, 0.2), (0.5, 0.36), (0.75, 0.24)):
            pos = bar * 4 + beat + sub
            local = beat + sub
            if local in skip:
                continue
            if local in open_on:
                note(pat, "openhat", 60, sw(pos), 0.4, 0.46)
                continue
            if sub in (0.25, 0.75) and rng.random() > busy:
                continue
            accent = 0.08 if beat % 2 == 0 and sub == 0 else 0.0
            note(pat, "hat", 60, sw(pos), 0.1, base + accent)


def groove_verse():
    """Four bars: syncopated kick, backbeat with ghost notes, dynamic hats,
    and a snare pickup in the last bar."""
    p = "groove-verse"
    kicks = [[(0, .92), (1.75, .55), (2.5, .84)],
             [(0, .9), (0.75, .48), (2.5, .8), (3.25, .58)],
             [(0, .92), (1.75, .55), (2.5, .84)],
             [(0, .9), (2.25, .6), (2.5, .8)]]
    ghosts = [[0.75, 2.25, 3.75], [1.75, 2.75, 3.5], [0.5, 2.25, 3.25], [0.75, 1.5]]
    for bar in range(4):
        for pos, v in kicks[bar]:
            note(p, "kick", 60, sw(bar * 4 + pos), 0.25, v)
        for pos, v in ((1, .86), (3, .9)):
            note(p, "snare", 60, bar * 4 + pos, 0.25, v)
        for pos in ghosts[bar]:
            note(p, "snare", 60, sw(bar * 4 + pos), 0.1, rng.uniform(0.12, 0.24))
        hats(p, bar, busy=0.55, open_on=(3.5,) if bar in (1, 3) else ())
    # Pickup into the next phrase: a soft snare crescendo.
    for pos, v in ((3.25, .3), (3.5, .42), (3.75, .55)):
        note(p, "snare", 60, sw(12 + pos), 0.1, v)
    note(p, "rim", 60, sw(6.5), 0.1, 0.4)


def groove_chorus():
    """Four bars with more push: extra kicks, clap layered on 4, open hats,
    a soft shaker bed, fewer ghosts."""
    p = "groove-chorus"
    kicks = [[(0, .95), (1.5, .6), (2.5, .86), (2.75, .5)],
             [(0, .93), (1.75, .58), (2.5, .85)],
             [(0, .95), (1.5, .6), (2.5, .86), (3.5, .55)],
             [(0, .93), (0.75, .5), (2.5, .85), (3.25, .6)]]
    for bar in range(4):
        for pos, v in kicks[bar]:
            note(p, "kick", 60, sw(bar * 4 + pos), 0.25, v)
        note(p, "snare", 60, bar * 4 + 1, 0.25, 0.9)
        note(p, "snare", 60, bar * 4 + 3, 0.25, 0.92)
        note(p, "clap", 60, bar * 4 + 3 + 0.01, 0.25, 0.62)
        for pos in (2.25, 3.75) if bar % 2 == 0 else (1.75,):
            note(p, "snare", 60, sw(bar * 4 + pos), 0.1, rng.uniform(0.14, 0.22))
        hats(p, bar, busy=0.8, open_on=(1.5, 3.5) if bar % 2 == 1 else (3.5,))
        for s in range(16):
            if s % 4 in (1, 3) or rng.random() < 0.25:
                note(p, "shaker", 60, sw(bar * 4 + s * 0.25), 0.1, 0.16 + (0.1 if s % 2 == 1 else 0))


def fill():
    """One-bar tom run with snare flams, into the chorus."""
    p = "fill"
    toms = [(0.0, 67, .7), (0.25, 67, .5), (0.5, 64, .72), (0.75, 64, .5),
            (1.0, 60, .75), (1.25, 60, .55), (1.5, 55, .8), (1.75, 55, .6)]
    for pos, pitch, v in toms:
        note(p, "tom", pitch, sw(pos), 0.25, v)
    note(p, "kick", 60, 0, 0.25, 0.9)
    note(p, "kick", 60, 2, 0.25, 0.85)
    for i, pos in enumerate((2.0, 2.25, 2.5, 2.75, 3.0, 3.25, 3.5, 3.75)):
        note(p, "snare", 60, sw(pos), 0.1, 0.35 + i * 0.075)
    note(p, "openhat", 60, 3.5, 0.5, 0.35)


def intro_perc():
    """Brushes-like: rim, soft hats and shaker, no kick."""
    p = "intro-perc"
    for bar in range(4):
        hats(p, bar, busy=0.35, skip=(0.0,) if bar == 0 else ())
        note(p, "rim", 60, bar * 4 + 1, 0.1, 0.42)
        note(p, "rim", 60, sw(bar * 4 + 3.25), 0.1, 0.3)
        for s in (1, 3, 5, 7, 9, 11, 13, 15):
            note(p, "shaker", 60, sw(bar * 4 + s * 0.25), 0.1, 0.14)


def outro_perc():
    p = "outro-perc"
    for bar in range(4):
        note(p, "kick", 60, bar * 4, 0.25, 0.62 - bar * 0.08)
        note(p, "rim", 60, bar * 4 + 1, 0.1, 0.4)
        note(p, "rim", 60, bar * 4 + 3, 0.1, 0.36)
        hats(p, bar, busy=0.3)


# ------------------------------------------------------------------ harmony

# (voicing for keys, bass root, fifth-ish tone) per half bar.
VERSE = [  # Dbmaj9 | Cm9 | Fm9 | Bbm9  Eb13
    ([53, 56, 60, 63], 37, 44, 4),
    ([51, 55, 58, 62], 36, 43, 4),
    ([56, 60, 63, 67], 41, 48, 4),
    ([56, 60, 61, 65], 34, 41, 2),
    ([55, 60, 61, 65], 39, 46, 2),
]
CHORUS = [  # Bbm9 | Eb9sus | Abmaj9 | Dbmaj9  C7(b9)
    ([56, 60, 61, 65], 34, 41, 4),
    ([56, 58, 61, 65], 39, 46, 4),
    ([55, 58, 60, 63], 32, 39, 4),
    ([53, 56, 60, 63], 37, 44, 2),
    ([52, 58, 61, 67], 36, 43, 2),
]

COMP = [  # (start, length, velocity) patterns; negative = anticipation
    [(0, 1.4, .62), (1.75, .6, .48), (2.5, 1.2, .58)],
    [(0, 2.6, .6), (3.0, .7, .44)],
    [(0.5, 1.0, .55), (2.0, .5, .5), (2.75, 1.0, .56)],
    [(0, 1.0, .6), (1.5, .45, .45), (3.25, .7, .52)],
]


def comp(pattern, prog):
    """Electric piano comping: varied rhythms, strummed voicings."""
    t = 0.0
    for i, (voicing, _root, _fifth, dur) in enumerate(prog):
        rhythm = COMP[i % len(COMP)] if dur == 4 else [(0, 1.3, .58), (1.25, .6, .46)]
        for start, length, vel in rhythm:
            if start >= dur:
                continue
            for k, pitch in enumerate(voicing):
                note(pattern, "keys", pitch, sw(t + start) + k * 0.012, min(length, dur - start), vel - k * 0.03)
        t += dur


def bassline(pattern, prog):
    """Melodic, syncopated bass with octave ghosts and chromatic approaches."""
    t = 0.0
    for i, (_v, root, fifth, dur) in enumerate(prog):
        nxt = prog[(i + 1) % len(prog)][1]
        approach = nxt - 1 if nxt > root else nxt + 1
        if dur == 4:
            figure = [(0, .7, root, .9), (0.75, .2, root + 12, .42), (1.5, .45, root, .72),
                      (2.5, .45, fifth, .74), (3.25, .2, root + 12, .46), (3.75, .25, approach, .62)]
        else:
            figure = [(0, .6, root, .88), (1.0, .4, fifth, .68), (1.75, .25, approach, .6)]
        for start, length, pitch, vel in figure:
            note(pattern, "bass", pitch, sw(t + start), length, vel)
        t += dur


def pad(pattern, prog, ch="pad", octave=12, vel=0.55):
    t = 0.0
    for voicing, _r, _f, dur in prog:
        for pitch in voicing[1:]:
            note(pattern, ch, pitch + octave, t, dur, vel, human=0.0)
        t += dur


def melody():
    """Chorus melody for the harp pluck (over Bbm9 | Eb9sus | Abmaj9 | Db C7b9)."""
    p = "melody"
    line = [
        (0, 72, .5, .7), (0.5, 73, .5, .55), (1.0, 77, 1.0, .72), (2.5, 75, .5, .58), (3.0, 73, .75, .62),
        (4.5, 72, .5, .6), (5.0, 70, 1.5, .66), (7.0, 68, .5, .5), (7.5, 70, .5, .55),
        (8.0, 72, 1.5, .7), (9.5, 75, .5, .56), (10.0, 79, 1.0, .72), (11.0, 77, .5, .6), (11.5, 75, .5, .55),
        (12.0, 77, 1.0, .68), (13.0, 75, .5, .55), (13.5, 73, .5, .52), (14.0, 76, 1.0, .64), (15.0, 73, 1.0, .58),
    ]
    for start, pitch, length, vel in line:
        note(p, "harp", pitch, sw(start), length, vel)


groove_verse()
groove_chorus()
fill()
intro_perc()
outro_perc()
comp("keys-verse", VERSE)
comp("keys-chorus", CHORUS)
bassline("bass-verse", VERSE)
bassline("bass-chorus", CHORUS)
pad("pad-verse", VERSE)
pad("pad-chorus", CHORUS)
pad("choir-chorus", CHORUS, ch="choir", octave=0, vel=0.5)
melody()
# One held note drives each generative / FX part.
note("pearls", "pearls", 77, 0, 16, 0.6, human=0)
note("air", "air", 60, 0, 32, 0.6, human=0)
note("embers", "embers", 53, 0, 16, 0.55, human=0)
note("ascent", "ascent", 60, 0, 0.5, 0.8, human=0)
note("riser", "riser", 60, 0, 0.5, 0.8, human=0)
note("impact", "impact", 60, 0, 0.5, 0.9, human=0)
note("fall", "fall", 60, 0, 0.5, 0.8, human=0)

PATTERNS = [
    ("groove-verse", "Verse Groove", GOLD, 16), ("groove-chorus", "Chorus Groove", GOLD, 16),
    ("fill", "Tom Fill", BRONZE, 4), ("intro-perc", "Intro Brushes", BRONZE, 16), ("outro-perc", "Outro Brushes", BRONZE, 16),
    ("keys-verse", "Keys · Verse", ROSE, 16), ("keys-chorus", "Keys · Chorus", ROSE, 16),
    ("bass-verse", "Bass · Verse", BURGUNDY, 16), ("bass-chorus", "Bass · Chorus", BURGUNDY, 16),
    ("pad-verse", "Pad · Verse", AMETHYST, 16), ("pad-chorus", "Pad · Chorus", AMETHYST, 16),
    ("choir-chorus", "Choir · Chorus", AMETHYST, 16), ("melody", "Harp Melody", SAPPHIRE, 16),
    ("pearls", "Pearl Rain", TEAL, 16), ("air", "Breath", EMERALD, 32), ("embers", "Embers", EMBER, 16),
    ("ascent", "Long Ascent", CHAMPAGNE, 16), ("riser", "Riser", CHAMPAGNE, 8),
    ("impact", "Impact", BRONZE, 8), ("fall", "Downlifter", CHAMPAGNE, 4),
]

# ------------------------------------------------------------------ arrangement
# Bars: intro 1-8 (0-32), verse 9-24 (32-96), chorus 25-36 (96-144), outro 37-40 (144-160).
TRACKS = ["Drums", "Fills", "Bass", "Keys", "Pads", "Melody", "Textures", "FX"]
CLIPS = [
    ("intro-perc", 0, 16, 16), ("groove-verse", 0, 32, 60), ("fill", 1, 92, 4),
    ("groove-chorus", 0, 96, 48), ("outro-perc", 0, 144, 16),
    ("bass-verse", 2, 32, 64), ("bass-chorus", 2, 96, 48),
    ("keys-verse", 3, 0, 96), ("keys-chorus", 3, 96, 48), ("keys-verse", 3, 144, 16),
    ("pad-verse", 4, 0, 96), ("pad-chorus", 4, 96, 48), ("pad-verse", 4, 144, 16),
    ("choir-chorus", 4, 112, 32),
    ("melody", 5, 96, 48),
    ("air", 6, 0, 32), ("pearls", 6, 112, 32), ("embers", 6, 144, 16),
    ("ascent", 7, 16, 16), ("impact", 7, 32, 8), ("riser", 7, 88, 8), ("impact", 7, 96, 8), ("fall", 7, 144, 4),
]


def fx(kind, params=None, options=None):
    return {"type": kind, "params": params or {}, "options": options or {}}


def ins(name, volume=1.0, effects=None):
    return {"name": name, "volume": volume, "pan": 0, "mute": False, "solo": False, "effects": effects or []}


MIXER = [
    ins("Master", 0.92, [
        fx("eq", {"low": 0.5, "lowFreq": 70, "high": -1.5, "highFreq": 9000}),
        fx("compressor", {"threshold": -16, "ratio": 2, "attack": 25, "release": 250, "makeup": 2}),
        fx("limiter", {"gain": 2, "ceiling": -0.8}),
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
    ins("Melody", 0.78, [
        fx("delay", {"time": 0.5, "feedback": 0.3, "tone": 3500, "mix": 0.2}, {"mode": "pingpong"}),
        fx("reverb", {"size": 0.8, "damping": 0.45, "mix": 0.3}),
    ]),
    ins("Textures", 0.7, [
        fx("filter", {"cutoff": 7000, "resonance": 0.1}, {"mode": "lowpass"}),
        fx("reverb", {"size": 0.9, "damping": 0.4, "mix": 0.4}),
    ]),
    ins("FX", 0.7, [fx("reverb", {"size": 0.8, "damping": 0.5, "mix": 0.25})]),
    ins("Insert 9"), ins("Insert 10"),
]

project = {
    "$schema": "./project.schema.json",
    "format": "rosaclef/1",
    "meta": {"title": "Velvet Hour", "author": "Rosaclef",
             "description": "Neo-soul in F minor — a demo of the Rosaclef studio and its factory presets."},
    "transport": {"bpm": BPM, "beatsPerBar": 4, "swing": 0},
    "channels": [
        {"id": i, "name": n, "color": c, "instrument": dev, "volume": vol, "pan": pan, "mute": False, "mixer": m}
        for (i, n, c, dev, m, vol, pan) in CHANNELS
    ],
    "patterns": [
        {"id": pid, "name": name, "color": color, "length": length,
         "notes": sorted(notes_of.get(pid, []), key=lambda n: (n["start"], n["channel"], n["pitch"]))}
        for (pid, name, color, length) in PATTERNS
    ],
    "playlist": {
        "tracks": [{"name": t, "mute": False} for t in TRACKS],
        "clips": [{"pattern": p, "track": tr, "start": s, "length": l} for (p, tr, s, l) in CLIPS],
    },
    "mixer": {"inserts": MIXER},
}
json.dump(project, sys.stdout, indent=2, ensure_ascii=False)
print()
