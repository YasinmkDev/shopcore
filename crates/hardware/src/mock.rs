//! Mock drivers for tests and the dev UI. The mocks implement the same
//! traits the real drivers will, so use cases can be tested with no
//! hardware attached.

use tokio::sync::mpsc;

use crate::error::HardwareError;
use crate::print_doc::PrintDoc;
use crate::traits::{BarcodeScanner, CashDrawer, ReceiptPrinter, ScanEvent};

/// A receipt printer that records every document it was asked to print.
/// `set_offline(true)` makes `print` fail with [`HardwareError::Offline`]
/// so the queueing path can be tested.
#[derive(Debug, Default)]
pub struct MockPrinter {
    printed: Vec<PrintDoc>,
    offline: bool,
}

impl MockPrinter {
    /// Create an online mock printer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The documents this printer was asked to print, in order.
    #[must_use]
    pub fn printed_docs(&self) -> &[PrintDoc] {
        &self.printed
    }

    /// Simulate the printer going offline / coming back.
    pub fn set_offline(&mut self, offline: bool) {
        self.offline = offline;
    }

    /// Whether the printer is currently offline (the status bar reads
    /// this; the mock starts online).
    #[must_use]
    pub fn is_offline(&self) -> bool {
        self.offline
    }
}

impl ReceiptPrinter for MockPrinter {
    #[allow(clippy::unused_async_trait_impl)]
    async fn print(&mut self, doc: &PrintDoc) -> Result<(), HardwareError> {
        if self.offline {
            return Err(HardwareError::Offline("mock printer offline".to_string()));
        }
        self.printed.push(doc.clone());
        Ok(())
    }
}

/// A barcode scanner that feeds scans through the `mpsc` channel (ADR-030).
/// Create it with [`MockScanner::new`], hand the receiver to the scanner
/// worker via [`BarcodeScanner::take_receiver`], then drive scans with
/// [`MockScanner::feed_scan`].
#[derive(Debug)]
pub struct MockScanner {
    sender: mpsc::Sender<ScanEvent>,
    receiver: Option<mpsc::Receiver<ScanEvent>>,
}

impl Default for MockScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl MockScanner {
    /// Create a mock scanner. The channel holds up to 64 queued scans.
    #[must_use]
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel(64);
        Self {
            sender,
            receiver: Some(receiver),
        }
    }

    /// Feed one scanned code into the channel.
    ///
    /// # Errors
    ///
    /// Fails with [`HardwareError::QueueFull`] if the worker has not
    /// drained the queue, or [`HardwareError::Driver`] if the receiver
    /// is gone.
    pub fn feed_scan(&self, code: &str) -> Result<(), HardwareError> {
        self.sender
            .try_send(ScanEvent::Barcode {
                code: code.to_string(),
            })
            .map_err(|err| match err {
                mpsc::error::TrySendError::Full(_) => HardwareError::QueueFull,
                mpsc::error::TrySendError::Closed(_) => {
                    HardwareError::Driver("scan receiver closed".to_string())
                }
            })
    }
}

impl BarcodeScanner for MockScanner {
    fn take_receiver(&mut self) -> Option<mpsc::Receiver<ScanEvent>> {
        self.receiver.take()
    }
}

/// A cash drawer that records whether it was kicked open.
#[derive(Debug, Default)]
pub struct MockDrawer {
    open_count: u32,
}

impl MockDrawer {
    /// Create a closed mock drawer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many times the drawer was kicked open.
    #[must_use]
    pub fn open_count(&self) -> u32 {
        self.open_count
    }
}

impl CashDrawer for MockDrawer {
    #[allow(clippy::unused_async_trait_impl)]
    async fn open(&mut self) -> Result<(), HardwareError> {
        self.open_count += 1;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_printer_records_docs_and_fails_offline() {
        let mut printer = MockPrinter::new();
        let doc = PrintDoc {
            header: vec!["Shop".to_string()],
            lines: vec![],
            subtotal_minor: 0,
            net_subtotal_minor: 0,
            discount_minor: 0,
            tax_minor: 0,
            total_minor: 0,
            payments: vec![],
            footer: vec![],
            code_payload: None,
        };
        printer.print(&doc).await.unwrap();
        assert_eq!(printer.printed_docs().len(), 1);
        printer.set_offline(true);
        assert!(matches!(
            printer.print(&doc).await,
            Err(HardwareError::Offline(_))
        ));
    }

    #[tokio::test]
    async fn mock_scanner_feeds_scans_through_mpsc() {
        let mut scanner = MockScanner::new();
        let mut rx = scanner.take_receiver().unwrap();
        // The receiver can only be taken once.
        assert!(scanner.take_receiver().is_none());
        scanner.feed_scan("1234567890123").unwrap();
        let event = rx.recv().await.unwrap();
        assert_eq!(
            event,
            ScanEvent::Barcode {
                code: "1234567890123".to_string()
            }
        );
    }

    #[tokio::test]
    async fn mock_drawer_counts_opens() {
        let mut drawer = MockDrawer::new();
        drawer.open().await.unwrap();
        drawer.open().await.unwrap();
        assert_eq!(drawer.open_count(), 2);
    }
}
