import { describe, expect, it } from "vitest";
import { formatMoney, formatQty, formatUnitPrice } from "./money";

describe("formatMoney", () => {
  it("formats the 001c-1 acceptance total exactly", () => {
    // Cash rounding OFF: 52.32 shows exactly, no float artefacts.
    expect(formatMoney(5232, 2)).toBe("52.32");
  });

  it("formats zero, small, and negative values", () => {
    expect(formatMoney(0, 2)).toBe("0.00");
    expect(formatMoney(1, 2)).toBe("0.01");
    expect(formatMoney(99, 2)).toBe("0.99");
    expect(formatMoney(100, 2)).toBe("1.00");
    expect(formatMoney(101, 2)).toBe("1.01");
    expect(formatMoney(-5232, 2)).toBe("-52.32");
    expect(formatMoney(999_999, 2)).toBe("9999.99");
  });

  it("honours exponent 0 and 3 currencies", () => {
    expect(formatMoney(100, 0)).toBe("100");
    expect(formatMoney(5, 3)).toBe("0.005");
    expect(formatMoney(1234, 3)).toBe("1.234");
  });

  it("round-trips every minor-unit input (no float artefacts)", () => {
    // Parse the formatted string back with integer arithmetic and
    // require the exact input — a float artefact would fail here.
    const parse = (text: string): number => {
      const negative = text.startsWith("-");
      const digits = (negative ? text.slice(1) : text).replace(".", "");
      const value = Number(digits);
      return negative ? -value : value;
    };
    for (let minor = 0; minor <= 100_000; minor += 7) {
      expect(parse(formatMoney(minor, 2))).toBe(minor);
    }
    // The classic float traps: 0.1 + 0.2 territory, as minor units.
    for (const minor of [10, 20, 30, 100, 200, 300, 1001, 1999, 2_000_001]) {
      expect(parse(formatMoney(minor, 2))).toBe(minor);
    }
  });

  it("rejects values outside the safe-integer range", () => {
    expect(() => formatMoney(2 ** 53, 2)).toThrow();
    expect(() => formatMoney(Number.NaN, 2)).toThrow();
  });

  it("rejects bad exponents", () => {
    expect(() => formatMoney(100, -1)).toThrow();
    expect(() => formatMoney(100, 4)).toThrow();
    expect(() => formatMoney(100, 1.5)).toThrow();
  });
});

describe("formatQty", () => {
  it("trims trailing zeros", () => {
    expect(formatQty(2000)).toBe("2");
    expect(formatQty(1000)).toBe("1");
    expect(formatQty(500)).toBe("0.5");
    expect(formatQty(1250)).toBe("1.25");
    expect(formatQty(1)).toBe("0.001");
    expect(formatQty(0)).toBe("0");
  });
});

describe("formatUnitPrice", () => {
  it("keeps at least the currency exponent's digits", () => {
    expect(formatUnitPrice(1_000_000, 2)).toBe("100.00");
    expect(formatUnitPrice(10_000, 2)).toBe("1.00");
  });

  it("keeps sub-minor precision instead of rounding it away", () => {
    // 0.0050 per unit (the 001a golden) is a real price, not 0.01.
    expect(formatUnitPrice(50, 2)).toBe("0.005");
    expect(formatUnitPrice(5, 2)).toBe("0.0005");
  });
});
