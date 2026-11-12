/**
 * NumPad tests (spec 001c-2): every key calls onKey with its label,
 * and a disabled pad calls nothing.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { NumPad } from "./NumPad";
import type { NumPadKey } from "./NumPad";

describe("NumPad", () => {
  it("emits each key", () => {
    const onKey = vi.fn();
    render(<NumPad onKey={onKey} />);
    for (const key of ["1", "2", "3", "4", "5", "6", "7", "8", "9", "00", "0", ".", "C", "⌫"] as NumPadKey[]) {
      fireEvent.click(screen.getByRole("button", { name: key }));
    }
    expect(onKey).toHaveBeenCalledTimes(14);
    expect(onKey).toHaveBeenNthCalledWith(1, "1");
    expect(onKey).toHaveBeenNthCalledWith(10, "00");
    expect(onKey).toHaveBeenNthCalledWith(14, "⌫");
  });

  it("calls nothing while disabled", () => {
    const onKey = vi.fn();
    render(<NumPad onKey={onKey} disabled />);
    fireEvent.click(screen.getByRole("button", { name: "5" }));
    expect(onKey).not.toHaveBeenCalled();
  });
});
