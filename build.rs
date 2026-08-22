//! Compile the bundled default config WASM from WAT at build time and emit it
//! so the binary embeds and loads a genuinely compiled `.wasm` at startup.
//! The compiled module lives on the cordis-rs WASM boundary (core kernel set 1:
//! `ctx_set`/`ctx_remove`/`ctx_read` only) — exactly like any user config.wasm,
//! so builtins share the same ABI (no privileged path).

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let wat_path = PathBuf::from("config.wat");
    println!("cargo:rerun-if-changed={}", wat_path.display());

    let wat = fs::read_to_string(&wat_path).expect("read config.wat");
    let wasm = wat::parse_str(&wat).expect("compile default config.wat to wasm");

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    fs::write(out.join("config.wasm"), &wasm).expect("write config.wasm");
}
