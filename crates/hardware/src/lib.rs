//! `ShopCore` hardware crate: driver traits, implementations, and mock
//! drivers for receipt printers, barcode scanners, cash drawers, scales,
//! customer displays, payment terminals, and label printers.
//!
//! Rules (see `context/06-hardware-integrations.md`): every device sits
//! behind a trait with a mock, runs in its own worker task with timeouts,
//! and can never block the sale path. Hardware failures are non-fatal —
//! the sale completes and the action queues.
//!
//! Spec 001b-1 lands the traits + mocks for the three checkout devices
//! (`ReceiptPrinter`, `BarcodeScanner`, `CashDrawer`) and the semantic
//! `PrintDoc`. Real drivers plug in via specs 003/004 with no change to
//! this code. Scanner events flow through a `tokio::sync::mpsc` channel
//! (ADR-030) — there is no `futures` / `tokio-stream` dependency.

pub mod error;
pub mod mock;
pub mod print_doc;
pub mod traits;

pub use error::HardwareError;
pub use mock::{MockDrawer, MockPrinter, MockScanner};
pub use print_doc::{PrintDoc, PrintLine, PrintPayment};
pub use traits::{BarcodeScanner, CashDrawer, ReceiptPrinter, ScanEvent};
