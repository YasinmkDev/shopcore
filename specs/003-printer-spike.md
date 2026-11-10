# Spec 003 — Receipt printer, RTL text, and cash drawer spike

Status: Draft (needs human approval)   Phase: 0   Depends on: 000
Needs hardware: an 80 mm ESC/POS thermal printer (USB or Ethernet) with a cash-drawer port; ideally also a 58 mm printer.

## Goal
Prove we can print a correct receipt (Latin + RTL, logo, QR, barcode), cut, and kick the drawer, reliably, without ever blocking the app.

## In scope
- `crates/hardware`: `ReceiptPrinter` trait (+ `MockPrinter`), `PrintDoc` model, ESC/POS encoder, transports: Windows spooler raw (WritePrinter), TCP 9100, and USB if the spooler path is not enough.
- Layout engine: columns, wrap, alignment, bold/size, divider, logo bitmap, QR, Code128/EAN barcode.
- **Bitmap text mode**: render lines as images (text shaping for Arabic/Urdu/Hindi etc.) then dither to 1-bit raster. Bundled OFL fonts (e.g., Noto Naskh Arabic, Noto Nastaliq Urdu, Noto Sans). Glyph cache for speed.
- Persistent print queue (survives restart) with retry/backoff and "DUPLICATE" reprint marker.
- Status polling: online, paper out, cover open (non-blocking).
- A debug screen in the app: "Print test receipt", "Open drawer", printer status, queue list.

## Out of scope
Receipt designer UI, real sale integration (spec 001 uses this), label printers, fiscal formats.

## Requirements
1. Test receipt prints on 80 mm (and 58 mm if available): header with logo, 20 lines of items (mixed Latin and RTL names), totals, QR, barcode, footer, partial cut.
2. RTL lines shape correctly (joined letters, right-aligned, mixed numerals/Latin inside RTL lines).
3. Drawer opens via printer pulse on command.
4. Print job for a 40-line receipt **starts printing in < 500 ms** from the call (measure bitmap render time separately; cache fonts and glyphs).
5. All device I/O async with timeouts; a hung or unplugged printer never blocks the caller (returns `PRINT_QUEUED`).
6. Unplug during print, replug, and the queue resumes without app restart; no duplicate receipts.
7. Mock printer simulates: slow, offline, out of paper, garbled response; contract tests run against mock.

## Data / security
Queue stored in local SQLite table `print_job` (id UUIDv7, payload, status, attempts, created_at). No customer PII in logs.

## Tests
ESC/POS encoder golden-byte tests; layout wrap/alignment tests; bitmap render determinism test (golden image hash); queue persistence/restart test; mock failure-mode tests; manual hardware checklist (below) with photos of printed output attached to the session.

## Manual hardware checklist (human runs, attaches results)
Print Latin receipt · print Arabic/Urdu receipt · print logo + QR + barcode (QR scans correctly with a phone) · cut · drawer kick · paper out → recovery · cable pull mid-print → recovery · 100 receipts in a row without a stall.

## Dependencies to propose
`windows` (spooler APIs), `serialport` only if needed, `rustybuzz`/`cosmic-text` (shaping), `fontdue` or `ab_glyph` (raster), `qrcode`, `image`. Justify each.

## Acceptance
All requirements met on the real printer; photos of output; timing numbers recorded; ADR for any transport/library choices; tracker updated.

## Risks
Printer code-page quirks; Urdu Nastaliq rendering complexity (fallback to Naskh if shaping is unacceptable, via ADR); driver differences between brands.
