import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * CSP guard (spec 000b, req. 8): the data-theme init is an external script
 * precisely because the CSP never allows 'unsafe-inline' for scripts.
 * SCOPE: script execution only. `style-src` keeps 'unsafe-inline' for
 * Vite dev HMR style injection — this test does not touch it, and the
 * value must not be removed (that would break Vite dev styling).
 */
function asRecord(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("expected a JSON object");
  }
  return value as Record<string, unknown>;
}

describe("CSP (spec 000b)", () => {
  it("never allows 'unsafe-inline' in script-src or default-src", () => {
    const confPath = join(
      process.cwd(),
      "..",
      "..",
      "crates",
      "desktop",
      "src-tauri",
      "tauri.conf.json",
    );
    const parsed: unknown = JSON.parse(readFileSync(confPath, "utf8"));
    const csp = asRecord(asRecord(asRecord(parsed).app).security).csp;
    if (typeof csp !== "string") throw new Error("CSP missing from tauri.conf.json");
    const directives = csp
      .split(";")
      .map((directive) => directive.trim())
      .filter((directive) => directive.length > 0);
    expect(
      directives.some((directive) => directive.startsWith("script-src")),
      "script-src must be present (the test must fail if it is dropped)",
    ).toBe(true);
    for (const directive of directives) {
      const [name, ...values] = directive.split(/\s+/);
      if (name === "script-src" || name === "default-src") {
        expect(values, `${name} must not allow 'unsafe-inline'`).not.toContain(
          "'unsafe-inline'",
        );
      }
    }
  });

  it("index.html contains no inline <script> tag", () => {
    const html = readFileSync(join(process.cwd(), "index.html"), "utf8");
    const inlineScripts = html.match(/<script(?![^>]*\bsrc\s*=)[^>]*>/gi) ?? [];
    expect(inlineScripts).toEqual([]);
    // The external init script loads before the module script.
    expect(html).toContain('<script src="/theme-init.js"></script>');
    expect(html.indexOf("/theme-init.js")).toBeLessThan(html.indexOf("/src/main.tsx"));
  });
});
