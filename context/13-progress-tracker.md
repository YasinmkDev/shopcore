# 13 — Progress Tracker (update at the end of every session)

## Current phase
Phase 0 — Foundations and spikes

## Status board
| Spec | Title | Status | Notes |
|---|---|---|---|
| 000 | Repo scaffold + CI + `just check` | Done | Implemented 2026-11-09; `just check` green (local Linux + Windows CI). Repo: github.com/YasinmkDev/shopcore (public, main). Windows exe delivered 2026-11-10 (GoFile https://gofile.io/d/rrHlhKea). Manual Ping click-through PASSED on the user's Windows 10 (screenshot: Online pill, Ping → "Core replied: pong", Ready footer). |
| 000b | ui-kit: Button, TextField + dark-theme wiring | Spec drafted | `specs/000b-ui-kit-button-textfield-dark.md` (drafted 2026-11-09, needs human approval before implementation) |
| 001 | Checkout: scan → cart → cash sale → receipt | Spec drafted | `specs/001-checkout-sale.md` |
| — | Hardware spikes (printer, scanner, drawer) | Not started | see 06 |
| — | Benchmark on weakest PC | Not started | fill table in 08 |
| — | Two-node sync spike | Not started | |

Status values: Not started · Spec drafted · Spec approved · In progress · In review · Done · Blocked.

## Decisions pending (need human)
- Launch markets, tax/fiscal rules, currencies, languages
- First payment terminal
- Weakest supported PC (replace assumption ADR-011)

## Session log (newest first)
| Date | What was done | Tests | Next |
|---|---|---|---|
| 2026-11-09 | Spec 000 implemented: Cargo + pnpm workspaces, rust-toolchain 1.99.1, justfile, CI (windows-latest), lint/format/deny configs, Tauri shell (white theme, StatusPill, ping round-trip), gen-types via ts-rs. ADRs 013–021 recorded. | `just check` green: fmt, clippy -D warnings (workspace), cargo deny (all ok), nextest 2 passed, tsc x4, eslint x3, vitest 3+4 passed. gen-types freshness verified (identical md5). Xvfb boot smoke: app stayed alive 20s, no panic. | Spec 000b (Button/TextField + dark-theme wiring) or spec 001 (checkout) after Phase 0 printer/scanner spikes. Manual Ping click-through still to do on Windows. |
| 2026-11-10 | Spec compliance re-verified FR1–10 + pins + tokens + file tree; `just check` re-run green. Fixed real gaps found: (1) desktop crate moved to standard Tauri layout (`crates/desktop/src-tauri` crate root — the CLI requires `src-tauri/Cargo.toml`, `just dev`/`just build` could never run); (2) `frontendDist` corrected to `../../../apps/ui/dist` (old value resolved by accident; old debug binary may have embedded no frontend); (3) `just dev`/`just build` now call the Tauri CLI binary directly (pnpm exec re-chroots to apps/ui). Pushed to github.com/YasinmkDev/shopcore (public). CI fixes across 5 runs: dtolnay/rust-toolchain (rust-lang action doesn't exist), OPENSSL_SRC_PERL pin to Strawberry Perl (ADR-022, R2 triggered), `icons/icon.ico` added (tauri-build needs it on Windows), `.ico` added to `bundle.icon` (bundler needs it). Windows `check` job green; `build-windows` green — exe + NSIS + MSI uploaded as artifact `shopcore-windows-0.1.0`, downloaded and verified (PE x64, MSI OLE, embedded frontend). Delivered via GoFile https://gofile.io/d/rrHlhKea (verified 200). | `just check` green locally (Linux) and on Windows CI. Local `just build` (Linux) exit 0; embedded assets verified (Brotli blobs decompress to the exact dist files, incl. "Core replied"). Release binary boots 20s under Xvfb, no panic. Windows exe verified as PE32+ x64, 8.5 MB. | Manual Ping click-through PASSED on the user's Windows 10 (2026-11-10, screenshot). User asked why the UI is all blank white — answered: spec 000 is the scaffold only, the real shop UI comes with 000b/001. Next: his pick, 000b or 001. |

## Known issues / tech debt
(none yet)

## Learnings from pilot
(none yet)
