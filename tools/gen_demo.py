#!/usr/bin/env python3
"""Generate the bundled demo song "Arietta".

A jazz waltz on the Arietta of Beethoven's last piano sonata (Op. 111, second
movement, 1822): its theme, note for note, with Beethoven's 9/16 lilt (a
dotted-eighth beat split long-short) heard as a swung 3/4; and changes taken
from the harmonies of its third variation, the "boogie-woogie" one (the
diminished passing chords, the chromatic basses, A minor with a major
seventh, E7 with its flat ninth), voiced as jazz chords under the melody.
Tenor sax, piano, double bass and drums, all from the General MIDI soundfont
(MuseScore General).

  Intro      the piano alone plays the theme's first half, in Beethoven's own
             harmony; the sax picks up on the last beat
  Head       A A' B B' (32 bars): the sax states the theme, brushes, then sticks
  Sax solo   a chorus and a half over the changes, building
  Trading    fours with the drums over B B'
  Head out   the theme again, the band behind it
  Coda       a tag on the motif (C, G, G) and a ritardando to the last chord

The drums are a drum part (`drums` in the project): grooves from the
library (Waltz brushes, Jazz waltz, Waltz drum solo), fills and crashes,
written by `rosaclef drums`. The source score is Craig Sapp's Humdrum
encoding of the Durand edition (github.com/craigsapp/beethoven-piano-sonatas);
the music is in the public domain.

Usage:
    cargo build --release -p rosaclef
    python3 tools/gen_demo.py crates/studio/assets/demo/project.json
"""

import json
import os
import random
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.environ.get("ROSACLEF", os.path.join(ROOT, "target", "release", "rosaclef"))
BPM = 156
BAR = 3  # beats (3/4)
L, S = 2 / 3, 1 / 3  # a swung beat: long, short
rng = random.Random(1822)

GOLD, ROSE, CHAMPAGNE, BURGUNDY, EMERALD, SAPPHIRE, AMETHYST, BRONZE = (
    "#d4af37", "#c97b84", "#e8d5b0", "#8e3b46", "#3f8f7a", "#4a6fa5", "#8a6bb0", "#b08d57")

# ------------------------------------------------------------------ chords

PC = {"C": 0, "C#": 1, "Db": 1, "D": 2, "D#": 3, "Eb": 3, "E": 4, "F": 5, "F#": 6, "Gb": 6,
      "G": 7, "G#": 8, "Ab": 8, "A": 9, "A#": 10, "Bb": 10, "B": 11}
MAJOR, LYDIAN = [0, 2, 4, 5, 7, 9, 11], [0, 2, 4, 6, 7, 9, 11]
MIXO, LYD_DOM = [0, 2, 4, 5, 7, 9, 10], [0, 2, 4, 6, 7, 9, 10]
HALF_WHOLE, ALTERED = [0, 1, 3, 4, 6, 7, 9, 10], [0, 1, 3, 4, 6, 8, 10]
DORIAN, MEL_MINOR, LOCRIAN2, WHOLE_HALF = [0, 2, 3, 5, 7, 9, 10], [0, 2, 3, 5, 7, 9, 11], [0, 2, 3, 5, 6, 8, 10], [0, 2, 3, 5, 6, 8, 9, 11]
# quality -> (chord tones for lines, rootless voicing for the piano, scale)
QUALITY = {
    "6/9": ([0, 4, 7, 9, 2], [4, 9, 2, 7], MAJOR),
    "6": ([0, 4, 7, 9], [4, 7, 9, 2], MAJOR),
    "maj7": ([0, 4, 7, 11], [4, 7, 11, 2], LYDIAN),
    "7": ([0, 4, 7, 10], [4, 10, 2, 9], MIXO),
    "13": ([0, 4, 7, 10, 9], [10, 4, 9, 2], MIXO),
    "7#11": ([0, 4, 7, 10, 6], [4, 10, 2, 6], LYD_DOM),
    "7b9": ([0, 4, 7, 10, 1], [4, 10, 1, 7], HALF_WHOLE),
    "7b13": ([0, 4, 10, 8], [4, 10, 8, 2], ALTERED),
    "7#5": ([0, 4, 8, 10], [4, 10, 8, 2], ALTERED),
    "7alt": ([0, 4, 10, 1, 3, 8], [10, 4, 3, 8], ALTERED),
    "m7": ([0, 3, 7, 10], [3, 10, 2, 7], DORIAN),
    "m9": ([0, 3, 7, 10, 2], [3, 10, 2, 7], DORIAN),
    "m6": ([0, 3, 7, 9], [3, 9, 2, 7], DORIAN),
    "mMaj7": ([0, 3, 7, 11], [3, 11, 2, 7], MEL_MINOR),
    "m7b5": ([0, 3, 6, 10], [3, 6, 10, 0], LOCRIAN2),
    "o7": ([0, 3, 6, 9], [0, 3, 6, 9], WHOLE_HALF),
    "": ([0, 4, 7], [4, 7, 0, 9], MAJOR),
    "m": ([0, 3, 7], [3, 7, 0, 10], DORIAN),
}


