# The browser-only studio

Rosaclef also ships as a folder of static files: no server to install, no
Rust toolchain for the person using it. Any static host can serve it (GitHub
Pages, Netlify, an S3 bucket, `python3 -m http.server`), at a domain's root
or under a sub-path.

```sh
tools/build-static.sh            # → dist/ (rebuilds the WebAssembly modules first)
python3 -m http.server -d dist 8080
# open http://localhost:8080
```

Projects are saved in the browser's storage (IndexedDB). On the first visit
the library holds a copy of the demo song.

## What works

Almost all of the studio. The server's own code runs in the page, compiled to
WebAssembly, so the browser-only studio does what the native server does and
behaves the same way:

| | |
|---|---|
| editing | everything: rack, piano roll, playlist, mixer, automation, undo/redo |
| playback | the engine in an AudioWorklet (the same as the native studio's *Browser* output) |
| saving | every edit is written to the browser's storage, as the server writes `project.json` |
| library | create, open, duplicate, rename, delete (to a trash you can empty), filter |
| files | import audio into `samples/`, rename (the song follows), delete, listen, download |
| import | LMMS projects (`.mmp`, `.mmpz`), MIDI files (as a new project, or added to the open song) |
| sampled instruments | the General MIDI soundfont: each program's pieces are fetched from `soundfonts/` the first time a song plays it (the browser caches them) |
| backup | download any project as a `.zip`; import a `.zip` as a new project |
| export | the **Export** window: render the song offline to a WAV (16-bit, 24-bit or 32-bit float; 44.1 or 48 kHz; saved in `renders/` and downloaded), or download the project's `project.json` or the whole project as a `.zip` |
| address | the page's address names the open project (`#project=<name>`): reload, bookmark or share it to open that project again; Back returns to the project before |
| recording | from the microphone onto a playlist track |
| waveforms | audio clips show their waveform |
| terminal | the **Rosaclef shell**: `summary`, `validate`, `get` / `set` / `del` on the song, `render`, `note`, `catalog`, `presets`, `ls`, `cat`, `open`, … |
| several tabs | they share one back end (a SharedWorker), like several windows on one server: an edit in one tab shows in the others |

A zip downloaded from the browser opens in the native studio (Projects →
Import…) and the other way round — the way to move a song to a machine with
your coding agent, or to back it up.

## What needs the native studio

| | why |
|---|---|
| coding agents (Claude Code, Codex, Gemini CLI, …) | they are programs that run in a terminal on your machine; a web page cannot start them. The browser's terminal runs the Rosaclef shell instead. |
| the *Studio* audio output | playing through the machine's audio device with the lowest latency needs a native process |
| CLAP plugins | plugins are native libraries |
| samples referenced by an LMMS project | the importer cannot reach files on your disk from the page; import the `.mmpz`, then import the samples into `samples/` |

## How it works

```
┌─────────────────────────── page ───────────────────────────┐
│ studio UI (web/src) ── platform.js ── backend.js           │
│                                          │ requests, /ws,   │
│ AudioWorklet ── rosaclef.wasm (engine)   │ /ws/term          │
└──────────────────────────────────────────┼──────────────────┘
┌──────────── SharedWorker (web/local/worker.js) ─────────────┐
│ rosaclef-local.wasm: the server's API over an in-memory     │
│ file tree (crates/local → crates/studio → crates/fs)        │
│ persists every change to IndexedDB                          │
└─────────────────────────────────────────────────────────────┘
```

- **The same UI.** `web/lib/backend.js` decides where requests go: the
  static build carries `<meta name="rosaclef-backend" content="local">`;
  otherwise the page asks `api/info` and falls back to the browser back end
  when no server answers. `?backend=local` forces it, e.g. to try the
  browser-only studio on a running server. `platform.js` routes `getJson`,
  uploads, the two WebSockets and `/files/...` URLs through it, so the
  application code does not know which back end it talks to.
- **The same server code.** Everything the server does that needs no
  operating system lives in `crates/studio` (project folders, the library, the
  file manager, zip archives, the agent guides, audio decoding with
  symphonia, offline rendering), written against the file system trait of
  `crates/fs`. The native server uses it on the disk; `crates/local` uses it
  on a `MemFs` and adds the API routes and the socket protocol. Its tests
  (`crates/local/tests/backend.rs`) drive it the way the worker does.
- **Storage.** The worker keeps two IndexedDB stores: the file tree (path →
  size, time, blob id) and the blobs. Renames and copies touch only the tree.
  Audio is loaded into the module only when something needs its samples (a
  render, a waveform) and dropped after; files are served to the page straight
  from storage. The page asks the browser to keep the storage persistent;
  the Projects window shows how much is used.
- **Soundfonts.** `soundfonts/` is part of the site. The page's font worker
  (`engine/fonts.js`) fetches the pieces a preset needs and decodes them for
  the audio worklet; a render in the back-end worker asks for the same files
  (`needFonts` in its replies) and the worker fetches them from the site.
- **One back end for every tab.** A SharedWorker, where the browser has one;
  otherwise (Chrome on Android) each tab has its own worker, and tabs do not
  see each other's edits until reloaded.

Storage belongs to the site's origin: the same studio on another domain (or
another browser) has its own library. Clearing the site's data deletes the
projects — download the ones you care about.

## Deploying

`tools/build-static.sh DIR` writes the site into `DIR` (default `dist/`).
It needs a Rust toolchain with the `wasm32-unknown-unknown` target: it builds
both WebAssembly modules, which are generated and not kept in git. With
`SKIP_WASM=1` it uses the ones already built in `web/engine` and `web/local`
(by `tools/build-wasm.sh`, or an earlier build). The GitHub Pages workflow
builds them on every deploy. Serve the folder as it is: all URLs are relative, and
`.nojekyll` keeps GitHub Pages from filtering files.

### GitHub Pages

`.github/workflows/pages.yml` builds the site from source and publishes it on
every push to `main` (or by hand: Actions → *Deploy the browser-only studio*
→ Run workflow). Turn it on once in the repository's Settings → Pages →
Source: **GitHub Actions**. The studio is then at
`https://<owner>.github.io/<repository>/`.

To check a build end to end in a real browser (Playwright):

```sh
python3 -m http.server -d dist 8765 &
node web/test/static-smoke.mjs http://localhost:8765/
```
