//! Driver traits for the checkout devices (spec 001b-1).
//!
//! Every device runs in its own worker task with a timeout; a hung device
//! never blocks the sale path (see `context/06-hardware-integrations.md`).
//! Real drivers plug in via specs 003/004 behind these same traits.
//! Scanner events arrive on a `tokio::sync::mpsc` channel (ADR-030).

use tokio::sync::mpsc;

use crate::error::HardwareError;
use crate::print_doc::PrintDoc;

/// An event from a barcode scanner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanEvent {
    /// A barcode was scanned. `code` is the raw scanned string.
    Barcode {
        /// The scanned code, as read.
        code: String,
    },
    /// The scanner disconnected. The worker retries per the 06 policy;
    /// the sale path is unaffected.
    Disconnected,
}

/// A receipt printer. `print` queues/renders a semantic `PrintDoc`;
/// the driver owns the `ESC/POS` rendering (spec 003).
pub trait ReceiptPrinter: Send {
    /// Print a receipt. Fails with [`HardwareError::Offline`] when the
    /// printer is not reachable — the caller then queues the print job
    /// (001b-2) instead of failing the sale.
    fn print(
        &mut self,
        doc: &PrintDoc,
    ) -> impl std::future::Future<Output = Result<(), HardwareError>> + Send;
}

/// A barcode scanner. Scans arrive as [`ScanEvent`]s on the channel
/// handed out by [`BarcodeScanner::take_receiver`]; the core's scanner
/// worker owns the receiver.
pub trait BarcodeScanner: Send {
    /// Hand out the scan-event receiver. Returns `None` if it was
    /// already taken — there is exactly one consumer (the scanner
    /// worker).
    fn take_receiver(&mut self) -> Option<mpsc::Receiver<ScanEvent>>;
}

/// A cash drawer.
pub trait CashDrawer: Send {
    /// Kick the drawer open. Fails with [`HardwareError::Offline`] when
    /// the drawer is not reachable.
    fn open(&mut self) -> impl std::future::Future<Output = Result<(), HardwareError>> + Send;
}
