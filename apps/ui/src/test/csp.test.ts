import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * CSP guard (spec 000b, req. 8; ADR-033): the data-theme init is an
 * external script precisely because the CSP never allows 'unsafe-inline'
 * for scripts. Production is as strict as the app allows:
 * - `style-src 'self'` (no 'unsafe-inline'): the production build emits
 *   an external CSS file and no inline <style> element exists in dist.
 * - `style-src-attr 'unsafe-inline'`: the ONLY style exception — the
 *   @tanstack/react-virtual rows in BenchDebug render inline `style`
 *   attributes at runtime (the only `style={{...}}` props in the
 *   source). Inline <style> ELEMENTS stay blocked.
 * - Dev-only relaxations live in `devCsp` (Vite HMR injects <style>
 *   tags in dev) and never in the production CSP.
 */
function asRecord(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("expected a JSON object");
  }
  return value as Record<string, unknown>;
}

function readTauriConf(): Record<string, unknown> {
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
  return asRecord(asRecord(asRecord(parsed).app).security);
}

function directivesOf(csp: string): string[] {
  return csp
    .split(";")
    .map((directive) => directive.trim())
    .filter((directive) => directive.length > 0);
}

function directiveValues(directives: string[], name: string): string[] {
  const found = directives.find((directive) => directive.startsWith(`${name} `));
  if (found === undefined) return [];
  const [, ...values] = found.split(/\s+/);
  return values;
}

describe("CSP (spec 000b, ADR-033)", () => {
  it("production CSP never allows 'unsafe-inline' in script-src, default-src or style-src", () => {
    const security = readTauriConf();
    const csp = security.csp;
    if (typeof csp !== "string") throw new Error("CSP missing from tauri.conf.json");
    const directives = directivesOf(csp);
    for (const name of ["script-src", "default-src", "style-src"] as const) {
      expect(
        directives.some((directive) => directive.startsWith(`${name} `)),
        `${name} must be present (the test must fail if it is dropped)`,
      ).toBe(true);
      expect(
        directiveValues(directives, name),
        `${name} must not allow 'unsafe-inline'`,
      ).not.toContain("'unsafe-inline'");
    }
  });

  it("production CSP allows 'unsafe-inline' ONLY in style-src-attr (virtualization rows)", () => {
    const security = readTauriConf();
    const csp = security.csp;
    if (typeof csp !== "string") throw new Error("CSP missing from tauri.conf.json");
    const directives = directivesOf(csp);
    expect(
      directiveValues(directives, "style-src-attr"),
      "style-src-attr must be 'unsafe-inline' for the virtualizer's inline style attributes",
    ).toEqual(["'unsafe-inline'"]);
  });

  it("devCsp exists and relaxes ONLY style-src (Vite HMR), never scripts", () => {
    const security = readTauriConf();
    const devCsp = security.devCsp;
    if (typeof devCsp !== "string") throw new Error("devCsp missing from tauri.conf.json");
    const directives = directivesOf(devCsp);
    expect(
      directiveValues(directives, "style-src"),
      "devCsp style-src must allow 'unsafe-inline' for Vite HMR <style> injection",
    ).toContain("'unsafe-inline'");
    for (const name of ["script-src", "default-src"] as const) {
      expect(
        directiveValues(directives, name),
        `devCsp ${name} must not allow 'unsafe-inline'`,
      ).not.toContain("'unsafe-inline'");
    }
  });

  it("index.html contains no inline <script> or <style> element", () => {
    const html = readFileSync(join(process.cwd(), "index.html"), "utf8");
    const inlineScripts = html.match(/<script(?![^>]*\bsrc\s*=)[^>]*>/gi) ?? [];
    expect(inlineScripts).toEqual([]);
    const inlineStyles = html.match(/<style(?![^>]*\bsrc\s*=)[^>]*>/gi) ?? [];
    expect(inlineStyles).toEqual([]);
    // The external init script loads before the module script.
    expect(html).toContain('<script src="/theme-init.js"></script>');
    expect(html.indexOf("/theme-init.js")).toBeLessThan(html.indexOf("/src/main.tsx"));
  });
});
