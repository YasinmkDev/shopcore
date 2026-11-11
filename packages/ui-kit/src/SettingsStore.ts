/**
 * SettingsStore — the persistence seam for per-terminal display settings
 * (spec 000b, Q1). The theme provider (apps/ui) and `useMotionTier`
 * (ui-kit) read and write through this interface only.
 *
 * The 000b implementation is backed by localStorage. Moving to the core
 * settings store later is a one-file change: swap the backing store passed
 * to the provider/hook, not the call sites. All access tolerates
 * localStorage being unavailable (private mode, old WebView) — reads
 * return null, writes are dropped.
 */

export type ThemeSetting = "light" | "dark" | "auto";

/** Manual motion override (Settings → Appearance → Motion). */
export type MotionOverride = "full" | "reduced" | "off";

/** Adaptive motion tier (context/10-ui-system.md): 0 full, 1 reduced, 2 minimal. */
export type MotionTierValue = 0 | 1 | 2;

export interface SettingsStore {
  getTheme(): ThemeSetting | null;
  setTheme(theme: ThemeSetting): void;
  /** Auto-detected motion tier from the rAF sampler (not the manual override). */
  getMotionTier(): MotionTierValue | null;
  setMotionTier(tier: MotionTierValue): void;
  getMotionOverride(): MotionOverride | null;
  setMotionOverride(override: MotionOverride | null): void;
}

const THEME_KEY = "shopcore.theme";
const MOTION_TIER_KEY = "shopcore.motionTier";
const MOTION_OVERRIDE_KEY = "shopcore.motionOverride";

function read(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string): void {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    /* Storage unavailable — the setting is dropped, not fatal. */
  }
}

function remove(key: string): void {
  try {
    window.localStorage.removeItem(key);
  } catch {
    /* Storage unavailable — nothing to remove. */
  }
}

/** localStorage-backed store (spec 000b). */
export const localStorageSettingsStore: SettingsStore = {
  getTheme(): ThemeSetting | null {
    const value = read(THEME_KEY);
    return value === "light" || value === "dark" || value === "auto" ? value : null;
  },
  setTheme(theme: ThemeSetting): void {
    write(THEME_KEY, theme);
  },
  getMotionTier(): MotionTierValue | null {
    const value = read(MOTION_TIER_KEY);
    return value === "0" || value === "1" || value === "2"
      ? Number(value) as MotionTierValue
      : null;
  },
  setMotionTier(tier: MotionTierValue): void {
    write(MOTION_TIER_KEY, String(tier));
  },
  getMotionOverride(): MotionOverride | null {
    const value = read(MOTION_OVERRIDE_KEY);
    return value === "full" || value === "reduced" || value === "off" ? value : null;
  },
  setMotionOverride(override: MotionOverride | null): void {
    if (override === null) {
      remove(MOTION_OVERRIDE_KEY);
    } else {
      write(MOTION_OVERRIDE_KEY, override);
    }
  },
};

/** In-memory store for tests and for environments without localStorage. */
export function createMemorySettingsStore(initial?: Partial<{
  theme: ThemeSetting;
  motionTier: MotionTierValue;
  motionOverride: MotionOverride;
}>): SettingsStore {
  let theme: ThemeSetting | null = initial?.theme ?? null;
  let motionTier: MotionTierValue | null = initial?.motionTier ?? null;
  let motionOverride: MotionOverride | null = initial?.motionOverride ?? null;
  return {
    getTheme: () => theme,
    setTheme: (value: ThemeSetting) => {
      theme = value;
    },
    getMotionTier: () => motionTier,
    setMotionTier: (value: MotionTierValue) => {
      motionTier = value;
    },
    getMotionOverride: () => motionOverride,
    setMotionOverride: (value: MotionOverride | null) => {
      motionOverride = value;
    },
  };
}
