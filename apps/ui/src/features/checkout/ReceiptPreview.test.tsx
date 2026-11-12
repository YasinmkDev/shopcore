/**
 * ReceiptPreview tests (spec 001c-2): the receipt renders the
 * pre-tax subtotal (02 rule), the discount and rounding rows only
 * when they apply, tender/change, and the print state with Reprint.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ReceiptDto } from "@shopcore/protocol-ts";
import { ReceiptPreview } from "./ReceiptPreview";

const RECEIPT: ReceiptDto = {
  receipt_no: "MAIN-T1-20261112-1",
  header: ["ShopCore Test Store"],
  lines: [
    { name: "Test 44", qty_milli: 1000, unit_price_e4: 443_400, line_total_minor: 4434 },
  ],
  subtotal_minor: 4434,
  net_subtotal_minor: 4434,
  discount_minor: 0,
  tax_minor: 798,
  total_minor: 5232,
  tender_minor: 6000,
  change_minor: 768,
  rounding_line_minor: 0,
  payments: [{ method: "cash", amount_minor: 6000 }],
  footer: ["Thank you"],
};

function renderPreview(receipt: ReceiptDto = RECEIPT, receiptQueued = false): void {
  render(
    <ReceiptPreview
      receipt={receipt}
      currencyExponent={2}
      receiptQueued={receiptQueued}
      reprintBusy={false}
      onReprint={() => undefined}
      onNewSale={() => undefined}
    />,
  );
}

describe("ReceiptPreview", () => {
  it("renders the lines, totals, tender and change", () => {
    renderPreview();
    expect(screen.getByText("MAIN-T1-20261112-1")).toBeInTheDocument();
    expect(screen.getByText("Test 44")).toBeInTheDocument();
    // Line total + pre-tax subtotal both read 44.34.
    expect(screen.getAllByText("44.34")).toHaveLength(2);
    expect(screen.getByText("52.32")).toBeInTheDocument();
    expect(screen.getByText("60.00")).toBeInTheDocument();
    expect(screen.getByText("7.68")).toBeInTheDocument();
    expect(screen.getByText("Receipt printed")).toBeInTheDocument();
    expect(screen.queryByText("Rounding")).not.toBeInTheDocument();
  });

  it("shows the pre-tax subtotal and the discount row when a discount applies", () => {
    renderPreview({
      ...RECEIPT,
      subtotal_minor: 10_000,
      net_subtotal_minor: 9_000,
      discount_minor: 1_000,
      tax_minor: 1_620,
      total_minor: 10_620,
      tender_minor: 12_000,
      change_minor: 1_380,
      lines: [{ name: "Milk 1L", qty_milli: 1000, unit_price_e4: 1_000_000, line_total_minor: 9_000 }],
    });
    // Pre-tax subtotal 100.00 (net 90.00 + discount 10.00); the line
    // also shows the unit price as "1 × 100.00".
    expect(screen.getByText("100.00")).toBeInTheDocument();
    expect(screen.getByText(/1 × 100\.00/)).toBeInTheDocument();
    expect(screen.getByText("−10.00")).toBeInTheDocument();
    expect(screen.getByText("106.20")).toBeInTheDocument();
    expect(screen.getByText("120.00")).toBeInTheDocument();
    expect(screen.getByText("13.80")).toBeInTheDocument();
  });

  it("shows the rounding row only when it is non-zero", () => {
    renderPreview({ ...RECEIPT, rounding_line_minor: -2, total_minor: 5230 });
    expect(screen.getByText("Rounding")).toBeInTheDocument();
    expect(screen.getByText("-0.02")).toBeInTheDocument();
  });

  it("shows the queued banner and calls reprint", () => {
    const onReprint = vi.fn();
    render(
      <ReceiptPreview
        receipt={RECEIPT}
        currencyExponent={2}
        receiptQueued
        reprintBusy={false}
        onReprint={onReprint}
        onNewSale={() => undefined}
      />,
    );
    expect(screen.getByText("Printer offline — receipt queued")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Reprint receipt" }));
    expect(onReprint).toHaveBeenCalledTimes(1);
  });

  it("calls New sale", () => {
    const onNewSale = vi.fn();
    render(
      <ReceiptPreview
        receipt={RECEIPT}
        currencyExponent={2}
        receiptQueued={false}
        reprintBusy={false}
        onReprint={() => undefined}
        onNewSale={onNewSale}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "New sale" }));
    expect(onNewSale).toHaveBeenCalledTimes(1);
  });
});
