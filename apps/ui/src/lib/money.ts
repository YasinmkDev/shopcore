/**
 * Money / quantity formatting (spec 001c-1, req. 5 + amendment 3).
 *
 * Money is formatted from INTEGER minor units with the tenant currency
 * exponent — NEVER float division (no `minor / 100` in floating point
 * anywhere in the UI). These formatters work on the decimal STRING of
 * the integer, so they cannot produce float artefacts (no `0.1 + 0.2`
 * style rounding) for any safe-integer input. Values outside the JSON
 * safe-integer range are rejected — the Rust side already enforces
 * that bound, and this is the second line of defence.
 */

/** The largest integer that survives a JSON round-trip exactly (2^53-1). */
export const MAX_SAFE_INTEGER = 9_007_199_254_740_991;

/** Split an integer's decimal string into whole + fraction at `exponent` places. */
function splitDecimal(absText: string, exponent: number): { whole: string; frac: string } {
  if (exponent === 0) {
    return { whole: absText, frac: "" };
  }
  const padded = absText.padStart(exponent + 1, "0");
  return {
    whole: padded.slice(0, -exponent),
    frac: padded.slice(-exponent),
  };
}

function checkExponent(exponent: number): void {
  if (!Number.isInteger(exponent) || exponent < 0 || exponent > 3) {
    throw new Error(`currency exponent must be an integer 0-3, got ${String(exponent)}`);
  }
}

function checkSafe(minor: number): void {
  if (!Number.isSafeInteger(minor)) {
    throw new Error(`value ${String(minor)} is outside the JSON safe-integer range`);
  }
}

/**
 * Format integer minor units as a decimal string with exactly
 * `exponent` fraction digits — e.g. `formatMoney(5232, 2)` is `"52.32"`,
 * `formatMoney(100, 0)` is `"100"`, `formatMoney(-5, 3)` is `"-0.005"`.
 */
export function formatMoney(minor: number, exponent: number): string {
  checkSafe(minor);
  checkExponent(exponent);
  const negative = minor < 0;
  const { whole, frac } = splitDecimal(String(Math.abs(minor)), exponent);
  const sign = negative ? "-" : "";
  return frac === "" ? `${sign}${whole}` : `${sign}${whole}.${frac}`;
}

/**
 * Parse a typed decimal amount into integer minor units — e.g.
 * `parseMoneyToMinor("52.32", 2)` is `5232`, `parseMoneyToMinor("60", 2)`
 * is `6000`. Integer string math only, never float division. Returns
 * `null` for anything that is not a plain non-negative decimal
 * (empty, letters, multiple dots, a negative sign, or more fraction
 * digits than `exponent`). A trailing dot (`"52."`) is not a complete
 * amount — the caller treats it as an in-progress prefix.
 */
export function parseMoneyToMinor(text: string, exponent: number): number | null {
  checkExponent(exponent);
  const trimmed = text.trim();
  if (trimmed === "") {
    return null;
  }
  const match = /^(\d+)(?:\.(\d+))?$/.exec(trimmed);
  if (match === null) {
    return null;
  }
  const whole = match[1] ?? "";
  const frac = match[2] ?? "";
  if (frac.length > exponent) {
    return null;
  }
  const minor = Number(whole + frac.padEnd(exponent, "0"));
  if (!Number.isSafeInteger(minor)) {
    return null;
  }
  return minor;
}

/**
 * Format a quantity in milli-units (3 decimals) with trailing zeros
 * trimmed — e.g. `formatQty(2000)` is `"2"`, `formatQty(500)` is
 * `"0.5"`, `formatQty(1250)` is `"1.25"`, `formatQty(1)` is `"0.001"`.
 */
export function formatQty(qtyMilli: number): string {
  checkSafe(qtyMilli);
  const negative = qtyMilli < 0;
  const { whole, frac } = splitDecimal(String(Math.abs(qtyMilli)), 3);
  const trimmed = frac.replace(/0+$/, "");
  const sign = negative ? "-" : "";
  return trimmed === "" ? `${sign}${whole}` : `${sign}${whole}.${trimmed}`;
}

/**
 * Format a unit price in `e4` (price x 10^4) as a decimal string with
 * at least `exponent` fraction digits, keeping extra precision when
 * the price has it — e.g. `formatUnitPrice(1_000_000, 2)` is `"100.00"`,
 * `formatUnitPrice(50, 2)` is `"0.005"` (a half-cent price is real).
 */
export function formatUnitPrice(priceE4: number, exponent: number): string {
  checkSafe(priceE4);
  checkExponent(exponent);
  const negative = priceE4 < 0;
  const { whole, frac } = splitDecimal(String(Math.abs(priceE4)), 4);
  // Keep at least `exponent` digits; trim the rest only if they are zeros.
  const extra = frac.slice(exponent);
  const trimmedExtra = extra.replace(/0+$/, "");
  const shownFrac = frac.slice(0, exponent) + trimmedExtra;
  const sign = negative ? "-" : "";
  return shownFrac === "" ? `${sign}${whole}` : `${sign}${whole}.${shownFrac}`;
}
