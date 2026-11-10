# Spec 004 — Barcode scanner capture and barcode parsing spike

Status: Draft (needs human approval)   Phase: 0   Depends on: 000
Needs hardware: a USB barcode scanner in keyboard-wedge mode (ideally also one serial/HID scanner).

## Goal
Reliably tell a scanner burst from human typing, on the sell screen, without stealing focus, and parse all supported barcode formats in the pure `domain` crate.

## In scope
- `ScanCapture` in `apps/ui/src/features/sell/` (hook + tiny service): buffers key events, detects a scan by inter-key timing (default < 30 ms, configurable), terminator (Enter/Tab, configurable), min length (default 4), optional prefix/suffix. Emits `ScanEvent { code, source, at }`. Works whether focus is on the scan input or elsewhere on the sell screen; never breaks normal typing in other fields. `packages/ui-kit` keeps only the `ScanInput` presentational component (registry); capture behaviour does not belong in ui-kit.
- Layout safety: handle scanners while a non-Latin keyboard layout is active (use `event.code`/layout-independent mapping for digits; document behavior and test with Arabic layout).
- `crates/domain::barcode`: pure parser/validator: EAN-13, EAN-8, UPC-A, UPC-E (expand), Code 128/39 passthrough, QR passthrough, check-digit validation, **embedded weight/price barcodes** (configurable prefixes, e.g., 2x), returning `Parsed { kind, base_code, embedded: Option<Weight|Price> }`.
- `hardware::BarcodeScanner` trait + `MockScanner`; serial/HID implementation only if hardware is available.
- Debug screen: live scan log with inter-key timings, classification (scan/typing), and settings.

## Out of scope
Item lookup, cart integration (spec 001).

## Requirements
1. Scan-to-event latency < 20 ms after the terminator arrives.
2. Zero false positives for human typing, including fast typists (test with 3 people typing digits at full speed); zero false negatives on 1,000 real scans (including long Code128).
3. A scan does not insert stray characters into focused inputs (clear or redirect per policy), and does not trigger shortcuts (F-keys, `*`, +/−).
4. Works with prefix/suffix characters and different terminators; configurable per terminal.
5. Parser golden tests cover valid/invalid check digits, UPC-E expansion, embedded weight/price examples, and garbage input (never panics).
6. Scan while the DB write path is under load (spec 002 scenario 5 setup: background writer inserting batches of 200 rows); scan-to-event latency stays in budget (`06`, spike 4).

## Data changes
None. No new tables; the parser is pure `domain` code and `ScanCapture` holds only in-memory buffers.

## Security / audit
Scanned codes are not PII; do not log full barcodes beyond debug output.

## Definition of done
Standard checklist in `AGENTS.md` + all acceptance criteria above met on a real scanner.

## Tests
Vitest with fake timers and recorded key-stream fixtures (scanner-speed 5–15 ms, human-speed 80–250 ms, mixed); property tests for the parser (valid codes always validate; any single-digit corruption fails check digit); component test that a scan on the sell screen does not alter other inputs. Manual protocol: scan 1,000 codes from a printed sheet, record misses; type fast for 2 minutes, record false triggers; repeat with Arabic keyboard layout active.

## Dependencies
None expected beyond existing UI stack; `serialport`/`hidapi` only if a serial/HID scanner is tested.

## Acceptance
Requirements met on a real scanner; recorded stats (misses/false triggers) in the session summary; parser tests green; ADR for the default timing thresholds.

## Risks
Scanners configured with unusual speeds or inter-character delays; IME/layout issues; browser focus behavior in WebView2.
