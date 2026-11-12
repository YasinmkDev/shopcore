/**
 * PaymentPanel (spec 001c-2): the cash payment side panel. CASH ONLY
 * in v1 — no method selector. Total due on top, tender input + numpad,
 * quick amounts (exact + the next round multiples), live change due
 * (or the remaining amount), Complete with Enter. Esc closes.
 *
 * The tender is a STRING until completion: the input is typed digit by
 * digit, and only `parseMoneyToMinor` turns it into integer minor
 * units — no float money anywhere. Cash rounding is OFF by default,
 * so the change shown here is exact; a rounding line, when the tenant
 * turns it on, appears on the completed receipt, not here.
 */
import { useEffect, useRef, useState } from "react";
import type { ReactElement } from "react";
import { Button } from "@shopcore/ui-kit";
import type { MotionTier } from "@shopcore/ui-kit";
import { t } from "@shopcore/i18n";
import { formatMoney, parseMoneyToMinor } from "../../lib/money";
import { NumPad } from "./NumPad";
import type { NumPadKey } from "./NumPad";

/** Quick amounts: the exact total, then the next multiples of 5 / 10 /
 * 50 / 100 above it (integer math, deduped, sorted). */
export function quickAmounts(totalMinor: number): number[] {
  const amounts = new Set<number>([totalMinor]);
  for (const step of [500, 1000, 5000, 10000]) {
    amounts.add(Math.ceil(totalMinor / step) * step);
  }
  return [...amounts].sort((a, b) => a - b);
}

/** Whether `text` is a tender prefix still being typed (digits, at
 * most one dot, at most `exponent` fraction digits). */
function isTenderPrefix(text: string, exponent: number): boolean {
  if (text === "") {
    return true;
  }
  const match = /^(\d+)(?:\.(\d*))?$/.exec(text);
  if (match === null) {
    return false;
  }
  return (match[2] ?? "").length <= exponent;
}

export interface PaymentPanelProps {
  totalMinor: number;
  currencyExponent: number;
  motionTier: MotionTier;
  /** A command is in flight (complete is disabled). */
  busy: boolean;
  /** The last completion error (e.g. INSUFFICIENT_TENDER), if any. */
  error: string | null;
  onComplete: (tenderMinor: number) => void;
  onClose: () => void;
}

export function PaymentPanel({
  totalMinor,
  currencyExponent,
  motionTier,
  busy,
  error,
  onComplete,
  onClose,
}: PaymentPanelProps): ReactElement {
  const [tenderText, setTenderText] = useState("");
  const tenderMinor = parseMoneyToMinor(tenderText, currencyExponent);
  const canComplete = tenderMinor !== null && tenderMinor >= totalMinor && !busy;
  const inputRef = useRef<HTMLInputElement | null>(null);

  // Focus the tender input on open, so typing + Enter work at once.
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const applyKey = (key: NumPadKey): void => {
    if (key === "C") {
      setTenderText("");
      return;
    }
    if (key === "⌫") {
      setTenderText((current) => current.slice(0, -1));
      return;
    }
    setTenderText((current) => {
      const next = current + key;
      return isTenderPrefix(next, currencyExponent) ? next : current;
    });
  };

  const complete = (): void => {
    if (canComplete) {
      // `canComplete` already implies `tenderMinor !== null`.
      onComplete(tenderMinor);
    }
  };

  return (
    <aside
      role="dialog"
      aria-label={t("sell.payment.title")}
      className={`absolute bottom-0 right-0 top-0 z-20 flex w-[360px] flex-col border-l border-border-strong bg-surface shadow-pop ${
        motionTier === 2 ? "payment-panel-enter-minimal" : "payment-panel-enter"
      }`}
    >
      <div className="flex items-center justify-between border-b border-border px-4 py-3">
        <h2 className="text-base font-semibold">{t("sell.payment.title")}</h2>
        <span className="rounded border border-border-strong px-2 py-0.5 text-sm text-muted">
          {t("sell.payment.cash")}
        </span>
      </div>
      <div className="flex flex-1 flex-col gap-4 overflow-y-auto px-4 py-4">
        <div className="flex items-baseline justify-between">
          <span className="text-base text-muted">{t("sell.payment.totalDue")}</span>
          <span className="text-2xl font-bold tabular-nums">
            {formatMoney(totalMinor, currencyExponent)}
          </span>
        </div>
        <div>
          <label htmlFor="payment-tender" className="text-sm font-medium text-text">
            {t("sell.payment.tender")}
          </label>
          <input
            ref={inputRef}
            id="payment-tender"
            type="text"
            inputMode="decimal"
            autoComplete="off"
            value={tenderText}
            disabled={busy}
            onChange={(event) => {
              const next = event.target.value;
              if (isTenderPrefix(next, currencyExponent)) {
                setTenderText(next);
              }
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                complete();
              } else if (event.key === "Escape") {
                event.preventDefault();
                onClose();
              }
            }}
            className="mt-1 h-12 w-full rounded-md border border-border-input bg-surface px-4 text-xl tabular-nums outline-none placeholder:text-faint focus-visible:ring-2 focus-visible:ring-focus-ring"
          />
        </div>
        <div className="flex flex-wrap gap-2">
          {quickAmounts(totalMinor).map((amount) => (
            <button
              key={amount}
              type="button"
              disabled={busy}
              onClick={() => {
                setTenderText(formatMoney(amount, currencyExponent));
              }}
              className="rounded-md border border-border-strong bg-surface px-3 py-2 text-base tabular-nums outline-none hover:bg-surface-subtle focus-visible:ring-2 focus-visible:ring-focus-ring disabled:cursor-not-allowed disabled:opacity-50"
            >
              {formatMoney(amount, currencyExponent)}
            </button>
          ))}
        </div>
        <NumPad onKey={applyKey} disabled={busy} />
        <div className="flex items-baseline justify-between border-t border-border pt-3">
          {tenderMinor !== null && tenderMinor >= totalMinor ? (
            <>
              <span className="text-base text-muted">{t("sell.payment.change")}</span>
              <span className="text-xl font-semibold tabular-nums">
                {formatMoney(tenderMinor - totalMinor, currencyExponent)}
              </span>
            </>
          ) : (
            <>
              <span className="text-base text-muted">{t("sell.payment.remaining")}</span>
              <span className="text-xl font-semibold tabular-nums">
                {formatMoney(totalMinor - (tenderMinor ?? 0), currencyExponent)}
              </span>
            </>
          )}
        </div>
        {error !== null ? (
          <p role="alert" className="rounded-md bg-warning-bg px-3 py-2 text-base text-warning">
            {error}
          </p>
        ) : null}
      </div>
      <div className="flex gap-2 border-t border-border px-4 py-3">
        <Button variant="secondary" size="lg" onClick={onClose} disabled={busy} className="flex-1">
          {t("sell.payment.cancel")}
        </Button>
        <Button variant="primary" size="lg" onClick={complete} disabled={!canComplete} className="flex-1">
          {t("sell.payment.complete")}
        </Button>
      </div>
    </aside>
  );
}
