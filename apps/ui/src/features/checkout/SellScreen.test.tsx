/**
 * SellScreen tests (spec 001c-1): every state (loading / empty /
 * error / offline), keyboard, F9/F6 no-op, live region, status bar,
 * cancel confirm, `*N` quantity, RTL render, 150% text scaling.
 *
 * The screen talks to the core through a fake `CoreClient` — never an
 * invoke string literal in a test file (the ADR-013 drift guard scans
 * all of apps/ui/src for them, comments included).
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactElement, ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

/**
 * jsdom has no layout (clientHeight is 0), so the real virtualizer
 * renders no rows. The virtualizer is a library — its own behaviour
 * is covered by the BenchDebug test — so these tests render every
 * row through a stub that keeps the same `getVirtualItems` contract.
 */
vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getTotalSize: () => count * 48,
    getVirtualItems: () =>
      Array.from({ length: count }, (_, index) => ({ index, start: index * 48, size: 48 })),
  }),
}));
import type {
  CartLineDto,
  CartTotalsDto,
  CartViewDto,
  SampleBarcodeDto,
  SessionDto,
  SkuHitDto,
  StatusDto,
} from "@shopcore/protocol-ts";
import { createMemorySettingsStore } from "@shopcore/ui-kit";
import { ThemeProvider } from "../../lib/theme";
import type { CoreClient } from "../../lib/core-client";
import { SessionProvider } from "./SessionProvider";
import { SellScreen } from "./SellScreen";

const MILK: SkuHitDto = {
  sku_id: "sku-milk",
  item_id: "item-milk",
  code: "MILK1L",
  name: "Milk 1L",
  tax_group_id: "std",
  price_e4: 1_000_000,
};
const BREAD: SkuHitDto = {
  sku_id: "sku-bread",
  item_id: "item-bread",
  code: "BREAD",
  name: "Bread Loaf",
  tax_group_id: "std",
  price_e4: 500_000,
};
const NO_PRICE: SkuHitDto = {
  sku_id: "sku-noprice",
  item_id: "item-noprice",
  code: "NOPRICE",
  name: "No Price Item",
  tax_group_id: "std",
  price_e4: null,
};

const SESSION: SessionDto = {
  tenant_id: "test-tenant",
  branch_id: "branch-1",
  branch_code: "MAIN",
  terminal_id: "terminal-1",
  terminal_code: "T1",
  username: "cashier",
  shift_id: "shift-1",
  currency_code: "PKR",
  currency_exponent: 2,
};

function totalsFor(lines: CartLineDto[]): CartTotalsDto {
  const subtotal = lines.reduce((sum, line) => sum + line.line_total_minor, 0);
  // The fake prices exclusive 18% tax (STD group), so the gross
  // subtotal IS the net subtotal here.
  const tax = Math.round((subtotal * 18) / 100);
  return {
    subtotal_minor: subtotal,
    discount_minor: 0,
    net_subtotal_minor: subtotal,
    tax_minor: tax,
    total_minor: subtotal + tax,
  };
}

/** An error carrying the Rust side's stable error code. */
function appError(code: string, message: string): Error {
  return Object.assign(new Error(message), { code });
}

/** Minimal in-memory till: lines + totals, no persistence. */
class FakeClient implements CoreClient {
  session: SessionDto | null = SESSION;
  status: StatusDto = {
    shift_open: false,
    printer_status: "ready",
    sync_configured: false,
    dev_build: false,
  };
  /** The dev seed's sample barcodes (001c-1 follow-up 4). */
  sampleBarcodes: SampleBarcodeDto[] = [
    { barcode: "1234567890123", name: "Milk 1L", price_minor: 25_000, tax_mode: "exclusive", rate_bp: 1800 },
    { barcode: "1234567890124", name: "Bread 500g", price_minor: 12_000, tax_mode: "zero", rate_bp: 0 },
  ];
  lines: CartLineDto[] = [];
  draftId = "draft-1";
  /** The last removed line, for draftRestoreLine. */
  removed: CartLineDto | null = null;
  /** Barcode -> hit, for lookupBarcode. */
  barcodes: Record<string, SkuHitDto> = { "1234567890100": MILK };
  /** Search results, for searchSkus. */
  searchHits: SkuHitDto[] = [MILK, BREAD];
  /** Fail the next getCart. */
  cartError = false;

