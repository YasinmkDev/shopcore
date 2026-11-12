/**
 * SellScreen (spec 001c-1): the checkout core. Scan input on top,
 * cart (left ~2/3) + totals (right ~1/3) below, status bar at the
 * bottom. No PAY button (001c-2); F9/F6 are NOT bound (amendment 2).
 *
 * Keyboard: Enter submit, Esc clear input / close results / disarm
 * cancel, ↑/↓ search highlight when results are open else cart
 * selection, +/− quantity on the selected line, Del remove, Ctrl+Z
 * undo (restore the last removed line), `*N` + Enter sets the pending
 * quantity for the next scan.
 *
 * Focus: the scan input is focused on mount and refocused after
 * every action (focus return < 100 ms, req. 11).
 *
 * Accessibility (amendment 7): a `role="status"` live region
 * announces the scanned item and the new total.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactElement } from "react";
import { useMotionTier } from "@shopcore/ui-kit";
import { t } from "@shopcore/i18n";
import type { CartViewDto, SkuHitDto } from "@shopcore/protocol-ts";
import type { CoreClient } from "../../lib/core-client";
import { formatMoney, formatQty } from "../../lib/money";
import { useSession } from "./SessionProvider";
import { ScanInput } from "./ScanInput";
import { CartTable } from "./CartTable";
import { TotalsPanel } from "./TotalsPanel";
import { StatusBar } from "./StatusBar";

/** The stable error code the Rust side returns for an unknown barcode. */
const BARCODE_NOT_FOUND = "BARCODE_NOT_FOUND";

function errorCode(err: unknown): string | null {
  if (typeof err === "object" && err !== null && "code" in err) {
    const code: unknown = err.code;
    return typeof code === "string" ? code : null;
  }
  return null;
}

/** Parse a `*N` quantity command (N whole units) — null when not one. */
function parseQtyCommand(value: string): number | null {
  const unitsText = /^\*(\d+)$/.exec(value.trim())?.[1];
  if (unitsText === undefined) {
    return null;
  }
  const units = Number(unitsText);
  if (!Number.isSafeInteger(units) || units <= 0) {
    return null;
  }
  return units * 1000;
}

