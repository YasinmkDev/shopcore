import { useEffect, useState } from "react";
import type { ReactElement } from "react";
import { t } from "@shopcore/i18n";
import { Button, StatusPill } from "@shopcore/ui-kit";
import type { ThemeSetting } from "@shopcore/ui-kit";
import { useTheme } from "../../lib/theme";
import type { CoreClient } from "../../lib/core-client";
import { SessionProvider } from "../checkout/SessionProvider";
import { SellScreen } from "../checkout/SellScreen";

/**
 * Shell (spec 000 + 001c-1): window chrome (title, theme control,
 * online pill) around the sell screen. The online pill is driven by
 * a ping on mount — that also keeps the `ping` invoke alive for the
 * ADR-013 drift guard. The Ping demo button is gone (001c-1): the
 * sell screen is the main content, and the StatusBar is its footer.
 * All strings come from i18n keys; all styling from tokens.
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
  const [online, setOnline] = useState(true);

  useEffect(() => {
    let cancelled = false;
    client
      .ping()
      .then(() => {
        if (!cancelled) {
          setOnline(true);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setOnline(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [client]);

  return (
    <div className="flex min-h-screen flex-col bg-bg text-text">
      <header className="flex shrink-0 items-center justify-between border-b border-border px-6 py-3">
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
          <StatusPill status={online ? "online" : "offline"} label={online ? t("status.online") : t("status.offline")} />
        </div>
      </header>
      <main className="flex min-h-0 flex-1 flex-col">
        <SessionProvider client={client}>
          <SellScreen client={client} />
        </SessionProvider>
      </main>
    </div>
  );
}