def parse(sym):
    """'C#o7/E' -> (root pc, quality, bass pc)."""
    name, _, bass = sym.rpartition("/")
    if "/" not in sym or not bass[:1].isalpha():  # "6/9" is a quality, not a bass note
        name, bass = sym, ""
    root = name[:2] if len(name) > 1 and name[1] in "#b" else name[:1]
    q = name[len(root):]
    return PC[root], q, PC[bass] if bass else PC[root]


def tones(sym):
    r, q, _ = parse(sym)
    return [(r + i) % 12 for i in QUALITY[q][0]]


def scale(sym):
    r, q, _ = parse(sym)
    return [(r + i) % 12 for i in QUALITY[q][2]]


def guide(sym):
    """The 3rd and 7th (or 6th): the notes that say what a chord is."""
    r, q, _ = parse(sym)
    ct = QUALITY[q][0]
    return [(r + ct[1]) % 12, (r + ct[3 if len(ct) > 3 else 2]) % 12]


# ------------------------------------------------------------------ the tune
# Changes from the Arietta's third variation, three beats a bar: (chord, beats).
A = [
    [("C6/9", 2), ("G7#11", 1)],
    [("G7/B", 2), ("C#o7", 1)],
    [("C#o7/E", 2), ("C/E", 1)],
    [("F#o7/G", 1), ("G7b9", 2)],
    [("Cmaj7/E", 3)],
    [("A7b13", 1), ("Dm9", 2)],
    [("G13", 1), ("Dm7b5/Ab", 1), ("Am7", 1)],
]
A_END1 = [("G7/B", 1), ("C6", 1), ("G7/D", 1)]  # back to the top
A_END2 = [("G7/B", 1), ("C6", 1), ("E7b9", 1)]  # on to A minor
B = [
    [("AmMaj7", 2), ("Am6", 1)],
    [("E7/G#", 2), ("E7b9", 1)],
    [("E7/G#", 1), ("Am7", 1), ("B7alt/D#", 1)],
    [("E7#5", 1), ("Dm6/F", 1), ("Am7", 1)],
    [("Dm9/G", 2), ("G13", 1)],
    [("Cmaj7", 1), ("A7b13/C#", 1), ("Dm7", 1)],
    [("G13", 2), ("G7b9", 1)],
]
B_END1 = [("G13", 1), ("C6", 1), ("E7b9", 1)]  # back to A minor
B_END2 = [("G13", 1), ("C6/9", 1), ("G7alt", 1)]  # around to the top
CHORUS = A + [A_END1] + A + [A_END2] + B + [B_END1] + B + [B_END2]
assert len(CHORUS) == 32