  private view(): CartViewDto {
    // Fresh objects on every call, like the real transport (Tauri
    // serialises/deserialises every response) — callers compare
    // snapshots by id, so a shared mutable array would break them.
    return {
      draft_id: this.draftId,
      lines: this.lines.map((line) => ({ ...line })),
      totals: totalsFor(this.lines),
      currency_exponent: 2,
    };
  }

  ping = vi.fn(() => Promise.resolve({ message: "pong" }));
  getSession = vi.fn(() => Promise.resolve(this.session));
  getStatus = vi.fn(() => Promise.resolve(this.status));
  listSampleBarcodes = vi.fn(() => Promise.resolve(this.sampleBarcodes));
  getCart = vi.fn(() => {
    if (this.cartError) {
      return Promise.reject(new Error("cart failed"));
    }
    return Promise.resolve(this.view());
  });
  searchSkus = vi.fn(() => Promise.resolve(this.searchHits));
  lookupBarcode = vi.fn((code: string) => {
    const hit = this.barcodes[code];
    if (hit === undefined) {
      return Promise.reject(appError("BARCODE_NOT_FOUND", `Barcode ${code} not found`));
    }
    return Promise.resolve(hit);
  });
  draftAddLine = vi.fn((_draftId: string, skuId: string, qtyMilli: number, unitPriceE4: number) => {
    const hit = [MILK, BREAD, NO_PRICE].find((candidate) => candidate.sku_id === skuId);
    const existing = this.lines.find(
      (line) => line.sku_id === skuId && line.unit_price_e4 === unitPriceE4,
    );
    if (existing !== undefined) {
      existing.qty_milli += qtyMilli;
      existing.line_total_minor = (existing.qty_milli * unitPriceE4) / 100_000;
    } else {
      this.lines.push({
        id: "line-" + String(this.lines.length + 1),
        sku_id: skuId,
        name: hit?.name ?? skuId,
        qty_milli: qtyMilli,
        unit_price_e4: unitPriceE4,
        discount_minor: 0,
        line_total_minor: (qtyMilli * unitPriceE4) / 100_000,
      });
    }
    return Promise.resolve(this.view());
  });
  draftSetQty = vi.fn((_draftId: string, lineId: string, qtyMilli: number) => {
    const line = this.lines.find((candidate) => candidate.id === lineId);
    if (line !== undefined) {
      if (qtyMilli === 0) {
        this.removed = line;
        this.lines = this.lines.filter((candidate) => candidate.id !== lineId);
      } else {
        line.qty_milli = qtyMilli;
        line.line_total_minor = (qtyMilli * line.unit_price_e4) / 100_000;
      }
    }
    return Promise.resolve(this.view());
  });
  draftRemoveLine = vi.fn((_draftId: string, lineId: string) => {
    const line = this.lines.find((candidate) => candidate.id === lineId);
    if (line !== undefined) {
      this.removed = line;
      this.lines = this.lines.filter((candidate) => candidate.id !== lineId);
    }
    return Promise.resolve(this.view());
  });
  draftCancel = vi.fn(() => {
    this.lines = [];
    this.draftId = "draft-2";
    return Promise.resolve(this.view());
  });
  draftRestoreLine = vi.fn(() => {
    if (this.removed !== null) {
      this.lines.push(this.removed);
      this.removed = null;
    }
    return Promise.resolve(this.view());
  });
  /** 001c-2: no discount/payment behaviour in this fake yet — CP3
   * wires the components and gives these fakes real behaviour. */
  draftApplyDiscount = vi.fn(() =>
    Promise.reject(new Error("discount not implemented in this fake")),
  );
  completeSaleCash = vi.fn(() =>
    Promise.reject(new Error("payment not implemented in this fake")),
  );
  reprintReceipt = vi.fn(() =>
    Promise.reject(new Error("reprint not implemented in this fake")),
  );
  subscribe = vi.fn(() => () => {
    /* no events in these tests */
  });
}

