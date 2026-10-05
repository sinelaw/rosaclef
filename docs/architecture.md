# Rosaclef architecture

## Principles

1. **The project file is the interface.** Everything about a song lives in
   `project.json`. The UI, the engines and the agent all read and write that
   one document; nothing important hides in UI state. An agent needs no
   plugin API — it edits a file whose schema and semantics are documented in
   `AGENTS.md`, validates it with `rosaclef validate`, and the studio reacts.
2. **One engine, two hosts.** `crates/engine` is pure DSP (no I/O, no threads),
   so the same code runs natively and as WebAssembly in the browser. (Native
   offline renders opt into its `parallel` feature: see below.)
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
  input is captured in the worklet for recording. Soundfont presets
  (`soundfont` instruments) are fetched and decoded by a worker
  (`web/engine/fonts.js`, a second instance of the same module) and streamed
  to the worklet in acknowledged 512 kB chunks, so neither the page nor the
  audio thread does the heavy work.
- **Native**: the server runs the same engine in a cpal output callback
  (`crates/server/src/device.rs`), records from the default input, and renders
  mixdowns offline (`/api/render`, `rosaclef render`). CLAP plugins load here.
  Offline renders use every core (the engine's `parallel` feature,
  `crates/engine/src/crew.rs`): each block, the mixer inserts (each with the
  channels routed to it) play on spinning worker threads, and a soundfont's
  samples decode in parallel. Every sum keeps its order, so the audio is bit
  for bit what one thread makes (`crates/engine/tests/parallel.rs`).
  Soundfont presets load on a background thread (`crates/studio/src/fonts.rs`).
- **Soundfonts**: `crates/engine/src/soundfont.rs` reads SF2/SF3 files
  (generators, modulators, Ogg Vorbis samples) and resolves presets;
  `instruments/soundfont.rs` plays them (the SoundFont 2 voice model). The
  built-in General MIDI soundfont is split into an index and 1 MB pieces
  (`tools/split_soundfont.py`), so a song fetches only what it plays.

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

### The Critic

- `rosaclef_core::critic` lints a project with pure, deterministic checks (no
  AI): `critique(project, off)` returns findings. Each finding has a level, a
  stable key, the place it points at (notes of a pattern, a channel, an
  insert, a bar, a lane) and, for a suggestion, a fix. A fix is a list of
  JSON Patch operations on `project.json`. `apply_fixes` applies fixes one at
  a time, looking each up again first. Suppressions and the checks turned
  off live in the project (`critic`). The checks and their thresholds are
  catalogued in [`critic.md`](critic.md).
- `POST /api/critic` (the server, and `rosaclef-local` for the browser-only
  studio), `rosaclef critic` and the shell's `critic` all call it.
  `web/src/ui/critic.js` is the panel, a plugin tab of the Maestro panel. It
  covers the terminal, which stays mounted and keeps its size. It asks for
  findings 350 ms after an edit, never during a drag. It applies a fix by
  replacing the project inside `commit`, so a fix is one undo step.

### Forward compatibility

- Editing is strict: validation rejects unknown keys and devices. Playing is
  lenient. `rosaclef_core::compat::for_playback` drops what the JSON schema
  doesn't define, plays unknown instruments on a stand-in and bypasses unknown
  effects, so an engine older than the project still plays it. The
  WebAssembly engine and `rosaclef render` load projects this way, and the
  Critic lists each fallback as a warning.

### Voice to notes

- `rosaclef_studio::transcribe` analyzes a take once: YIN pitch tracking on a
  ~16 kHz copy (10 ms frames), confirmed by subharmonic summation, cut into
  syllables at silences and level dips and into notes by a dynamic-programming
  fit of whole semitones (a cost per note change, five detail levels, the
  singer's own tuning) (melody); SuperFlux onsets band by band (low, mid,
  high, each against its own typical hit), so one moment can hold several
  drums, read from how much its body, click, noise and hiss rose — with the
  spill between drums learned from the take's clear hits — into kick, tom,
  snare, hat and open hat (beatbox, `transcribe/drums.rs`; scored against
  beats played on the `drum` machine by `crates/studio/tests/drum_beats.rs`).
  It returns raw seconds and fractional pitches (`GET /api/transcribe`, on
  the server and in `rosaclef-local`).
- `web/src/voice.js` turns that into notes on every redraw — cropping
  (`cropTake`), quantizing, snapping to a scale (Krumhansl–Schmuckler key
  detection), keeping hits by strength — so the settings in the Voice dock
  (`web/src/ui/voice.js`) apply instantly.

### Sheet music

- `web/src/notation.js` writes a scope (the song, a playlist track or a
  pattern) down as notation: clips expanded into sounding notes that remember
  their pattern note, one staff per channel (a grand staff split at middle C;
  synthesized drums share a drum staff), the key (named in `project.score`, or
  Krumhansl–Schmuckler over the song), pitches spelled on the line of fifths,
  onsets quantized to the grid or — beat by beat — to triplets, one voice per
  staff, durations split into written values by meter rules (ties across beats
  and the middle of a 4/4 bar), accidentals per measure, beam groups,
  multi-measure rests.
- `web/src/engrave.js` lays it out in staff spaces with Bravura's metrics
  (`web/src/smufl.js`, generated by `tools/gen_smufl.py`): columns shared by
  all staves with duration-based springs and collision rods, optimal line
  breaking over the whole piece, justification by one stretch factor per
  system, stems, slope-limited beams snapped to quarter spaces, zig-zag
  accidental columns, ties, tuplet numbers, and staves spaced by their ink.
  Each system is a few filled paths and glyph runs per color, plus the
  noteheads and a time→x map for hit-testing and the playhead.
