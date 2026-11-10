import { useState } from "react";
import type { ReactElement } from "react";
import { t } from "@shopcore/i18n";
import { StatusPill } from "@shopcore/ui-kit";
import type { CoreClient } from "../../lib/core-client";

/**
 * Scaffold shell (spec 000): white-theme window with a StatusPill and a
 * Ping button proving the UI → Rust round-trip. No business logic.
 * All strings come from i18n keys; all styling from tokens.
 */
export function Shell({ client }: { client: CoreClient }): ReactElement {
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
    <div className="flex min-h-screen flex-col bg-bg text-text" data-theme="light">
      <header className="flex items-center justify-between border-b border-border px-6 py-3">
        <h1 className="text-lg font-semibold">{t("app.title")}</h1>
        <StatusPill status="online" label={t("status.online")} />
      </header>
      <main className="mx-auto flex-1 px-6 py-10">
        <p className="mb-4 text-muted">{t("shell.hint")}</p>
        <button
          type="button"
          onClick={() => void onPing()}
          disabled={loading}
          className="rounded bg-accent px-4 py-2 text-base font-medium text-on-accent hover:bg-accent-hover active:bg-accent-active disabled:opacity-50"
        >
          {loading ? t("shell.pinging") : t("shell.ping")}
        </button>
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
