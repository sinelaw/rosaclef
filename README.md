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
rustup target add wasm32-unknown-unknown         # once: the browser's audio engine is WebAssembly
cargo build --release
./target/release/rosaclef serve my-song --demo   # creates my-song/ with the demo song
# open http://127.0.0.1:7470
```

The build also makes the browser's audio engine, `web/engine/rosaclef.wasm`
(it is not kept in git): `cargo build` and `cargo run` rebuild it when the
engine changes. Without the WebAssembly target the server still builds, with a
warning, but audio in the browser does not start (`ROSACLEF_SKIP_WASM=1` skips
it on purpose).

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
| `rosaclef critic [DIR] [--fix KEY\|RULE\|all] [--suppress KEY\|RULE] [--enable\|--disable RULE] [--json]` | lint the song against production rules of thumb; apply fixes, suppress findings, turn checks on or off (see [`docs/critic.md`](docs/critic.md)) |
| `rosaclef render [DIR] [--pattern ID] [--out FILE] [--bits 16\|24\|32]` | offline mixdown to WAV (shows progress on a terminal) |
| `rosaclef mixcheck [DIR] [--range BAR:BAR\|--section NAME] [--focus ID] [--what-if PATCH] [--text]` | mix diagnostics from one render: loudness, true peak, the limiter, masking and audibility, spectrum, phase — with fixes to the mixer (see [`docs/mixcheck.md`](docs/mixcheck.md)) |
| `rosaclef note --channel ID --pitch 60 --out samples/x.wav` | synthesize a note into a sample |
| `rosaclef import-lmms FILE.mmp[z] [--name N] [--library LIB]` | import an LMMS project as a new project (prints what was approximated) |
| `rosaclef import-midi FILE.mid [--name N] [--library LIB] [--synth]` | import a Standard MIDI File as a new project: tempo and time signature changes, sustain pedal, program changes, volume/pan automation; played on the sampled General MIDI instruments (`--synth`: on Rosaclef's synthesizers) |
| `rosaclef grooves` | list the drum grooves |
| `rosaclef drums [DIR] [--groove G] [--kit K] [--guess] [--reset-edits]` | write the project's drum part into drum patterns and clips |
| `rosaclef fmt`, `schema`, `catalog`, `guide` | formatting, JSON schema, device catalog, agent guides |

## The Critic

The **Critic** tab in the Maestro panel lints the song: 77 mechanical checks
(no AI) of harmony, melody, rhythm, arrangement, low end, mix, stereo, effects
and the master (and 10 more measured on a render: see **Mix check** below). Each check rests on a rule of thumb of production: muddy low
voicings, notes out of key or beyond a real instrument's range, robotic
velocities, loopitis, a bass panned off center, a limiter that isn't last.

- **Suggestions** apply with one click and undo with Ctrl+Z.
- **Issues** are for information, and in the native studio one click hands
  an issue to your agent.
- **Suppressing** one finding, or turning a check off, is saved in the
  project. The checks of classical theory (keys, counterpoint, singable
  melodies) start off; turn them on when the song wants them.

The same checks run on the command line: `rosaclef critic`, with `--fix`,
`--suppress` and `--json` for agents. See [`docs/critic.md`](docs/critic.md).

## Mix check

The **Mix check** tab in the Maestro panel measures the mix from one render,
like a mastering meter: integrated, short-term and momentary loudness against a
delivery target (Spotify, Apple Music, EBU R128, …), true peak, the peak before
the limiter and every compressor's gain reduction, PLR, LRA, phase correlation,
the loudness history, the spectrum against a level-matched reference track —
and, beyond any meter, how audible each part is under the others (a masking
model) and who masks it. Findings come with mixer fixes (faders, EQ, dynamics
— never the notes) to hear **Before / after** from the playhead or **Apply** (one
undo step).

The agent gets the same numbers as JSON: `rosaclef mixcheck --range 52:59
--focus rbass` (or `POST /api/mixcheck`). See [`docs/mixcheck.md`](docs/mixcheck.md).

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
- **Beatbox** — kicks (a "p", a low "b"/"boom"), toms (a hummed "dum"), snares
  ("pf", "k"), hats ("ts") and open hats (a long "tsss") become a drum loop on
  drum machine channels (existing ones are reused). A drum recording works too:
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

The analysis runs in Rust
(`crates/studio/src/transcribe.rs`, `GET /api/transcribe?path=…&mode=melody|drums`),
natively or in the browser-only build.

## Drums

The **Drums** tab in the bottom dock (F4) is one screen that always works on
a drum pattern — the one at the song cursor, or the clip selected in the
playlist — and says where it plays (the track, the bars, its time signature
and kit):

- **Grooves** (left): a library of 25 grooves (rock, pop, funk, soul,
  shuffle, jazz, hip-hop, house, techno, disco, drum & bass, reggae, Latin,
  country, metal, 3/4 and 6/8), the ones in the time signature at the cursor
  first. A click plays one in the pattern — or, where the song has no drums
  yet, makes a pattern there on a Drums track; **▶** lets you hear a groove
  first, looping, without changing the song.
- **The pattern** (middle): what it plays (groove A, the bigger B, hits, a
  count-in or rest), a fill at its end, a crash on its 1, a turnaround every
  4th bar, its length, kit, feel and swing; and its own notes as a step grid
  to click (a hit, an accent, a ghost note, a rest).
- **The song** (below): the drum clips along the bars — a click edits that
  clip's pattern, a click elsewhere moves the song cursor. In the target bar,
  − and + lengthen the clip at the cursor, **Copy** makes a variation that
  plays there instead, and the trash removes a pattern with its clips.
- **Song drummer** (folded underneath): the whole song's drums at once. The
  sections are guessed from the playlist; say what each plays, with a fill
  into the next and a crash on its first downbeat, then **Write drums** turns
  the part into patterns and clips, in one undoable step, with a turnaround
  every 4th bar and fills that do not repeat. Writing again keeps your hand
  edits, follows them into the groove's crash and fill bars, and moves them
  to a new kit. Where a pattern of your own plays — made in the tab, or one
  of the drummer's you took over by picking a groove for it — writing leaves
  those bars to it.

A pattern's recipe lives on the pattern (`drums` in `project.json`), and the
song drummer's part under the top-level `drums`, so the agent can edit both;
`rosaclef grooves` lists the library, `rosaclef drums` writes the part and
`rosaclef drums --pattern ID` makes one pattern again. The design and its
trade-offs are in [`docs/drums.md`](docs/drums.md).

## Instruments and the keys

The **browser** on the left is one searchable tree of everything that can
play: the song's channels (**In this song**), then each instrument with what
it holds — Grand Orchestra's General MIDI programs by family and its drum
kits, the drum machine's sounds, each synth's presets, your samples and any
CLAP plugins. Click one to try it on the keys (Z–/ and Q–[, or the
on-screen piano) without changing the song; double-click or **+** adds it as
a channel; **⇄** swaps it into the selected channel, keeping its notes, mixer
route and volume. Dragging it onto a channel row of the rack swaps it too,
and the inspector's **Instrument** choice changes the type in place. The
piano's header always names what the keys play — a channel, or *Trying …*
for an instrument from the browser.

The **channel rack** lists the channels and, for the selected pattern, each
one's part: a step sequencer (one square per 16th note, bars and beats
counted above) where every note fits a step at one pitch — drums mostly —
or a small picture of the notes that opens the piano roll.

## Sampled instruments

**Grand Orchestra** (`"type": "soundfont"`) plays sampled instruments: the 128
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

## Languages

The studio speaks English, Spanish, Portuguese (Brazil), French, German,
Italian, Japanese, Korean, Chinese (Simplified) and Russian: pick one with the
globe at the right end of the top bar (at first it follows the browser's
language). The translations are JSON files in `web/locales/`; see
[`docs/i18n.md`](docs/i18n.md) to add a text or a language.

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
| `crates/engine` | portable DSP engine: sequencer, instruments (analog, FM, additive, wavetable, granular, generative, transitions, drums, sampler, soundfont), effects, mixer, offline render |
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
| `web/locales/` | the interface's translations, one JSON file a language |
| `docs/` | architecture and notes |

## Development

```sh
cargo test --workspace                 # Rust tests (engine, validation, CLAP host)
./tools/build-wasm.sh                  # build web/engine/rosaclef.wasm and web/local/rosaclef-local.wasm (generated, not in git)
node web/test/tree.test.js             # UI tree tests (no browser needed)
node web/test/voice.test.js            # voice-to-notes logic (quantize, auto-tune, drums)
node web/test/film.test.js             # the film's director and camera
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

GPL-3.0-or-later. `web/vendor/xterm` is MIT (xterm.js). `web/fonts` holds
Bravura (SIL Open Font License). `web/soundfonts/gm` is MuseScore General
0.2 (MIT; see its [license](web/soundfonts/gm/LICENSE.md), and who recorded
each instrument in [`SOURCES.csv`](web/soundfonts/gm/SOURCES.csv)). The studio
shows this where the samples are used: the ⓘ button beside Grand Orchestra in
the browser, and the **Credits & license** line on a Grand Orchestra
channel, which names the source of the chosen instrument's samples.
`web/vendor/mp4-muxer` is MIT (mp4-muxer, by Vanilagy).
