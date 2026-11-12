/**
 * CoreClient — the single interface the UI uses to talk to the Rust core
 * (see context/03-architecture.md and context/09-engineering-standards.md).
 * The UI never touches the DB, filesystem, or hardware directly.
 *
 * Two transports, one interface, so the UI is unaware of deployment mode:
 * - TauriClient: in-process commands/events (embedded mode).
 * - HttpClient: HTTP + WebSocket to a store hub (client mode; also used
 *   for browser-based e2e tests). Lands with the hub spec (Phase 5).
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type {
  CartViewDto,
  HardwareStatusPayload,
  Pong,
  ReprintDto,
  SaleCompletedDto,
  SaleCompletedPayload,
  SampleBarcodeDto,
  SessionDto,
  SkuHitDto,
  StatusDto,
} from "@shopcore/protocol-ts";

/**
 * Core → UI events. `sale.completed` is emitted by the desktop after
 * `complete_sale_cash` commits (001c-2). `hardware.status` is emitted
 * by the desktop for the mock printer; real driver state changes land
 * with spec 003.
 */
export interface ShopCoreEvents {
  "sale.completed": SaleCompletedPayload;
  "hardware.status": HardwareStatusPayload;
}

export interface CoreClient {
  ping(): Promise<Pong>;
  getSession(): Promise<SessionDto | null>;
  getStatus(): Promise<StatusDto>;
  /** Dev builds only: the sample barcodes (001c-1 follow-up 4). */
  listSampleBarcodes(): Promise<SampleBarcodeDto[]>;
  getCart(): Promise<CartViewDto>;
  searchSkus(query: string): Promise<SkuHitDto[]>;
  lookupBarcode(code: string): Promise<SkuHitDto>;
  draftAddLine(
    draftId: string,
    skuId: string,
    qtyMilli: number,
    unitPriceE4: number,
  ): Promise<CartViewDto>;
  draftSetQty(draftId: string, lineId: string, qtyMilli: number): Promise<CartViewDto>;
  draftRemoveLine(draftId: string, lineId: string): Promise<CartViewDto>;
  draftCancel(draftId: string): Promise<CartViewDto>;
  draftRestoreLine(draftId: string): Promise<CartViewDto>;
  /** 001c-2: line discount; above the cashier's ceiling the call
   * needs a manager username + PIN (else `DISCOUNT_NEEDS_APPROVAL`). */
  draftApplyDiscount(
    draftId: string,
    lineId: string,
    kind: "percent" | "fixed",
    value: number,
    managerUsername?: string,
    managerPin?: string,
  ): Promise<CartViewDto>;
  /** 001c-2: complete a cash sale; returns the sale + receipt. */
  completeSaleCash(draftId: string, tenderMinor: number): Promise<SaleCompletedDto>;
  /** 001c-2: reprint a sale's receipt; the reprint is audit-logged. */
  reprintReceipt(saleId: string): Promise<ReprintDto>;
  subscribe<E extends keyof ShopCoreEvents>(
    event: E,
    cb: (payload: ShopCoreEvents[E]) => void,
  ): () => void;
}

export class TauriClient implements CoreClient {
  ping(): Promise<Pong> {
    return invoke<Pong>("ping");
  }

  getSession(): Promise<SessionDto | null> {
    return invoke<SessionDto | null>("get_session");
  }

  getStatus(): Promise<StatusDto> {
    return invoke<StatusDto>("get_status");
  }

  listSampleBarcodes(): Promise<SampleBarcodeDto[]> {
    return invoke<SampleBarcodeDto[]>("list_sample_barcodes");
  }

  getCart(): Promise<CartViewDto> {
    return invoke<CartViewDto>("get_cart");
  }

  searchSkus(query: string): Promise<SkuHitDto[]> {
    return invoke<SkuHitDto[]>("search_skus", { query });
  }

  lookupBarcode(code: string): Promise<SkuHitDto> {
    return invoke<SkuHitDto>("lookup_barcode", { code });
  }

  draftAddLine(
    draftId: string,
    skuId: string,
    qtyMilli: number,
    unitPriceE4: number,
  ): Promise<CartViewDto> {
    return invoke<CartViewDto>("draft_add_line", {
      draft_id: draftId,
      sku_id: skuId,
      qty_milli: qtyMilli,
      unit_price_e4: unitPriceE4,
    });
  }

  draftSetQty(draftId: string, lineId: string, qtyMilli: number): Promise<CartViewDto> {
    return invoke<CartViewDto>("draft_set_qty", {
      draft_id: draftId,
      line_id: lineId,
      qty_milli: qtyMilli,
    });
  }