function renderSellScreen(client: FakeClient): void {
  const wrapper = ({ children }: { children: ReactNode }): ReactElement => (
    <ThemeProvider store={createMemorySettingsStore()}>
      <SessionProvider client={client}>{children}</SessionProvider>
    </ThemeProvider>
  );
  render(<SellScreen client={client} />, { wrapper });
}

function scanInput(): HTMLElement {
  return screen.getByLabelText("Scan or search");
}

describe("SellScreen", () => {
  it("shows the empty cart and the totals at zero", async () => {
    renderSellScreen(new FakeClient());
    await waitFor(() => {
      expect(screen.getByText("Scan an item to start the sale.")).toBeInTheDocument();
    });
    expect(screen.getAllByText("0.00")).toHaveLength(3);
    // Status bar: sync is never faked as configured (req. 8).
    expect(screen.getByText("Sync: not configured")).toBeInTheDocument();
    expect(screen.getByText("Shift closed")).toBeInTheDocument();
    expect(screen.getByText("Printer ready")).toBeInTheDocument();
    expect(screen.getByText("T1")).toBeInTheDocument();
    // Not a dev build: no DEV BUILD banner.
    expect(screen.queryByText("DEV BUILD — sample data")).not.toBeInTheDocument();
  });

  it("shows the DEV BUILD banner with sample barcodes on a dev build", async () => {
    const client = new FakeClient();
    client.status = { ...client.status, dev_build: true };
    renderSellScreen(client);
    // The banner mounts with the status; the rows arrive one tick
    // later from listSampleBarcodes — wait for a row, not the title.
    await waitFor(() => {
      expect(screen.getByText("Milk 1L")).toBeInTheDocument();
    });
    expect(screen.getByText("DEV BUILD — sample data")).toBeInTheDocument();
    expect(screen.getByText("Sample data only — do not use for real sales.")).toBeInTheDocument();
    expect(screen.getByText("Milk 1L")).toBeInTheDocument();
    expect(screen.getByText("1234567890123")).toBeInTheDocument();
    expect(screen.getByText("250.00")).toBeInTheDocument();
    expect(screen.getByText("Tax excl. 18%")).toBeInTheDocument();
    expect(screen.getByText("Bread 500g")).toBeInTheDocument();
    expect(screen.getByText("Zero-rated")).toBeInTheDocument();
    expect(client.listSampleBarcodes).toHaveBeenCalled();
  });

  it("focuses the scan input on mount", async () => {
    renderSellScreen(new FakeClient());
    await waitFor(() => {
      expect(screen.getByText("Scan an item to start the sale.")).toBeInTheDocument();
    });
    expect(document.activeElement).toBe(scanInput());
  });

  it("adds a scanned barcode and announces the item and the total", async () => {
    const client = new FakeClient();
    renderSellScreen(client);
    await waitFor(() => {
      expect(scanInput()).toBeInTheDocument();
    });
    fireEvent.change(scanInput(), { target: { value: "1234567890100" } });
    fireEvent.keyDown(scanInput(), { key: "Enter" });
    await waitFor(() => {
      expect(screen.getByText("Milk 1L")).toBeInTheDocument();
    });
    expect(screen.getByText("118.00")).toBeInTheDocument();
    expect(screen.getByRole("status", { name: "Announcements" })).toHaveTextContent("Added 1 × Milk 1L");
    expect(screen.getByRole("status", { name: "Announcements" })).toHaveTextContent("Total 118.00");
    expect(client.draftAddLine).toHaveBeenCalledWith("draft-1", "sku-milk", 1000, 1_000_000);
  });

  it("merges a second scan of the same SKU", async () => {
    const client = new FakeClient();
    renderSellScreen(client);
    await waitFor(() => {
      expect(scanInput()).toBeInTheDocument();
    });
    fireEvent.change(scanInput(), { target: { value: "1234567890100" } });
    fireEvent.keyDown(scanInput(), { key: "Enter" });
    await waitFor(() => {
      expect(screen.getByText("118.00")).toBeInTheDocument();
    });
    fireEvent.change(scanInput(), { target: { value: "1234567890100" } });
    fireEvent.keyDown(scanInput(), { key: "Enter" });
    await waitFor(() => {
      expect(screen.getByText("236.00")).toBeInTheDocument();
    });
    expect(screen.getAllByText("Milk 1L")).toHaveLength(1);
  });

  it("shows an unknown barcode inline with the scanned code and keeps the cart", async () => {
    const client = new FakeClient();
    renderSellScreen(client);
    await waitFor(() => {
      expect(scanInput()).toBeInTheDocument();
    });
    fireEvent.change(scanInput(), { target: { value: "1234567890123" } });
    fireEvent.keyDown(scanInput(), { key: "Enter" });
    await waitFor(() => {
      expect(screen.getByRole("alert")).toHaveTextContent("Barcode 1234567890123 not found");
    });
    // The cart is kept (still empty) and there is no "Add item" action.
    expect(screen.getByText("Scan an item to start the sale.")).toBeInTheDocument();
    expect(screen.queryByText(/Add item/)).not.toBeInTheDocument();
  });

  it("shows a no-price message instead of adding a free line", async () => {
    const client = new FakeClient();
    client.barcodes["9999999999999"] = NO_PRICE;
    renderSellScreen(client);
    await waitFor(() => {
      expect(scanInput()).toBeInTheDocument();
    });
    fireEvent.change(scanInput(), { target: { value: "9999999999999" } });
    fireEvent.keyDown(scanInput(), { key: "Enter" });
    await waitFor(() => {
      expect(screen.getByRole("alert")).toHaveTextContent("No price set for No Price Item");
    });
    expect(client.draftAddLine).not.toHaveBeenCalled();
  });

  it("searches by name and adds the highlighted result on Enter", async () => {
    const client = new FakeClient();
    renderSellScreen(client);
    await waitFor(() => {
      expect(scanInput()).toBeInTheDocument();
    });
    fireEvent.change(scanInput(), { target: { value: "milk" } });
    await waitFor(() => {
      expect(screen.getByRole("option", { name: /Milk 1L/ })).toBeInTheDocument();
    });
    fireEvent.keyDown(scanInput(), { key: "Enter" });
    await waitFor(() => {
      expect(screen.getByText("118.00")).toBeInTheDocument();
    });
    expect(client.draftAddLine).toHaveBeenCalledWith("draft-1", "sku-milk", 1000, 1_000_000);
  });

  it("adds the second search result with ArrowDown + Enter", async () => {
    const client = new FakeClient();
    renderSellScreen(client);
    await waitFor(() => {
      expect(scanInput()).toBeInTheDocument();
    });
    fireEvent.change(scanInput(), { target: { value: "bread" } });
    await waitFor(() => {
      expect(screen.getByRole("option", { name: /Bread Loaf/ })).toBeInTheDocument();
    });
    fireEvent.keyDown(scanInput(), { key: "ArrowDown" });
    fireEvent.keyDown(scanInput(), { key: "Enter" });
    await waitFor(() => {
      expect(screen.getByText("Bread Loaf")).toBeInTheDocument();
    });
    expect(client.draftAddLine).toHaveBeenCalledWith("draft-1", "sku-bread", 1000, 500_000);
  });

  it("sets the pending quantity with *N + Enter for the next scan", async () => {
    const client = new FakeClient();
    renderSellScreen(client);
    await waitFor(() => {
      expect(scanInput()).toBeInTheDocument();
    });
    fireEvent.change(scanInput(), { target: { value: "*3" } });
    fireEvent.keyDown(scanInput(), { key: "Enter" });
    await waitFor(() => {
      expect(screen.getByText("Quantity 3 for next scan")).toBeInTheDocument();
    });
    fireEvent.change(scanInput(), { target: { value: "1234567890100" } });
    fireEvent.keyDown(scanInput(), { key: "Enter" });
    await waitFor(() => {
      expect(screen.getByText("354.00")).toBeInTheDocument();
    });
    expect(client.draftAddLine).toHaveBeenCalledWith("draft-1", "sku-milk", 3000, 1_000_000);
  });

  it("adjusts quantity with +/-, removes with Del, restores with Ctrl+Z", async () => {
    const client = new FakeClient();
    renderSellScreen(client);
    await waitFor(() => {
      expect(scanInput()).toBeInTheDocument();
    });
    fireEvent.change(scanInput(), { target: { value: "1234567890100" } });
    fireEvent.keyDown(scanInput(), { key: "Enter" });
    await waitFor(() => {
      expect(screen.getByText("118.00")).toBeInTheDocument();
    });
    fireEvent.keyDown(scanInput(), { key: "+" });
    await waitFor(() => {
      expect(screen.getByText("236.00")).toBeInTheDocument();
    });
    expect(client.draftSetQty).toHaveBeenCalledWith("draft-1", "line-1", 2000);
    fireEvent.keyDown(scanInput(), { key: "Delete" });
    await waitFor(() => {
      expect(screen.getByText("Scan an item to start the sale.")).toBeInTheDocument();
    });
    expect(screen.getByRole("status", { name: "Announcements" })).toHaveTextContent("Removed Milk 1L");
    fireEvent.keyDown(scanInput(), { key: "z", ctrlKey: true });
    await waitFor(() => {
      expect(screen.getByText("Milk 1L")).toBeInTheDocument();
    });
    expect(screen.getByRole("status", { name: "Announcements" })).toHaveTextContent("Restored Milk 1L");
  });

  it("does nothing on F9 and F6 (amendment 2)", async () => {
    const client = new FakeClient();
    renderSellScreen(client);
    await waitFor(() => {
      expect(scanInput()).toBeInTheDocument();
    });
    fireEvent.change(scanInput(), { target: { value: "1234567890100" } });
    fireEvent.keyDown(scanInput(), { key: "Enter" });
    await waitFor(() => {
      expect(screen.getByText("118.00")).toBeInTheDocument();
    });
    fireEvent.keyDown(scanInput(), { key: "F9" });
    fireEvent.keyDown(scanInput(), { key: "F6" });
    expect(client.draftAddLine).toHaveBeenCalledTimes(1);
    expect(client.draftCancel).not.toHaveBeenCalled();
  });

  it("cancels only on the second click (confirm), then shows a fresh cart", async () => {
    const client = new FakeClient();
    renderSellScreen(client);
    await waitFor(() => {
      expect(scanInput()).toBeInTheDocument();
    });
    fireEvent.change(scanInput(), { target: { value: "1234567890100" } });
    fireEvent.keyDown(scanInput(), { key: "Enter" });
    await waitFor(() => {
      expect(screen.getByText("118.00")).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("button", { name: "Cancel sale" }));
    expect(screen.getByRole("button", { name: "Confirm cancel?" })).toBeInTheDocument();
    expect(client.draftCancel).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Confirm cancel?" }));
    await waitFor(() => {
      expect(screen.getByText("Scan an item to start the sale.")).toBeInTheDocument();
    });
    expect(screen.getByRole("status", { name: "Announcements" })).toHaveTextContent("Sale cancelled.");
    expect(client.draftCancel).toHaveBeenCalledWith("draft-1");
  });

  it("shows the error state with a retry when the cart fails to load", async () => {
    const client = new FakeClient();
    client.cartError = true;
    renderSellScreen(client);
    await waitFor(() => {
      expect(screen.getByText("Could not load the cart. Try again.")).toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("shows the no-session state when no session is available", async () => {
    const client = new FakeClient();
    client.session = null;
    renderSellScreen(client);
    await waitFor(() => {
      expect(screen.getByText("No session. Sign-in comes with the next update.")).toBeInTheDocument();
    });
  });

  it("renders in RTL without dropping the scan input or totals", async () => {
    const client = new FakeClient();
    document.documentElement.dir = "rtl";
    try {
      renderSellScreen(client);
      await waitFor(() => {
        expect(scanInput()).toBeInTheDocument();
      });
      expect(screen.getByText("Totals")).toBeInTheDocument();
      expect(screen.getAllByText("0.00")).toHaveLength(3);
    } finally {
      document.documentElement.dir = "ltr";
    }
  });

  it("renders at 150% text scaling", async () => {
    const client = new FakeClient();
    document.documentElement.style.fontSize = "150%";
    try {
      renderSellScreen(client);
      await waitFor(() => {
        expect(scanInput()).toBeInTheDocument();
      });
      expect(screen.getByText("Totals")).toBeInTheDocument();
      expect(screen.getByText("Scan an item to start the sale.")).toBeInTheDocument();
    } finally {
      document.documentElement.style.fontSize = "";
    }
  });
});