export function SellScreen({ client }: { client: CoreClient }): ReactElement {
  const { session, status, loading: sessionLoading, error: sessionError, refresh } = useSession();
  const { tier } = useMotionTier();

  const [cart, setCart] = useState<CartViewDto | null>(null);
  const [cartError, setCartError] = useState(false);
  const [input, setInput] = useState("");
  const [results, setResults] = useState<SkuHitDto[]>([]);
  const [highlight, setHighlight] = useState(0);
  const [inlineMessage, setInlineMessage] = useState<string | null>(null);
  const [pendingQtyMilli, setPendingQtyMilli] = useState<number | null>(null);
  const [selectedLineId, setSelectedLineId] = useState<string | null>(null);
  const [newLineId, setNewLineId] = useState<string | null>(null);
  const [cancelArmed, setCancelArmed] = useState(false);
  const [announcement, setAnnouncement] = useState("");
  // Printer status comes from the till status; a hardware.status event
  // overrides it live until the next status load.
  const [printerOverride, setPrinterOverride] = useState<string | null>(null);
  const printerStatus = printerOverride ?? status?.printer_status ?? "unknown";
  // Synchronous in-flight guard. A state flag is NOT enough here:
  // the barcode-lookup -> add chain would read a stale `busy` and
  // drop the scan (React state updates are async).
  const busyRef = useRef(false);

  /** Claim the in-flight slot; false when a command is already running. */
  const beginWork = (): boolean => {
    if (busyRef.current) {
      return false;
    }
    busyRef.current = true;
    return true;
  };
  const endWork = (): void => {
    busyRef.current = false;
  };

  const inputRef = useRef<HTMLInputElement | null>(null);
  const searchTimerRef = useRef<number | null>(null);
  const cancelTimerRef = useRef<number | null>(null);

  const focusScan = useCallback(() => {
    inputRef.current?.focus();
  }, []);

  const announce = useCallback((message: string) => {
    setAnnouncement(message);
  }, []);

  const applyCart = useCallback(
    (next: CartViewDto, addedLineId: string | null, addedText?: string) => {
      setCart(next);
      const selected =
        addedLineId !== null && next.lines.some((line) => line.id === addedLineId)
          ? addedLineId
          : (next.lines[next.lines.length - 1]?.id ?? null);
      setSelectedLineId(selected);
      setNewLineId(addedLineId);
      if (addedText !== undefined) {
        announce(
          `${addedText} — ${t("sell.announce.total", {
            total: formatMoney(next.totals.total_minor, next.currency_exponent),
          })}`,
        );
      }
      focusScan();
    },
    [announce, focusScan],
  );

  // Initial cart load + live printer status.
  useEffect(() => {
    let cancelled = false;
    client
      .getCart()
      .then((next) => {
        if (!cancelled) {
          setCart(next);
          setCartError(false);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setCartError(true);
        }
      });
    const unsubscribe = client.subscribe("hardware.status", (payload) => {
      setPrinterOverride(payload.status);
    });
    return () => {
      cancelled = true;
      unsubscribe();
    };
  }, [client]);

  // Clear the search debounce + the cancel-arm timer on unmount.
  useEffect(
    () => () => {
      if (searchTimerRef.current !== null) {
        window.clearTimeout(searchTimerRef.current);
      }
      if (cancelTimerRef.current !== null) {
        window.clearTimeout(cancelTimerRef.current);
      }
    },
    [],
  );

  const clearSearchTimer = (): void => {
    if (searchTimerRef.current !== null) {
      window.clearTimeout(searchTimerRef.current);
      searchTimerRef.current = null;
    }
  };

  const runSearch = useCallback(
    (value: string) => {
      if (searchTimerRef.current !== null) {
        window.clearTimeout(searchTimerRef.current);
      }
      if (value.trim().length < 2 || value.startsWith("*")) {
        setResults([]);
        return;
      }
      searchTimerRef.current = window.setTimeout(() => {
        void client.searchSkus(value).then((hits) => {
          setResults(hits);
          setHighlight(0);
        });
      }, 150);
    },
    [client],
  );

  const handleInputChange = (value: string): void => {
    setInput(value);
    setInlineMessage(null);
    runSearch(value);
  };

  const addHit = (hit: SkuHitDto): void => {
    if (cart === null) {
      return;
    }
    clearSearchTimer();
    if (hit.price_e4 === null) {
      setInlineMessage(t("sell.noPrice", { name: hit.name }));
      setInput("");
      setResults([]);
      focusScan();
      return;
    }
    if (!beginWork()) {
      return;
    }
    const qtyMilli = pendingQtyMilli ?? 1000;
    const existing = cart.lines.find(
      (line) => line.sku_id === hit.sku_id && line.unit_price_e4 === hit.price_e4,
    );
    client
      .draftAddLine(cart.draft_id, hit.sku_id, qtyMilli, hit.price_e4)
      .then((next) => {
        const addedId =
          existing?.id ?? next.lines.find((line) => !cart.lines.some((old) => old.id === line.id))?.id ?? null;
        setPendingQtyMilli(null);
        setInput("");
        setResults([]);
        applyCart(
          next,
          addedId,
          t("sell.announce.added", { qty: formatQty(qtyMilli), name: hit.name }),
        );
      })
      .catch(() => {
        setInlineMessage(t("sell.error.generic"));
        focusScan();
      })
      .finally(endWork);
  };

  const handleSubmit = (): void => {
    clearSearchTimer();
    const value = input.trim();
    if (value === "") {
      return;
    }
    // `*N` + Enter: set the pending quantity for the next scan.
    const qtyCommand = parseQtyCommand(value);
    if (qtyCommand !== null) {
      setPendingQtyMilli(qtyCommand);
      setInput("");
      setResults([]);
      focusScan();
      return;
    }
    if (results.length > 0) {
      const hit = results[highlight];
      if (hit !== undefined) {
        addHit(hit);
      }
      return;
    }
    // No search hit: treat the input as a scanned barcode.
    if (cart === null || !beginWork()) {
      return;
    }
    client
      .lookupBarcode(value)
      .then((hit) => {
        endWork();
        addHit(hit);
      })
      .catch((err: unknown) => {
        endWork();
        if (errorCode(err) === BARCODE_NOT_FOUND) {
          // Inline message with the scanned code; the cart is kept,
          // and there is no "Add item" action until the catalog spec.
          setInlineMessage(t("sell.barcodeNotFound", { code: value }));
        } else {
          setInlineMessage(t("sell.error.generic"));
        }
        setInput("");
        setResults([]);
        focusScan();
      });
  };

  const handleEscape = (): void => {
    clearSearchTimer();
    if (cancelArmed) {
      setCancelArmed(false);
      return;
    }
    setInput("");
    setResults([]);
    setInlineMessage(null);
    focusScan();
  };

  const moveHighlight = (delta: number): void => {
    if (results.length > 0) {
      setHighlight((current) => {
        const next = current + delta;
        if (next < 0) {
          return results.length - 1;
        }
        if (next >= results.length) {
          return 0;
        }
        return next;
      });
      return;
    }
    if (cart !== null && cart.lines.length > 0) {
      setSelectedLineId((current) => {
        const index = cart.lines.findIndex((line) => line.id === current);
        const next = index < 0 ? 0 : index + delta;
        const clamped = Math.max(0, Math.min(cart.lines.length - 1, next));
        return cart.lines[clamped]?.id ?? null;
      });
    }
  };

  const adjustSelectedQty = (deltaMilli: number): void => {
    if (cart === null || !beginWork()) {
      return;
    }
    const line = cart.lines.find((candidate) => candidate.id === selectedLineId)
      ?? cart.lines[cart.lines.length - 1];
    if (line === undefined) {
      return;
    }
    const nextQty = line.qty_milli + deltaMilli;
    if (nextQty < 0) {
      endWork();
      return;
    }
    client
      .draftSetQty(cart.draft_id, line.id, nextQty)
      .then((next) => {
        applyCart(next, line.id);
      })
      .catch(() => {
        setInlineMessage(t("sell.error.generic"));
        focusScan();
      })
      .finally(endWork);
  };

  /** Remove one line by id (shared by Del and the row ✕ button). */
  const removeLine = (lineId: string): void => {
    if (cart === null || !beginWork()) {
      return;
    }
    const line = cart.lines.find((candidate) => candidate.id === lineId);
    if (line === undefined) {
      endWork();
      return;
    }
    client
      .draftRemoveLine(cart.draft_id, line.id)
      .then((next) => {
        applyCart(next, null, t("sell.announce.removed", { name: line.name }));
      })
      .catch(() => {
        setInlineMessage(t("sell.error.generic"));
        focusScan();
      })
      .finally(endWork);
  };

  const removeSelected = (): void => {
    const line = cart?.lines.find((candidate) => candidate.id === selectedLineId)
      ?? cart?.lines[cart.lines.length - 1];
    if (line !== undefined) {
      removeLine(line.id);
    }
  };

  const restoreLine = (): void => {
    if (cart === null || !beginWork()) {
      return;
    }
    client
      .draftRestoreLine(cart.draft_id)
      .then((next) => {
        const restored = next.lines.find((line) => !cart.lines.some((old) => old.id === line.id));
        applyCart(
          next,
          restored?.id ?? null,
          restored === undefined ? undefined : t("sell.announce.restored", { name: restored.name }),
        );
      })
      .catch(() => {
        setInlineMessage(t("sell.error.generic"));
        focusScan();
      })
      .finally(endWork);
  };

  const handleCancelClick = (): void => {
    if (cart === null) {
      return;
    }
    if (!cancelArmed) {
      setCancelArmed(true);
      if (cancelTimerRef.current !== null) {
        window.clearTimeout(cancelTimerRef.current);
      }
      cancelTimerRef.current = window.setTimeout(() => {
        setCancelArmed(false);
      }, 3000);
      return;
    }
    if (cancelTimerRef.current !== null) {
      window.clearTimeout(cancelTimerRef.current);
    }
    setCancelArmed(false);
    if (!beginWork()) {
      return;
    }
    client
      .draftCancel(cart.draft_id)
      .then((next) => {
        setInput("");
        setResults([]);
        setInlineMessage(null);
        setPendingQtyMilli(null);
        applyCart(next, null, t("sell.announce.cancelled"));
      })
      .catch(() => {
        setInlineMessage(t("sell.error.generic"));
        focusScan();
      })
      .finally(endWork);
  };

  if (sessionLoading) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <p className="text-base text-muted">{t("sell.cart.loading")}</p>
      </div>
    );
  }

  if (sessionError) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-4">
        <p className="text-base text-danger">{t("sell.error.load")}</p>
        <button
          type="button"
          onClick={refresh}
          className="rounded-md border border-border-strong bg-surface px-4 py-2 text-base outline-none focus-visible:ring-2 focus-visible:ring-focus-ring"
        >
          {t("sell.retry")}
        </button>
      </div>
    );
  }

  if (session === null) {
    // No session and no fake signed-in user: sign-in lands in 001d.
    return (
      <div className="flex flex-1 items-center justify-center px-6">
        <p className="text-base text-muted">{t("sell.session.none")}</p>
      </div>
    );
  }

  if (cartError || cart === null) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-4">
        <p className="text-base text-danger">{t("sell.error.load")}</p>
        <button
          type="button"
          onClick={() => {
            void client.getCart().then((next) => {
              setCart(next);
              setCartError(false);
            });
          }}
          className="rounded-md border border-border-strong bg-surface px-4 py-2 text-base outline-none focus-visible:ring-2 focus-visible:ring-focus-ring"
        >
          {t("sell.retry")}
        </button>
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="relative shrink-0 px-4 pt-4">
        <ScanInput
          value={input}
          onChange={handleInputChange}
          onSubmit={handleSubmit}
          onEscape={handleEscape}
          onArrowUp={() => {
            moveHighlight(-1);
          }}
          onArrowDown={() => {
            moveHighlight(1);
          }}
          onAdjustQty={adjustSelectedQty}
          onDelete={removeSelected}
          onRestore={restoreLine}
          inputRef={inputRef}
        />
        {pendingQtyMilli !== null ? (
          <p className="mt-1 text-sm text-muted">{t("sell.qty.next", { qty: formatQty(pendingQtyMilli) })}</p>
        ) : null}
        <p className="mt-1 text-sm text-faint">{t("sell.shortcuts.hint")}</p>
        {inlineMessage !== null ? (
          <p role="alert" className="mt-2 rounded-md bg-warning-bg px-3 py-2 text-base text-warning">
            {inlineMessage}
          </p>
        ) : null}
        {results.length > 0 ? (
          <ul
            role="listbox"
            aria-label={t("sell.search.results")}
            className="absolute left-4 right-4 z-10 mt-1 max-h-72 overflow-y-auto rounded-md border border-border-strong bg-surface shadow-pop"
          >
            {results.map((hit, index) => (
              <li
                key={hit.sku_id}
                role="option"
                aria-selected={index === highlight}
                onMouseDown={(event) => {
                  // MouseDown (not click): the input keeps focus.
                  event.preventDefault();
                  addHit(hit);
                }}
                onMouseEnter={() => {
                  setHighlight(index);
                }}
                className={`cursor-pointer px-4 py-2 text-base ${
                  index === highlight ? "bg-accent-subtle" : ""
                }`}
              >
                <span className="font-medium">{hit.name}</span>
                <span className="ml-2 text-muted">{hit.code}</span>
              </li>
            ))}
          </ul>
        ) : null}
      </div>

      <div className="mt-4 flex min-h-0 flex-1 border-t border-border">
        <div className="flex min-h-0 flex-1 flex-col">
          <CartTable
            lines={cart.lines}
            currencyExponent={cart.currency_exponent}
            newLineId={newLineId}
            onRowAnimationEnd={() => {
              setNewLineId(null);
            }}
            selectedLineId={selectedLineId}
            onSelect={(lineId) => {
              setSelectedLineId(lineId);
              focusScan();
            }}
            onRemove={removeLine}
            motionTier={tier}
          />
        </div>
        <TotalsPanel
          totals={cart.totals}
          currencyExponent={cart.currency_exponent}
          motionTier={tier}
          cancelArmed={cancelArmed}
          onCancelClick={handleCancelClick}
        />
      </div>

      <StatusBar
        shiftOpen={status?.shift_open ?? false}
        printerStatus={printerStatus}
        syncConfigured={status?.sync_configured ?? false}
        terminalCode={session.terminal_code}
        username={session.username ?? ""}
      />

      {/* Amendment 7: announces the scanned item and the new total. */}
      <div aria-live="polite" role="status" aria-label={t("sell.announce.label")} className="sr-only">
        {announcement}
      </div>
    </div>
  );
}
