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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    /// `#[tauri::command]` fn names in this file (`main.rs`).
    fn rust_commands() -> BTreeSet<String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");
        let text = std::fs::read_to_string(&path).expect("read main.rs");
        let mut out = BTreeSet::new();
        let mut lines = text.lines().peekable();
        while let Some(line) = lines.next() {
            if line.trim() == "#[tauri::command]" {
                for next in lines.by_ref() {
                    let next = next.trim();
                    if next.is_empty() || next.starts_with("///") || next.starts_with("//") {
                        continue;
                    }
                    if let Some(rest) = next.strip_prefix("fn ") {
                        let name: String = rest.chars().take_while(|c| *c != '(').collect();
                        out.insert(name.trim().to_string());
                    }
                    break;
                }
            }
        }
        out
    }

    /// `invoke("name")` / `invoke<T>("name")` string literals in every
    /// `.ts` / `.tsx` file under `apps/ui/src`. No regex crate: a small
    /// byte scanner that skips the optional `<...>` generic after
    /// `invoke`. Byte-based on purpose: the sources contain multi-byte
    /// UTF-8 (e.g. `…` in comments), so char-index slicing would panic.
    /// Indexing-free (`.get()` only): the workspace denies
    /// `clippy::indexing_slicing`.
    fn ts_invokes() -> BTreeSet<String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../apps/ui/src");
        let mut files = Vec::new();
        collect_ts_files(&root, &mut files);
        let mut out = BTreeSet::new();
        for file in files {
            let text = std::fs::read_to_string(&file).expect("read TS file");
            let bytes = text.as_bytes();
            let mut i = 0;
            while i + 6 <= bytes.len() {
                if bytes.get(i..i + 6).is_some_and(|s| s == b"invoke") {
                    let mut j = i + 6;
                    // Skip whitespace and an optional <...> generic.
                    while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
                        j += 1;
                    }
                    if bytes.get(j) == Some(&b'<') {
                        while bytes.get(j).is_some_and(|b| *b != b'>') {
                            j += 1;
                        }
                        j += 1;
                    }
                    while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
                        j += 1;
                    }
                    if bytes.get(j) == Some(&b'(') {
                        j += 1;
                        while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
                            j += 1;
                        }
                        if bytes.get(j).is_some_and(|b| *b == b'"' || *b == b'\'') {
                            let quote = bytes.get(j).copied().unwrap_or(b'"');
                            j += 1;
                            let start = j;
                            while bytes.get(j).is_some_and(|b| *b != quote) {
                                j += 1;
                            }
                            if let Some(name) = bytes.get(start..j) {
                                out.insert(String::from_utf8_lossy(name).into_owned());
                            }
                        }
                    }
                    i = j.max(i + 1);
                } else {
                    i += 1;
                }
            }
        }
        out
    }

    fn collect_ts_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_ts_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "ts" || e == "tsx") {
                out.push(path);
            }
        }
    }

    /// ADR-013 drift guard: every Rust `#[tauri::command]` must have a
    /// matching `invoke("name")` in the UI, and every `invoke` in the
    /// UI must have a matching Rust command. A rename on one side
    /// without the other breaks the call at runtime with no compile
    /// error, so this test is the only thing that catches it.
    /// (The bench commands are invoked directly from `BenchDebug.tsx`,
    /// not through `TauriClient`, so the scan covers all of
    /// `apps/ui/src`, not just the client.)
    #[test]
    fn tauri_commands_match_ui_invokes() {
        let rust = rust_commands();
        let ts = ts_invokes();
        assert!(
            !rust.is_empty(),
            "no #[tauri::command] found in main.rs — the scan is broken"
        );
        assert!(
            !ts.is_empty(),
            "no invoke() found under apps/ui/src — the scan is broken"
        );
        let missing_in_ui: Vec<_> = rust.difference(&ts).collect();
        let missing_in_rust: Vec<_> = ts.difference(&rust).collect();
        assert!(
            missing_in_ui.is_empty() && missing_in_rust.is_empty(),
            "Tauri command / UI invoke drift: commands with no UI invoke: {missing_in_ui:?}; \
             UI invokes with no Rust command: {missing_in_rust:?}"
        );
    }
}
