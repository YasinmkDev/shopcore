import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * AA contrast checks computed from the token values in tokens.css
 * (context/10-ui-system.md: "contrast AA minimum for all text").
 *
 * KNOWN CONFLICT (reported, not silently weakened): --text-faint fails
 * the 4.5:1 bar in BOTH themes (light 3.28:1, dark 4.02:1 — computed
 * below). The token is defined in context/10 as "placeholders only;
 * never for essential text", so this test asserts the 3:1 floor for it
 * (the UI-component / non-essential-text bar) and the 4.5:1 bar for
 * every essential text pair. Darkening --text-faint to 4.5:1 needs a
 * context-file change (context/10) and the owner's word — it is NOT
 * done here, because tokens.css is a verbatim copy of context/10.
 */

const AA_TEXT = 4.5; // WCAG AA, normal text.
const AA_NON_TEXT = 3; // WCAG AA, UI components / large text / borders.

function relativeLuminance(hex: string): number {
  const channel = (i: number): number => {
    const v = Number.parseInt(hex.slice(i, i + 2), 16) / 255;
    return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5);
}

function contrastRatio(a: string, b: string): number {
  const l1 = relativeLuminance(a);
  const l2 = relativeLuminance(b);
  return (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);
}

type Tokens = Map<string, string>;

function parseTokens(css: string, block: "light" | "dark"): Tokens {
  /* The header comment mentions [data-theme="dark"], so split on the
     selector followed by " {" (the comment has a "." after the bracket). */
  const selector = block === "light" ? ":root {" : '[data-theme="dark"] {';
  const start = css.indexOf(selector);
  const open = start < 0 ? -1 : css.indexOf("{", start);
  const close = open < 0 ? -1 : css.indexOf("}", open);
  const source = open < 0 || close < 0 ? "" : css.slice(open + 1, close);
  const tokens: Tokens = new Map();
  const re = /--([a-z-]+)\s*:\s*(#[0-9A-Fa-f]{6})/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(source)) !== null) {
    const name = match[1];
    const value = match[2];
    if (name !== undefined && value !== undefined) tokens.set(name, value);
  }
  return tokens;
}

function token(tokens: Tokens, name: string): string {
  const value = tokens.get(name);
  if (value === undefined) throw new Error(`token --${name} missing from tokens.css`);
  return value;
}

/* Vitest runs with cwd = the package root (pnpm --filter), so the token
   file is read relative to the working directory. import.meta.url is not
   a file: URL under the Vite transform pipeline, so it cannot be used. */
const css = readFileSync(join(process.cwd(), "tokens.css"), "utf8");

describe("token contrast (AA, computed from tokens.css)", () => {
  it.each(["light", "dark"] as const)("theme %s: text pairs meet AA 4.5:1", (theme) => {
    const tokens = parseTokens(css, theme);
    const pairs: [string, string][] = [
      // --text on every background it can sit on.
      ["text", "bg"],
      ["text", "surface"],
      ["text", "surface-subtle"],
      ["text", "surface-sunken"],
      ["text", "accent-subtle"],
      ["text", "success-bg"],
      ["text", "warning-bg"],
      ["text", "danger-bg"],
      ["text", "info-bg"],
      // --text-muted on app backgrounds.
      ["text-muted", "bg"],
      ["text-muted", "surface"],
      ["text-muted", "surface-subtle"],
      ["text-muted", "surface-sunken"],
      // --text-on-accent on the accent background.
      ["text-on-accent", "accent"],
      // --accent as text on the surface and on its subtle background.
      ["accent", "surface"],
      ["accent", "accent-subtle"],
      // Semantic text on its tint background.
      ["success", "success-bg"],
      ["warning", "warning-bg"],
      ["danger", "danger-bg"],
      ["info", "info-bg"],
    ];
    for (const [fg, bg] of pairs) {
      const ratio = contrastRatio(token(tokens, fg), token(tokens, bg));
      expect(
        ratio,
        `--${fg} on --${bg} (${theme}) must be ≥ ${AA_TEXT.toFixed(1)}:1, got ${ratio.toFixed(2)}:1`,
      ).toBeGreaterThanOrEqual(AA_TEXT);
    }
  });

  it.each(["light", "dark"] as const)(
    "theme %s: --text-faint meets the 3:1 floor (placeholders only)",
    (theme) => {
      // See the header comment: --text-faint is placeholders-only per
      // context/10 and fails 4.5:1 (light 3.28:1, dark 4.02:1). The 3:1
      // floor is asserted here so the pair is covered and any regression
      // below it fails loudly.
      const tokens = parseTokens(css, theme);
      const ratio = contrastRatio(token(tokens, "text-faint"), token(tokens, "surface"));
      expect(
        ratio,
        `--text-faint on --surface (${theme}) must be ≥ ${AA_NON_TEXT.toFixed(1)}:1, got ${ratio.toFixed(2)}:1`,
      ).toBeGreaterThanOrEqual(AA_NON_TEXT);
    },
  );

  it.each(["light", "dark"] as const)(
    "theme %s: --border-input and --focus-ring are ≥ 3:1 against the surface",
    (theme) => {
      const tokens = parseTokens(css, theme);
      const surface = token(tokens, "surface");
      for (const name of ["border-input", "focus-ring"] as const) {
        const ratio = contrastRatio(token(tokens, name), surface);
        expect(
          ratio,
          `--${name} on --surface (${theme}) must be ≥ ${AA_NON_TEXT.toFixed(1)}:1, got ${ratio.toFixed(2)}:1`,
        ).toBeGreaterThanOrEqual(AA_NON_TEXT);
      }
    },
  );
});
