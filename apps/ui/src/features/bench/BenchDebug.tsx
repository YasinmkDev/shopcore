/**
 * BenchDebug — debug-only benchmark route (spec 002, scenarios 7–8).
 *
 * Toggled with Ctrl+Shift+B. Not a product screen: plain strings (no i18n),
 * never linked from the UI. Measures inside the real WebView2:
 * - a 100k-row virtualized grid, scrolled for 10 s (frame p95 + long tasks)
 * - a synthetic cart, 50 rows added rapidly (re-renders per row)
 * - startup (process start → window interactive) and route-change time
 * "Save results" writes `shopcore-ui-results.json` to the OS temp dir via
 * the `bench_ui_results` debug command; `just bench --ui-results <that file>`
 * merges scenarios 7–8 into the results.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactElement } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useVirtualizer } from "@tanstack/react-virtual";

const ROW_COUNT = 100_000;
const SCROLL_SECONDS = 10;

interface Row {
  id: number;
  code: string;
  name: string;
}

interface UiResults {
  startup_ms: number;
  scroll_p95_ms: number;
  long_tasks: number;
  cart_rerenders_per_row: number;
  route_change_ms: number;
}

function percentile(sorted: number[], q: number): number {
  if (sorted.length === 0) {
    return 0;
  }
  const index = Math.min(sorted.length - 1, Math.floor((q / 100) * sorted.length));
  return sorted[index] ?? 0;
}

export function BenchDebug({ openedAtMs }: { openedAtMs: number }): ReactElement {
  const parentRef = useRef<HTMLDivElement | null>(null);
  const [startupMs, setStartupMs] = useState<number | null>(null);
  const [routeMs, setRouteMs] = useState<number | null>(null);
  const [scrollP95, setScrollP95] = useState<number | null>(null);
  const [longTasks, setLongTasks] = useState(0);
  const [scrolling, setScrolling] = useState(false);
  const [cart, setCart] = useState<Row[]>([]);
  const [cartRenders, setCartRenders] = useState(0);
  const [savedPath, setSavedPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const longTaskCount = useRef(0);

  const rows = useMemo<Row[]>(
    () =>
      Array.from({ length: ROW_COUNT }, (_, i) => ({
        id: i,
        code: `SKU-${String(i).padStart(6, "0")}`,
        name: `Item ${String(i)}`,
      })),
    [],
  );

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 36,
    overscan: 5,
  });

  // Startup + route-change timing on mount.
  useEffect(() => {
    setRouteMs(performance.now() - openedAtMs);
    void invoke<number>("bench_startup_ms")
      .then((ms) => {
        setStartupMs(ms);
      })
      .catch((err: unknown) => {
        setError(`bench_startup_ms failed: ${String(err)}`);
      });
  }, [openedAtMs]);

  // Count long tasks (> 50 ms) while this view is open.
  useEffect(() => {
    const observer = new PerformanceObserver((list) => {
      longTaskCount.current += list.getEntries().length;
      setLongTasks(longTaskCount.current);
    });
    observer.observe({ entryTypes: ["longtask"] });
    return () => {
      observer.disconnect();
    };
  }, []);

  // Count cart renders (debug-only side effect in the render path).
  const cartRenderCount = useRef(0);
  cartRenderCount.current += 1;
  useEffect(() => {
    setCartRenders(cartRenderCount.current);
  }, [cart]);

  const runScrollTest = useCallback(() => {
    const el = parentRef.current;
    if (el === null || scrolling) {
      return;
    }
    setScrolling(true);
    setScrollP95(null);
    longTaskCount.current = 0;
    setLongTasks(0);
    const frames: number[] = [];
    let last = performance.now();
    const endAt = last + SCROLL_SECONDS * 1000;
    const total = el.scrollHeight;
    const step = () => {
      const now = performance.now();
      frames.push(now - last);
      last = now;
      if (now < endAt) {
        // Sweep down then back up so the grid re-renders throughout.
        const progress = (now - (endAt - SCROLL_SECONDS * 1000)) / (SCROLL_SECONDS * 1000);
        el.scrollTop = total * (progress < 0.5 ? progress * 2 : (1 - progress) * 2);
        requestAnimationFrame(step);
      } else {
        frames.sort((a, b) => a - b);
        setScrollP95(percentile(frames, 95));
        setScrolling(false);
      }
    };
    requestAnimationFrame(step);
  }, [scrolling]);

  const addFiftyRows = useCallback(() => {
    const base = cart.length;
    for (let i = 0; i < 50; i += 1) {
      const row: Row = {
        id: base + i,
        code: `CART-${String(base + i)}`,
        name: `Cart item ${String(base + i)}`,
      };
      setCart((prev) => [...prev, row]);
    }
  }, [cart.length]);

  const results: UiResults | null = useMemo(
    () =>
      startupMs !== null && routeMs !== null
        ? {
            startup_ms: startupMs,
            scroll_p95_ms: scrollP95 ?? 0,
            long_tasks: longTasks,
            cart_rerenders_per_row: cart.length > 0 ? cartRenders / cart.length : 0,
            route_change_ms: routeMs,
          }
        : null,
    [startupMs, routeMs, scrollP95, longTasks, cartRenders, cart.length],
  );

  const saveResults = useCallback(() => {
    if (results === null) {
      return;
    }
    void invoke<string>("bench_ui_results", { payload: JSON.stringify(results, null, 2) })
      .then((path) => {
        setSavedPath(path);
      })
      .catch((err: unknown) => {
        setError(`bench_ui_results failed: ${String(err)}`);
      });
  }, [results]);

  const copyJson = useCallback(() => {
    if (results === null) {
      return;
    }
    void navigator.clipboard.writeText(JSON.stringify(results, null, 2));
  }, [results]);

  return (
    <div className="flex min-h-screen flex-col bg-bg p-4 text-text">
      <h1 className="text-lg font-semibold">Bench debug (Ctrl+Shift+B to close)</h1>
      <p className="text-sm text-muted">
        Debug-only. Measures scenarios 7–8 inside the real WebView2. Save the results, then run{" "}
        <code>just bench --ui-results &lt;path&gt;</code>.
      </p>
      {error !== null && <p className="mt-2 text-sm text-danger">{error}</p>}
      <div className="mt-4 grid grid-cols-2 gap-4">
        <div className="rounded-lg border border-border bg-surface p-3">
          <h2 className="font-medium">Startup / route</h2>
          <p className="text-sm">Startup (process → window): {startupMs?.toFixed(1) ?? "…"} ms</p>
          <p className="text-sm">Route change (first paint): {routeMs?.toFixed(1) ?? "…"} ms</p>
          <p className="text-sm">Budget: startup &lt; 4000 ms, route &lt; 150 ms</p>
        </div>
        <div className="rounded-lg border border-border bg-surface p-3">
          <h2 className="font-medium">Scroll test (100k rows, 10 s)</h2>
          <button
            type="button"
            onClick={runScrollTest}
            disabled={scrolling}
            className="mt-1 rounded bg-accent px-3 py-1 text-sm text-on-accent disabled:opacity-50"
          >
            {scrolling ? "Scrolling…" : "Run scroll test"}
          </button>
          <p className="mt-1 text-sm">Frame p95: {scrollP95?.toFixed(2) ?? "—"} ms (budget 16.7 ms)</p>
          <p className="text-sm">Long tasks (&gt; 50 ms): {longTasks}</p>
        </div>
        <div className="rounded-lg border border-border bg-surface p-3">
          <h2 className="font-medium">Synthetic cart</h2>
          <button
            type="button"
            onClick={addFiftyRows}
            className="mt-1 rounded bg-accent px-3 py-1 text-sm text-on-accent"
          >
            Add 50 rows
          </button>
          <p className="mt-1 text-sm">Rows: {cart.length}</p>
          <p className="text-sm">
            Re-renders per row:{" "}
            {cart.length > 0 ? (cartRenders / cart.length).toFixed(2) : "—"}
          </p>
        </div>
        <div className="rounded-lg border border-border bg-surface p-3">
          <h2 className="font-medium">Results</h2>
          <button
            type="button"
            onClick={saveResults}
            disabled={results === null}
            className="mt-1 rounded bg-accent px-3 py-1 text-sm text-on-accent disabled:opacity-50"
          >
            Save results
          </button>{" "}
          <button
            type="button"
            onClick={copyJson}
            disabled={results === null}
            className="mt-1 rounded border border-border px-3 py-1 text-sm disabled:opacity-50"
          >
            Copy JSON
          </button>
          {savedPath !== null && <p className="mt-1 text-sm">Saved: {savedPath}</p>}
          <pre className="mt-2 overflow-auto text-xs">
            {results === null ? "…" : JSON.stringify(results, null, 2)}
          </pre>
        </div>
      </div>
      <div ref={parentRef} className="mt-4 h-96 overflow-auto rounded-lg border border-border">
        <div
          style={{ height: `${String(virtualizer.getTotalSize())}px`, position: "relative" }}
        >
          {virtualizer.getVirtualItems().map((item) => {
            const row = rows[item.index];
            return (
              <div
                key={row?.id ?? item.index}
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  height: `${String(item.size)}px`,
                  transform: `translateY(${String(item.start)}px)`,
                }}
                className="flex items-center gap-4 border-b border-border px-3 text-sm"
              >
                <span>{row?.code}</span>
                <span>{row?.name}</span>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
