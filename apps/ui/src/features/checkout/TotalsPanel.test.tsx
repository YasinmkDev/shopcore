/**
 * TotalsPanel tests (spec 001c-2): the displayed subtotal is the
 * PRE-TAX subtotal (`net_subtotal + discounts`, context/02), so the
 * numbers shown satisfy `subtotal − discounts + tax == total` in
 * every tax mode — including an inclusive line, where the gross
 * subtotal would break the equation.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { CartTotalsDto } from "@shopcore/protocol-ts";
import { TotalsPanel } from "./TotalsPanel";

function renderPanel(totals: CartTotalsDto): void {
  render(
    <TotalsPanel
      totals={totals}
      currencyExponent={2}
      motionTier={1}
      cancelArmed={false}
      onCancelClick={() => undefined}
      canPay
      onPayClick={() => undefined}
      onDiscountClick={() => undefined}
    />,
  );
}

describe("TotalsPanel", () => {
  it("shows the exclusive totals as-is", () => {
    renderPanel({
      subtotal_minor: 10_000,
      discount_minor: 0,
      net_subtotal_minor: 10_000,
      tax_minor: 1_800,
      total_minor: 11_800,
    });
    expect(screen.getByText("100.00")).toBeInTheDocument();
    expect(screen.getByText("18.00")).toBeInTheDocument();
    expect(screen.getByText("118.00")).toBeInTheDocument();
    // No discount row: the row label is a span; the Discount button
    // is not one.
    expect(screen.queryByText("Discount", { selector: "span" })).not.toBeInTheDocument();
  });

  it("shows the pre-tax subtotal for an inclusive line, not the gross", () => {
    // TST118: 118.00 inclusive — gross 118.00, net 100.00, tax 18.00,
    // total 118.00. A gross subtotal would read 118.00 − 0 + 18.00 =
    // 136.00 ≠ 118.00; the pre-tax subtotal reads 100.00.
    renderPanel({
      subtotal_minor: 11_800,
      discount_minor: 0,
      net_subtotal_minor: 10_000,
      tax_minor: 1_800,
      total_minor: 11_800,
    });
    const rows = screen.getAllByText("100.00");
    expect(rows).toHaveLength(1);
    expect(screen.getByText("118.00")).toBeInTheDocument();
  });

  it("shows net + discount as the subtotal on a mixed cart with a discount", () => {
    // Exclusive 100.00 + inclusive 118.00, 10.00 discount: net 190.00,
    // tax 34.20, total 224.20. Displayed subtotal 200.00, and
    // 200.00 − 10.00 + 34.20 == 224.20.
    renderPanel({
      subtotal_minor: 21_800,
      discount_minor: 1_000,
      net_subtotal_minor: 19_000,
      tax_minor: 3_420,
      total_minor: 22_420,
    });
    expect(screen.getByText("200.00")).toBeInTheDocument();
    expect(screen.getByText("−10.00")).toBeInTheDocument();
    expect(screen.getByText("34.20")).toBeInTheDocument();
    expect(screen.getByText("224.20")).toBeInTheDocument();
  });

  it("calls the pay and discount handlers, and disables them with an empty cart", () => {
    const onPayClick = vi.fn();
    const onDiscountClick = vi.fn();
    render(
      <TotalsPanel
        totals={{
          subtotal_minor: 0,
          discount_minor: 0,
          net_subtotal_minor: 0,
          tax_minor: 0,
          total_minor: 0,
        }}
        currencyExponent={2}
        motionTier={1}
        cancelArmed={false}
        onCancelClick={() => undefined}
        canPay={false}
        onPayClick={onPayClick}
        onDiscountClick={onDiscountClick}
      />,
    );
    expect(screen.getByRole("button", { name: "Pay" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Discount" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Pay" }));
    expect(onPayClick).not.toHaveBeenCalled();
  });

  it("enables Pay and Discount when the cart has lines", () => {
    const onPayClick = vi.fn();
    const onDiscountClick = vi.fn();
    render(
      <TotalsPanel
        totals={{
          subtotal_minor: 10_000,
          discount_minor: 0,
          net_subtotal_minor: 10_000,
          tax_minor: 1_800,
          total_minor: 11_800,
        }}
        currencyExponent={2}
        motionTier={1}
        cancelArmed={false}
        onCancelClick={() => undefined}
        canPay
        onPayClick={onPayClick}
        onDiscountClick={onDiscountClick}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Pay" }));
    expect(onPayClick).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Discount" }));
    expect(onDiscountClick).toHaveBeenCalledTimes(1);
  });
});
