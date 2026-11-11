import { createContext, useContext, useEffect, useMemo, useState } from "react";
import type { ReactElement, ReactNode } from "react";
import type { SettingsStore, ThemeSetting } from "@shopcore/ui-kit";
import { localStorageSettingsStore } from "@shopcore/ui-kit";

/**
 * Theme provider (spec 000b, req. 6–8). Setting Light (default) / Dark /
 * Auto (follows the OS via matchMedia). Sets `data-theme` on the app root
 * (<html>), so the dark block in tokens.css takes effect. Persists via the
 * SettingsStore interface (localStorage-backed in 000b).
 *
 * No-flash: public/theme-init.js already set `data-theme` synchronously
 * before React mounts; this provider re-applies the same value on mount
 * and keeps it in sync afterwards. No component reads the OS setting on
 * its own.
 */
export type ResolvedTheme = "light" | "dark";

export interface ThemeContextValue {
  theme: ThemeSetting;
  resolvedTheme: ResolvedTheme;
  setTheme: (theme: ThemeSetting) => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

export function ThemeProvider({
  children,
  store,
}: {
  children: ReactNode;
  store?: SettingsStore;
}): ReactElement {
  const settings: SettingsStore = store ?? localStorageSettingsStore;
  const [theme, setThemeState] = useState<ThemeSetting>(() => settings.getTheme() ?? "light");
  /* The OS colour scheme, tracked live so Auto follows it. `resolvedTheme`
     is derived from `theme` + `osDark` during render — never synced via
     an effect (react-hooks/set-state-in-effect). */
  const [osDark, setOsDark] = useState<boolean>(
    () =>
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches,
  );
  const resolvedTheme: ResolvedTheme =
    theme === "auto" ? (osDark ? "dark" : "light") : theme;

  /* Follow the OS while the setting is Auto. */
  useEffect(() => {
    if (theme !== "auto" || typeof window.matchMedia !== "function") return;
    const mql = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = (event: MediaQueryListEvent): void => {
      setOsDark(event.matches);
    };
    mql.addEventListener("change", onChange);
    return () => {
      mql.removeEventListener("change", onChange);
    };
  }, [theme]);

  /* Publish the resolved theme on the app root. */
  useEffect(() => {
    document.documentElement.setAttribute("data-theme", resolvedTheme);
  }, [resolvedTheme]);

  const value = useMemo<ThemeContextValue>(
    () => ({
      theme,
      resolvedTheme,
      setTheme: (next: ThemeSetting): void => {
        settings.setTheme(next);
        setThemeState(next);
      },
    }),
    [theme, resolvedTheme, settings],
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
  const context = useContext(ThemeContext);
  if (context === null) {
    throw new Error("useTheme must be used inside ThemeProvider");
  }
  return context;
}
