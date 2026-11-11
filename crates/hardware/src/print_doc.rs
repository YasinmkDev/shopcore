//! The semantic receipt document (spec 001b-1, amendment 6).
//!
//! `PrintDoc` is CONTENT only — header lines, line items, totals,
//! payments, footer lines, and an optional QR/barcode payload. It
//! contains NO `ESC/POS` bytes and no printer-specific layout. Rendering
//! a `PrintDoc` to `ESC/POS` bytes is spec 003's job; 001b-2 only queues
//! a `PrintDoc`.
//!
//! Money is `i64` minor units; prices are `i64` at scale 10^4 (the
//! domain `UnitPrice`); quantities are milli-units (the domain `Qty`).
//! No floats anywhere.

/// One receipt, as content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintDoc {
    /// Header lines (shop name, branch, address, receipt number, date).
    pub header: Vec<String>,
    /// The sold lines.
    pub lines: Vec<PrintLine>,
    /// Subtotal, minor units (after discounts, before tax).
    pub subtotal_minor: i64,
    /// Total tax, minor units.
    pub tax_minor: i64,
    /// Grand total, minor units.
    pub total_minor: i64,
    /// How the sale was paid.
    pub payments: Vec<PrintPayment>,
    /// Footer lines (thank-you, return policy, tax registration).
    pub footer: Vec<String>,
    /// Optional QR/barcode payload (e.g. a receipt lookup code).
    /// Content only — never rendered bytes.
    pub code_payload: Option<String>,
}

/// One sold line on the receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintLine {
    /// Item/SKU display name.
    pub name: String,
    /// Quantity in milli-units.
    pub qty_milli: i64,
    /// Unit price at scale 10^4.
    pub unit_price_e4: i64,
    /// Line total, minor units (after line discount, before tax).
    pub line_total_minor: i64,
}

/// One payment on the receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintPayment {
    /// Payment method display name (e.g. `"Cash"`).
    pub method: String,
    /// Amount tendered/applied, minor units.
    pub amount_minor: i64,
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn print_doc_is_semantic_content_only() {
        let doc = PrintDoc {
            header: vec!["Shop".to_string()],
            lines: vec![PrintLine {
                name: "Milk 1L".to_string(),
                qty_milli: 1000,
                unit_price_e4: 2_500_000,
                line_total_minor: 250,
            }],
            subtotal_minor: 250,
            tax_minor: 0,
            total_minor: 250,
            payments: vec![PrintPayment {
                method: "Cash".to_string(),
                amount_minor: 250,
            }],
            footer: vec!["Thank you".to_string()],
            code_payload: Some("RCPT-123".to_string()),
        };
        // The document is plain strings + integers. There is no byte
        // buffer and no printer command anywhere in this type.
        assert_eq!(doc.lines[0].name, "Milk 1L");
        assert_eq!(doc.total_minor, 250);
        assert!(!format!("{doc:?}").contains('\x1b'));
    }
}
