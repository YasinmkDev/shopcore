# 13 — Progress Tracker (update at the end of every session)

## Current phase
Phase 0 — Foundations and spikes

## Status board
| Spec | Title | Status | Notes |
|---|---|---|---|
| 000 | Repo scaffold + CI + `just check` | Done | Implemented 2026-11-09; `just check` green (local Linux); CI is windows-latest (not run — no repo/remote yet) |
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

## Known issues / tech debt
(none yet)

## Learnings from pilot
(none yet)
