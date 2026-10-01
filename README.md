# Rosaclef

**An opulent, FL Studio–inspired music workstation for the AI era.**

Rosaclef is a pattern-based DAW — channel rack, piano roll, playlist, mixer —
with a persistent agent panel on the right: a real terminal running *your own*
coding agent (Claude Code, Codex, Gemini CLI, OpenCode, Aider or a shell)
inside the project folder. The whole song is one JSON document
(`project.json`, with a JSON Schema); the agent edits it like any source
file, and the studio picks up every change live — you hear it immediately and
can undo it with Ctrl+Z.

```
┌──────────────────────────── Browser ────────────────────────────┐
│ UI: plain ES modules, type-checked with inty, no build step     │
│   rack · piano roll · playlist · mixer · transport              │
│   agent panel (xterm.js) ─────────────── ws /ws/term ────────┐  │
│   AudioWorklet ── rosaclef.wasm (the Rust engine)            │  │
└────────────┬─────────────────────────────────────────────────┼──┘
             │ ws /ws  (project sync, native transport)        │
┌────────────▼──────────── rosaclef (native Rust) ─────────────▼──┐
│ owns project.json: validate → write → watch (agent edits)       │
│ PTY host for the agent (AGENTS.md / CLAUDE.md in the folder)    │
│ native engine → audio device (cpal), recording, offline render  │
│ CLAP plugin host · sample decoding · HTTP API                   │
└─────────────────────────────────────────────────────────────────┘
```

## Quick start

```sh
cargo build --release
./target/release/rosaclef serve my-song --demo   # creates my-song/ with the demo song
# open http://127.0.0.1:7470
```

Pick an agent in the right-hand panel (it must be installed and on your
`PATH`), then ask it for music: *"write a 4-bar bassline for the selected
pattern"*, *"arrange an intro and a breakdown"*, *"mix this and check the
levels"*.

Audio plays in the browser through the WebAssembly engine by default. Switch
the output to **Studio** to use the native engine on the server's audio device
(required for CLAP plugins; lowest latency).

## In the browser only

The studio also builds into a static site — no server, nothing to install:

```sh
tools/build-static.sh                 # → dist/, for GitHub Pages or any static host
python3 -m http.server -d dist 8080   # try it at http://localhost:8080
```

The server's own code runs in the page (compiled to WebAssembly), so nearly
everything works: editing, playback, the project library and files, LMMS/MIDI
import, rendering to WAV, recording. Projects are saved in the browser's
storage and download as `.zip` files that also open in the native studio. The
terminal runs the built-in **Rosaclef shell** instead of a coding agent, and
there is no Studio audio output or CLAP plugins. See
[`docs/static.md`](docs/static.md).

## Command line

