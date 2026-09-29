# Rosaclef

An opulent, FL Studio–inspired music workstation for the AI era — **work in progress**.

- **Rust core** (`crates/core`): the project model (`project.json`), device catalog, validation with JSON-path errors, JSON Schema generation.
- **Portable Rust audio engine** (`crates/engine`): sequencer, instruments (subtractive synth, FM, drum synth, sampler), effects, mixer and offline renderer. Pure DSP with no I/O, so it runs natively and compiles to WebAssembly for the browser (AudioWorklet).
- **Native server + CLI** (`crates/server`, binary `rosaclef`): project folder management, agent guides (`AGENTS.md` / `CLAUDE.md`), render/validate/summary tools. The web UI with its bring-your-own-agent terminal is in progress.
- **CLAP plugin hosting** (`crates/clap-host`, in progress).

```sh
cargo build --release
./target/release/rosaclef new my-song --demo
./target/release/rosaclef render my-song      # -> my-song/renders/velvet-hour.wav
```
