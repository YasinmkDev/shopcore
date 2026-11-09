// Standard Tauri layout: Cargo.toml and tauri.conf.json both live in
// src-tauri/, so plain tauri_build::build() finds the config on its own.
// `try_build` + explicit exit keeps the workspace's unwrap/expect deny
// intact (build scripts are linted too).
fn main() {
    if let Err(error) = tauri_build::try_build(tauri_build::Attributes::new()) {
        eprintln!("failed to run tauri-build: {error:#}");
        std::process::exit(1);
    }
}