| command | |
|---|---|
| `rosaclef serve [DIR] [--port 7470] [--demo] [--library LIB]` | open a project folder in the studio; the **Projects** window (Ctrl+O) manages the projects in `LIB` (default: the parent of `DIR`) and the open project's files |
| `rosaclef new DIR [--demo]` | create a project folder |
| `rosaclef validate [DIR\|FILE]` | check `project.json` (errors carry JSON paths) |
| `rosaclef summary [DIR]` | compact overview of a project |
| `rosaclef render [DIR] [--pattern ID] [--out FILE] [--bits 16\|24\|32]` | offline mixdown to WAV |
| `rosaclef note --channel ID --pitch 60 --out samples/x.wav` | synthesize a note into a sample |
| `rosaclef import-lmms FILE.mmp[z] [--name N] [--library LIB]` | import an LMMS project as a new project (prints what was approximated) |
| `rosaclef import-midi FILE.mid [--name N] [--library LIB] [--synth]` | import a Standard MIDI File as a new project: tempo and time signature changes, sustain pedal, program changes, volume/pan automation; played on the sampled General MIDI instruments (`--synth`: on Rosaclef's synthesizers) |
| `rosaclef fmt`, `schema`, `catalog`, `guide` | formatting, JSON schema, device catalog, agent guides |

## Voice to notes

The **Voice** tab in the bottom dock (F8) turns the microphone into an
instrument:

- **Melody** — sing, hum or whistle a line. The pitch is tracked (two
  methods that must agree) and fitted to whole notes, so slides, scoops and
  vibrato don't turn into stray little notes; **Detail** goes from *Smooth*
  to *Every note* for quick runs. You shape the notes before they land:
  quantize grid and strength, note
  ends, legato, auto-tune to a key and scale (or let it detect the key),
  octave, and velocities that follow how loud you sang. The result is a new
  pattern for the piano roll, on the selected channel or a new one.
- **Beatbox** — kicks (a low "b"/"boom"), toms (a hummed "dum"), snares
  ("pf", "k"), hats ("ts") and open hats (a long "tsss") become a drum loop on
  Atelier channels (existing ones are reused). A drum recording works too:
  several drums on one beat (a kick and a hat) are told apart, and rolls,
  fills and ghost notes come through. Set the grid, the sensitivity (ghost
  notes), the separation (hits closer than it to the one before join it — a
  flam, or one sound heard as two, becomes one hit), accents, the loop length
  and how many times it repeats; click a hit to make it another drum.

It works in three steps:

1. **Take** — record (silently, and the first note starts the pattern, or
   playing along with the pattern or the song, and the take keeps its place
   in time), **Open a recording…** from the phone or computer, or pick any
   recording in `samples/`.
2. **Shape** — **Crop** the take by dragging its left or right handle in
   (the part outside is left out, and the result's time starts at the left
   handle), adjust the settings above and **Play** the result, looping, on
   its channels; changes apply as it plays, and the song is not touched.
   **Take** plays the recording to compare; **Analyze again** re-reads it.
3. **Add to song** — the pattern and a playlist clip, in one undoable step.
   Type the **Lyrics** of a sung take (in the notation below) and they come
   along as the pattern's words, one syllable per note.

The analysis runs in Rust
(`crates/studio/src/transcribe.rs`, `GET /api/transcribe?path=…&mode=melody|drums`),
natively or in the browser-only build.

## Sheet music

The **Score** tab beside the playlist writes the whole song as engraved sheet
music, and the dock's **Score** (F10, or the score button in the piano roll)
does the same for the pattern in the piano roll — or any playlist track or
pattern, picked from its menu.

- **Engraved, not drawn**: one staff per instrument (a grand staff for wide
  piano parts, one drum staff for the drum channels with the drummer's
  noteheads), measures from the meters, a key signature (guessed, or set),
  pitches spelled in the key, durations split to show the beat and tied,
  triplets, beams, multi-measure rests. Glyphs come from the
  [Bravura](web/fonts/Bravura-OFL.txt) music font (SMuFL, SIL OFL); spacing
  follows durations on columns shared by every staff, systems are chosen for
  the whole piece at once and justified, and staves are spaced by their ink.
- **Parts**: show or hide instruments, leave playlist tracks out, pick a clef,
  hide staves that rest (as in orchestral scores); paper or night ink.
- **PDF**: download what the view shows as vector pages ready to print (A4,
  or US Letter in the US and Canada), paginated with a title page heading.
- **Repeats**: drag across some bars and press **Repeat** to put repeat
  signs around them; set how many times they play (×2, ×3…) and make bars
  **endings** ("1.", "2.", "1.–2.") that play on chosen passes only. The
  song plays them — the playhead jumps back at the end sign, skips the
  endings that are not this pass's — and the playlist's ruler shows them.
- **Colors**: drag across the music to color a passage (on some staves or
  all) and label it; a passage colored in a pattern is colored wherever the
  pattern plays.
- **Lyrics**: the words of a channel that sings are set under its staff —
  syllables centered under their notes, hyphens within words, extender lines
  under held syllables. A pattern's verses are stacked and numbered; in the
  song each clip sings the verse that plays there.
- **Editing**: click notes to select them (they are the piano roll's
  selection), drag them up or down by step and along the bar, delete or
  transpose them, or switch to **Write** and click notes in with a chosen
  value. Everything is undoable and plays at once.

The settings live in `project.json` under `score`, so the agent can set the
key, hide parts or color a chorus too.

## Patterns used by reference, and lyrics

A pattern can play other patterns, or ranges of them, by reference
(`uses`: moved in time, transposed, on another channel), so a motif written
once changes everywhere it plays. And a pattern can carry the words its
notes sing (`lyrics`), with several verses.

- **In the piano roll** the notes a pattern uses are drawn as ghost notes in
  a box labeled with the pattern and how it is changed ("Hook +5 v2"); its
  menu opens the pattern, makes the notes plain (**Make unique**),
  transposes, picks the verse it sings or removes it. Select notes and press
  the link button (**Make reference**) to turn them into a new pattern
  played where they were.
- **The lyric row** under the notes shows each note's syllable. Click a note's
  cell and type: Tab goes to the next note, `_` holds the syllable before,
  a trailing `-` joins the next syllable to the word. The verse buttons (and
  `+`) pick or start a verse; **Lyrics…** edits a whole verse as text and
  counts its syllables against the notes.
- **The notation**: a space ends a word, `Hel-lo` splits syllables, `_` holds
  a syllable over the next note, `/` ends a line and `//` a paragraph,
  `word[w ɜ d]` gives a pronunciation, `(br)` a breath.
- **Verses**: a clip sings the verse picked in the playlist's tools
  (**Sings**, shown as a "v2" badge), else the pass of the repeat it plays
  in — so one pattern carries all the verses of a strophic song.

See [`docs/voice-and-lyrics.md`](docs/voice-and-lyrics.md) for the design.

## Sampled instruments

**Orchestre** (`"type": "soundfont"`) plays sampled instruments: the 128
General MIDI programs (pianos, strings, brass, winds, guitars, basses,
choirs, …) and 8 drum kits, from the built-in
[MuseScore General](web/soundfonts/gm/LICENSE.md) soundfont (MIT). Pick one
with `options.program`, e.g. `"Acoustic Grand Piano"` or `"Standard Kit"`.
Imported MIDI files use it by default.

The soundfont ships split into 1 MB pieces (`web/soundfonts/gm/`, made by
`tools/split_soundfont.py`), and a song loads only the pieces its programs
use: a string section is about 1 MB, the grand piano 15 MB. Loading never
blocks the studio: in the browser a worker fetches and decodes the samples
and hands them to the audio engine in small steps; natively the server loads
them on a background thread. An instrument stays silent until its sounds
arrive.

## A project folder

```
my-song/
  project.json          the song — the single source of truth
  project.schema.json   JSON Schema for it
  AGENTS.md             guide for coding agents (CLAUDE.md / GEMINI.md import it)
  samples/  renders/    audio in and out
  .rosaclef/            live state: status.json (last load), context.json (what the UI shows)
```

Times are in beats, pitches are MIDI numbers, instruments and effects are
`{ "type", "params", "options" }` described by a catalog (`rosaclef catalog`).
**Automation** lanes (`"automation": [{ "target": "channel/pad/cutoff", "points": [...] }]`)
drive the tempo, swing, channel and insert volume/pan or any device parameter over the song;
right-click a knob, fader or the tempo display to create one, and edit its curve under the
playlist tracks. **Transpose** (beside the tempo; `transport.transpose`, −12…12) shifts
every pitched instrument by semitones to suit a singer: the notes stay as written, drums
and audio clips are not shifted.
See [`docs/architecture.md`](docs/architecture.md) for the design.

## Repository

| path | |
|---|---|
| `crates/core` | project model, device catalog, validation, JSON Schema, formatter |
| `crates/engine` | portable DSP engine: sequencer, synths (Aurum subtractive, Lumière FM, Atelier drums, Vault sampler), effects, mixer, offline render |
| `crates/wasm` | the engine compiled to WebAssembly (C ABI for the AudioWorklet) |
| `crates/import` | importers: LMMS projects (.mmp/.mmpz) and Standard MIDI Files |
| `crates/fs` | the file system the studio works on: the disk, or an in-memory tree the browser persists |
| `crates/studio` | the server's portable logic: project folders, library, file manager, zip archives, agent guides, audio decoding, rendering |
| `crates/local` | the server's API and sockets in the browser (WebAssembly, for the static build) + the Rosaclef shell |
| `crates/clap-host` | CLAP plugin hosting (scan, parameters, processing) + tests |
| `crates/clap-testplug` | a tiny CLAP bundle used by the tests |
| `crates/server` | the `rosaclef` binary: server, file watching, PTY agent terminal, native audio, CLI |
| `web/` | the studio UI (plain JS checked by [inty](https://sinelaw.github.io/inty/)) |
| `web/soundfonts/` | the General MIDI soundfont, split for loading on demand |
| `docs/` | architecture and notes |

## Development

```sh
cargo test --workspace                 # Rust tests (engine, validation, CLAP host)
./tools/build-wasm.sh                  # rebuild web/engine/rosaclef.wasm and web/local/rosaclef-local.wasm
node web/test/tree.test.js             # UI tree tests (no browser needed)
node web/test/voice.test.js            # voice-to-notes logic (quantize, auto-tune, drums)
node tools/bench-engine.mjs            # real-time load of the WebAssembly engine (demo song)
web/check.sh                           # type-check the frontend with inty
cargo fmt --all                        # format Rust
(cd web && npm run format)             # format JS, CSS, HTML, JSON (Prettier)
ruff format . && ruff check .          # format and lint Python (tools/)
```

CI (`.github/workflows/ci.yml`) runs all of these on every pull request —
formatting is checked, not fixed — plus the browser tests in `web/test/`.

The frontend has no build step: `web/` is served as-is. Its code is plain
JavaScript that inty type-checks; browser APIs inty doesn't describe yet go
through a small typed boundary (`web/lib/platform.js` + `web/types/platform.d.js`).
Known inty rough edges are tracked in [`docs/inty-notes.md`](docs/inty-notes.md).

## License

GPL-3.0-or-later. `web/vendor/xterm` is MIT (xterm.js). `web/soundfonts/gm`
is MuseScore General (MIT; see its [license](web/soundfonts/gm/LICENSE.md)).
