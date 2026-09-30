# Rosaclef architecture

## Principles

1. **The project file is the interface.** Everything about a song lives in
   `project.json`. The UI, the engines and the agent all read and write that
   one document; nothing important hides in UI state. An agent needs no
   plugin API — it edits a file whose schema and semantics are documented in
   `AGENTS.md`, validates it with `rosaclef validate`, and the studio reacts.
2. **One engine, two hosts.** `crates/engine` is pure DSP (no I/O, no threads),
   so the same code runs natively and as WebAssembly in the browser.
3. **The browser is a control surface.** It renders, edits and plays, but the
   server owns the file on disk, runs the agent and does what browsers can't
   (audio devices, plugins, fast offline rendering).
4. **The server's logic is portable too.** What the server does without the
   operating system (project folders, the library, files, rendering) is
   written against a file system trait, so the static build runs the same
   code in a worker, on an in-memory tree kept in IndexedDB
   ([`static.md`](static.md)).

## The pieces

| layer | runs | role |
|---|---|---|
| `rosaclef-core` | native + wasm | model, device catalog (single source of truth for params), validation with JSON paths, schema, compact formatter |
| `rosaclef-engine` | native + wasm | sequencer, instruments, effects, mixer, offline render; `PluginHost` / `ExternalProcessor` traits for plugins |
| `rosaclef-wasm` | browser AudioWorklet | C-ABI wrapper around the engine (no JS glue) |
| `rosaclef-import` | native + wasm | LMMS and MIDI importers |
| `rosaclef-fs` | native + wasm | the `Fs` trait: `DiskFs`, and `MemFs` (lazily loaded blobs, a change journal for the host to persist) |
| `rosaclef-studio` | native + wasm | the server's portable logic on `Fs`: project folders, library, file manager, zip archives, agent guides, symphonia decoding, offline render |
| `rosaclef-clap` | native | CLAP plugin host implementing the engine's plugin traits |
| `rosaclef` (server) | native | HTTP/WebSocket server, file watcher, PTY agent terminal, cpal device I/O, recording, CLI — on `rosaclef-studio` with the disk |
| `rosaclef-local` | browser worker | the server's HTTP API and socket protocol on `rosaclef-studio` with a `MemFs`, plus the Rosaclef shell for the terminal (the static build) |
| `web/` | browser | the studio UI; `web/lib/backend.js` sends its requests to the server or to `rosaclef-local` |

### Project sync

- UI edits are debounced and sent as `{t: "put", project}` over `/ws`; the
  server validates, writes `project.json` atomically, broadcasts to other
  clients and updates the native engine.
- The server watches the folder. When `project.json` changes on disk (an
  agent edit), it validates it: valid → broadcast `{t: "project", origin:
  "disk"}` (the UI pushes an undo step, so Ctrl+Z reverts agent edits);
  invalid → `{t: "invalid", issues}` and `.rosaclef/status.json`, while the
  studio keeps playing the last valid version.
- The UI reports what the producer is looking at to `.rosaclef/context.json`
  so "make *this* pattern funkier" has a referent.

### Audio

- **Browser**: `web/engine/worklet.js` instantiates `rosaclef.wasm` inside an
  `AudioWorkletProcessor`; the project JSON is posted to it on change; samples
  are decoded with `decodeAudioData` and passed as float arrays; microphone
  input is captured in the worklet for recording.
- **Native**: the server runs the same engine in a cpal output callback
  (`crates/server/src/device.rs`), records from the default input, and renders
  mixdowns offline (`/api/render`, `rosaclef render`). CLAP plugins load here.

### Automation

- `project.automation` holds lanes of breakpoints (`beat`, `value`, optional
  `curve`) for one target each: `tempo`, `swing`, `channel/<id>/volume|pan|<param>`,
  `insert/<n>/volume|pan`, `insert/<n>/effect/<k>/<param>`. The target is parsed
  into `rosaclef_core::automation::AutomationTarget`, which also gives its value
  range (from the catalog); `value_at`/`shape` define the interpolation, and
  `web/src/automation.js` mirrors them so the UI shows what plays.
- The engine compiles lanes on `set_project` (targets → channel/insert/effect
  slots; parameter keys pre-inserted into per-device working copies, so the audio
  path does not allocate) and, in song mode, evaluates them every ≤ 64 frames.
  Tempo lanes drive the beat clock (evaluated at the block midpoint), audio
  clips follow a `TempoMap` (seconds as a function of beats), and tempo-synced
  devices are reconfigured when the tempo moves. Volumes and pans go through the
  gain ramps; device parameters call `set_device` only when a value moves by more
  than 1e-4 of its range. Stop, pause, pattern mode and lane removal restore the
  project values; offline renders use the tempo map for their length and keep
  the final automated values during the release tail.
