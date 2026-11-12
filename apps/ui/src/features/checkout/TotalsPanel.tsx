/**
 * TotalsPanel (spec 001c-1 + 001c-2): the right ~1/3 of the sell screen.
 * Subtotal / discount / tax / total, always visible (req. 6), the
 * grand total at 40 px. Total changes flash a 250 ms tint fade
 * (tier 2: no tint — opacity-only motion).
 *
 * 001c-2: the subtotal line shows the PRE-TAX subtotal
 * (`net_subtotal + discounts`, context/02) so the numbers shown
 * satisfy `subtotal − discounts + tax == total` in both tax modes.
 * The PAY (F9) and Discount (F6) buttons live here — the right column
 * is the actions column (see 10-ui-system.md).
 */
import type { ReactElement } from "react";
import { Button } from "@shopcore/ui-kit";
import type { MotionTier } from "@shopcore/ui-kit";
import { t } from "@shopcore/i18n";
import type { CartTotalsDto } from "@shopcore/protocol-ts";
import { formatMoney } from "../../lib/money";

export interface TotalsPanelProps {
  totals: CartTotalsDto;
  currencyExponent: number;
  motionTier: MotionTier;
  /** Two-click cancel: first click arms, second confirms. */
  cancelArmed: boolean;
  onCancelClick: () => void;
  /** The cart has lines: PAY and Discount are enabled. */
  canPay: boolean;
  onPayClick: () => void;
  onDiscountClick: () => void;
  disabled?: boolean;
}

export function TotalsPanel({
  totals,
  currencyExponent,
  motionTier,
  cancelArmed,
  onCancelClick,
  canPay,
  onPayClick,
  onDiscountClick,
  disabled = false,
}: TotalsPanelProps): ReactElement {
  // The pre-tax subtotal (02 rule): the gross subtotal would break
  // the displayed equation for any inclusive line, so the line shows
  // net + discounts instead. Integer math only.
  const preTaxSubtotal = totals.net_subtotal_minor + totals.discount_minor;
  return (
    <aside className="flex w-[320px] shrink-0 flex-col border-l border-border bg-surface">
      <h2 className="border-b border-border px-4 py-3 text-base font-semibold">
        {t("sell.totals.title")}
      </h2>
      <div className="flex flex-1 flex-col justify-end gap-2 px-4 py-4">
        <div className="flex items-baseline justify-between text-base">
          <span className="text-muted">{t("sell.totals.subtotal")}</span>
          <span className="tabular-nums">{formatMoney(preTaxSubtotal, currencyExponent)}</span>
        </div>
        {totals.discount_minor > 0 ? (
          <div className="flex items-baseline justify-between text-base">
            <span className="text-muted">{t("sell.totals.discount")}</span>
            <span className="tabular-nums">
              −{formatMoney(totals.discount_minor, currencyExponent)}
            </span>
          </div>
        ) : null}
        <div className="flex items-baseline justify-between text-base">
          <span className="text-muted">{t("sell.totals.tax")}</span>
          <span className="tabular-nums">{formatMoney(totals.tax_minor, currencyExponent)}</span>
        </div>
        {/* The key remounts the row on every total change, replaying the
            250 ms tint fade (tier 2: no tint at all). */}
        <div
          key={totals.total_minor}
          className={`-mx-2 flex items-baseline justify-between rounded-md px-2 py-2 ${
            motionTier === 2 ? "" : "total-flash"
          }`}
        >
          <span className="text-lg font-semibold">{t("sell.totals.total")}</span>
          <span className="text-[40px] font-bold leading-none tabular-nums">
            {formatMoney(totals.total_minor, currencyExponent)}
          </span>
        </div>
      </div>
      <div className="flex flex-col gap-2 border-t border-border px-4 py-3">
        <Button variant="primary" size="lg" onClick={onPayClick} disabled={disabled || !canPay} className="w-full">
          {t("sell.pay.button")}{" "}
          <kbd aria-hidden="true" className="rounded border border-white/40 px-1 text-sm font-normal">
            F9
          </kbd>
        </Button>
        <Button
          variant="secondary"
          size="lg"
          onClick={onDiscountClick}
          disabled={disabled || !canPay}
          className="w-full"
        >
          {t("sell.discount.button")}{" "}
          <kbd aria-hidden="true" className="rounded border border-border-strong px-1 text-sm font-normal">
            F6
          </kbd>
        </Button>
        <Button
          variant={cancelArmed ? "danger" : "secondary"}
          size="lg"
          onClick={onCancelClick}
          disabled={disabled}
          className="w-full"
        >
          {cancelArmed ? t("sell.cancel.confirm") : t("sell.cancel")}
        </Button>
      </div>
    </aside>
  );
}