# The theme (Beethoven's notes, an octave lower for the tenor): per bar,
# (beat, length, MIDI pitch). The pickup C-G belongs to the bar before.
PICKUP = [(2, L, 72), (2 + L, S, 67)]
THEME_A = [
    [(0, 2, 67), (2, L, 74), (2 + L, S, 67)],
    [(0, 2, 67), (2, 1, 67)],
    [(0, 1, 67), (1, 1, 76), (2, 1, 72)],
    [(0, 1, 72), (1, 1, 71), (2, 1, 71)],
    [(0, 1, 72), (1, 1, 76), (2, 1, 79)],
    [(0, 1, 79), (1, 1, 77), (2, L, 74), (2 + L, S, 72)],
    [(0, 1, 71), (1, 1, 72), (2, L, 74), (2 + L, S, 67)],
]
THEME_A_END1 = [(0, 2, 67)] + PICKUP
THEME_A_END2 = [(0, 2, 67), (2, 1, 64)]
THEME_B = [
    [(0, 2, 72), (2, L, 72), (2 + L, S, 71)],
    [(0, 2, 71), (2, L, 71), (2 + L, S, 76)],
    [(0, 1, 76), (1, 1, 76), (2, L, 74), (2 + L, S, 72)],
    [(0, 1, 72), (1, 1, 71), (2, 1, 72)],
    [(0, 2, 74), (2, 1, 74)],
    [(0, 2, 76), (2, 1, 77)],
    [(0, 1, 74), (1, 1, 74), (2, L, 74), (2 + L, S, 79)],
]
THEME_B_END1 = [(0, L, 79), (L, S, 76), (1, 1, 76), (2, 1, 64)]
THEME_B_END2 = [(0, L, 79), (L, S, 76), (1, 1, 76)] + PICKUP
THEME = THEME_A + [THEME_A_END1] + THEME_A + [THEME_A_END2] + THEME_B + [THEME_B_END1] + THEME_B + [THEME_B_END2]

# Beethoven's own harmony for the piano intro (the first half, as written):
# per beat, (bass, chord tones).
CLASSIC = [
    [(36, "C E G"), (38, "G B F"), (35, "G B D")],
    [(35, "G B D"), (36, "C E G"), (38, "G B F")],
    [(40, "C E G"), (36, "C E G"), (40, "C E G")],
    [(31, "C E G"), (43, "G B D"), (31, "G B F")],
    [(28, "C E G"), (28, "C E G"), (28, "C G E")],
    [(26, "D F A"), (26, "D F A"), (29, "D F A")],
    [(31, "G B D"), (33, "A C E"), (35, "G B D")],
    [(35, "G B D"), (36, "C E G"), (38, "G B F")],
]
INTRO_MELODY = [[(b, d, p + 12) for (b, d, p) in bar] for bar in THEME_A] + [[(0, 2, 79)]]

# ------------------------------------------------------------------ notes & patterns

patterns = {}  # id -> {name, color, length, notes}
order = []
clips = []
TRACKS = ["Drums", "Sax", "Piano", "Bass"]
T_DRUMS, T_SAX, T_PIANO, T_BASS = range(4)


def pattern(pid, name, color, length):
    patterns[pid] = {"name": name, "color": color, "length": length, "notes": []}
    order.append(pid)
    return pid


def clip(pid, track, start):
    clips.append((pid, track, start, patterns[pid]["length"]))


def note(pid, ch, pitch, start, length, vel, human=0.012):
    t = max(0.0, start + rng.uniform(-human, human))
    v = min(1.0, max(0.08, vel + rng.uniform(-0.04, 0.04)))
    patterns[pid]["notes"].append(
        {"channel": ch, "pitch": int(pitch), "start": round(t, 4), "length": round(max(0.05, length), 4), "velocity": round(v, 3)})


def timeline(bars):
    """Chord changes of some bars: [(beat, length, chord)]."""
    out, t = [], 0.0
    for bar in bars:
        for sym, beats in bar:
            out.append((t, beats, sym))
            t += beats
    return out


def chord_at(tl, beat):
    for t, d, sym in tl:
        if t <= beat + 1e-6 < t + d:
            return sym
    return tl[-1][2]


def nearest(pc, near, lo, hi):
    best = None
    for p in range(lo, hi + 1):
        if p % 12 == pc and (best is None or abs(p - near) < abs(best - near)):
            best = p
    return best


# ------------------------------------------------------------------ bass


