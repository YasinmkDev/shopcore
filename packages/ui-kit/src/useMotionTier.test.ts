import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMemorySettingsStore } from "./SettingsStore";
import type { SettingsStore } from "./SettingsStore";
import { useMotionTier } from "./useMotionTier";
import type { MotionTierDeps } from "./useMotionTier";

/**
 * Controllable rAF: the hook asks for frames, the test drives them with
 * exact timestamps. A window is 61 callbacks (60 frame deltas).
 */
interface RafHarness {
  queue: { id: number; cb: FrameRequestCallback }[];
  raf: (cb: FrameRequestCallback) => number;
  caf: (id: number) => void;
}

function createRafHarness(): RafHarness {
  const queue: { id: number; cb: FrameRequestCallback }[] = [];
  let nextId = 1;
  return {
    queue,
    raf: (cb: FrameRequestCallback): number => {
      const id = nextId;
      nextId += 1;
      queue.push({ id, cb });
      return id;
    },
    caf: (id: number): void => {
      const index = queue.findIndex((entry) => entry.id === id);
      if (index >= 0) queue.splice(index, 1);
    },
  };
}

/** Invoke `count` queued rAF callbacks, advancing the clock by `stepMs` each frame. */
function driveFrames(harness: RafHarness, count: number, stepMs: number, startAt: number): number {
  let t = startAt;
  for (let i = 0; i < count; i++) {
    const entry = harness.queue.shift();
    if (entry === undefined) throw new Error(`no queued rAF callback at frame ${String(i)}`);
    entry.cb(t);
    t += stepMs;
  }
  return t;
}

/** One sampling window (61 callbacks = 60 deltas) at a fixed frame time. */
function runWindow(harness: RafHarness, stepMs: number, startAt: number): number {
  return driveFrames(harness, 61, stepMs, startAt);
}

function stubMatchMedia(initialMatches: boolean): {
  setMatches: (value: boolean) => void;
} {
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  const mql = {
    matches: initialMatches,
    media: "(prefers-reduced-motion: reduce)",
    addEventListener: (_type: string, cb: (event: MediaQueryListEvent) => void): void => {
      listeners.add(cb);
    },
    removeEventListener: (_type: string, cb: (event: MediaQueryListEvent) => void): void => {
      listeners.delete(cb);
    },
  };
  vi.stubGlobal("matchMedia", vi.fn(() => mql));
  return {
    setMatches: (value: boolean): void => {
      mql.matches = value;
      for (const cb of listeners) cb({ matches: value } as MediaQueryListEvent);
    },
  };
}

