/**
 * DevBanner (001c-1 follow-up 4): the dev-build banner on the sell
 * screen. Rendered only when the till status flags a dev build
 * (`status.dev_build`). Shows the "DEV BUILD — sample data" banner
 * and the list of sample barcodes (barcode, name, price, tax mode)
 * from the dev seed, so the tester can scan without a real catalog.
 * Never renders in a release build.
 */
import { useEffect, useState } from "react";
import type { ReactElement } from "react";
import { t } from "@shopcore/i18n";
import type { SampleBarcodeDto } from "@shopcore/protocol-ts";
import type { CoreClient } from "../../lib/core-client";
import { formatMoney } from "../../lib/money";

/** The tax-mode label for a sample row, with the rate when taxed. */
function taxLabel(row: SampleBarcodeDto): string {
  const mode =
    row.tax_mode === "inclusive"
      ? t("dev.tax.inclusive")
      : row.tax_mode === "zero"
        ? t("dev.tax.zero")
        : t("dev.tax.exclusive");
  return row.rate_bp === 0 ? mode : `${mode} ${String(row.rate_bp / 100)}%`;
}

export function DevBanner({
  client,
  currencyExponent,
}: {
  client: CoreClient;
  currencyExponent: number;
}): ReactElement {
  const [rows, setRows] = useState<SampleBarcodeDto[] | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    client
      .listSampleBarcodes()
      .then((next) => {
        if (!cancelled) {
          setRows(next);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setFailed(true);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [client]);

  return (
    <section
      aria-label={t("dev.banner.title")}
      className="mb-3 rounded-md bg-warning-bg px-3 py-2 text-base text-warning"
    >
      <p className="font-medium">{t("dev.banner.title")}</p>
      <p className="text-sm">{t("dev.banner.note")}</p>
      {failed ? (
        <p className="mt-1 text-sm">{t("dev.barcodes.error")}</p>
      ) : rows === null ? null : rows.length === 0 ? (
        <p className="mt-1 text-sm">{t("dev.barcodes.empty")}</p>
      ) : (
        <ul aria-label={t("dev.barcodes.title")} className="mt-2 space-y-1 text-sm">
          {rows.map((row) => (
            <li key={row.barcode} className="flex flex-wrap items-baseline gap-x-2">
              <span className="font-medium">{row.name}</span>
              <span>{row.barcode}</span>
              <span>{formatMoney(row.price_minor, currencyExponent)}</span>
              <span>{taxLabel(row)}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
