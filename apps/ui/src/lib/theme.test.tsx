import { act, fireEvent, render, screen } from "@testing-library/react";
import type { ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMemorySettingsStore } from "@shopcore/ui-kit";
import type { SettingsStore } from "@shopcore/ui-kit";
import { ThemeProvider, useTheme } from "./theme";

function stubColorScheme(initialDark: boolean): {
  setDark: (value: boolean) => void;
} {
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  const mql = {
    matches: initialDark,
    media: "(prefers-color-scheme: dark)",
    addEventListener: (_type: string, cb: (event: MediaQueryListEvent) => void): void => {
      listeners.add(cb);
    },
    removeEventListener: (_type: string, cb: (event: MediaQueryListEvent) => void): void => {
      listeners.delete(cb);
    },
  };
  vi.stubGlobal("matchMedia", vi.fn(() => mql));
  return {
    setDark: (value: boolean): void => {
      mql.matches = value;
      for (const cb of listeners) cb({ matches: value } as MediaQueryListEvent);
    },
  };
}

function Probe(): ReactElement {
  const { theme, resolvedTheme, setTheme } = useTheme();
  return (
    <div>
      <span data-testid="theme">{theme}</span>
      <span data-testid="resolved">{resolvedTheme}</span>
      <button
        type="button"
        onClick={() => {
          setTheme("dark");
        }}
      >
        Set dark
      </button>
    </div>
  );
}

describe("ThemeProvider", () => {
  beforeEach(() => {
    stubColorScheme(false);
    document.documentElement.removeAttribute("data-theme");
    window.localStorage.clear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    document.documentElement.removeAttribute("data-theme");
    window.localStorage.clear();
  });

  function renderWith(store: SettingsStore): void {
    render(
      <ThemeProvider store={store}>
        <Probe />
      </ThemeProvider>,
    );
  }

  it("defaults to light and sets data-theme on the app root", () => {
    renderWith(createMemorySettingsStore());
    expect(screen.getByTestId("theme")).toHaveTextContent("light");
    expect(screen.getByTestId("resolved")).toHaveTextContent("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("restores a stored dark setting", () => {
    renderWith(createMemorySettingsStore({ theme: "dark" }));
    expect(screen.getByTestId("theme")).toHaveTextContent("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("follows the OS in Auto mode", () => {
    stubColorScheme(true);
    renderWith(createMemorySettingsStore({ theme: "auto" }));
    expect(screen.getByTestId("theme")).toHaveTextContent("auto");
    expect(screen.getByTestId("resolved")).toHaveTextContent("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("follows OS changes while in Auto mode", () => {
    const scheme = stubColorScheme(false);
    renderWith(createMemorySettingsStore({ theme: "auto" }));
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    act(() => {
      scheme.setDark(true);
    });
    expect(screen.getByTestId("resolved")).toHaveTextContent("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("persists the choice via the store and updates the attribute", () => {
    const store = createMemorySettingsStore();
    renderWith(store);
    fireEvent.click(screen.getByRole("button", { name: "Set dark" }));
    expect(store.getTheme()).toBe("dark");
    expect(screen.getByTestId("theme")).toHaveTextContent("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("falls back to light for an invalid stored value", () => {
    window.localStorage.setItem("shopcore.theme", "sepia");
    render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>,
    );
    expect(screen.getByTestId("theme")).toHaveTextContent("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("keeps the pre-render data-theme (no flash) when the store agrees", () => {
    // theme-init.js sets the attribute before React mounts; the provider
    // must re-apply the same value, never flash through the other theme.
    document.documentElement.setAttribute("data-theme", "dark");
    renderWith(createMemorySettingsStore({ theme: "dark" }));
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });
});
