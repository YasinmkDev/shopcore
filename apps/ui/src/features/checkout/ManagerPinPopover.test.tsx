/**
 * ManagerPinPopover tests (spec 001c-2): percent / fixed value entry,
 * the first attempt without an approver, and the manager username +
 * PIN fields once approval is needed.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ManagerPinPopover } from "./ManagerPinPopover";

function renderPopover(needsApproval = false): ReturnType<typeof vi.fn> {
  const onApply = vi.fn();
  render(
    <ManagerPinPopover
      lineName="Milk 1L"
      currencyExponent={2}
      needsApproval={needsApproval}
      busy={false}
      onApply={onApply}
      onClose={() => undefined}
    />,
  );
  return onApply;
}

describe("ManagerPinPopover", () => {
  it("applies a percent discount without a manager on the first attempt", () => {
    const onApply = renderPopover();
    expect(screen.getByText("Milk 1L")).toBeInTheDocument();
    expect(screen.queryByLabelText("Manager username")).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Percent off"), { target: { value: "5" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply discount" }));
    expect(onApply).toHaveBeenCalledWith("percent", 500);
  });

  it("applies a fixed discount in minor units", () => {
    const onApply = renderPopover();
    fireEvent.click(screen.getByLabelText("Fixed amount"));
    fireEvent.change(screen.getByLabelText("Amount off"), { target: { value: "10.00" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply discount" }));
    expect(onApply).toHaveBeenCalledWith("fixed", 1000);
  });

  it("disables Apply for an empty or out-of-range value", () => {
    renderPopover();
    expect(screen.getByRole("button", { name: "Apply discount" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Percent off"), { target: { value: "0" } });
    expect(screen.getByRole("button", { name: "Apply discount" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Percent off"), { target: { value: "101" } });
    expect(screen.getByRole("button", { name: "Apply discount" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Percent off"), { target: { value: "10" } });
    expect(screen.getByRole("button", { name: "Apply discount" })).toBeEnabled();
  });

  it("reveals the manager fields once approval is needed and retries with them", () => {
    const onApply = renderPopover(true);
    expect(screen.getByText("Above your limit — a manager must approve.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Approve & apply" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Percent off"), { target: { value: "10" } });
    expect(screen.getByRole("button", { name: "Approve & apply" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Manager username"), { target: { value: "manager" } });
    fireEvent.change(screen.getByLabelText("Manager PIN"), { target: { value: "4321" } });
    fireEvent.click(screen.getByRole("button", { name: "Approve & apply" }));
    expect(onApply).toHaveBeenCalledWith("percent", 1000, "manager", "4321");
  });

  it("renders in RTL", () => {
    document.documentElement.dir = "rtl";
    try {
      renderPopover();
      expect(screen.getByRole("dialog", { name: "Discount" })).toBeInTheDocument();
    } finally {
      document.documentElement.dir = "ltr";
    }
  });
});