def bass_line(pid, tl, walk=True, vel=0.8):
    """Walking quarters (or a two-feel): the root (or slash bass) where a
    chord starts, chord and scale tones between, a chromatic approach into
    the next chord."""
    prev = 36
    beats = int(round(tl[-1][0] + tl[-1][1]))
    for k, (t, d, sym) in enumerate(tl):
        root = parse(sym)[2]
        nxt = parse(tl[(k + 1) % len(tl)][2])[2]
        p = nearest(root, prev, 31, 50)
        if not walk:
            note(pid, "bass", p, t, d * 0.92, vel + 0.05)
            if d >= 2 and rng.random() < 0.5:
                q = nearest(rng.choice([(root + 7) % 12, nxt]), p, 31, 50)
                note(pid, "bass", q, t + d - 1, 0.9, vel - 0.1)
            prev = p
            continue
        note(pid, "bass", p, t, 0.95, vel + 0.06)
        prev = p
        for i in range(1, int(d)):
            last = i == int(d) - 1
            if last and rng.random() < 0.7:
                target = nearest(nxt, prev, 31, 50)
                q = target + rng.choice([-1, 1])
            else:
                cands = [nearest(pc, prev, 31, 50) for pc in tones(sym)[1:4]]
                cands = [c for c in cands if c != prev]
                q = rng.choice(cands) if cands else prev + 2
            note(pid, "bass", q, t + i, 0.95, vel - 0.04)
            prev = q
    return beats


# ------------------------------------------------------------------ piano

prev_voicing = [52, 57, 60, 64]


def voicing(sym):
    """A rootless voicing in the left-hand register, led from the last one."""
    global prev_voicing
    r, q, _ = parse(sym)
    pcs = [(r + i) % 12 for i in QUALITY[q][1]]
    best, cost = None, None
    for lo in range(50, 61):
        v = []
        for pc in pcs:
            p = nearest(pc, lo + 4, lo, lo + 15)
            if p is not None and p not in v:
                v.append(p)
        v.sort()
        if len(v) < 3 or v[-1] > 74:
            continue
        c = sum(abs(a - b) for a, b in zip(v, prev_voicing)) + abs(len(v) - len(prev_voicing)) * 4
        if cost is None or c < cost:
            best, cost = v, c
    prev_voicing = best
    return best


COMP = [  # jazz-waltz comping: (beat, length) hits in a bar
    [(0, 0.6), (1 + L, 0.3)],
    [(1, 0.5), (2, 0.5)],
    [(0, 2.2)],
    [(1 + L, 0.9)],
    [(0, 0.5), (2 + L, 0.3)],
    [(L, 0.3), (2, 0.6)],
    [(0, 0.7)],
]


def comp(pid, tl, bars, vel=0.5, density=1.0):
    for bar in range(bars):
        if rng.random() > density:
            continue
        hits = rng.choice(COMP)
        for b, ln in hits:
            t = bar * BAR + b
            # An anticipation plays the chord it anticipates.
            sym = chord_at(tl, t + (0.4 if (b % 1) > 0.5 else 0))
            for p in voicing(sym):
                note(pid, "piano", p, t, ln, vel, human=0.008)


def intro(pid):
    """The theme's first half on the piano alone, as Beethoven harmonized it."""
    for bar, beats in enumerate(CLASSIC):
        for i, (bass, chord) in enumerate(beats):
            t = BAR + bar * BAR + i  # the intro starts after the pickup bar
            bass += 12  # in the bass staff, clear of the low register's mud
            note(pid, "piano", bass, t, 0.95, 0.42)
            mel = [p for (b, d, p) in INTRO_MELODY[bar] if b <= i < b + d]
            top = mel[0] if mel else 76
            for name in chord.split():
                p = nearest(PC[name], top - 7, max(55, bass + 3), top - 1)
                if p is not None:
                    note(pid, "piano", p, t, 0.9, 0.3, human=0.004)
        for b, d, p in INTRO_MELODY[bar]:
            note(pid, "piano", p, BAR + bar * BAR + b, d * 0.98, 0.5, human=0.004)
    # The pickup into the theme, as the piece itself begins.
    for b, d, p in PICKUP:
        note(pid, "piano", p + 12, b, d * 0.98, 0.45, human=0.002)


# ------------------------------------------------------------------ the sax


def melody(pid, bars, t0, vel=0.72, octave=0):
    for bar, notes in enumerate(bars):
        for b, d, p in notes:
            accent = 0.06 if b % 1 == 0 else -0.04
            note(pid, "sax", p + octave, t0 + bar * BAR + b, d * 0.93, vel + accent)


CELLS = {  # rhythms of one beat: (offset, length)
    "q": [(0, 1)],
    "e": [(0, L), (L, S)],
    "t": [(0, S), (S, S), (2 * S, S)],
    "a": [(L, S)],
    "r": [],
}


