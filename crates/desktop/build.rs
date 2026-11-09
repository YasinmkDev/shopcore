// The Tauri config lives in `src-tauri/tauri.conf.json` (repo layout in
// 03-architecture.md puts the app in crates/desktop, not in src-tauri
// itself), so the build script must point tauri-build at it explicitly.
// `config_path` is relative to this crate's directory; capabilities then
// resolve from `src-tauri/capabilities/` next to the config.
fn main() {
    let attributes = tauri_build::Attributes::new()
        .config_path("src-tauri/tauri.conf.json")
        .codegen(tauri_build::CodegenContext::new());
    if let Err(error) = tauri_build::try_build(attributes) {
        eprintln!("failed to run tauri-build: {error:#}");
        std::process::exit(1);
    }
}