  draftRemoveLine(draftId: string, lineId: string): Promise<CartViewDto> {
    return invoke<CartViewDto>("draft_remove_line", { draft_id: draftId, line_id: lineId });
  }

  draftCancel(draftId: string): Promise<CartViewDto> {
    return invoke<CartViewDto>("draft_cancel", { draft_id: draftId });
  }

  draftRestoreLine(draftId: string): Promise<CartViewDto> {
    return invoke<CartViewDto>("draft_restore_line", { draft_id: draftId });
  }

  draftApplyDiscount(
    draftId: string,
    lineId: string,
    kind: "percent" | "fixed",
    value: number,
    managerUsername?: string,
    managerPin?: string,
  ): Promise<CartViewDto> {
    return invoke<CartViewDto>("draft_apply_discount", {
      draft_id: draftId,
      line_id: lineId,
      kind,
      value,
      manager_username: managerUsername ?? null,
      manager_pin: managerPin ?? null,
    });
  }

  completeSaleCash(draftId: string, tenderMinor: number): Promise<SaleCompletedDto> {
    return invoke<SaleCompletedDto>("complete_sale_cash", {
      draft_id: draftId,
      tender_minor: tenderMinor,
    });
  }

  reprintReceipt(saleId: string): Promise<ReprintDto> {
    return invoke<ReprintDto>("reprint_receipt", { sale_id: saleId });
  }

  subscribe<E extends keyof ShopCoreEvents>(
    event: E,
    cb: (payload: ShopCoreEvents[E]) => void,
  ): () => void {
    let unlisten: UnlistenFn | undefined;
    void listen<ShopCoreEvents[E]>(event, (e) => {
      cb(e.payload);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }
}

export class HttpClient implements CoreClient {
  ping(): Promise<Pong> {
    return Promise.reject(new Error("HttpClient is not implemented yet (hub spec, Phase 5)"));
  }

  getSession(): Promise<SessionDto | null> {
    return Promise.reject(new Error("HttpClient is not implemented yet (hub spec, Phase 5)"));
  }

  getStatus(): Promise<StatusDto> {
    return Promise.reject(new Error("HttpClient is not implemented yet (hub spec, Phase 5)"));
  }

  listSampleBarcodes(): Promise<SampleBarcodeDto[]> {
    return Promise.reject(new Error("HttpClient is not implemented yet (hub spec, Phase 5)"));
  }

  getCart(): Promise<CartViewDto> {
    return Promise.reject(new Error("HttpClient is not implemented yet (hub spec, Phase 5)"));
  }

  searchSkus(): Promise<SkuHitDto[]> {
    return Promise.reject(new Error("HttpClient is not implemented yet (hub spec, Phase 5)"));
  }

  lookupBarcode(): Promise<SkuHitDto> {
    return Promise.reject(new Error("HttpClient is not implemented yet (hub spec, Phase 5)"));
  }

  draftAddLine(): Promise<CartViewDto> {
    return Promise.reject(new Error("HttpClient is not implemented yet (hub spec, Phase 5)"));
  }

  draftSetQty(): Promise<CartViewDto> {
    return Promise.reject(new Error("HttpClient is not implemented yet (hub spec, Phase 5)"));
  }

  draftRemoveLine(): Promise<CartViewDto> {
    return Promise.reject(new Error("HttpClient is not implemented yet (hub spec, Phase 5)"));
  }

  draftCancel(): Promise<CartViewDto> {
    return Promise.reject(new Error("HttpClient is not implemented yet (hub spec, Phase 5)"));
  }

  draftRestoreLine(): Promise<CartViewDto> {
    return Promise.reject(new Error("HttpClient is not implemented yet (hub spec, Phase 5)"));
  }

  draftApplyDiscount(): Promise<CartViewDto> {
    return Promise.reject(new Error("HttpClient is not implemented yet (hub spec, Phase 5)"));
  }

  completeSaleCash(): Promise<SaleCompletedDto> {
    return Promise.reject(new Error("HttpClient is not implemented yet (hub spec, Phase 5)"));
  }

  reprintReceipt(): Promise<ReprintDto> {
    return Promise.reject(new Error("HttpClient is not implemented yet (hub spec, Phase 5)"));
  }

  subscribe(): () => void {
    throw new Error("HttpClient is not implemented yet (hub spec, Phase 5)");
  }
}

/** The client for the current deployment mode (embedded in spec 000). */
export const coreClient: CoreClient = new TauriClient();