def solo(pid, tl, t_end, lo_int, hi_int, t0=0.0):
    """A sax solo over the changes: phrases of one to four bars with rests
    between; chord tones on the beats, scale and chromatic notes between;
    busier and higher as the intensity rises; now and then the Arietta's
    motif (a falling fourth, repeated) on the chord of the moment."""
    t = t0 + rng.choice([0, 1])
    p = 62
    while t < t_end - 1:
        x = lo_int + (hi_int - lo_int) * (t - t0) / max(1, t_end - t0)
        lo, hi = 55, int(70 + 6 * x)
        length = rng.choice([3, 3, 6, 6, 9, 12]) if x > 0.5 else rng.choice([3, 3, 6, 6, 9])
        end = min(t_end, t + length)
        if rng.random() < 0.18:
            # The motif: root, then the fourth below, twice.
            r = nearest(parse(chord_at(tl, t))[0], p, lo + 5, hi)
            for b, d, q in [(0, L, r), (L, S, r - 5), (1, 1, r - 5)]:
                note(pid, "sax", q, t + b, d * 0.93, 0.66 + 0.2 * x)
            t, p = t + 2, r - 5
            continue
        direction = rng.choice([-1, 1])
        while t < end - 1e-6:
            sym = chord_at(tl, t)
            weights = {"q": 3 - 2 * x, "e": 4, "t": 0.4 + 2.5 * x, "a": 0.6, "r": 0.5}
            cell = rng.choices(list(weights), list(weights.values()))[0]
            for off, d in CELLS[cell]:
                on_beat = off == 0
                pcs = tones(sym) if on_beat else scale(sym)
                if rng.random() < 0.18:
                    direction = -direction
                if p >= hi - 2:
                    direction = -1
                if p <= lo + 2:
                    direction = 1
                step = rng.choice([1, 1, 2, 2, 3]) if on_beat else rng.choice([1, 1, 2])
                cands = [q for q in range(p + direction, p + direction * (step + 4), direction)
                         if q % 12 in pcs and lo <= q <= hi]
                q = cands[0] if cands else p + direction
                # A chromatic approach into the next beat's chord tone.
                if not on_beat and off + d >= 1 - 1e-6 and rng.random() < 0.35:
                    nxt = tones(chord_at(tl, t + 1))
                    target = min((nearest(pc, q, lo, hi) for pc in nxt), key=lambda z: abs(z - q))
                    q = target + (1 if direction < 0 else -1)
                v = 0.6 + 0.25 * x + (0.07 if on_beat else -0.05)
                if not on_beat and rng.random() < 0.15:
                    v -= 0.2  # a ghosted note
                note(pid, "sax", q, t + off, d * 0.92, v)
                p = q
            t += 1
        # End the phrase on a guide tone, held, then breathe.
        if t < t_end:
            g = min((nearest(pc, p, lo, hi) for pc in guide(chord_at(tl, t))), key=lambda z: abs(z - p))
            hold = rng.choice([1, 2, 2, 3])
            note(pid, "sax", g, t, hold * 0.9, 0.62 + 0.2 * x)
            p = g
            t += hold + rng.choice([1, 1, 2, 3] if x < 0.6 else [1, 1, 2])


# ------------------------------------------------------------------ arrangement

sections = []  # (name, start bar, bars)
bar = 0


def section(name, bars):
    global bar
    sections.append((name, bar, bars))
    bar += bars
    return (bar - bars) * BAR


# The intro: a pickup bar, then the theme's first half on the piano.
t_intro = section("Intro", 9)
p = pattern("piano-intro", "Piano · Intro", CHAMPAGNE, 9 * BAR)
intro(p)
clip(p, T_PIANO, t_intro)
# The sax's pickup into the head, under the intro's last bar.
p = pattern("sax-pickup", "Sax · Pickup", GOLD, BAR)
melody(p, [PICKUP], 0, vel=0.7, octave=0)
clip(p, T_SAX, t_intro + 8 * BAR)

