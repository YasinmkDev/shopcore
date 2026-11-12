/**
 * ReceiptPreview (spec 001c-2): the completed sale's receipt, as the
 * completion view shows it. The numbers come straight from the
 * `SaleCompletedDto.receipt` — the UI never recomputes them.
 *
 * The subtotal line follows the context/02 rule (pre-tax subtotal =
 * `net_subtotal + discounts`), so `subtotal − discounts + tax == total`
 * holds in both tax modes; the rounding line shows only when it is
 * non-zero (cash rounding is OFF by default). Below the receipt: the
 * print state (printed / queued with the offline banner) and the
 * Reprint + New sale actions.
 */
import type { ReactElement } from "react";
import { Button } from "@shopcore/ui-kit";
import { t } from "@shopcore/i18n";
import type { ReceiptDto } from "@shopcore/protocol-ts";
import { formatMoney, formatQty, formatUnitPrice } from "../../lib/money";

export interface ReceiptPreviewProps {
  receipt: ReceiptDto;
  currencyExponent: number;
  /** True while the receipt is still queued (printer offline). */
  receiptQueued: boolean;
  reprintBusy: boolean;
  onReprint: () => void;
  onNewSale: () => void;
}

export function ReceiptPreview({
  receipt,
  currencyExponent,
  receiptQueued,
  reprintBusy,
  onReprint,
  onNewSale,
}: ReceiptPreviewProps): ReactElement {
  // Pre-tax subtotal (02 rule) — the same number TotalsPanel shows.
  const preTaxSubtotal = receipt.net_subtotal_minor + receipt.discount_minor;
  return (
    <div className="flex w-[360px] flex-col gap-4 rounded-md border border-border bg-surface p-4">
      <div>
        <h2 className="text-base font-semibold">{t("sell.receipt.title")}</h2>
        <p className="text-sm text-muted">{receipt.receipt_no}</p>
      </div>
      {receipt.header.map((line) => (
        <p key={line} className="text-center text-sm text-muted">
          {line}
        </p>
      ))}
      <ul className="flex flex-col gap-1 border-t border-border pt-3">
        {receipt.lines.map((line, index) => (
          <li
            // Receipt lines are static (never reordered); the index
            // keeps the key unique when two lines share a name.
            key={`${line.name}-${String(index)}`}
            className="flex items-baseline justify-between gap-2 text-base"
          >
            <span>
              {line.name}{" "}
              <span className="text-muted">
                {formatQty(line.qty_milli)} × {formatUnitPrice(line.unit_price_e4, currencyExponent)}
              </span>
            </span>
            <span className="tabular-nums">{formatMoney(line.line_total_minor, currencyExponent)}</span>
          </li>
        ))}
      </ul>
      <div className="flex flex-col gap-1 border-t border-border pt-3 text-base">
        <div className="flex items-baseline justify-between">
          <span className="text-muted">{t("sell.totals.subtotal")}</span>
          <span className="tabular-nums">{formatMoney(preTaxSubtotal, currencyExponent)}</span>
        </div>
        {receipt.discount_minor > 0 ? (
          <div className="flex items-baseline justify-between">
            <span className="text-muted">{t("sell.totals.discount")}</span>
            <span className="tabular-nums">
              −{formatMoney(receipt.discount_minor, currencyExponent)}
            </span>
          </div>
        ) : null}
        <div className="flex items-baseline justify-between">
          <span className="text-muted">{t("sell.totals.tax")}</span>
          <span className="tabular-nums">{formatMoney(receipt.tax_minor, currencyExponent)}</span>
        </div>
        {receipt.rounding_line_minor !== 0 ? (
          <div className="flex items-baseline justify-between">
            <span className="text-muted">{t("sell.receipt.rounding")}</span>
            <span className="tabular-nums">
              {formatMoney(receipt.rounding_line_minor, currencyExponent)}
            </span>
          </div>
        ) : null}
        <div className="flex items-baseline justify-between text-lg font-semibold">
          <span>{t("sell.totals.total")}</span>
          <span className="tabular-nums">{formatMoney(receipt.total_minor, currencyExponent)}</span>
        </div>
        <div className="flex items-baseline justify-between">
          <span className="text-muted">{t("sell.receipt.tender")}</span>
          <span className="tabular-nums">{formatMoney(receipt.tender_minor, currencyExponent)}</span>
        </div>
        <div className="flex items-baseline justify-between">
          <span className="text-muted">{t("sell.receipt.change")}</span>
          <span className="tabular-nums">{formatMoney(receipt.change_minor, currencyExponent)}</span>
        </div>
      </div>
      {receipt.footer.map((line) => (
        <p key={line} className="text-center text-sm text-muted">
          {line}
        </p>
      ))}
      {receiptQueued ? (
        <p role="alert" className="rounded-md bg-warning-bg px-3 py-2 text-base text-warning">
          {t("sell.complete.queued")}
        </p>
      ) : (
        <p className="rounded-md bg-surface-subtle px-3 py-2 text-base text-muted">
          {t("sell.complete.printed")}
        </p>
      )}
      <div className="flex gap-2">
        <Button variant="secondary" size="md" onClick={onReprint} disabled={reprintBusy} className="flex-1">
          {t("sell.complete.reprint")}
        </Button>
        <Button variant="primary" size="md" onClick={onNewSale} className="flex-1">
          {t("sell.complete.newSale")}
        </Button>
      </div>
    </div>
  );
}
