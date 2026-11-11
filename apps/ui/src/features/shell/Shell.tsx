import { useState } from "react";
import type { ReactElement } from "react";
import { t } from "@shopcore/i18n";
import { Button, StatusPill } from "@shopcore/ui-kit";
import type { ThemeSetting } from "@shopcore/ui-kit";
import { useTheme } from "../../lib/theme";
import type { CoreClient } from "../../lib/core-client";

/**
 * Scaffold shell (spec 000): white-theme window with a StatusPill and a
 * Ping button proving the UI → Rust round-trip. No business logic.
 * All strings come from i18n keys; all styling from tokens.
 * Spec 000b: the Ping button is the registry Button (primary, loading
 * while pinging) and the header gains a Light/Dark/Auto theme control.
 * `data-theme` is owned by the theme provider (and theme-init.js before
 * first render) — never hardcoded here.
 */
const THEME_OPTIONS: { value: ThemeSetting; labelKey: "theme.light" | "theme.dark" | "theme.auto" }[] = [
  { value: "light", labelKey: "theme.light" },
  { value: "dark", labelKey: "theme.dark" },
  { value: "auto", labelKey: "theme.auto" },
];

export function Shell({ client }: { client: CoreClient }): ReactElement {
  const { theme, setTheme } = useTheme();
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function onPing(): Promise<void> {
    setLoading(true);
    setError(null);
    try {
      const pong = await client.ping();
      setResult(pong.message);
    } catch {
      setError(t("shell.error"));
      setResult(null);
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="flex min-h-screen flex-col bg-bg text-text">
      <header className="flex items-center justify-between border-b border-border px-6 py-3">
        <h1 className="text-lg font-semibold">{t("app.title")}</h1>
        <div className="flex items-center gap-4">
          <div role="group" aria-label={t("theme.label")} className="flex items-center gap-1">
            {THEME_OPTIONS.map((option) => (
              <Button
                key={option.value}
                size="sm"
                variant={theme === option.value ? "secondary" : "ghost"}
                aria-pressed={theme === option.value}
                onClick={() => {
                  setTheme(option.value);
                }}
              >
                {t(option.labelKey)}
              </Button>
            ))}
          </div>
          <StatusPill status="online" label={t("status.online")} />
        </div>
      </header>
      <main className="mx-auto flex-1 px-6 py-10">
        <p className="mb-4 text-muted">{t("shell.hint")}</p>
        <Button variant="primary" loading={loading} onClick={() => void onPing()}>
          {loading ? t("shell.pinging") : t("shell.ping")}
        </Button>
        {result !== null && (
          <p className="mt-4">
            {t("shell.resultLabel")}: {result}
          </p>
        )}
        {error !== null && (
          <p className="mt-4 text-danger" role="alert">
            {error}
          </p>
        )}
      </main>
      <footer className="border-t border-border px-6 py-2 text-sm text-muted">
        {t("app.ready")}
      </footer>
    </div>
  );
}
