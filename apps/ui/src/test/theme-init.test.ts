import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

/**
 * Tests for public/theme-init.js — the external sync script that sets
 * `data-theme` before the first React render (no flash). The script is
 * executed in the test's jsdom window via window.eval, with localStorage
 * and matchMedia controlled per test.
 */
const script = readFileSync(join(process.cwd(), "public", "theme-init.js"), "utf8");

describe("theme-init.js", () => {
  beforeEach(() => {
    window.localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    window.localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
  });

  function run(): void {
    window.eval(script);
  }

  function stubColorScheme(dark: boolean): void {
    vi.stubGlobal(
      "matchMedia",
      vi.fn(
        () =>
          ({
            matches: dark,
          }) as unknown as MediaQueryList,
      ),
    );
  }

  it("applies a stored dark setting before first render", () => {
    window.localStorage.setItem("shopcore.theme", "dark");
    run();
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("applies a stored light setting", () => {
    window.localStorage.setItem("shopcore.theme", "light");
    run();
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("resolves Auto via matchMedia (dark OS)", () => {
    window.localStorage.setItem("shopcore.theme", "auto");
    stubColorScheme(true);
    run();
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("resolves Auto via matchMedia (light OS)", () => {
    window.localStorage.setItem("shopcore.theme", "auto");
    stubColorScheme(false);
    run();
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("defaults to light with no stored setting", () => {
    run();
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("defaults to light for an invalid stored setting", () => {
    window.localStorage.setItem("shopcore.theme", "sepia");
    run();
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("defaults to light when localStorage throws", () => {
    vi.spyOn(window.Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    run();
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });
});