- In the studio, lanes live below the playlist tracks (`web/src/ui/lanes.js`),
  sharing the playlist's zoom and scroll; controls bound to a target open an
  automation menu on right-click and show a gold dot when automated.

### Voice to notes

- `rosaclef_studio::transcribe` analyzes a take once: YIN pitch tracking on a
  ~16 kHz copy (10 ms frames) cut into notes at pitch changes, silences and
  level dips (melody); band-normalized SuperFlux onsets, each hit summed up
  by a *tone* (spectral centroid lowered by its share below 200 Hz) that sorts
  it into kick / snare / hat (beatbox). It returns raw seconds and fractional
  pitches (`GET /api/transcribe`, on the server and in `rosaclef-local`).
- `web/src/voice.js` turns that into notes on every redraw — quantizing,
  snapping to a scale (Krumhansl–Schmuckler key detection), sorting hits with
  the user's tone boundaries (`classify` mirrors the Rust one) — so the
  settings in the Voice dock (`web/src/ui/voice.js`) apply instantly.

### The UI library (`web/src/ui/tree.js`)

A small retained, reconciling tree in the spirit of
[fresh-ui](https://github.com/sinelaw/fresh/tree/master/crates/fresh-ui):

- **Descriptions** are immutable, flat values (each node records its parent
  index) rebuilt from state on every flush — one allocation per node.
- **Elements** persist across rebuilds, matched by their path of (type, key);
  they own the backend handle, the last-applied properties and the current
  handlers.
- **Backends** implement a handful of primitives on numeric handles: the DOM
  (`web/lib/platform.js`) and an in-memory test backend
  (`web/src/ui/memory.js`) used by `web/test/tree.test.js`.
- State flows down as arguments, events flow up as callbacks; any change calls
  `mark()` and the next animation frame rebuilds and reconciles everything.
- Dense editors (piano roll, playlist) compute one layout (rectangles) that
  both rendering and hit-testing read, and only emit nodes for what is visible.
  Canvas is used only for per-pixel content (waveforms, clip previews).
- The studio layout — which panels are minimized or maximized, the dock
  height, the agent width — lives in one place, `web/src/ui/panes.js`
  (persisted in `localStorage`); the shell turns it into grid columns and a
  flex basis, and CSS transitions animate the change.
- A window too small for the three columns (a phone, either way up) gets the
  compact layout: one view at a time — browser, playlist, dock or agent —
  picked from a bottom navigation bar (`web/src/ui/compact.css`). On a touch
  screen a finger on the playlist or piano roll scrolls it; a tap acts as a
  click (`pressOrTap` in `web/lib/platform.js`).
- The on-screen piano (`web/src/ui/keyboard.js`) plays the selected channel
  with the mouse or several fingers, and lights the keys the computer-keyboard
  piano (`web/src/keys.js`) plays.

### Types in the frontend

The UI is plain JavaScript type-checked by
[inty](https://sinelaw.github.io/inty/) — no transpilation. Global model and
UI types live in `web/types/globals.d.js`; integers with different meanings are
`nominal type`s (`InsertIx`, `TrackIx`, `NoteIx`, `Handle`, …) declared in the
same file, erased at runtime by identity casts (`web/lib/brands.js`).
`web/check.sh` checks every module in one run (about 2 s). The only unchecked code is the platform boundary
(`web/lib/platform.js`, typed by `web/types/platform.d.js`, and the back-end
switch `web/lib/backend.js` behind it), the AudioWorklet processor and the
static build's worker (`web/local/worker.js`).

## Agent integration

`rosaclef serve` writes `AGENTS.md` (plus `CLAUDE.md` / `GEMINI.md` importing
it) into the project folder: the data model, conventions, the device catalog
generated from the Rust tables, and the workflow (read context → edit →
validate → render to check levels). The agent terminal is a PTY started in the
project folder with `rosaclef` on its `PATH` and `ROSACLEF_URL` set; the
browser attaches with xterm.js over `/ws/term` (scrollback is replayed on
reconnect). All WebSocket and mutating HTTP endpoints reject cross-origin
requests, and the server binds to localhost by default.
