# Drums: a drummer in the studio

A design for helping composers make good drum tracks the way professional
drummers do: by choosing from a bank of known grooves, switching parts by
section and energy, playing fills into changes and a crash on the "one", and
keeping it human. This document records the research, the concepts that were
considered, the trade-offs and decisions, and the design of the **basic
version** that is built first.

## 1. How drummers actually make patterns

A drummer on a session does not write notes one at a time. From the
pedagogy (Dawson, Chester, Chaffee, Garibaldi, Harrison) and from how
working drummers play a chart:

- **Grooves come from a vocabulary.** Each style has known time-keeping
  patterns ("straight 8ths", "half-time shuffle", "one drop", "bossa"), and
  new grooves are mostly *morphs* of known ones: the hats of one, the
  backbeat of another, the kick of a third (the "Rosanna" shuffle is Purdie's
  shuffle hats, Bonham's half-time snare and a Bo Diddley clave kick).
- **A song is a road map.** Sections, bar counts, an energy curve
  (closed hats in the verse, ride or open hats in the chorus), **fills** at
  the end of sections, a **crash** on the downbeat that follows, **hits**
  with the band, a count-in and an ending.
- **A pattern has layers**, and drummers vary them independently:

  | layer | decides | where it comes from |
  |---|---|---|
  | rhythm | where notes fall on the grid | Reed's *Syncopation* lines, Chaffee's kick permutations |
  | limbs / sticking | which limb plays each note | Chaffee's sticking groups, linear playing, Chester's ostinato + melody |
  | orchestration | which drum or cymbal each note is on | moving kicks to the floor tom, hats to the ride, closed vs open hats |
  | dynamics | accent, normal, ghost, feathered | Chaffee's full/down/tap/up strokes, Garibaldi's "two sound levels" |
  | feel | microtiming and phrasing | swing ratio, a snare that sits back, a 2-bar accent shape (the Rosanna study) |

- **Transformations** turn one groove into many: displacing it by a 16th
  (Garibaldi, Harrison), re-voicing it on other drums, opening the hats.
- **Humanity is structure, not noise.** Ghost notes are quiet because of the
  stroke that precedes them, the backbeat is late on purpose, and the timing
  shape repeats per phrase. Random velocity and timing jitter is not "feel".
- **Playability matters.** Two hands and two feet, a top speed per limb at a
  given tempo. Parts a human could play sound like a human played them.

## 2. Concepts considered

Four user-experience concepts were explored. They are not exclusive; they
stack on one data model.

### A · Groove Crate — browse, audition, drop

A browser of **groove families** organized by *Style → Feel → Energy*. A
family is not one loop but a drummer's set of parts: main A/B/C, fills of
several lengths, intro, ending, break. Audition in time with the song,
drag onto the playlist. Variations (displace, re-voice, open hats, ghost
level) are generated from the groove's layers. An optional live mode
triggers parts like an arranger keyboard.

- **For:** familiar (EZdrummer, BFD), immediate, easy to build.
- **Against:** the composer still places every fill by hand; tedious for a
  whole song.

### B · Drummer's Chart — describe the song, the drummer plays it

The composer writes what you would hand a session drummer: sections with bar
counts, energy, surface (hats, ride, rim, toms), feel (half/double time),
fills, crashes, hits and stops. The tool picks grooves and variations, puts
fills at section ends, crashes on the following downbeat, varies every 4th
or 8th bar and never plays the same fill twice in a row.

- **For:** a whole song's drums in minutes, in the composer's terms; the
  chart is small JSON the agent can edit ("make the second chorus bigger").
- **Against:** the hardest to make sound musical; needs good selection rules.

### C · Four Limbs — build grooves from a limb vocabulary

Each limb picks from a vocabulary (right hand: 8ths, 16ths, ride, bell,
cascara; left hand: backbeat, ghosts, cross-stick, clave; right foot:
four-on-the-floor, rock, bossa, one drop; left foot: hat on 2 & 4, off). A
**Linear** toggle forbids simultaneous limbs. Per-limb feel (kick on the
beat, snare late). Chester's ostinato + "melody on one limb" exercise is the
model.

- **For:** endless grooves from a small library; teaches drumming.
- **Against:** needs drum knowledge; makes loops, not songs.

### D · Interpret the band — drums from the composer's own lines

Dawson's students *interpret* a one-line rhythm across the kit with fixed
systems. The composer already has those lines: the bass rhythm, horn hits,
the melody. Pick a source line and a reading system ("accents on crash +
kick, fill the gaps with snare 8ths", "line on the kick under steady 8ths")
over a time-keeping ostinato, and get drums that lock with the arrangement.
Beatbox (Voice tab) is Dawson's "sing it first".

- **For:** unique to Rosaclef; drums follow the arrangement instead of
  ignoring it.
- **Against:** less predictable; needs good default systems.

### Comparison

| | A · Crate | B · Chart | C · Limbs | D · Interpret |
|---|---|---|---|---|
| composer thinks in | loops and parts | sections and energy | rhythm building blocks | their own lines |
| best for | sketches, producers | whole songs, non-drummers | custom feels, drummers | locking with the band |
| effort to build | low–medium | high | medium | medium |
| agent fit | good | excellent | good | good |

## 3. Decisions and trade-offs

| # | question | decision | why / what we give up |
|---|---|---|---|
| 1 | Generate notes, or a "drummer" device that plays live? | **Generate ordinary notes** into patterns and playlist clips. | Everything downstream keeps working: piano roll, drum staff in the score, rendering, the agent reading notes. The engine does not change. We give up per-pass variation inside a looping clip and must handle hand edits on regeneration (decision 6). |
| 2 | Where does the groove library live? | **In `rosaclef-core`**, as compiled-in data. | One library for the UI, the CLI, the agent and the browser-only build. Adding grooves needs a rebuild; user groove libraries are a later feature. |
| 3 | Library format | **Step strings per drum role**, one character per step: `X` accent, `x` normal, `g` ghost, `f` feathered, `.` rest. 16 steps per 4/4 bar (16ths) or 12 (triplet 8ths) per bar. | Readable and reviewable in a diff, writable by a drummer or the agent. Captures rhythm, orchestration (the row) and dynamics (the letter). Microtiming is not in the grid — it comes from the feel (decision 7). Finer tuplets (quintuplets, 32nd rolls) are out. |
| 4 | The five layers in v1? | Rows are **roles** (rhythm + orchestration + dynamics). **Limbs** are derived from a fixed role → limb table and used only to test playability. **Feel** is separate. | A pragmatic subset that can grow into the full model (Four Limbs, linear mode) without changing stored grooves. |
| 5 | Library source | **Hand-written, original grooves**, named descriptively ("half-time shuffle"), not after songs or drummers. Fill vocabularies shared per subdivision, not per groove. | Clean licensing; the method books' *ideas* (permutation, sticking groups) are free, their exercises are not copied. Fewer grooves to start (about two dozen) but each one is checked. Imported MIDI groove packs: later. |
| 6 | Re-editing: one-shot generator or persisted spec? | **Persisted spec** (`drums` in `project.json`) + "Write" regenerates the drum patterns it owns. Changing the chorus from A to B or a fill size is one click, not a redo. Costs a small schema addition. | |
| 6b | Hand edits to written patterns? | **Yours, kept.** Written patterns are ordinary patterns, edited in place in the piano roll. Writing again keeps an edited pattern note for note (`kept`, notes stored by drum role so they follow a kit change), and an edit to a groove's plain pattern also becomes the song's version of the groove (`grooves`), so its crash, fill and turnaround bars follow. The groove grid in the tab edits the same thing directly. **Reset** gives a pattern (or a groove) back to the drummer. | The tool and the piano roll never fight: no dialog, nothing lost. A kept pattern no longer follows feel changes (it is yours); folding an edit into the grid snaps it to the groove's steps and four stroke levels — the kept pattern itself stays exact. |
| 7 | Humanize | **Deterministic, structured feel**: *Tight*, *Natural* or *Loose* sets per-role timing (snare backbeat a few ms late, ghosts a little later, kick on the grid) and a small seeded variation in time and velocity. Optional swing for straight grooves; shuffles are written in triplets. | Repeatable output (same spec → same notes), no "random" button. Because patterns are reused, each repetition of a pattern is identical — accepted for v1. |
| 8 | Pattern granularity | **Reuse**: one pattern for the plain groove, and one-bar patterns for bars that differ (crash bar, fill bar, turnaround bar). Runs of identical bars become one looping clip. | The playlist reads like a chart, and editing the main groove once changes it everywhere. A per-bar "re-roll" would need one pattern per bar — not in v1. |
| 9 | Sound | **Both sound sources** through a role map: a General MIDI kit (one Grand Orchestra channel, GM drum map) or the Ebony Drum Machine (one channel per drum, crash and ride approximated as in the MIDI importer). Each groove suggests a kit. | One library plays on acoustic and electronic sounds. Ebony has no real cymbals, so acoustic styles default to a GM kit. |
| 10 | Where sections come from | The **drum part's own section list**, prefilled from the playlist: a new section where the set of playing patterns changes (drum-only patterns aside), changes shorter than 4 bars joined to the section before, named from a Score passage label or from the pattern names' shared suffix ("Bass · Chorus", "Keys · Chorus" → *Chorus*), neighbours with the same name joined, and groove B for the sections fuller than usual. | Works on any project today. A song-wide "sections" concept shared with the score and playlist would be better; it is an open question, not a v1 blocker. |
| 11 | UI home | **One "Drums" tab in the bottom dock**, beside Voice. No new windows, no new playlist chrome. | Matches how Voice works (shape, preview, add to song). |
| 12 | Meters | v1 requires the drum part to be in **one meter** that matches the groove (4/4, or 3/4 and 6/8 grooves). | Odd and changing meters are rare in the target songs; the panel says why a groove is unavailable. |
| 13 | Advanced ideas (displacement, linear, clave direction, metric modulation, Interpret, live triggering) | **Not in v1.** | Kept in the roadmap (section 5); the data model leaves room for them. |

## 4. The basic version

The goal: a composer with no drum knowledge gets a convincing, editable
drum track for a whole song in under a minute, with **three choices**:
a groove, a kit, and what each section does. Everything else has a good
default.

### 4.1 What the user does

1. Open the **Drums** tab in the dock (F4).
2. Pick a **groove** (grouped by style). Press **▶** to hear it looped in
   time with the song; the arrows step to the previous/next groove while it
   plays, so comparing is quick.
3. Check the **sections** strip. It is prefilled from the playlist. Click a
   section to set what the drummer plays there:
   - **Part**: *Groove A* (verse), *Groove B* (chorus, bigger), *Hits*
     (crash and kick on each downbeat), *Count-in*, *Rest*;
   - **Fill into the next section**: none, 1 beat, 2 beats, 1 bar;
   - **Crash** on its first downbeat (on by default after a fill).
   Drag a boundary to resize sections; right-click to split or merge.
4. Optionally pick a **kit** and a **feel** (Tight / Natural / Loose, and
   swing for straight grooves). Each groove suggests a kit.
5. Press **Write drums**. Patterns and clips appear on a "Drums" playlist
   track, in one undoable step. Edit them like any other notes.

### 4.2 Layout

```
┌ Drums ─────────────────────────────────────────────────────────────────────────┐
│ Groove [Rock · Straight 8ths      ▾] ◀ ▶  ▶ Play     Kit [Standard Kit ▾]      │
│ 70–170 BPM · 4/4                         Feel [Natural ▾]  Swing ──●────── 0%  │
│ ┌ A ─────────────────────────────┐ ┌ B ────────────────────────────┐           │
│ │ Hat   X.x.X.x.X.x.X.x.         │ │ Ride  X.x.X.x.X.x.X.x.        │           │
│ │ Snare ....X.......X...         │ │ Snare ....X.......X...        │           │
│ │ Kick  X.......X.X.....         │ │ Kick  X.....X.X.X.....        │           │
│ └────────────────────────────────┘ └───────────────────────────────┘           │
├────────────────────────────────────────────────────────────────────────────────┤
│ Sections   from bar [1]          [Guess from playlist]   Ending [Hit ▾]        │
│ │Intro 1 │Verse 8          │Chorus 8         │Verse 8          │Chorus 8    │   │
│ │count   │A        ▸1 beat │B ✶      ▸1 bar  │A ✶      ▸1 beat │B ✶         │   │
├────────────────────────────────────────────────────────────────────────────────┤
│ Small variations [✓]                                 [ Write drums ]           │
└────────────────────────────────────────────────────────────────────────────────┘
   ✶ = crash on the first downbeat   ▸ = fill into the next section
```

The groove preview is the step grid of parts A and B, read-only — it shows
what a groove *is* at a glance and teaches the notation the agent uses.

### 4.3 What gets written

For each section the writer lays out bars:

- the **groove part** (A or B) on plain bars;
- a **crash bar** first when *crash* is on: crash and kick on the "one", the
  hat or ride stroke there dropped;
- a **fill bar** last when a fill is set: the groove up to the fill, then
  the fill (drawn from the groove's fill vocabulary by length, energy and a
  seed; never the same fill twice in a row; bigger fills lead into B);
- with **small variations** on, a **turnaround** on every 4th bar that is
  not a crash or fill bar (the last "&" opens the hat, or a pickup kick when
  the groove has no hats);
- **Hits** bars: crash and kick on each downbeat; **Count-in** bars: side
  stick on every beat; **Rest** bars: nothing;
- the **ending**: a crash and kick on the downbeat after the last section.

Identical consecutive bars become one clip of a looping pattern. Patterns
are named for what they are ("Drums · Straight 8ths A", "… A + crash",
"… B + 1-bar fill") and get ids starting `drums-`.

Drum channels the previous write used that nothing plays any more (after a
kit change) are removed. Other drums already in the song are left alone, and
the tab says which tracks hold them, since they will play along.

### 4.3b Editing by hand

Written drum patterns are ordinary patterns: open one from the playlist or the
pattern list and change it in the piano roll (a General MIDI kit's keys are
named for their drums: Kick, Snare, Closed Hat…). The plain groove pattern is
shared by every plain bar of its part, so one edit changes the whole song,
the way a drummer changes the groove.

When the part is written again (to change a section, the kit, the feel):

- every written pattern that was edited is **kept note for note** (in
  `kept`, by slot). Its notes are stored by drum role (`snare`, `gm:50`,
  `channel:<id>:<pitch>`), so a kit change moves them to the new kit;
- an edited **plain groove** also becomes the song's version of that groove
  (in `grooves`, as step rows), so its crash, fill and turnaround bars carry
  the same change;
- the groove grid in the tab shows and edits those rows directly (click a
  step: hit → accent → ghost → rest; **+ Drum…** adds a row). Editing the grid
  makes it the groove's source again, replacing a kept copy of its plain
  pattern;
- **Reset** next to a kept pattern, or **Reset groove**, gives it back to
  the drummer; `rosaclef drums --reset-edits` forgets them all.

Arrangement changes stay with the sections: moving or deleting clips on the
Drums track is undone by the next write.

### 4.4 `project.json`

```jsonc
"drums": {
  "groove": "rock-8ths",          // a groove id (rosaclef grooves)
  "kit": "Standard Kit",          // a GM drum kit, or "Ebony" (drum machine channels)
  "feel": "natural",              // tight | natural | loose
  "swing": 0,                     // 0..1, straight grooves only
  "start": 1,                     // bar the first section starts on (from 1)
  "ending": "hit",                // hit | none
  "variations": true,
  "seed": 1,
  "sections": [
    { "name": "Intro",  "bars": 1, "play": "count" },
    { "name": "Verse",  "bars": 8, "play": "a", "fill": "beat" },
    { "name": "Chorus", "bars": 8, "play": "b", "fill": "bar", "crash": true },
    { "name": "Bridge", "bars": 8, "play": "a", "groove": "rock-halftime", "crash": true }
  ],
  "written": { "drums-rock-8ths-a": "5f2c91d0" }   // managed: fingerprints of written patterns
}
```

- `play`: `a`, `b`, `hits`, `count`, `rest`. `fill`: `none`, `beat`, `half`,
  `bar`. `groove` on a section overrides the part's groove there (a
  half-time bridge).
- The spec changes nothing that plays: the notes it wrote do. Editing the
  spec (by hand, by the agent) and then writing again is the loop.

### 4.5 The groove library

```rust
Groove {
    id: "rock-8ths", style: "Rock", name: "Straight 8ths",
    bar_beats: 4, steps: 16, tempo: (70, 170), kit: "Standard Kit", swing: 0.0,
    a: &[("hat", "X.x.X.x.X.x.X.x."), ("snare", "....X.......X..."), ("kick", "X.......X.X.....")],
    b: &[("ride", "X.x.X.x.X.x.X.x."), ("snare", "....X.......X..."), ("kick", "X.....X.X.X.....")],
}
```

- **Roles**: `kick`, `snare`, `rim` (side stick), `clap`, `hat`, `pedal`
  (hat with the foot), `openhat`, `ride`, `bell`, `crash`, `tom1`, `tom2`,
  `tom3` (high, mid, floor), `cowbell`, `shaker`, `tamb`.
- **Dynamics**: `X` 1.0, `x` 0.78, `g` 0.32, `f` 0.2 (before feel).
- **Fills** are shared vocabularies per subdivision (16ths, triplets): rows
  over 1, 2 or 4 beats, each tagged with an energy.
- **v1 styles**: Rock, Pop, Ballad, Funk, Soul, Shuffle, Jazz, Hip-hop,
  Trap, House, Techno, Disco, Drum & bass, Reggae, Latin (bossa, samba),
  Country, Metal, and 3/4 and 6/8 grooves.
- **Tests** check every groove and fill: rows have whole bars, letters are
  valid, roles are known, and it is **playable** — at most two hand strokes
  at once, at most one stroke per foot, and no hand faster than 16ths at the
  groove's top tempo.

### 4.6 Kits

| role | GM key | Ebony channel (kind, approximations) |
|---|---|---|
| kick | 36 | Kick (`kick`) |
| snare / rim / clap | 38 / 37 / 39 | Snare, Rim, Clap |
| hat / pedal / openhat | 42 / 44 / 46 | Hi-Hat (`hat`), Open Hat (`openhat`) |
| ride / bell | 51 / 53 | Ride (`openhat`, shorter decay) |
| crash | 49 | Crash (`openhat`, long decay) |
| tom1 / tom2 / tom3 | 48 / 45 / 43 | Toms (`tom`, tuned by pitch) |
| cowbell / shaker / tamb | 56 / 70 / 54 | Cowbell, Shaker, Shaker |

Existing channels are reused: a Grand Orchestra channel already playing a
drum kit, or Ebony channels of the right kind.

### 4.7 Feel

| feel | snare backbeat | ghosts | other strokes | random spread (time / velocity) |
|---|---|---|---|---|
| Tight | on the grid | on the grid | on the grid | none |
| Natural | +6 ms | +4 ms | on the grid | ±3 ms / ±0.04 |
| Loose | +12 ms | +8 ms | on the grid | ±7 ms / ±0.08 |

The spread is seeded by the spec's `seed`, the pattern and the note, so
writing twice gives the same notes. Swing delays the off 16ths (or off 8ths
for an 8th-note grid) by up to a triplet, like the transport's swing.

### 4.8 Where it lives

| piece | place |
|---|---|
| spec types, groove and fill library, kit map, writer | `crates/core/src/drums/` (native + wasm) |
| `drums` in the model, schema and validation | `crates/core/src/model.rs`, `schema.rs`, `validate.rs` |
| API: `GET /api/grooves`, `POST /api/drums[?guess=true]` (project in → written project + hand-edited pattern ids out), `POST /api/drums/pattern?id=` (project in → that pattern made from its recipe) | `crates/server`, `crates/local` (browser-only build) |
| CLI: `rosaclef grooves`, `rosaclef drums [DIR] [--pattern ID]` | `crates/server` |
| agent guide | the generated `AGENTS.md` |
| the Drums tab | `web/src/ui/drums.js`, `drums.css` |

The writer is a pure function of the project, so the studio, the CLI, the
agent and the browser-only build produce the same notes.

### 4.9 Out of scope for v1

Per-bar re-roll; live triggering; displacement / re-voicing buttons; linear
mode and the Four Limbs editor; Interpret the band; clave direction; odd or
changing meters; tempo-specific groove variants; user groove libraries and
MIDI groove import; brushes.

## 4.10 The tab, one screen (v2)

The first tab was a wizard: a start screen to pick a style, then an editing
screen for the whole song's part, and nothing on either said where the notes
would go. The tab is now one screen, built on a single idea: **it always
works on a drum pattern, and says where that pattern plays.**

```
┌ TARGET ─────────────────────────────────────────────────────────────┐
│ DRUMS [Drums · Waltz ▾] → Drums track, bars 3–6  3/4  Jazz Kit      │
│                                  [−] 4 bars [+]  ▶  ⧉ copy  🗑       │
├ GROOVES ──────────┬ PATTERN ────────────────────────────────────────┤
│ 🔍 Grooves in 3/4 │ Jazz waltz · 3/4 · 110–220 BPM          ‹ ›      │
│ WALTZ             │ Plays [A][B][Hits][Count-in][Rest]  Fill [..]   │
│ ▶ Waltz           │ Crash [On the 1]  Turnaround [Off]              │
│ JAZZ              │ Pattern bars 4  Kit [Jazz Kit▾]  Feel  Swing    │
│ ▶ Jazz waltz  ✓   │ Ride  ▮▯▯▮▯▮ ▮▯▯▮▯▮ …   (the pattern's notes)    │
│ Not in 3/4 …      │ Kick  ▮▯▯▯▯▯ ▮▯▯▯▯▯ …   [+ Drum…]               │
├ SONG ─────────────┴─────────────────────────────────────────────────┤
│ 1   5   9   13  …   ▕Waltz▏ ▕A▏▕A▏▕B+fill▏ …    (drum clips, cursor) │
├─────────────────────────────────────────────────────────────────────┤
│ ▸ Song drummer  14 sections, bars 1–141 · 24 patterns written       │
└─────────────────────────────────────────────────────────────────────┘
```

**The target** is the drum pattern at the song cursor or the selected clip
(clicking a clip in the playlist or on the song strip, or moving the cursor
while stopped, retargets it), or one picked from the list. With no drums at
the cursor it says so and offers **+ New pattern at bar N**.

**The flows:**

1. *No drums yet:* put the cursor where they should start (or select the
   clip of the section), click a groove. A pattern is made there: four bars
   (or the selected clip's, or up to the next drum clip), its clip on the
   Drums track (made if needed), the kit the last pattern used.
2. *Drums for this chorus:* select the chorus clip, click a groove (or
   **Copy** the verse's pattern there), then B and a fill.
3. *Edit drums:* click a drum clip; change the recipe (the pattern is made
   again, one undo step) or click its notes (marked as edited by hand: a
   recipe change makes it again, Ctrl+Z brings the edits back).
4. *Try before choosing:* ▶ on a groove plays it in the target, looping,
   without changing the song; ▶ in the target bar loops the pattern itself.
5. *Mixed time signatures:* the target's bars decide its time signature; the
   grooves that fit come first and the others are greyed out.
6. *The whole song at once:* open **Song drummer** (the v1 part, unchanged:
   sections, Write drums). Its patterns show on the strip and can be
   targeted like any other; picking a groove for one makes it a pattern of
   its own (the song drummer leaves it alone after).
7. *Remove:* the trash removes the target with its clips (Ctrl+Z).

**In the project** a pattern made this way carries its recipe
(`patterns[].drums`: groove, play, fill, crash, turnaround, kit, feel, swing,
seed, edited); `POST /api/drums/pattern?id=` — and `rosaclef drums --pattern
ID` — make its notes from it with the song drummer's own code (grids, fills,
feel, kit channels), so the agent can make and edit drum patterns too. A kit
change switches the kit channel the pattern plays on (the patterns sharing
it follow).

## 5. After v1

1. **Variations strip** on a groove (concept A): displace ±1/16, re-voice
   kicks to the floor tom and hats to the ride, open hats on the "&"s, ghost
   level. All are transforms of the step rows.
2. **Interpret the band** (concept D): source line + reading system →
   rows, reusing the same writer.
3. **Fill builder** from sticking modules (Chaffee groups chained to a
   length, voiced over the toms), checked for playability.
4. **Four Limbs editor** (concept C) once limbs are stored per stroke, with
   a Linear toggle and saved feel profiles (swing ratio, per-limb offsets,
   2-bar accent shape).
5. **Chart extras** (concept B): hits and stops synced to the band, clave
   direction for Latin grooves, half-time / double-time per section,
   build-ups, metric-modulation transitions.
6. **Live mode**: Intro / A / B / Fill / Ending pads while the song plays,
   recorded to the playlist.

## 6. Open questions

- Should sections become a song-wide concept (shared by the playlist, the
  score and the drums) rather than living in the drum part?
- How large should the built-in library grow, and do we accept contributed
  groove packs?
- Should Write offer to keep hand-edited patterns instead of only warning?
