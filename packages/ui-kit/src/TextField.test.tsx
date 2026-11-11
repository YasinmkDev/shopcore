import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TextField } from "./TextField";

describe("TextField", () => {
  it("renders the label and is labelled by it", () => {
    render(<TextField label="Item name" />);
    expect(screen.getByLabelText("Item name")).toBeInTheDocument();
  });

  it("renders helper text and a placeholder", () => {
    render(
      <TextField label="Item name" helperText="Shown on the receipt" placeholder="Rice 5kg" />,
    );
    expect(screen.getByText("Shown on the receipt")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Rice 5kg")).toBeInTheDocument();
  });

  it("marks required fields with a visible marker and the required attribute", () => {
    // The "*" marker is aria-hidden, so the accessible name stays "Item name";
    // the label's full text is "Item name *", so match by regex.
    render(<TextField label="Item name" required />);
    const input = screen.getByLabelText(/Item name/);
    expect(input).toBeRequired();
    expect(screen.getByText("*", { exact: false })).toBeInTheDocument();
  });

  it("renders the disabled state", () => {
    render(<TextField label="Item name" disabled />);
    expect(screen.getByLabelText("Item name")).toBeDisabled();
  });

  it("shows the error inline with aria-invalid and aria-describedby", () => {
    render(<TextField label="Item name" error="Enter a name." />);
    const input = screen.getByLabelText("Item name");
    expect(input).toHaveAttribute("aria-invalid", "true");
    const describedBy = input.getAttribute("aria-describedby");
    expect(describedBy).not.toBeNull();
    const error = screen.getByText("Enter a name.");
    expect(error.id).toBe(describedBy);
    expect(error).toHaveClass("text-danger");
    // The border also signals the error (text is the primary cue).
    expect(input).toHaveClass("border-danger");
  });

  it("hides the helper text while an error is shown", () => {
    render(<TextField label="Item name" helperText="Shown on the receipt" error="Enter a name." />);
    expect(screen.queryByText("Shown on the receipt")).toBeNull();
    expect(screen.getByText("Enter a name.")).toBeInTheDocument();
  });

  it("accepts typing and reports the value", () => {
    render(<TextField label="Item name" />);
    const input = screen.getByLabelText<HTMLInputElement>("Item name");
    fireEvent.change(input, { target: { value: "Rice" } });
    expect(input.value).toBe("Rice");
  });

  it("uses the --border-input token and the --focus-ring token", () => {
    render(<TextField label="Item name" />);
    expect(screen.getByLabelText("Item name")).toHaveClass(
      "border-border-input",
      "focus-visible:outline-focus-ring",
    );
  });

  it("renders in RTL without breaking", () => {
    render(
      <div dir="rtl">
        <TextField label="Item name" />
      </div>,
    );
    const input = screen.getByLabelText<HTMLInputElement>("Item name");
    fireEvent.change(input, { target: { value: "Rice" } });
    expect(input.value).toBe("Rice");
  });
});
