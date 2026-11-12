/**
 * ManagerPinPopover (spec 001c-2): the line-discount popover. Discount
 * type (percent / fixed) + value; the first attempt goes without an
 * approver, and when the core answers `DISCOUNT_NEEDS_APPROVAL` the
 * popover reveals the manager username + PIN fields and retries with
 * them. The manager is typed, not picked from a list — there is no
 * list-managers command; the dev seed's manager is `manager`.
 *
 * Values are integers only: percent is whole percent (1–100, sent as
 * basis points), fixed is a decimal amount (parsed to minor units).
 * The popover never computes money beyond that parse.
 */
import { useState } from "react";
import type { ReactElement } from "react";
import { Button } from "@shopcore/ui-kit";
import { t } from "@shopcore/i18n";
import { parseMoneyToMinor } from "../../lib/money";

export interface ManagerPinPopoverProps {
  /** The line the discount applies to (shown in the heading). */
  lineName: string;
  currencyExponent: number;
  /** True once the core answered DISCOUNT_NEEDS_APPROVAL. */
  needsApproval: boolean;
  /** A command is in flight (Apply is disabled). */
  busy: boolean;
  onApply: (
    kind: "percent" | "fixed",
    value: number,
    managerUsername?: string,
    managerPin?: string,
  ) => void;
  onClose: () => void;
}

export function ManagerPinPopover({
  lineName,
  currencyExponent,
  needsApproval,
  busy,
  onApply,
  onClose,
}: ManagerPinPopoverProps): ReactElement {
  const [kind, setKind] = useState<"percent" | "fixed">("percent");
  const [valueText, setValueText] = useState("");
  const [managerUsername, setManagerUsername] = useState("");
  const [managerPin, setManagerPin] = useState("");

  /** The value in the units the command takes (bps / minor units),
   * or null while the input is not a usable discount. */
  const parsedValue: number | null =
    kind === "percent"
      ? (() => {
          if (!/^\d+$/.test(valueText)) {
            return null;
          }
          const percent = Number(valueText);
          if (!Number.isSafeInteger(percent) || percent < 1 || percent > 100) {
            return null;
          }
          return percent * 100;
        })()
      : parseMoneyToMinor(valueText, currencyExponent);

  const managerReady =
    !needsApproval || (managerUsername.trim() !== "" && managerPin !== "");
  const canApply = parsedValue !== null && parsedValue > 0 && managerReady && !busy;

  const apply = (): void => {
    if (parsedValue === null) {
      return;
    }
    if (needsApproval) {
      onApply(kind, parsedValue, managerUsername.trim(), managerPin);
    } else {
      onApply(kind, parsedValue);
    }
  };

  return (
    <div
      role="dialog"
      aria-label={t("sell.discount.title")}
      className="absolute bottom-16 right-[336px] z-20 w-[320px] rounded-md border border-border-strong bg-surface p-4 shadow-pop"
    >
      <h2 className="text-base font-semibold">{t("sell.discount.title")}</h2>
      <p className="mt-1 truncate text-sm text-muted">{lineName}</p>
      <fieldset className="mt-3">
        <legend className="text-sm font-medium">{t("sell.discount.type")}</legend>
        <div className="mt-1 flex gap-4">
          <label className="flex items-center gap-1 text-base">
            <input
              type="radio"
              name="discount-kind"
              checked={kind === "percent"}
              onChange={() => {
                setKind("percent");
                setValueText("");
              }}
            />
            {t("sell.discount.percent")}
          </label>
          <label className="flex items-center gap-1 text-base">
            <input
              type="radio"
              name="discount-kind"
              checked={kind === "fixed"}
              onChange={() => {
                setKind("fixed");
                setValueText("");
              }}
            />
            {t("sell.discount.fixed")}
          </label>
        </div>
      </fieldset>
      <div className="mt-3">
        <label htmlFor="discount-value" className="text-sm font-medium">
          {kind === "percent" ? t("sell.discount.percentValue") : t("sell.discount.fixedValue")}
        </label>
        <input
          id="discount-value"
          type="text"
          inputMode="decimal"
          autoComplete="off"
          value={valueText}
          disabled={busy}
          onChange={(event) => {
            setValueText(event.target.value);
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              if (canApply) {
                apply();
              }
            } else if (event.key === "Escape") {
              event.preventDefault();
              onClose();
            }
          }}
          className="mt-1 h-11 w-full rounded border border-border-input bg-surface px-3 text-base tabular-nums outline-none placeholder:text-faint focus-visible:ring-2 focus-visible:ring-focus-ring"
        />
      </div>
      {needsApproval ? (
        <div className="mt-3">
          <p role="alert" className="rounded-md bg-warning-bg px-3 py-2 text-base text-warning">
            {t("sell.discount.needsApproval")}
          </p>
          <div className="mt-3">
            <label htmlFor="discount-manager" className="text-sm font-medium">
              {t("sell.discount.managerUsername")}
            </label>
            <input
              id="discount-manager"
              type="text"
              autoComplete="off"
              value={managerUsername}
              disabled={busy}
              onChange={(event) => {
                setManagerUsername(event.target.value);
              }}
              className="mt-1 h-11 w-full rounded border border-border-input bg-surface px-3 text-base outline-none placeholder:text-faint focus-visible:ring-2 focus-visible:ring-focus-ring"
            />
          </div>
          <div className="mt-3">
            <label htmlFor="discount-pin" className="text-sm font-medium">
              {t("sell.discount.managerPin")}
            </label>
            <input
              id="discount-pin"
              type="password"
              inputMode="numeric"
              autoComplete="off"
              value={managerPin}
              disabled={busy}
              onChange={(event) => {
                setManagerPin(event.target.value);
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  if (canApply) {
                    apply();
                  }
                }
              }}
              className="mt-1 h-11 w-full rounded border border-border-input bg-surface px-3 text-base tabular-nums outline-none placeholder:text-faint focus-visible:ring-2 focus-visible:ring-focus-ring"
            />
          </div>
        </div>
      ) : null}
      <div className="mt-4 flex gap-2">
        <Button variant="secondary" size="md" onClick={onClose} disabled={busy} className="flex-1">
          {t("sell.payment.cancel")}
        </Button>
        <Button
          variant="primary"
          size="md"
          onClick={apply}
          disabled={!canApply}
          className="flex-1"
        >
          {needsApproval ? t("sell.discount.approve") : t("sell.discount.apply")}
        </Button>
      </div>
    </div>
  );
}
