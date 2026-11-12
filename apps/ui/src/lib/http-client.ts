/**
 * HttpClient (spec 001c-2): the `CoreClient` transport over HTTP,
 * talking to the desktop's e2e shim (`POST /__e2e/:command`). Only
 * the Playwright e2e flow uses it — the release UI bundle must NOT
 * contain it (the `VITE_SHOPCORE_E2E` flag gates the import in
 * `core-client.ts`, and `verify-release-bundle.mjs` greps the
 * release bundle for the shim URL + token header).
 *
 * Shaped for Phase 5 (the store hub): the same command names and
 * snake_case args as the Tauri transport, the same `{code, message}`
 * error shape. `subscribe()` is a documented no-op — there is no
 * push channel over HTTP in 001c-2; the e2e flow drives state from
 * command returns.
 */
import type {
  CartViewDto,
  Pong,
  ReprintDto,
  SaleCompletedDto,
  SampleBarcodeDto,
  SessionDto,
  SkuHitDto,
  StatusDto,
} from "@shopcore/protocol-ts";
import type { CoreClient } from "./core-client";

/** An error carrying the Rust side's stable error code. */
function appError(code: string, message: string): Error {
  return Object.assign(new Error(message), { code });
}

export class HttpClient implements CoreClient {
  private readonly baseUrl: string;
  private readonly token: string;

  constructor(baseUrl: string, token: string) {
    this.baseUrl = baseUrl.replace(/\/$/, "");
    this.token = token;
  }

  private async call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
    const response = await fetch(`${this.baseUrl}/__e2e/${command}`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-shopcore-e2e-token": this.token,
      },
      body: JSON.stringify(args),
    });
    const body: unknown = await response.json().catch(() => null);
    if (!response.ok) {
      const code =
        typeof body === "object" && body !== null && "code" in body && typeof body.code === "string"
          ? body.code
          : `HTTP_${String(response.status)}`;
      const message =
        typeof body === "object" && body !== null && "message" in body && typeof body.message === "string"
          ? body.message
          : `Request to ${command} failed with HTTP ${String(response.status)}`;
      throw appError(code, message);
    }
    return body as T;
  }

  ping(): Promise<Pong> {
    return this.call<Pong>("ping");
  }

  getSession(): Promise<SessionDto | null> {
    return this.call<SessionDto | null>("get_session");
  }

  getStatus(): Promise<StatusDto> {
    return this.call<StatusDto>("get_status");
  }

  listSampleBarcodes(): Promise<SampleBarcodeDto[]> {
    return this.call<SampleBarcodeDto[]>("list_sample_barcodes");
  }

  getCart(): Promise<CartViewDto> {
    return this.call<CartViewDto>("get_cart");
  }

  searchSkus(query: string): Promise<SkuHitDto[]> {
    return this.call<SkuHitDto[]>("search_skus", { query });
  }

  lookupBarcode(code: string): Promise<SkuHitDto> {
    return this.call<SkuHitDto>("lookup_barcode", { code });
  }

  draftAddLine(
    draftId: string,
    skuId: string,
    qtyMilli: number,
    unitPriceE4: number,
  ): Promise<CartViewDto> {
    return this.call<CartViewDto>("draft_add_line", {
      draft_id: draftId,
      sku_id: skuId,
      qty_milli: qtyMilli,
      unit_price_e4: unitPriceE4,
    });
  }

  draftSetQty(draftId: string, lineId: string, qtyMilli: number): Promise<CartViewDto> {
    return this.call<CartViewDto>("draft_set_qty", {
      draft_id: draftId,
      line_id: lineId,
      qty_milli: qtyMilli,
    });
  }

  draftRemoveLine(draftId: string, lineId: string): Promise<CartViewDto> {
    return this.call<CartViewDto>("draft_remove_line", { draft_id: draftId, line_id: lineId });
  }

  draftCancel(draftId: string): Promise<CartViewDto> {
    return this.call<CartViewDto>("draft_cancel", { draft_id: draftId });
  }

  draftRestoreLine(draftId: string): Promise<CartViewDto> {
    return this.call<CartViewDto>("draft_restore_line", { draft_id: draftId });
  }

  draftApplyDiscount(
    draftId: string,
    lineId: string,
    kind: "percent" | "fixed",
    value: number,
    managerUsername?: string,
    managerPin?: string,
  ): Promise<CartViewDto> {
    return this.call<CartViewDto>("draft_apply_discount", {
      draft_id: draftId,
      line_id: lineId,
      kind,
      value,
      manager_username: managerUsername ?? null,
      manager_pin: managerPin ?? null,
    });
  }

  completeSaleCash(draftId: string, tenderMinor: number): Promise<SaleCompletedDto> {
    return this.call<SaleCompletedDto>("complete_sale_cash", {
      draft_id: draftId,
      tender_minor: tenderMinor,
    });
  }

  reprintReceipt(saleId: string): Promise<ReprintDto> {
    return this.call<ReprintDto>("reprint_receipt", { sale_id: saleId });
  }

  // No push channel over HTTP in 001c-2 (see the module doc).
  // The params are part of the `CoreClient` interface; the class
  // takes none of them, which is assignable (fewer params).
  subscribe(): () => void {
    return () => {
      /* no events over HTTP */
    };
  }
}
