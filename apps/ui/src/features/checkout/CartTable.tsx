/**
 * CartTable (spec 001c-1): the virtualized cart list. Left ~2/3 of the
 * sell screen; rows are 48 px (POS density, 10-ui-system). The newest
 * line animates in (100 ms opacity + 4 px translate; tier 2 =
 * opacity-only, ≤ 100 ms) — the animation class is cleared after it
 * plays so a scrolled-back row never re-animates.
 */
import { useRef } from "react";
import type { ReactElement } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { t } from "@shopcore/i18n";
import type { MotionTier } from "@shopcore/ui-kit";
import type { CartLineDto } from "@shopcore/protocol-ts";
import { formatMoney, formatQty, formatUnitPrice } from "../../lib/money";

export interface CartTableProps {
  lines: CartLineDto[];
  currencyExponent: number;
  /** The id of the line whose insert animation is playing, if any. */
  newLineId: string | null;
  onRowAnimationEnd: () => void;
  selectedLineId: string | null;
  onSelect: (lineId: string) => void;
  onRemove: (lineId: string) => void;
  motionTier: MotionTier;
}

export function CartTable({
  lines,
  currencyExponent,
  newLineId,
  onRowAnimationEnd,
  selectedLineId,
  onSelect,
  onRemove,
  motionTier,
}: CartTableProps): ReactElement {
  const parentRef = useRef<HTMLDivElement | null>(null);
  const virtualizer = useVirtualizer({
    count: lines.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 48,
    overscan: 5,
  });

  if (lines.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center px-6 py-10">
        <p className="text-base text-muted">{t("sell.cart.empty")}</p>
      </div>
    );
  }

  return (
    <div ref={parentRef} className="min-h-0 flex-1 overflow-y-auto" role="listbox" aria-label={t("sell.cart.title")}>
      <div
        className="relative w-full"
        style={{ height: `${String(virtualizer.getTotalSize())}px` }}
      >
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const line = lines[virtualRow.index];
          if (line === undefined) {
            return null;
          }
          const isNew = line.id === newLineId;
          const isSelected = line.id === selectedLineId;
          return (
            <div
              key={line.id}
              role="option"
              aria-selected={isSelected}
              tabIndex={0}
              onClick={() => {
                onSelect(line.id);
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  onSelect(line.id);
                }
              }}
              style={{
                position: "absolute",
                top: 0,
                left: 0,
                width: "100%",
                height: `${String(virtualRow.size)}px`,
                transform: `translateY(${String(virtualRow.start)}px)`,
              }}
            >
              <div
                onAnimationEnd={() => {
                  if (isNew) {
                    onRowAnimationEnd();
                  }
                }}
                className={`flex h-full cursor-pointer items-center gap-3 border-b border-border px-4 text-base ${
                  isSelected ? "bg-accent-subtle" : "bg-surface hover:bg-surface-subtle"
                } ${isNew ? (motionTier === 2 ? "cart-row-enter-minimal" : "cart-row-enter") : ""}`}
              >
                <span className="w-6 shrink-0 text-right text-muted">{virtualRow.index + 1}</span>
                <span className="min-w-0 flex-1 truncate font-medium">{line.name}</span>
                <span className="w-20 shrink-0 text-right tabular-nums">{formatQty(line.qty_milli)}</span>
                <span className="w-24 shrink-0 text-right tabular-nums text-muted">
                  {formatUnitPrice(line.unit_price_e4, currencyExponent)}
                </span>
                <span className="w-24 shrink-0 text-right tabular-nums font-semibold">
                  {formatMoney(line.line_total_minor, currencyExponent)}
                </span>
                <button
                  type="button"
                  aria-label={t("sell.cart.removeLineFor", { name: line.name })}
                  onClick={(event) => {
                    event.stopPropagation();
                    onRemove(line.id);
                  }}
                  className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md text-muted outline-none hover:bg-danger-bg hover:text-danger focus-visible:ring-2 focus-visible:ring-focus-ring"
                >
                  ✕
                </button>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
