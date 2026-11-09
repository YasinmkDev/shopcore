//! `ShopCore` hardware crate: driver traits, implementations, and mock
//! drivers for receipt printers, barcode scanners, cash drawers, scales,
//! customer displays, payment terminals, and label printers.
//!
//! Rules (see `context/06-hardware-integrations.md`): every device sits
//! behind a trait with a mock, runs in its own worker task with timeouts,
//! and can never block the sale path. Hardware failures are non-fatal —
//! the sale completes and the action queues.
//!
//! Spec 000: this crate is empty. Driver traits and mocks land with the
//! Phase 0 hardware spike specs.
