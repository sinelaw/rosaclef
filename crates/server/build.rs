//! Builds the browser's audio engine, `web/engine/rosaclef.wasm` (crates/wasm
//! for wasm32-unknown-unknown), which the studio's page runs in an
//! AudioWorklet. It is generated, not kept in git, so `cargo build` and
//! `cargo run` make it whenever the engine's code changes.
//!
//! It needs the WebAssembly target (`rustup target add
//! wasm32-unknown-unknown`); without it the server still builds, with a
//! warning, and the browser's audio output does not start. Set
//! `ROSACLEF_SKIP_WASM=1` to skip it. The browser-only build
//! (tools/build-static.sh) makes both modules itself.

use std::path::{Path, PathBuf};
use std::process::Command;

const TARGET: &str = "wasm32-unknown-unknown";

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for dep in ["crates/wasm", "crates/engine", "crates/core", "Cargo.lock"] {
        println!("cargo:rerun-if-changed={}", root.join(dep).display());
    }
    println!("cargo:rerun-if-env-changed=ROSACLEF_SKIP_WASM");
    if std::env::var_os("ROSACLEF_SKIP_WASM").is_some() {
        return;
    }
    if let Err(e) = build(&root) {
        println!("cargo:warning=web/engine/rosaclef.wasm was not built: {e}");
        println!(
            "cargo:warning=The studio's browser audio needs it: rustup target add {TARGET} (or set ROSACLEF_SKIP_WASM=1)"
        );
    }
}

fn build(root: &Path) -> Result<(), String> {
    // A target directory of its own: the outer build holds the lock on the usual one.
    let target_dir = root.join("target").join("web-wasm");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .current_dir(root)
        .args([
            "build",
            "-p",
            "rosaclef-wasm",
            "--target",
            TARGET,
            "--profile",
            "wasm",
        ])
        .env("CARGO_TARGET_DIR", &target_dir)
        // The host build's flags are not for WebAssembly.
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .output()
        .map_err(|e| format!("could not run cargo: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err
            .lines()
            .rev()
            .find(|l| l.contains("error"))
            .unwrap_or("cargo failed");
        return Err(last.trim().to_string());
    }
    let built = target_dir
        .join(TARGET)
        .join("wasm")
        .join("rosaclef_wasm.wasm");
    let bytes = std::fs::read(&built).map_err(|e| format!("{}: {e}", built.display()))?;
    let dest = root.join("web").join("engine").join("rosaclef.wasm");
    // Written only when it changed, so the page's file watchers see no needless change.
    if std::fs::read(&dest).ok().as_deref() != Some(bytes.as_slice()) {
        std::fs::write(&dest, &bytes).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    Ok(())
}
