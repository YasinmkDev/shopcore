import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { StatusPill } from "./StatusPill";
import type { StatusPillStatus } from "./StatusPill";

describe("StatusPill", () => {
  it.each([
    ["online", "Online"],
    ["offline", "Offline"],
    ["syncing", "Syncing"],
    ["error", "Error"],
  ] as const)("renders %s with icon and text", (status: StatusPillStatus, label: string) => {
    render(<StatusPill status={status} label={label} />);
    const pill = screen.getByRole("status");
    expect(pill).toHaveTextContent(label);
    expect(pill.querySelector("svg")).not.toBeNull();
  });

  it("renders online with success tokens and an inline layout", () => {
    render(<StatusPill status="online" label="Online" />);
    const pill = screen.getByRole("status");
    // --success text/icon on --success-bg (context/10-ui-system.md tokens).
    expect(pill).toHaveClass("bg-success-bg", "text-success");
    // Inline: icon, 6px gap, label, vertically centered.
    expect(pill).toHaveClass("inline-flex", "items-center", "gap-1.5");
  });
});
