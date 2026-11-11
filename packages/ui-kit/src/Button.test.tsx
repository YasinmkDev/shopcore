import { Plus } from "lucide-react";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Button, IconButton } from "./Button";
import type { ButtonSize, ButtonVariant } from "./Button";

describe("Button", () => {
  it.each([
    ["primary", ["bg-accent", "text-on-accent"]],
    ["secondary", ["border-border-strong", "bg-surface", "text-text"]],
    ["ghost", ["bg-transparent", "text-text"]],
    ["danger", ["bg-danger", "text-on-accent"]],
  ] as [ButtonVariant, string[]][])(
    "renders the %s variant with token classes",
    (variant, classes) => {
      render(<Button variant={variant}>Save</Button>);
      expect(screen.getByRole("button", { name: "Save" })).toHaveClass(...classes);
    },
  );

  it.each([
    ["sm", "h-8"],
    ["md", "h-11"],
    ["lg", "h-12"],
  ] as [ButtonSize, string][])("renders size %s (%s tall)", (size, height) => {
    render(<Button size={size}>Save</Button>);
    expect(screen.getByRole("button", { name: "Save" })).toHaveClass(height);
  });

  it("md and lg meet the 44 px POS target; sm is back-office only", () => {
    // h-11 = 44 px, h-12 = 48 px, h-8 = 32 px (Tailwind spacing scale).
    render(<Button size="md">M</Button>);
    expect(screen.getByRole("button", { name: "M" })).toHaveClass("h-11");
    render(<Button size="lg">L</Button>);
    expect(screen.getByRole("button", { name: "L" })).toHaveClass("h-12");
    render(<Button size="sm">S</Button>);
    expect(screen.getByRole("button", { name: "S" })).toHaveClass("h-8");
  });

  it("shows a spinner and disables while loading; onClick does not fire", () => {
    const onClick = vi.fn();
    render(
      <Button loading onClick={onClick}>
        Save
      </Button>,
    );
    const button = screen.getByRole("button", { name: "Save" });
    expect(button).toBeDisabled();
    expect(button.querySelector("svg.animate-spin")).not.toBeNull();
    fireEvent.click(button);
    expect(onClick).not.toHaveBeenCalled();
  });

  it("does not fire onClick while disabled", () => {
    const onClick = vi.fn();
    render(
      <Button disabled onClick={onClick}>
        Save
      </Button>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(onClick).not.toHaveBeenCalled();
  });

  it("has a visible focus ring from the --focus-ring token", () => {
    render(<Button>Save</Button>);
    expect(screen.getByRole("button", { name: "Save" })).toHaveClass(
      "focus-visible:outline-2",
      "focus-visible:outline-focus-ring",
      "focus-visible:outline-offset-2",
    );
  });

  it("is a native button, so Enter/Space activation is the browser's", () => {
    // jsdom does not implement key-based activation for buttons (no click
    // fires on Enter/Space keydown — probed 2026-11-10), so the test
    // asserts the native element and exercises the DOM activation method
    // (HTMLElement.click), which is what the browser invokes on Enter/Space.
    const onClick = vi.fn();
    render(<Button onClick={onClick}>Save</Button>);
    const button = screen.getByRole("button", { name: "Save" });
    expect(button.tagName).toBe("BUTTON");
    expect(button).toHaveAttribute("type", "button");
    expect(button.tabIndex).toBe(0);
    button.click();
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("renders in RTL without breaking", () => {
    const onClick = vi.fn();
    render(
      <div dir="rtl">
        <Button onClick={onClick}>Save</Button>
      </div>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(onClick).toHaveBeenCalledTimes(1);
  });
});

describe("IconButton", () => {
  it("requires a non-empty aria-label", () => {
    expect(() =>
      render(<IconButton icon={<Plus size={16} />} aria-label="" />),
    ).toThrow(/aria-label/);
  });

  it("renders the icon with aria-label and tooltip", () => {
    render(<IconButton icon={<Plus size={16} />} aria-label="Add item" />);
    const button = screen.getByRole("button", { name: "Add item" });
    expect(button).toHaveAttribute("title", "Add item");
    expect(button.querySelector("svg")).not.toBeNull();
  });

  it("is a native button and fires onClick", () => {
    const onClick = vi.fn();
    render(
      <IconButton icon={<Plus size={16} />} aria-label="Add item" onClick={onClick} />,
    );
    const button = screen.getByRole("button", { name: "Add item" });
    expect(button.tagName).toBe("BUTTON");
    fireEvent.click(button);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("shows a spinner and disables while loading", () => {
    const onClick = vi.fn();
    render(
      <IconButton icon={<Plus size={16} />} aria-label="Add item" loading onClick={onClick} />,
    );
    const button = screen.getByRole("button", { name: "Add item" });
    expect(button).toBeDisabled();
    expect(button.querySelector("svg.animate-spin")).not.toBeNull();
    fireEvent.click(button);
    expect(onClick).not.toHaveBeenCalled();
  });
});
