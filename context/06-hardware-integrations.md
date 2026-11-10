# 06 — Hardware Integrations

Hardware is the riskiest unknown. Prove each device in Phase 0 spikes before building features on top. All hardware sits behind traits in `crates/hardware`, with a mock for every trait so tests never need a device.

## Isolation rules
- Each device runs in its own worker task with timeouts, retry policy, and automatic restart. A hung device never blocks the sale path.
- Hardware failures are non-fatal: the sale completes; the action queues (reprint, retry) and the status pill shows the problem with a one-tap fix.
- All device I/O is async, cancellable, and logged (without customer data).
- Device configuration is per terminal (stored in settings, synced as terminal-local).

## Traits (sketch; agent finalizes with an ADR)
```
ReceiptPrinter   { print(doc: PrintDoc) -> Result, status() -> PrinterStatus, open_drawer() }
BarcodeScanner   { events() -> tokio::sync::mpsc::Receiver<ScanEvent> }  // ADR-030: mpsc, not Stream — no futures/tokio-stream dep
CashDrawer       { open() }                      // usually via printer kick pulse
Scale            { read_weight() -> Result<Qty> } // stable-weight detection
CustomerDisplay  { show(lines) }
PaymentTerminal  { start(amount, ref) -> Result<PaymentResult>, cancel() }
LabelPrinter     { print(label: LabelDoc) }
```

## Receipt printers
- Targets: ESC/POS thermal printers (58 mm and 80 mm) over USB, serial/COM, Ethernet (TCP port 9100), and Windows spooler (raw mode).
- Render pipeline: receipt template → layout (columns, wrapping, alignment, QR/barcode, logo bitmap) → ESC/POS bytes. The same layout renders a PDF/HTML preview in the designer.
- Code pages and multi-language: Latin, Arabic/RTL, Urdu and other scripts often need **bitmap rendering** of text lines instead of printer fonts; support a "render text as image" mode per template.
- Paper cut, partial cut, drawer-kick pulse, feed, and print density are per-printer settings.
- Reprint queue persists across restarts. Receipt copies are marked "DUPLICATE" on reprint and logged in audit.
- Status polling (paper out, cover open, offline) with non-blocking checks.

## Barcode scanners
- Most are USB **keyboard-wedge**: they type characters fast and end with Enter. Detect a scan by inter-key timing (e.g., < 30 ms between keys, configurable) plus terminator and minimum length; capture globally on the sell screen regardless of focus, without stealing focus from real text inputs.
- Also support serial (COM) and HID POS scanners via the driver trait.
- Support EAN-13/8, UPC-A/E, Code 128, Code 39, QR, and **embedded-weight/price barcodes** (prefix-based formats, configurable) for scale-labeled items.
- Unknown barcode → inline "Add item?" flow, not an error dialog.

## Cash drawer
Kick via the receipt printer's drawer port (ESC/POS pulse) in v1; direct USB/serial drawers later. Open on cash tender, no-sale, and cash movements; every manual open requires permission and is audited.

## Scales
Serial/USB scales with stable-weight detection and tare; unit conversion via UoM. Weighted items prompt/read weight automatically; manual entry allowed only with permission when a scale is configured.

## Customer display
Secondary window on a second monitor (Tauri multi-window) or a serial pole display. Shows lines, total, change, and idle branding.

## Payment terminals
**Card data never touches our app** (keeps PCI scope minimal). Integrate terminals via their SDK/protocol as an external payment adapter: app sends amount + reference, terminal returns approved/declined + masked reference. Start with one integration chosen by market; manual "card (external terminal)" tender with a reference field is the v1 baseline. Wallet/QR payments follow the same adapter pattern.

## Label printers
ZPL/TSPL/EPL via raw spooler or TCP. Label designer produces templates with fields (name, barcode, price, expiry); batch printing from catalog/GRN.

## Phase 0 hardware spikes (must pass before feature work)
1. Print a Latin + RTL receipt with logo, QR and barcode on a real 80 mm printer; cut; kick drawer.
2. Read a keyboard-wedge scanner with correct scan-vs-typing discrimination under fast typing.
3. Read a stable weight from a scale (if targeted).
4. Run all three while the DB write path is under load: checkout latency stays in budget.
5. Unplug/replug each device mid-operation; app recovers without restart.

## Test strategy
Mock drivers simulate: slow, offline, out of paper, garbled, duplicate scans. Contract tests run the same suite against mock and (optionally, on a hardware bench machine) real drivers.
