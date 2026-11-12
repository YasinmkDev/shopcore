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
//!
//! `serde` derives (001b-2): the print queue stores the document as JSON
//! in `print_job.payload_json`, so the document must serialize.

/// One receipt, as content.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PrintDoc {
    /// Header lines (shop name, branch, address, receipt number, date).
    pub header: Vec<String>,
    /// The sold lines.
    pub lines: Vec<PrintLine>,
    /// Subtotal, minor units: sum of line grosses (shelf prices,
    /// before discounts and tax). For inclusive lines the gross
    /// already contains the tax, so this figure does NOT satisfy
    /// `subtotal - discounts + tax == total` — use
    /// `net_subtotal_minor` for that equation (see 02).
    pub subtotal_minor: i64,
    /// Net subtotal, minor units: sum of line nets after line
    /// discounts and bill-discount allocation (the domain's bill
    /// net). `net_subtotal + tax == total` in every tax mode
    /// (exclusive, inclusive, mixed).
    pub net_subtotal_minor: i64,
    /// Sum of line + bill discounts, minor units. Added in 001c-2
    /// so the receipt can show the 02 display rule (`displayed
    /// subtotal − discounts + tax == total`, with the displayed
    /// subtotal = `net_subtotal + discount`). `#[serde(default)]`
    /// keeps print jobs queued by older builds readable (they stored
    /// no discount — the field reads as 0).
    #[serde(default)]
    pub discount_minor: i64,
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
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PrintLine {
    /// Item/SKU display name.
    pub name: String,
    /// Quantity in milli-units.
    pub qty_milli: i64,
    /// Unit price at scale 10^4.
    pub unit_price_e4: i64,
    /// Line total, minor units (`net + tax`).
    pub line_total_minor: i64,
}

/// One payment on the receipt.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PrintPayment {
    /// Payment method display name (e.g. `"Cash"`).
    pub method: String,
    /// Amount tendered/applied, minor units.
    pub amount_minor: i64,
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]
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
            net_subtotal_minor: 250,
            discount_minor: 0,
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

    #[test]
    fn print_doc_without_discount_reads_as_zero() {
        // Backward compatibility (001c-2): print jobs queued by older
        // builds stored no `discount_minor` — the field defaults to 0
        // on deserialize, so the queued receipt still renders.
        let json = r#"{"header":["Shop"],"lines":[],"subtotal_minor":250,"net_subtotal_minor":250,"tax_minor":0,"total_minor":250,"payments":[],"footer":[],"code_payload":null}"#;
        let doc: PrintDoc = serde_json::from_str(json).expect("old payload parses");
        assert_eq!(doc.discount_minor, 0);
        assert_eq!(doc.total_minor, 250);
    }
}