# The head.
t_head = section("Head", 32)
tl = timeline(CHORUS)
p = pattern("sax-head", "Sax · Head", GOLD, 32 * BAR)
melody(p, THEME, 0)
clip(p, T_SAX, t_head)
p = pattern("piano-head", "Piano · Head", CHAMPAGNE, 32 * BAR)
comp(p, tl, 32, vel=0.46, density=0.85)
clip(p, T_PIANO, t_head)
p = pattern("bass-head", "Bass · Head", BURGUNDY, 32 * BAR)
bass_line(p, timeline(CHORUS[:16]), walk=False)
pb = pattern("bass-head-b", "Bass · Head (walking)", BURGUNDY, 16 * BAR)
bass_line(pb, timeline(CHORUS[16:]))
patterns[p]["length"] = 16 * BAR
clip(p, T_BASS, t_head)
clip(pb, T_BASS, t_head + 16 * BAR)

# The sax solo: a chorus and a half.
t_solo = section("Sax solo", 48)
solo_bars = CHORUS + CHORUS[:16]
tl = timeline(solo_bars)
p = pattern("sax-solo", "Sax · Solo", GOLD, 48 * BAR)
solo(p, tl, 48 * BAR, 0.2, 0.95)
clip(p, T_SAX, t_solo)
p = pattern("piano-solo", "Piano · Comping", CHAMPAGNE, 48 * BAR)
comp(p, tl, 48, vel=0.44, density=0.7)
clip(p, T_PIANO, t_solo)
p = pattern("bass-solo", "Bass · Walking", BURGUNDY, 48 * BAR)
bass_line(p, tl)
clip(p, T_BASS, t_solo)

# Trading fours over B B': the sax, then the drums alone.
t_trade = section("Trading fours", 16)
trade_bars = CHORUS[16:]
tl = timeline(trade_bars)
for k in range(0, 16, 8):
    sub = timeline(trade_bars[k:k + 4])
    p = pattern(f"sax-trade-{k}", f"Sax · Fours {k // 8 + 1}", GOLD, 4 * BAR)
    solo(p, sub, 4 * BAR, 0.9, 1.0)
    clip(p, T_SAX, t_trade + k * BAR)
    p = pattern(f"piano-trade-{k}", f"Piano · Fours {k // 8 + 1}", CHAMPAGNE, 4 * BAR)
    comp(p, sub, 4, vel=0.5, density=0.9)
    clip(p, T_PIANO, t_trade + k * BAR)
    p = pattern(f"bass-trade-{k}", f"Bass · Fours {k // 8 + 1}", BURGUNDY, 4 * BAR)
    bass_line(p, sub)
    clip(p, T_BASS, t_trade + k * BAR)

# The head out, and the tag.
t_out = section("Head out", 32)
tl = timeline(CHORUS)
p = pattern("sax-out", "Sax · Head out", GOLD, 32 * BAR)
melody(p, THEME[:31] + [[(0, L, 79), (L, S, 76), (1, 2, 76)]], 0, vel=0.76)
clip(p, T_SAX, t_out)
p = pattern("piano-out", "Piano · Head out", CHAMPAGNE, 32 * BAR)
comp(p, tl, 32, vel=0.5, density=0.9)
clip(p, T_PIANO, t_out)
p = pattern("bass-out", "Bass · Head out", BURGUNDY, 32 * BAR)
bass_line(p, tl)
clip(p, T_BASS, t_out)

CODA = [
    [("Ab7#11", 2), ("G13", 1)],
    [("C6/9", 3)],
    [("Ab7#11", 2), ("G7b9", 1)],
    [("C6/9", 3)],
    [("C6/9", 3)],
    [("C6/9", 3)],
]
t_coda = section("Coda", 6)
tl = timeline(CODA)
p = pattern("sax-coda", "Sax · Coda", GOLD, 6 * BAR)
# The motif, twice, slower each time, and home.
melody(p, [
    [(0, 1, 75), (1, L, 72), (1 + L, S, 67)],
    [(0, 3, 67)],
    [(0, 1, 74), (1, L, 72), (1 + L, S, 67)],
    [(0, 3, 64)],
    [(0, 6, 60)],
    [],
], 0, vel=0.68)
clip(p, T_SAX, t_coda)
p = pattern("piano-coda", "Piano · Coda", CHAMPAGNE, 6 * BAR)
for i, (t, d, sym) in enumerate(tl):
    hold = d * 0.95 if i < len(tl) - 3 else d
    for q in voicing(sym):
        note(p, "piano", q, t, hold, 0.42, human=0.02)
    note(p, "piano", parse(sym)[2] + 36, t, hold, 0.4)