- `web/src/ui/score.js` renders the visible systems as SVG (the Bravura font
  itself draws the glyphs), caches the engraving by `state.edits`, and maps
  edits back to pattern notes. `project.score` (`Score` in
  `crates/core/src/model.rs`) holds the key, hidden channels and tracks,
  clefs and colored passages. A staff's name opens its part's menu (the
  channel in the rack, the piano roll, the mixer), and a part, or a passage
  of it, moves to another channel with `moveRole` (`web/src/model.js`). A
  note counts where the score writes it (its start on the grid). In the
  song, a pattern whose clips lie wholly in the passage is retagged in place
  (or copied, if it also plays elsewhere); a clip that runs past an edge
  gets a pattern of its own, written out note for note over the clip, so no
  clip is cut and the song sounds as before but for the part's instrument.
  The page is laid out to fit the view at the music's size; the zoom only
  magnifies it. `web/src/ink.js` holds the look
  of ink and paper: the SVG filter the engraving is drawn through (wet ink lit
  as a raised, glossy surface; dry ink with wicked edges and a pooled rim) and
  the paper's textures (tiles of noise).
- Repeats (`project.repeats`: start/end beats, `times`, `endings` with the
  passes that play them) are the song's form. `crates/core/src/form.rs`
  unrolls them into the performance order (spans of written beats); the
  engine plays span by span, jumping at each span's end, and the song's
  length in seconds follows the unrolled form. The score draws the repeat
  signs, "×3" counts and volta brackets; the playlist's ruler shows them too.
- `web/src/pdf.js` engraves the score again for a printed page, paginates it
  (spreading the systems of full pages) and writes PDF objects: the glyphs as
  forms drawn from Bravura's outlines (no embedded font), text in the
  standard Times faces. `downloadPdf` in `web/lib/platform.js` compresses the
  streams and writes the file. For a PDF as on screen, `pageSvg` draws each
  page as SVG on the paper of `ink.js` and through its filter;
  `downloadImagePdf` turns each into a JPEG and writes a page per image.

### The film

- `project.animation` (`Animation` in `crates/core/src/model.rs`) directs a
  film of the score: `mode` (auto or manual), the desk's `surface`, `energy`,
  `effects` and `shots` (song beats; channels or a role; a frame size; tilt,
  turn, offset and their values at the end; the transition into the shot).
  Validation names every mistake by its path; nothing in it plays.
- `web/src/film.js` is pure data: the score engraved for printed pages
  (`pdfLayout`) laid on a desk (`layDesk`); each staff's role (`findRoles`:
  drums and low single lines keep the rhythm, chords and held notes are the
  background, the busiest high line is the lead); the director (`autoScenes`:
  phrases cut where a part comes in or is left alone, each framed on what
  carries it, an opening over the desk and a close on the page); the shots
  painted over it in manual mode (`plan`); and the camera as a function of the
  song beat (`cameraAt`): the framed region follows the playhead along its
  system, glides to the next system in its last bar, drifts to the shot's `to`
  values, and moves in from the previous scene (glide, swoop, whip, cut).
  `performance` unrolls the repeats as `form.rs` does, for the video's time.
- `web/src/ui/film.js` prepares frames (`GlFrame`, plain data) and edits the
  shots. Pages are bitmaps: `pdf.js` draws a page (`pagePart`, on plain
  paper), the platform layer rasterizes them, one at a time, the ones in view
  first; where the camera comes closer, tiles of the page are laid over it
  (1024 pixels square, 2.5 to 80 pixels a point), each at the sharpness its
  own part of the picture calls for (through the renderer's camera: leaning
  back, the near part is closer than the far), the coarser first. They are
  kept while the music stays the same (editing shots re-plans the scenes only).
- `web/lib/filmgl.js` (WebGL 2, behind the platform boundary) draws a frame,
  kept light so it is fast with or without a GPU: the desk, soft page
  shadows, the pages — each bitmap under the paper's texture (its tooth,
  formation and grain, tiled as on the paper view, and its toned edges, laid
  over in the shader, so tiles are drawn on plain paper and cost little) and
  lit by the lamp, the notes playing lit up (their noteheads' ink glowing
  warm amber, a soft glow on the paper around: the glow effect, on by
  default) — then a last pass: vignette, warmth and grain. The frame says
  what the screen leaves out (the panel's *On screen*: Performance drops the
  effects, the paper's texture, the ink's filters (Firefox runs an SVG filter
  on the main thread as the image is drawn: a tile with them takes hundreds of
  milliseconds there), the last pass and half the pixels, and draws close-ups
  half as sharp; exports and stills draw everything). While the music plays
  or the camera glides, `filmLive` draws the canvas every animation frame at
  the live playhead, and the page around it is rebuilt ten times a second
  (and when the engine reports) rather than every frame. `renderStill` draws
  one frame offscreen (the camera button: a PNG); `encodeFilm` renders frame
  after frame offscreen and encodes them with WebCodecs (H.264/AAC, or
  VP9/Opus) into an MP4 (`web/vendor/mp4-muxer`), with the mixdown from
  `/api/render`. (Raised, glossy ink, a three.js renderer and a path tracer
  were tried on the branch `claude/film-heavy-rendering`.)

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
