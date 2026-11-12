/**
 * StatusBar (spec 001c-1): the bottom strip of the sell screen.
 * Shift state, printer state, and sync state — each as an icon +
 * text pill (never colour alone). Sync shows "Sync: not configured"
 * until sync is actually configured; a fake synced state is never
 * shown (the spec is explicit about this).
 */
import type { ReactElement } from "react";
import { StatusPill } from "@shopcore/ui-kit";
import { t } from "@shopcore/i18n";

export interface StatusBarProps {
  shiftOpen: boolean;
  printerStatus: string;
  syncConfigured: boolean;
  /** "T1" etc. — empty when no session is available. */
  terminalCode: string;
  /** The signed-in user's name — empty when no session is available. */
  username: string;
}

export function StatusBar({
  shiftOpen,
  printerStatus,
  syncConfigured,
  terminalCode,
  username,
}: StatusBarProps): ReactElement {
  return (
    <footer className="flex shrink-0 items-center gap-2 border-t border-border bg-surface px-4 py-2 text-sm">
      <StatusPill
        status={shiftOpen ? "online" : "offline"}
        label={shiftOpen ? t("sell.status.shiftOpen") : t("sell.status.shiftClosed")}
      />
      <StatusPill
        status={printerStatus === "ready" ? "online" : printerStatus === "offline" ? "offline" : "error"}
        label={printerStatus === "ready" ? t("sell.status.printerReady") : t("sell.status.printerOffline")}
      />
      <StatusPill
        status={syncConfigured ? "online" : "offline"}
        label={syncConfigured ? t("sell.status.syncConfigured") : t("sell.status.syncNotConfigured")}
      />
      <span className="ml-auto flex items-center gap-2 text-muted">
        {username !== "" ? <span>{username}</span> : null}
        {terminalCode !== "" ? <span>{terminalCode}</span> : null}
      </span>
    </footer>
  );
}
