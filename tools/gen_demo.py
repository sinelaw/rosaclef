#!/usr/bin/env python3
"""Generate the bundled demo song "Velvet Hour" (deep house, F minor, 118 BPM).

Usage: python3 tools/gen_demo.py > crates/server/assets/demo/project.json
(then `rosaclef fmt` tidies the formatting).
"""
import json

GOLD, ROSE, CHAMPAGNE, BURGUNDY = "#d4af37", "#c97b84", "#e8d5b0", "#8e3b46"
EMERALD, SAPPHIRE, AMETHYST, BRONZE = "#3f8f7a", "#4a6fa5", "#8a6bb0", "#b08d57"


def ch(id, name, color, kind, mixer, params=None, options=None, volume=0.8, pan=0.0):
    return {
        "id": id, "name": name, "color": color,
        "instrument": {"type": kind, "params": params or {}, "options": options or {}},
        "volume": volume, "pan": pan, "mute": False, "mixer": mixer,
    }


channels = [
    ch("kick", "Kick", GOLD, "drum", 1, {"tone": 0.35, "snap": 0.55, "decay": 1.1, "tune": -1}, {"kind": "kick"}, 0.9),
    ch("clap", "Clap", ROSE, "drum", 1, {"tone": 0.45, "decay": 1.2}, {"kind": "clap"}, 0.62),
    ch("rim", "Rim", BRONZE, "drum", 1, {"tone": 0.6, "tune": 2}, {"kind": "rim"}, 0.35, 0.25),
    ch("hat", "Closed Hat", CHAMPAGNE, "drum", 2, {"tone": 0.62, "decay": 0.9}, {"kind": "hat"}, 0.5, -0.15),
    ch("openhat", "Open Hat", CHAMPAGNE, "drum", 2, {"tone": 0.55, "decay": 0.55}, {"kind": "openhat"}, 0.38, 0.15),
    ch("shaker", "Shaker", BRONZE, "drum", 2, {"tone": 0.5, "decay": 1.0}, {"kind": "shaker"}, 0.3, 0.3),
    ch("bass", "Velvet Bass", BURGUNDY, "synth", 3, {
        "osc2Semi": -12, "osc2Mix": 0.35, "osc2Detune": 3, "sub": 0.55, "cutoff": 340, "resonance": 0.35,
        "filterEnv": 0.42, "filterDecay": 0.16, "attack": 0.003, "decay": 0.25, "sustain": 0.55,
        "release": 0.08, "gain": 0.8,
    }, {"wave1": "saw", "wave2": "square", "filter": "lowpass"}, 0.85),
    ch("keys", "Rhodes Lumière", GOLD, "fm", 4, {
        "ratio": 1, "index": 2.2, "indexDecay": 0.9, "velocity": 0.7, "detune": 6, "attack": 0.002,
        "decay": 1.6, "sustain": 0.3, "release": 0.45, "gain": 0.6,
    }, {}, 0.7),
    ch("pad", "Silk Pad", AMETHYST, "synth", 5, {
        "osc2Detune": 9, "osc2Mix": 0.6, "unison": 5, "spread": 22, "cutoff": 1300, "resonance": 0.12,
        "filterEnv": 0.12, "filterDecay": 1.5, "attack": 1.4, "decay": 1.5, "sustain": 0.85, "release": 2.4,
        "gain": 0.32,
    }, {"wave1": "saw", "wave2": "saw", "filter": "lowpass"}, 0.6),
    ch("arp", "Crystal Arp", SAPPHIRE, "fm", 6, {
        "ratio": 3.5, "index": 2.4, "indexDecay": 0.3, "velocity": 0.5, "detune": 3, "attack": 0.001,
        "decay": 0.45, "sustain": 0.0, "release": 0.35, "gain": 0.45,
    }, {}, 0.5, 0.1),
]

# F minor progression: Fm9 | Dbmaj9 | Eb6/9 | Cm9
KEYS = [[53, 56, 60, 63, 67], [49, 53, 56, 60, 63], [51, 55, 58, 60, 65], [48, 51, 55, 58, 62]]
PAD = [[56, 60, 63, 67], [56, 60, 65, 68], [55, 58, 63, 67], [55, 58, 62, 67]]
ROOTS = [41, 37, 39, 36]
ARP = [[72, 75, 79, 80], [72, 77, 80, 84], [70, 74, 79, 82], [70, 74, 75, 79]]


def note(c, pitch, start, length, vel):
    return {"channel": c, "pitch": pitch, "start": start, "length": length, "velocity": round(vel, 3)}


# --- Drums (1 bar, loops) ----------------------------------------------------
groove, intro = [], []
for b in range(4):
    groove.append(note("kick", 60, b, 0.25, 0.95))
    intro.append(note("kick", 60, b, 0.25, 0.9))
    groove.append(note("hat", 60, b, 0.1, 0.42))
    groove.append(note("hat", 60, b + 0.25, 0.1, 0.28))
    groove.append(note("openhat", 60, b + 0.5, 0.25, 0.62))
    groove.append(note("hat", 60, b + 0.75, 0.1, 0.34))
    intro.append(note("hat", 60, b + 0.5, 0.1, 0.55))
    groove.append(note("shaker", 60, b + 0.25, 0.1, 0.3))
    groove.append(note("shaker", 60, b + 0.75, 0.1, 0.38))
for b in (1, 3):
    groove.append(note("clap", 60, b, 0.25, 0.85))
groove.append(note("rim", 60, 2.75, 0.1, 0.5))
groove.append(note("rim", 60, 3.25, 0.1, 0.38))

