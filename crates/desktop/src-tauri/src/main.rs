#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! `ShopCore` desktop entry point (Tauri 2 shell, Windows first).
//!
//! Thin by design (see `context/09-engineering-standards.md`): commands
//! deserialize, then call a use case in `shopcore-core`. No business logic
//! lives here. The Tauri main thread runs the window and IPC only and
//! never blocks.

use shopcore_core::ping as core_ping;
use shopcore_protocol::Pong;

/// Proves the UI → core round-trip. Not business logic.
#[tauri::command]
fn ping() -> Pong {
    core_ping()
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
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![ping])
        .run(tauri::generate_context!())
        .unwrap_or_else(|err| {
            eprintln!("failed to run ShopCore: {err}");
            std::process::exit(1);
        });
}