clip(p, T_PIANO, t_coda)
p = pattern("bass-coda", "Bass · Coda", BURGUNDY, 6 * BAR)
for t, d, sym in tl[:4]:
    note(p, "bass", nearest(parse(sym)[2], 36, 31, 48), t, d * 0.95, 0.75)
note(p, "bass", 36, 4 * BAR, 6, 0.78)
clip(p, T_BASS, t_coda)
SONG_END = bar * BAR

# ------------------------------------------------------------------ drums
# A drum part: the drummer plays it from the library (rosaclef drums).
DRUMS = {
    "groove": "jazz-waltz",
    "kit": "Jazz Kit",
    "feel": "natural",
    "start": 1,
    "ending": "hit",
    "variations": True,
    "seed": 5,
    "sections": [
        {"name": "Intro", "bars": 8, "play": "rest"},
        {"name": "Pickup", "bars": 1, "play": "a", "fill": "half", "groove": "jazz-waltz-brushes"},
        {"name": "Head A", "bars": 16, "play": "a", "fill": "beat", "crash": True, "groove": "jazz-waltz-brushes"},
        {"name": "Head B", "bars": 16, "play": "a", "fill": "half", "crash": True},
        {"name": "Solo", "bars": 16, "play": "a", "fill": "beat", "crash": True},
        {"name": "Solo B", "bars": 16, "play": "b", "fill": "half"},
        {"name": "Solo out", "bars": 16, "play": "b", "fill": "bar", "crash": True},
        {"name": "Sax fours", "bars": 4, "play": "b", "crash": True},
        {"name": "Drum fours", "bars": 4, "play": "a", "crash": True, "groove": "jazz-waltz-solo"},
        {"name": "Sax fours", "bars": 4, "play": "b", "crash": True},
        {"name": "Drum fours", "bars": 4, "play": "b", "crash": True, "groove": "jazz-waltz-solo"},
        {"name": "Head out A", "bars": 16, "play": "a", "fill": "beat", "crash": True},
        {"name": "Head out B", "bars": 16, "play": "b", "fill": "half"},
        {"name": "Coda", "bars": 4, "play": "a", "fill": "bar", "crash": True, "groove": "jazz-waltz-brushes"},
    ],
}
assert sum(s["bars"] for s in DRUMS["sections"]) == bar - 2, (sum(s["bars"] for s in DRUMS["sections"]), bar)

# The band lays out while the drums take their fours.
for k in (4, 12):
    lo, hi = (t_trade + k * BAR), (t_trade + (k + 4) * BAR)
    assert not any(c[2] < hi and c[2] + c[3] > lo and c[1] != T_DRUMS for c in clips)

# ------------------------------------------------------------------ mixer & automation


def fx(kind, params=None, options=None):
    return {"type": kind, "params": params or {}, "options": options or {}}


def ins(name, volume=1.0, effects=None):
    return {"name": name, "volume": volume, "pan": 0, "mute": False, "solo": False, "effects": effects or []}


MIXER = [
    ins("Master", 1.0, [fx("compressor", {"threshold": -16, "ratio": 2, "attack": 20, "release": 200, "makeup": 2}),
                        fx("limiter", {"gain": 2, "ceiling": -0.5})]),
    ins("Sax", 0.95, [fx("eq", {"low": -6, "lowFreq": 140, "mid": 1.5, "midFreq": 2200, "high": -1, "highFreq": 9000}),
                      fx("reverb", {"size": 0.62, "damping": 0.45, "predelay": 0.03, "mix": 0.2})]),
    ins("Piano", 1.0, [fx("eq", {"low": -3, "lowFreq": 160, "high": 1, "highFreq": 7000}),
                        fx("reverb", {"size": 0.6, "damping": 0.5, "mix": 0.18})]),
    ins("Bass", 1.0, [fx("eq", {"low": 2, "lowFreq": 90, "mid": -2, "midFreq": 300}),
                      fx("compressor", {"threshold": -20, "ratio": 3, "attack": 15, "release": 150, "makeup": 0})]),
    ins("Drums", 0.9, [fx("eq", {"low": -2, "lowFreq": 100, "high": 1.5, "highFreq": 8000}),
                       fx("reverb", {"size": 0.45, "damping": 0.5, "mix": 0.14})]),
]
CHANNELS = [
    {"id": "sax", "name": "Tenor Sax", "color": GOLD, "instrument": {"type": "soundfont", "params": {"gain": 1.0}, "options": {"program": "Tenor Sax"}}, "volume": 0.82, "pan": 0.12, "mute": False, "mixer": 1},
    {"id": "piano", "name": "Piano", "color": CHAMPAGNE, "instrument": {"type": "soundfont", "params": {"gain": 1.8}, "options": {"program": "Acoustic Grand Piano"}}, "volume": 0.9, "pan": -0.22, "mute": False, "mixer": 2},
    {"id": "bass", "name": "Double Bass", "color": BURGUNDY, "instrument": {"type": "soundfont", "params": {"gain": 1.0}, "options": {"program": "Acoustic Bass"}}, "volume": 0.7, "pan": 0.04, "mute": False, "mixer": 3},
    {"id": "drums", "name": "Drums", "color": BRONZE, "instrument": {"type": "soundfont", "params": {"gain": 0.95}, "options": {"program": "Jazz Kit"}}, "volume": 0.78, "pan": 0, "mute": False, "mixer": 4},
]


