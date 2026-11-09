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
});