# --- Bass (4 bars) -------------------------------------------------------------
bass = []
for bar, r in enumerate(ROOTS):
    o = bar * 4
    for (t, length, dp, v) in [(0.5, 0.42, 0, 0.9), (1.5, 0.42, 0, 0.8), (1.75, 0.2, 12, 0.6),
                               (2.5, 0.42, 0, 0.85), (3.25, 0.2, 7, 0.55), (3.5, 0.45, 0, 0.8)]:
        bass.append(note("bass", r + dp, o + t, length, v))

# --- Keys stabs (4 bars) ---------------------------------------------------------
keys = []
for bar, chord in enumerate(KEYS):
    o = bar * 4
    for (t, length, v) in [(0, 0.55, 0.78), (0.75, 0.45, 0.58), (2.5, 1.0, 0.72), (3.5, 0.35, 0.5)]:
        for i, p in enumerate(chord):
            keys.append(note("keys", p, o + t, length, v - i * 0.02))

# --- Pad (4 bars) --------------------------------------------------------------------
pad = []
for bar, chord in enumerate(PAD):
    for p in chord:
        pad.append(note("pad", p, bar * 4, 4, 0.7))

# --- Arp (4 bars of 8ths) -----------------------------------------------------------
arp = []
order = [0, 1, 2, 3, 2, 1, 3, 2]
for bar, chord in enumerate(ARP):
    for i, k in enumerate(order):
        arp.append(note("arp", chord[k], bar * 4 + i * 0.5, 0.4, 0.75 if i % 2 == 0 else 0.55))

patterns = [
    {"id": "intro-beat", "name": "Intro Beat", "color": BRONZE, "length": 4, "notes": intro},
    {"id": "groove", "name": "Groove", "color": GOLD, "length": 4, "notes": groove},
    {"id": "bassline", "name": "Bassline", "color": BURGUNDY, "length": 16, "notes": bass},
    {"id": "keys", "name": "Keys", "color": ROSE, "length": 16, "notes": keys},
    {"id": "pad", "name": "Silk Pad", "color": AMETHYST, "length": 16, "notes": pad},
    {"id": "arp", "name": "Crystal Arp", "color": SAPPHIRE, "length": 16, "notes": arp},
]

tracks = [{"name": n, "mute": False} for n in ["Drums", "Bass", "Keys", "Pad", "Arp", "Vocals", "FX", "Track 8"]]
clips = [
    {"pattern": "intro-beat", "track": 0, "start": 0, "length": 32},
    {"pattern": "groove", "track": 0, "start": 32, "length": 96},
    {"pattern": "bassline", "track": 1, "start": 32, "length": 96},
    {"pattern": "keys", "track": 2, "start": 16, "length": 112},
    {"pattern": "pad", "track": 3, "start": 0, "length": 128},
    {"pattern": "arp", "track": 4, "start": 64, "length": 64},
]


def fx(kind, params=None, options=None):
    return {"type": kind, "params": params or {}, "options": options or {}}


def ins(name, volume=1.0, pan=0.0, effects=None):
    return {"name": name, "volume": volume, "pan": pan, "mute": False, "solo": False, "effects": effects or []}


mixer = {"inserts": [
    ins("Master", 0.9, 0, [
        fx("eq", {"low": 1.0, "lowFreq": 90, "high": 1.5, "highFreq": 9000}),
        fx("compressor", {"threshold": -14, "ratio": 2, "attack": 20, "release": 200, "makeup": 2}),
        fx("limiter", {"gain": 3, "ceiling": -0.5}),
    ]),
    ins("Drums", 0.9, 0, [fx("compressor", {"threshold": -16, "ratio": 3, "attack": 8, "release": 90, "makeup": 3})]),
    ins("Hats", 0.8, 0, [fx("eq", {"low": -12, "lowFreq": 400, "high": 2}), fx("reverb", {"size": 0.4, "mix": 0.12})]),
    ins("Bass", 0.85, 0, [fx("drive", {"amount": 0.2, "tone": 3000, "mix": 0.6, "output": 0.9}), fx("eq", {"low": 2, "lowFreq": 70, "mid": -3, "midFreq": 300})]),
    ins("Keys", 0.8, 0, [
        fx("chorus", {"rate": 0.4, "depth": 0.5, "mix": 0.45}),
        fx("delay", {"time": 0.75, "feedback": 0.32, "tone": 3500, "mix": 0.18}, {"mode": "pingpong"}),
        fx("reverb", {"size": 0.75, "damping": 0.45, "mix": 0.25}),
    ]),
    ins("Pad", 0.75, 0, [fx("chorus", {"rate": 0.25, "depth": 0.7, "mix": 0.5}), fx("reverb", {"size": 0.9, "damping": 0.35, "mix": 0.4})]),
    ins("Arp", 0.7, 0, [
        fx("filter", {"cutoff": 6500, "resonance": 0.1}, {"mode": "lowpass"}),
        fx("delay", {"time": 0.75, "feedback": 0.45, "tone": 4000, "mix": 0.3}, {"mode": "pingpong"}),
        fx("reverb", {"size": 0.85, "damping": 0.4, "mix": 0.35}),
    ]),
    ins("Vocals"), ins("FX"), ins("Insert 9"), ins("Insert 10"),
]}

project = {
    "$schema": "./project.schema.json",
    "format": "rosaclef/1",
    "meta": {"title": "Velvet Hour", "author": "Rosaclef", "description": "Deep house in F minor — a demo of the Rosaclef studio."},
    "transport": {"bpm": 118, "beatsPerBar": 4, "swing": 0.12},
    "channels": channels,
    "patterns": patterns,
    "playlist": {"tracks": tracks, "clips": clips},
    "mixer": mixer,
}
print(json.dumps(project, indent=2))
