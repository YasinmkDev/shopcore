import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactElement, ReactNode } from "react";
import { describe, expect, it } from "vitest";
import type { Pong } from "@shopcore/protocol-ts";
import { createMemorySettingsStore } from "@shopcore/ui-kit";
import { ThemeProvider } from "../../lib/theme";
import { Shell } from "./Shell";
import type { CoreClient } from "../../lib/core-client";

function makeClient(outcome: Pong | Error): CoreClient {
  return {
    ping: () =>
      outcome instanceof Error ? Promise.reject(outcome) : Promise.resolve(outcome),
    subscribe: () => () => {
      /* no events in spec 000 */
    },
  };
}

/** Shell reads the theme from context (spec 000b), so tests provide it. */
function renderShell(client: CoreClient): void {
  const wrapper = ({ children }: { children: ReactNode }): ReactElement => (
    <ThemeProvider store={createMemorySettingsStore()}>{children}</ThemeProvider>
  );
  render(<Shell client={client} />, { wrapper });
}

describe("Shell", () => {
  it("pins the status bar to the bottom of a full-height column", () => {
    renderShell(makeClient({ message: "pong" }));
    const footer = screen.getByRole("contentinfo");
    const root = footer.parentElement;
    expect(root).not.toBeNull();
    // Root fills the viewport (min-height 100vh) as a column.
    expect(root).toHaveClass("min-h-screen", "flex", "flex-col");
    // Status bar is the last element; the content area takes the slack.
    expect(root?.lastElementChild).toBe(footer);
    expect(screen.getByRole("main")).toHaveClass("flex-1");
  });

  it("shows the pong message after ping", async () => {
    renderShell(makeClient({ message: "pong" }));
    fireEvent.click(screen.getByRole("button", { name: "Ping" }));
    await waitFor(() => {
      expect(screen.getByText(/Core replied/)).toBeInTheDocument();
    });
  });

  it("shows the error state when ping fails", async () => {
    renderShell(makeClient(new Error("boom")));
    fireEvent.click(screen.getByRole("button", { name: "Ping" }));
    await waitFor(() => {
      expect(screen.getByRole("alert")).toBeInTheDocument();
    });
  });

  it("disables the button while pinging", async () => {
    let resolvePing!: (pong: Pong) => void;
    const client: CoreClient = {
      ping: () =>
        new Promise<Pong>((resolve) => {
          resolvePing = resolve;
        }),
      subscribe: () => () => {
        /* no events in spec 000 */
      },
    };
    renderShell(client);
    fireEvent.click(screen.getByRole("button", { name: "Ping" }));
    expect(screen.getByRole("button", { name: "Pinging…" })).toBeDisabled();
    resolvePing({ message: "pong" });
    await waitFor(() => {
      expect(screen.getByText(/Core replied/)).toBeInTheDocument();
    });
  });

  it("shows the Light/Dark/Auto theme control", () => {
    renderShell(makeClient({ message: "pong" }));
    const group = screen.getByRole("group", { name: "Theme" });
    expect(group).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Light" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    fireEvent.click(screen.getByRole("button", { name: "Dark" }));
    expect(screen.getByRole("button", { name: "Dark" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });
});
