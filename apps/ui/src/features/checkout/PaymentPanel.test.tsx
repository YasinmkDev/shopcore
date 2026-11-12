/**
 * PaymentPanel tests (spec 001c-2): tender entry, quick amounts,
 * live change due, Enter completes, Esc closes. Cash only — there is
 * no method selector. All money is integer minor units; the tender
 * input is a string until completion.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PaymentPanel, quickAmounts } from "./PaymentPanel";

function renderPanel(totalMinor = 5_232): {
  onComplete: ReturnType<typeof vi.fn>;
  onClose: ReturnType<typeof vi.fn>;
} {
  const onComplete = vi.fn();
  const onClose = vi.fn();
  render(
    <PaymentPanel
      totalMinor={totalMinor}
      currencyExponent={2}
      motionTier={1}
      busy={false}
      error={null}
      onComplete={onComplete}
      onClose={onClose}
    />,
  );
  return { onComplete, onClose };
}

function tenderInput(): HTMLElement {
  return screen.getByLabelText("Tendered");
}

describe("quickAmounts", () => {
  it("is the exact total plus the next 5 / 10 / 50 / 100 multiples", () => {
    expect(quickAmounts(5_232)).toEqual([5_232, 5_500, 6_000, 10_000]);
    expect(quickAmounts(10_000)).toEqual([10_000]);
    expect(quickAmounts(100)).toEqual([100, 500, 1_000, 5_000, 10_000]);
  });
});

describe("PaymentPanel", () => {
  it("shows the total due, the exact quick amount, and no change yet", () => {
    renderPanel();
    // Total due + remaining + the exact quick amount all read 52.32.
    expect(screen.getAllByText("52.32")).toHaveLength(3);
    expect(screen.getByText("Remaining")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Complete sale" })).toBeDisabled();
    expect(screen.getByText("Cash")).toBeInTheDocument();
  });

  it("completes for exact tender via the quick amount", () => {
    const { onComplete } = renderPanel();
    fireEvent.click(screen.getByRole("button", { name: "52.32" }));
    expect(tenderInput()).toHaveValue("52.32");
    expect(screen.getByText("Change due")).toBeInTheDocument();
    expect(screen.getByText("0.00")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Complete sale" }));
    expect(onComplete).toHaveBeenCalledWith(5_232);
  });

  it("shows live change for 60.00 tender and completes on Enter", () => {
    const { onComplete } = renderPanel();
    fireEvent.change(tenderInput(), { target: { value: "60" } });
    expect(screen.getByText("Change due")).toBeInTheDocument();
    expect(screen.getByText("7.68")).toBeInTheDocument();
    fireEvent.keyDown(tenderInput(), { key: "Enter" });
    expect(onComplete).toHaveBeenCalledWith(6_000);
  });

  it("shows the remaining amount and blocks completion under tender", () => {
    const { onComplete } = renderPanel();
    fireEvent.change(tenderInput(), { target: { value: "50" } });
    expect(screen.getByText("Remaining")).toBeInTheDocument();
    expect(screen.getByText("2.32")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Complete sale" }));
    expect(onComplete).not.toHaveBeenCalled();
  });

  it("takes tender from the numpad, and C / backspace edit it", () => {
    const { onComplete } = renderPanel();
    fireEvent.click(screen.getByRole("button", { name: "6" }));
    fireEvent.click(screen.getByRole("button", { name: "0" }));
    expect(tenderInput()).toHaveValue("60");
    fireEvent.click(screen.getByRole("button", { name: "⌫" }));
    expect(tenderInput()).toHaveValue("6");
    fireEvent.click(screen.getByRole("button", { name: "C" }));
    expect(tenderInput()).toHaveValue("");
    fireEvent.change(tenderInput(), { target: { value: "60.00" } });
    fireEvent.click(screen.getByRole("button", { name: "Complete sale" }));
    expect(onComplete).toHaveBeenCalledWith(6_000);
  });

  it("ignores a second dot and a third fraction digit", () => {
    renderPanel();
    fireEvent.change(tenderInput(), { target: { value: "60.5" } });
    fireEvent.change(tenderInput(), { target: { value: "60.55" } });
    expect(tenderInput()).toHaveValue("60.55");
    fireEvent.change(tenderInput(), { target: { value: "60.555" } });
    expect(tenderInput()).toHaveValue("60.55");
  });

  it("closes on Esc and on Cancel", () => {
    const { onClose } = renderPanel();
    fireEvent.keyDown(tenderInput(), { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("shows the completion error", () => {
    render(
      <PaymentPanel
        totalMinor={5_232}
        currencyExponent={2}
        motionTier={1}
        busy={false}
        error="Something went wrong. Try again."
        onComplete={() => undefined}
        onClose={() => undefined}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("Something went wrong. Try again.");
  });

  it("renders in RTL without dropping the tender input or the total", () => {
    document.documentElement.dir = "rtl";
    try {
      renderPanel();
      expect(tenderInput()).toBeInTheDocument();
      expect(screen.getAllByText("52.32")).toHaveLength(3);
    } finally {
      document.documentElement.dir = "ltr";
    }
  });

  it("renders at 150% text scaling", () => {
    document.documentElement.style.fontSize = "150%";
    try {
      renderPanel();
      expect(screen.getByText("Payment")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Complete sale" })).toBeInTheDocument();
    } finally {
      document.documentElement.style.fontSize = "";
    }
  });
});
