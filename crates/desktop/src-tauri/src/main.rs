#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! `ShopCore` desktop entry point (Tauri 2 shell, Windows first).
//!
//! Thin by design (see `context/09-engineering-standards.md`): commands
//! deserialize, then call a use case in `shopcore-core`. No business logic
//! lives here. The Tauri main thread runs the window and IPC only and
//! never blocks.

use std::sync::OnceLock;
use std::time::Instant;

use shopcore_core::ping as core_ping;
use shopcore_protocol::Pong;

/// Process start, set in `main`. Read by the bench debug route (spec 002,
/// scenario 7: process start → window interactive).
static PROCESS_START: OnceLock<Instant> = OnceLock::new();

/// Proves the UI → core round-trip. Not business logic.
#[tauri::command]
fn ping() -> Pong {
    core_ping()
}

/// Debug-only: milliseconds since process start. Used by the bench debug
/// route (spec 002, scenario 7). Not a product feature; only the debug
/// route calls it. Present in release builds because the harness measures
/// release builds only.
#[tauri::command]
fn bench_startup_ms() -> f64 {
    PROCESS_START
        .get()
        .map_or(0.0, |start| start.elapsed().as_secs_f64() * 1_000.0)
}

/// Debug-only: write the UI bench results JSON to the OS temp dir so the
/// `just bench` runner can merge it (`--ui-results`). Returns the path.
/// Only the debug route calls it.
#[tauri::command]
fn bench_ui_results(payload: String) -> Result<String, String> {
    let path = std::env::temp_dir().join("shopcore-ui-results.json");
    std::fs::write(&path, payload).map_err(|err| err.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

// `generate_context!()` expands code generated from tauri.conf.json,
// which trips pedantic lints (manual_string_new, default_trait_access,
// single_match_else) and the workspace unwrap/expect denies. The allow
// is scoped to this function only; hand-written code elsewhere keeps
// the full deny set.
#[allow(
    clippy::manual_string_new,
    clippy::default_trait_access,
    clippy::single_match_else,
    clippy::unwrap_used,
    clippy::expect_used
)]
fn main() {
    let _ = PROCESS_START.set(Instant::now());
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            ping,
            bench_startup_ms,
            bench_ui_results
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|err| {
            eprintln!("failed to run ShopCore: {err}");
            std::process::exit(1);
        });
}
