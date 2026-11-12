import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactElement, ReactNode } from "react";
import { describe, expect, it } from "vitest";
import type { Pong } from "@shopcore/protocol-ts";
import { createMemorySettingsStore } from "@shopcore/ui-kit";
import { ThemeProvider } from "../../lib/theme";
import { Shell } from "./Shell";
import type { CoreClient } from "../../lib/core-client";

/**
 * Shell tests (spec 001c-1): the Ping demo is gone — the sell screen
 * is the main content and the StatusBar is its footer. The header
 * keeps the theme control and the online pill, now driven by a ping
 * on mount. The checkout methods never resolve here (the sell screen
 * stays in its loading state); these tests assert the chrome only.
 */
function makeClient(outcome: Pong | Error): CoreClient {
  const never = (): Promise<never> => new Promise(() => undefined);
  return {
    ping: () =>
      outcome instanceof Error ? Promise.reject(outcome) : Promise.resolve(outcome),
    getSession: () => never(),
    getStatus: () => never(),
    getCart: () => never(),
    searchSkus: () => never(),
    lookupBarcode: () => never(),
    draftAddLine: () => never(),
    draftSetQty: () => never(),
    draftRemoveLine: () => never(),
    draftCancel: () => never(),
    draftRestoreLine: () => never(),
    subscribe: () => () => {
      /* no events in these tests */
    },
  };
}

function renderShell(client: CoreClient): void {
  const wrapper = ({ children }: { children: ReactNode }): ReactElement => (
    <ThemeProvider store={createMemorySettingsStore()}>{children}</ThemeProvider>
  );
  render(<Shell client={client} />, { wrapper });
}

describe("Shell", () => {
  it("renders the sell screen as the main content (no Ping demo)", async () => {
    renderShell(makeClient({ message: "pong" }));
    const main = screen.getByRole("main");
    expect(main).toHaveClass("flex-1");
    // The sell screen is loading (checkout methods never resolve).
    await waitFor(() => {
      expect(screen.getByText("Loading cart…")).toBeInTheDocument();
    });
    expect(screen.queryByRole("button", { name: "Ping" })).not.toBeInTheDocument();
  });

  it("shows the online pill after the mount ping succeeds", async () => {
    renderShell(makeClient({ message: "pong" }));
    await waitFor(() => {
      expect(screen.getByText("Online")).toBeInTheDocument();
    });
  });

  it("shows the offline pill when the mount ping fails", async () => {
    renderShell(makeClient(new Error("boom")));
    await waitFor(() => {
      expect(screen.getByText("Offline")).toBeInTheDocument();
    });
  });

  it("shows the Light/Dark/Auto theme control", () => {
    renderShell(makeClient({ message: "pong" }));
    const group = screen.getByRole("group", { name: "Theme" });
    expect(group).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Light" })).toHaveAttribute("aria-pressed", "true");
    const dark = screen.getByRole("button", { name: "Dark" });
    fireEvent.click(dark);
    expect(dark).toHaveAttribute("aria-pressed", "true");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("has no footer — the StatusBar is the sell screen's footer", () => {
    renderShell(makeClient({ message: "pong" }));
    expect(screen.queryByRole("contentinfo")).not.toBeInTheDocument();
  });
});