describe("useMotionTier", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    stubMatchMedia(false);
    document.documentElement.removeAttribute("data-motion");
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.documentElement.removeAttribute("data-motion");
  });

  function mountHook(store: SettingsStore, harness: RafHarness) {
    const deps: MotionTierDeps = {
      store,
      requestAnimationFrame: harness.raf,
      cancelAnimationFrame: harness.caf,
    };
    return renderHook(() => useMotionTier(deps));
  }

  function advance(ms: number): void {
    act(() => {
      vi.advanceTimersByTime(ms);
    });
  }

  it("does not sample before first idle (load + ~1 s)", () => {
    const harness = createRafHarness();
    mountHook(createMemorySettingsStore(), harness);
    expect(harness.queue).toHaveLength(0);
    advance(500);
    expect(harness.queue).toHaveLength(0);
    advance(500);
    expect(harness.queue).toHaveLength(1);
  });

  it("waits for the load event when the document is still loading", () => {
    Object.defineProperty(document, "readyState", { value: "loading", configurable: true });
    try {
      const harness = createRafHarness();
      mountHook(createMemorySettingsStore(), harness);
      advance(5000);
      expect(harness.queue).toHaveLength(0);
      act(() => {
        window.dispatchEvent(new Event("load"));
      });
      advance(500);
      expect(harness.queue).toHaveLength(0);
      advance(500);
      expect(harness.queue).toHaveLength(1);
    } finally {
      Object.defineProperty(document, "readyState", {
        value: "complete",
        configurable: true,
      });
    }
  });

  it("stays at tier 0 on good frames (p95 ≤ 20 ms) and sets data-motion", () => {
    const store = createMemorySettingsStore();
    const harness = createRafHarness();
    const { result } = mountHook(store, harness);
    expect(result.current.tier).toBe(0);
    expect(document.documentElement.getAttribute("data-motion")).toBe("full");
    advance(1000);
    act(() => {
      runWindow(harness, 16.7, 1000);
    });
    expect(result.current.tier).toBe(0);
    expect(store.getMotionTier()).toBeNull();
  });

  it("does NOT downgrade after a single bad window", () => {
    const store = createMemorySettingsStore();
    const harness = createRafHarness();
    const { result } = mountHook(store, harness);
    advance(1000);
    act(() => {
      runWindow(harness, 25, 1000);
    });
    expect(result.current.tier).toBe(0);
    expect(store.getMotionTier()).toBeNull();
    // A good window resets the consecutive-bad counter.
    advance(1000);
    act(() => {
      runWindow(harness, 16.7, 5000);
    });
    advance(1000);
    act(() => {
      runWindow(harness, 25, 9000);
    });
    expect(result.current.tier).toBe(0);
  });

  it("downgrades to tier 1 after TWO consecutive bad windows (p95 > 20 ms)", () => {
    const store = createMemorySettingsStore();
    const harness = createRafHarness();
    const { result } = mountHook(store, harness);
    advance(1000);
    act(() => {
      runWindow(harness, 25, 1000);
    });
    expect(result.current.tier).toBe(0);
    advance(1000);
    act(() => {
      runWindow(harness, 25, 5000);
    });
    expect(result.current.tier).toBe(1);
    expect(document.documentElement.getAttribute("data-motion")).toBe("reduced");
    expect(store.getMotionTier()).toBe(1);
  });

  it("downgrades to tier 2 after two consecutive very bad windows (p95 > 33 ms)", () => {
    const store = createMemorySettingsStore();
    const harness = createRafHarness();
    const { result } = mountHook(store, harness);
    advance(1000);
    act(() => {
      runWindow(harness, 40, 1000);
    });
    advance(1000);
    act(() => {
      runWindow(harness, 40, 5000);
    });
    expect(result.current.tier).toBe(2);
    expect(document.documentElement.getAttribute("data-motion")).toBe("minimal");
    expect(store.getMotionTier()).toBe(2);
  });

  it("prefers-reduced-motion forces tier 2 and never samples", () => {
    stubMatchMedia(true);
    const store = createMemorySettingsStore();
    const harness = createRafHarness();
    const { result } = mountHook(store, harness);
    expect(result.current.tier).toBe(2);
    expect(document.documentElement.getAttribute("data-motion")).toBe("minimal");
    advance(10000);
    expect(harness.queue).toHaveLength(0);
    expect(store.getMotionTier()).toBeNull();
  });

  it("restores the persisted auto tier on load", () => {
    const store = createMemorySettingsStore({ motionTier: 1 });
    const harness = createRafHarness();
    const { result } = mountHook(store, harness);
    expect(result.current.tier).toBe(1);
    expect(document.documentElement.getAttribute("data-motion")).toBe("reduced");
  });

  it("manual override wins over the sampler and the persisted auto tier", () => {
    const store = createMemorySettingsStore({ motionTier: 2 });
    const harness = createRafHarness();
    const { result } = mountHook(store, harness);
    expect(result.current.tier).toBe(2);
    act(() => {
      result.current.setOverride("full");
    });
    expect(result.current.tier).toBe(0);
    expect(store.getMotionOverride()).toBe("full");
    // Bad frames do not move the tier while the override is set, and no
    // sampling runs.
    advance(5000);
    expect(harness.queue).toHaveLength(0);
    expect(result.current.tier).toBe(0);
    // Clearing the override returns to the persisted auto tier, and a
    // good window upgrades it back to 0.
    act(() => {
      result.current.setOverride(null);
    });
    expect(result.current.tier).toBe(2);
    expect(store.getMotionOverride()).toBeNull();
    advance(1000);
    act(() => {
      runWindow(harness, 16.7, 1000);
    });
    expect(result.current.tier).toBe(0);
  });

  it("override persists across remounts", () => {
    const store = createMemorySettingsStore();
    const harness = createRafHarness();
    const { result, unmount } = mountHook(store, harness);
    act(() => {
      result.current.setOverride("off");
    });
    expect(result.current.tier).toBe(2);
    unmount();
    const { result: result2 } = mountHook(store, createRafHarness());
    expect(result2.current.tier).toBe(2);
    expect(result2.current.override).toBe("off");
  });
});
