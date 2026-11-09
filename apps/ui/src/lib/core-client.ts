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
import type { Pong } from "@shopcore/protocol-ts";

/** Core → UI events. No events in spec 000; payload types land with the
 * specs that emit them (e.g. "sale.completed"). */
export interface ShopCoreEvents {
  // Intentionally empty for now (see ADR-013 note in the audit reply).
}

export interface CoreClient {
  ping(): Promise<Pong>;
  subscribe<E extends keyof ShopCoreEvents>(
    event: E,
    cb: (payload: ShopCoreEvents[E]) => void,
  ): () => void;
}

export class TauriClient implements CoreClient {
  ping(): Promise<Pong> {
    return invoke<Pong>("ping");
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

  subscribe(): () => void {
    throw new Error("HttpClient is not implemented yet (hub spec, Phase 5)");
  }
}

/** The client for the current deployment mode (embedded in spec 000). */
export const coreClient: CoreClient = new TauriClient();