def lane(lid, name, target, color, *points):
    pts = []
    for pt in points:
        d = {"beat": pt[0], "value": pt[1]}
        if len(pt) > 2 and pt[2]:
            d["curve"] = pt[2]
        pts.append(d)
    return {"id": lid, "name": name, "target": target, "color": color, "points": pts}


AUTOMATION = [
    # The coda slows to its last chord.
    lane("tempo", "Tempo", "tempo", GOLD, (0, BPM), (t_coda, BPM), (t_coda + 4 * BAR, 112, 0.35), (SONG_END, 104)),
]

MARK_COLORS = [CHAMPAGNE, GOLD, ROSE, AMETHYST, EMERALD, SAPPHIRE]
MARKS = [
    {"start": s * BAR, "end": (s + n) * BAR, "color": MARK_COLORS[i % len(MARK_COLORS)], "label": name}
    for i, (name, s, n) in enumerate(sections)
]

project = {
    "$schema": "./project.schema.json",
    "format": "rosaclef/1",
    "meta": {
        "title": "Arietta",
        "author": "after Beethoven, Op. 111",
        "description": "A jazz waltz on the Arietta of Beethoven's last piano sonata (Op. 111, 1822): its theme, "
        "and changes from its third variation. Tenor sax, piano, double bass and a drum part. "
        "A demo of the Rosaclef studio.",
    },
    "transport": {"bpm": BPM, "beatsPerBar": 3, "swing": 0},
    "channels": CHANNELS,
    "patterns": [
        {"id": pid, "name": patterns[pid]["name"], "color": patterns[pid]["color"], "length": patterns[pid]["length"],
         "notes": sorted(patterns[pid]["notes"], key=lambda n: (n["start"], n["channel"], n["pitch"]))}
        for pid in order
    ],
    "playlist": {
        "tracks": [{"name": t, "mute": False} for t in TRACKS],
        "clips": [{"pattern": p, "track": tr, "start": s, "length": ln}
                  for (p, tr, s, ln) in sorted(clips, key=lambda c: (c[2], c[1]))],
    },
    "mixer": {"inserts": MIXER},
    "automation": AUTOMATION,
    "score": {"key": "C", "marks": MARKS},
    "drums": DRUMS,
}

out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "crates", "studio", "assets", "demo", "project.json")
with tempfile.TemporaryDirectory() as d:
    subprocess.run([EXE, "new", f"{d}/demo"], check=True, capture_output=True)
    with open(f"{d}/demo/project.json", "w") as f:
        json.dump(project, f)
    # The drummer plays the drum part: its patterns and clips on the Drums track.
    r = subprocess.run([EXE, "drums", f"{d}/demo"], capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(r.stderr)
    print(r.stdout.strip(), file=sys.stderr)
    subprocess.run([EXE, "fmt", f"{d}/demo/project.json"], check=True, capture_output=True)
    text = open(f"{d}/demo/project.json").read()
with open(out, "w") as f:
    f.write(text)
print(f"wrote {out}: {bar} bars, {SONG_END / BPM * 60:.0f} s at {BPM} BPM", file=sys.stderr)
