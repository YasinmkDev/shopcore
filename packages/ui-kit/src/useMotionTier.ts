import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { MotionOverride, SettingsStore } from "./SettingsStore";
import { localStorageSettingsStore } from "./SettingsStore";

/**
 * useMotionTier — the single source of the motion tier (spec 000b, req. 15;
 * context/10-ui-system.md "Adaptive tiers"). No component decides the tier
 * on its own; animation code reads it from this hook.
 *
 * Tiers: 0 full (p95 frame ≤ 20 ms), 1 reduced (p95 > 20 ms — no
 * layout/shared-element animations or springs; fades/slides kept),
 * 2 minimal (p95 > 33 ms — opacity-only, ≤ 100 ms).
 *
 * Precedence: manual override (Full/Reduced/Off) > OS
 * `prefers-reduced-motion` (forces tier 2) > auto tier (sampled, persisted
 * per terminal behind SettingsStore).
 *
 * Sampling rules (amended 2026-11-10): sampling starts only after first
 * idle (window `load` + ~1 s); a window is the rAF frame-time sample over
 * ~60 frames; a tier DOWNGRADE requires TWO consecutive bad windows; the
 * auto tier persists per terminal and is restored on load; a manual
 * override always wins over the sampler and the persisted auto tier.
 * An upgrade (a good window after a bad one) applies immediately.
 *
 * The hook sets `data-motion` ("full" | "reduced" | "minimal") on the app
 * root (<html>), so CSS can gate animations on it.
 */
export type MotionTier = 0 | 1 | 2;

export interface MotionTierDeps {
  /** Defaults to the localStorage-backed store. */
  store?: SettingsStore;
  /** Defaults to window.requestAnimationFrame. Injectable for tests. */
  requestAnimationFrame?: (callback: FrameRequestCallback) => number;
  /** Defaults to window.cancelAnimationFrame. Injectable for tests. */
  cancelAnimationFrame?: (handle: number) => void;
}

const TIER_NAMES: Record<MotionTier, "full" | "reduced" | "minimal"> = {
  0: "full",
  1: "reduced",
  2: "minimal",
};

const OVERRIDE_TIERS: Record<MotionOverride, MotionTier> = {
  full: 0,
  reduced: 1,
  off: 2,
};

/** Frames per sampling window (~60 per context/10). */
const WINDOW_FRAMES = 60;
/** Delay after first idle before the first window starts. */
const IDLE_DELAY_MS = 1000;
/** Gap between consecutive sampling windows. */
const BETWEEN_WINDOWS_MS = 1000;

function tierForP95(p95Ms: number): MotionTier {
  if (p95Ms > 33) return 2;
  if (p95Ms > 20) return 1;
  return 0;
}

function p95(values: readonly number[]): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  const index = Math.max(0, Math.ceil(0.95 * sorted.length) - 1);
  return sorted[index] ?? 0;
}

export function useMotionTier(deps: MotionTierDeps = {}): {
  tier: MotionTier;
  override: MotionOverride | null;
  setOverride: (override: MotionOverride | null) => void;
} {
  const store: SettingsStore = useMemo(
    () => deps.store ?? localStorageSettingsStore,
    [deps.store],
  );
  const raf = useMemo(
    () =>
      deps.requestAnimationFrame ??
      ((callback: FrameRequestCallback): number => window.requestAnimationFrame(callback)),
    [deps.requestAnimationFrame],
  );
  const caf = useMemo(
    () =>
      deps.cancelAnimationFrame ??
      ((handle: number): void => {
        window.cancelAnimationFrame(handle);
      }),
    [deps.cancelAnimationFrame],
  );

  const [autoTier, setAutoTier] = useState<MotionTier>(() => store.getMotionTier() ?? 0);
  const autoTierRef = useRef<MotionTier>(autoTier);
  const [override, setOverrideState] = useState<MotionOverride | null>(() =>
    store.getMotionOverride(),
  );
  const [reducedMotion, setReducedMotion] = useState<boolean>(() =>
    typeof window.matchMedia === "function"
      ? window.matchMedia("(prefers-reduced-motion: reduce)").matches
      : false,
  );

  const tier: MotionTier =
    override !== null ? OVERRIDE_TIERS[override] : reducedMotion ? 2 : autoTier;

  const setOverride = useCallback(
    (next: MotionOverride | null): void => {
      store.setMotionOverride(next);
      setOverrideState(next);
    },
    [store],
  );

  /* OS reduced-motion: listen for live changes. */
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const mql = window.matchMedia("(prefers-reduced-motion: reduce)");
    const onChange = (event: MediaQueryListEvent): void => {
      setReducedMotion(event.matches);
    };
    mql.addEventListener("change", onChange);
    return () => {
      mql.removeEventListener("change", onChange);
    };
  }, []);

  /* Publish the effective tier on the app root. */
  useEffect(() => {
    document.documentElement.setAttribute("data-motion", TIER_NAMES[tier]);
  }, [tier]);

  /* rAF sampler. Runs only when neither an override nor reduced-motion
     decides the tier. Restarts whenever the override/reduced-motion state
     changes (e.g. clearing an override resumes sampling). */
  useEffect(() => {
    if (override !== null || reducedMotion) return;
    let stopped = false;
    let rafId = 0;
    let timerId: ReturnType<typeof setTimeout> | undefined;
    let badWindows = 0;
    let loadListener: (() => void) | null = null;

    const applyAutoTier = (next: MotionTier): void => {
      if (next === autoTierRef.current) return;
      autoTierRef.current = next;
      store.setMotionTier(next);
      setAutoTier(next);
    };

    const sampleWindow = (): void => {
      const timestamps: number[] = [];
      const onFrame = (t: number): void => {
        if (stopped) return;
        timestamps.push(t);
        if (timestamps.length > WINDOW_FRAMES) {
          const deltas: number[] = [];
          for (let i = 1; i < timestamps.length; i++) {
            const prev = timestamps[i - 1];
            const cur = timestamps[i];
            if (prev !== undefined && cur !== undefined) deltas.push(cur - prev);
          }
          const candidate = tierForP95(p95(deltas));
          if (candidate > autoTierRef.current) {
            badWindows += 1;
            if (badWindows >= 2) {
              badWindows = 0;
              applyAutoTier(candidate);
            }
          } else {
            badWindows = 0;
            if (candidate < autoTierRef.current) applyAutoTier(candidate);
          }
          timerId = setTimeout(() => {
            if (!stopped) sampleWindow();
          }, BETWEEN_WINDOWS_MS);
        } else {
          rafId = raf(onFrame);
        }
      };
      rafId = raf(onFrame);
    };

    const startAfterIdle = (): void => {
      timerId = setTimeout(() => {
        if (!stopped) sampleWindow();
      }, IDLE_DELAY_MS);
    };

    if (document.readyState === "complete") {
      startAfterIdle();
    } else {
      const onLoad = (): void => {
        startAfterIdle();
      };
      loadListener = onLoad;
      window.addEventListener("load", onLoad, { once: true });
    }

    return () => {
      stopped = true;
      caf(rafId);
      if (timerId !== undefined) clearTimeout(timerId);
      if (loadListener !== null) window.removeEventListener("load", loadListener);
    };
  }, [override, reducedMotion, store, raf, caf]);

  return { tier, override, setOverride };
}
