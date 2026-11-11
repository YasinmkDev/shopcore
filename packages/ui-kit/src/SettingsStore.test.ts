import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createMemorySettingsStore,
  localStorageSettingsStore,
} from "./SettingsStore";

describe("localStorageSettingsStore", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  afterEach(() => {
    window.localStorage.clear();
    vi.restoreAllMocks();
  });

  it("persists and restores the theme", () => {
    expect(localStorageSettingsStore.getTheme()).toBeNull();
    localStorageSettingsStore.setTheme("dark");
    expect(localStorageSettingsStore.getTheme()).toBe("dark");
    expect(window.localStorage.getItem("shopcore.theme")).toBe("dark");
  });

  it("persists and restores the motion tier and override", () => {
    localStorageSettingsStore.setMotionTier(2);
    expect(localStorageSettingsStore.getMotionTier()).toBe(2);
    localStorageSettingsStore.setMotionOverride("reduced");
    expect(localStorageSettingsStore.getMotionOverride()).toBe("reduced");
    localStorageSettingsStore.setMotionOverride(null);
    expect(localStorageSettingsStore.getMotionOverride()).toBeNull();
  });

  it("returns null for invalid stored values", () => {
    window.localStorage.setItem("shopcore.theme", "sepia");
    window.localStorage.setItem("shopcore.motionTier", "9");
    window.localStorage.setItem("shopcore.motionOverride", "wild");
    expect(localStorageSettingsStore.getTheme()).toBeNull();
    expect(localStorageSettingsStore.getMotionTier()).toBeNull();
    expect(localStorageSettingsStore.getMotionOverride()).toBeNull();
  });

  it("tolerates localStorage being unavailable", () => {
    vi.spyOn(window.Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    vi.spyOn(window.Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("denied");
    });
    vi.spyOn(window.Storage.prototype, "removeItem").mockImplementation(() => {
      throw new Error("denied");
    });
    expect(localStorageSettingsStore.getTheme()).toBeNull();
    expect(localStorageSettingsStore.getMotionTier()).toBeNull();
    expect(localStorageSettingsStore.getMotionOverride()).toBeNull();
    expect(() => {
      localStorageSettingsStore.setTheme("dark");
    }).not.toThrow();
    expect(() => {
      localStorageSettingsStore.setMotionTier(1);
    }).not.toThrow();
    expect(() => {
      localStorageSettingsStore.setMotionOverride(null);
    }).not.toThrow();
  });
});

describe("createMemorySettingsStore", () => {
  it("holds theme, tier and override in memory", () => {
    const store = createMemorySettingsStore();
    expect(store.getTheme()).toBeNull();
    store.setTheme("auto");
    store.setMotionTier(1);
    store.setMotionOverride("off");
    expect(store.getTheme()).toBe("auto");
    expect(store.getMotionTier()).toBe(1);
    expect(store.getMotionOverride()).toBe("off");
    store.setMotionOverride(null);
    expect(store.getMotionOverride()).toBeNull();
  });

  it("accepts initial values", () => {
    const store = createMemorySettingsStore({ theme: "dark", motionTier: 2 });
    expect(store.getTheme()).toBe("dark");
    expect(store.getMotionTier()).toBe(2);
  });
});
