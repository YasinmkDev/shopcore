/**
 * Playwright e2e (spec 001c-2): the full checkout flow against the
 * REAL desktop binary — the global setup spawns it with the
 * `e2e-shim` + `dev-session` features (dev seed: cashier 1234 /
 * manager 4321, catalog with TST44) and serves the e2e UI build
 * (`vite preview`). The browser talks to the desktop only through
 * the shim (`HttpClient`), exactly like the future hub client.
 *
 * Acceptance covered here:
 * - PIN-approved discount: 10% on TST44 → total 47.09, tender
 *   50.00 → change 2.91, receipt printed, cart cleared.
 * - Plain cash completion: 52.32 / 60.00 / 7.68, receipt printed,
 *   cart cleared, focus back on the scan input.
 */
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";

interface E2eAuth {
  previewUrl: string;
  shimUrl: string;
  token: string;
}

async function openTill(page: Page): Promise<void> {
  const auth = JSON.parse(
    await readFile(join(fileURLToPath(new URL(".", import.meta.url)), "auth.json"), "utf8"),
  ) as E2eAuth;
  await page.goto(
    `${auth.previewUrl}/?shim=${encodeURIComponent(auth.shimUrl)}&token=${encodeURIComponent(auth.token)}`,
  );
  // Dev auto-session signs in as cashier; the scan input is the
  // sell screen's ready signal.
  await expect(page.getByLabel("Scan or search")).toBeVisible();
  await expect(page.getByLabel("Scan or search")).toBeFocused();
}

async function scanTst44(page: Page): Promise<void> {
  const scan = page.getByLabel("Scan or search");
  await scan.fill("1234567890999");
  await scan.press("Enter");
  await expect(page.getByRole("listbox", { name: "Cart" })).toContainText("TST44");
  await expect(page.getByText("52.32").first()).toBeVisible();
}

test("PIN-approved discount then cash payment completes the sale", async ({ page }) => {
  await openTill(page);
  await scanTst44(page);

  // F6 opens the discount popover for the last line; 10% is above
  // the cashier's 5% ceiling, so a manager must approve.
  await page.keyboard.press("F6");
  await expect(page.getByRole("dialog", { name: "Discount" })).toBeVisible();
  await page.getByLabel("Percent off").fill("10");
  await page.getByRole("button", { name: "Apply discount" }).click();
  await expect(page.getByText("Above your limit — a manager must approve.")).toBeVisible();
  await page.getByLabel("Manager username").fill("manager");
  await page.getByLabel("Manager PIN").fill("4321");
  await page.getByRole("button", { name: "Approve & apply" }).click();
  await expect(page.getByRole("dialog", { name: "Discount" })).toBeHidden();
  // 44.34 − 10% (4.43) = 39.91 net + 7.18 tax = 47.09.
  await expect(page.getByText("47.09").first()).toBeVisible();

  // F9 opens payment; tender 50.00 → change 2.91.
  await page.keyboard.press("F9");
  await expect(page.getByRole("dialog", { name: "Payment" })).toBeVisible();
  const tender = page.getByLabel("Tendered");
  await tender.fill("50.00");
  await expect(page.getByText("2.91").first()).toBeVisible();
  await tender.press("Enter");
  await expect(page.getByRole("heading", { name: "Sale complete" })).toBeVisible();
  await expect(page.getByText("Receipt printed")).toBeVisible();
  await expect(page.getByLabel("Scan or search")).toBeFocused();
  // The receipt replaces the cart until dismissed, so the empty-cart
  // message only appears after "New sale".
  await page.getByRole("button", { name: "New sale" }).click();
  await expect(page.getByText("Scan an item to start the sale.")).toBeVisible();
});

test("plain cash completion 52.32 / 60.00 / 7.68 with receipt printed", async ({ page }) => {
  await openTill(page);
  await scanTst44(page);

  await page.keyboard.press("F9");
  await expect(page.getByRole("dialog", { name: "Payment" })).toBeVisible();
  const tender = page.getByLabel("Tendered");
  await tender.fill("60.00");
  await expect(page.getByText("7.68").first()).toBeVisible();
  await tender.press("Enter");
  await expect(page.getByRole("heading", { name: "Sale complete" })).toBeVisible();
  await expect(page.getByText("Receipt printed")).toBeVisible();
  // Receipt shows the tender and the change.
  await expect(page.getByText("60.00").first()).toBeVisible();
  await expect(page.getByText("7.68").first()).toBeVisible();
  // Cart cleared, focus back on the scan input, New sale dismisses.
  // (The empty-cart message only appears once the receipt is
  // dismissed — the receipt replaces the cart until then.)
  await expect(page.getByLabel("Scan or search")).toBeFocused();
  await page.getByRole("button", { name: "New sale" }).click();
  await expect(page.getByText("Scan an item to start the sale.")).toBeVisible();
});
