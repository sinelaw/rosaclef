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
| `rosaclef import-midi FILE.mid [--name N] [--library LIB]` | import a Standard MIDI File as a new project |
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
- **Beatbox** — kicks (a low "b"/"boom"), snares ("pf", "k") and hats ("ts")
  become a drum loop on Atelier kick / snare / hat channels (existing ones are
  reused). Set the grid, the sensitivity (ghost notes), where kicks end and
  hats begin (the preview shows each hit by its tone), accents, the loop length
  and how many times it repeats; click a hit to make it another drum.

Record silently (the first note starts the pattern) or play along with the
pattern or the song (the take keeps its place in time). Any recording in
`samples/` can be picked and analyzed again (**Analyze again** re-runs the
current one), and **Open a recording…** brings in an audio file from the
phone or computer. **Add to song** creates the pattern and a
playlist clip in one undoable step. The analysis runs in Rust
(`crates/studio/src/transcribe.rs`, `GET /api/transcribe?path=…&mode=melody|drums`),
natively or in the browser-only build.

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
playlist tracks.
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
| `docs/` | architecture and notes |

## Development

```sh
cargo test --workspace                 # Rust tests (engine, validation, CLAP host)
./tools/build-wasm.sh                  # rebuild web/engine/rosaclef.wasm and web/local/rosaclef-local.wasm
node web/test/tree.test.js             # UI tree tests (no browser needed)
node web/test/voice.test.js            # voice-to-notes logic (quantize, auto-tune, drums)
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

GPL-3.0-or-later. `web/vendor/xterm` is MIT (xterm.js).
