import { CheckCircle2, CloudOff, LoaderCircle, TriangleAlert } from "lucide-react";
import type { ReactElement } from "react";

/**
 * Status pill (see context/10-ui-system.md, component registry).
 * Shows a state with an icon AND text — never colour alone.
 * States: online / offline / syncing / error. Live wiring (sync, printer,
 * shift) lands with the core/sync specs; spec 000 renders it statically.
 */
export type StatusPillStatus = "online" | "offline" | "syncing" | "error";

export interface StatusPillProps {
  status: StatusPillStatus;
  label: string;
}

const STATUS_STYLES: Record<StatusPillStatus, string> = {
  online: "border-border bg-success-bg text-success",
  offline: "border-border bg-surface-subtle text-muted",
  syncing: "border-border bg-warning-bg text-warning",
  error: "border-border bg-danger-bg text-danger",
};

export function StatusPill({ status, label }: StatusPillProps): ReactElement {
  const Icon =
    status === "online"
      ? CheckCircle2
      : status === "offline"
        ? CloudOff
        : status === "syncing"
          ? LoaderCircle
          : TriangleAlert;
  return (
    <span
      role="status"
      className={`inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-sm font-medium ${STATUS_STYLES[status]}`}
    >
      <Icon size={16} strokeWidth={1.5} aria-hidden="true" />
      {label}
    </span>
  );
}
