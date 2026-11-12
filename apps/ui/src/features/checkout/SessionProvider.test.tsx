/**
 * SessionProvider tests (spec 001c-1, amendment 4): loads the session
 * + till status once on mount, reports loading / error / no-session,
 * and reloads on `refresh()`. A tiny consumer renders the value.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactElement } from "react";
import { describe, expect, it, vi } from "vitest";
import type { SessionDto, StatusDto } from "@shopcore/protocol-ts";
import type { CoreClient } from "../../lib/core-client";
import { SessionProvider, useSession } from "./SessionProvider";

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
const STATUS: StatusDto = {
  shift_open: true,
  printer_status: "ready",
  sync_configured: false,
  dev_build: false,
};

function Consumer(): ReactElement {
  const { session, status, loading, error, refresh } = useSession();
  if (loading) {
    return <p>loading</p>;
  }
  if (error) {
    return (
      <div>
        <p>error</p>
        <button type="button" onClick={refresh}>
          retry
        </button>
      </div>
    );
  }
  return (
    <div>
      <p>{session === null ? "no session" : session.username ?? "no user"}</p>
      <p>{status === null ? "no status" : status.shift_open ? "shift open" : "shift closed"}</p>
    </div>
  );
}

function makeClient(overrides: Partial<Pick<CoreClient, "getSession" | "getStatus">>): CoreClient {
  const never = (): Promise<never> => new Promise(() => undefined);
  return {
    ping: () => Promise.resolve({ message: "pong" }),
    getSession: overrides.getSession ?? (() => Promise.resolve(SESSION)),
    getStatus: overrides.getStatus ?? (() => Promise.resolve(STATUS)),
    listSampleBarcodes: () => Promise.resolve([]),
    getCart: () => never(),
    searchSkus: () => never(),
    lookupBarcode: () => never(),
    draftAddLine: () => never(),
    draftSetQty: () => never(),
    draftRemoveLine: () => never(),
    draftCancel: () => never(),
    draftRestoreLine: () => never(),
    subscribe: () => () => {
      /* no events in these tests */
    },
  };
}

describe("SessionProvider", () => {
  it("loads the session and status on mount", async () => {
    const client = makeClient({});
    render(
      <SessionProvider client={client}>
        <Consumer />
      </SessionProvider>,
    );
    expect(screen.getByText("loading")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByText("cashier")).toBeInTheDocument();
    });
    expect(screen.getByText("shift open")).toBeInTheDocument();
  });

  it("reports a null session instead of inventing one", async () => {
    const client = makeClient({ getSession: () => Promise.resolve(null) });
    render(
      <SessionProvider client={client}>
        <Consumer />
      </SessionProvider>,
    );
    await waitFor(() => {
      expect(screen.getByText("no session")).toBeInTheDocument();
    });
  });

  it("reports an error and reloads on retry", async () => {
    const getSession = vi
      .fn()
      .mockRejectedValueOnce(new Error("boom"))
      .mockResolvedValueOnce(SESSION);
    const client = makeClient({ getSession });
    render(
      <SessionProvider client={client}>
        <Consumer />
      </SessionProvider>,
    );
    await waitFor(() => {
      expect(screen.getByText("error")).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("button", { name: "retry" }));
    await waitFor(() => {
      expect(screen.getByText("cashier")).toBeInTheDocument();
    });
    expect(getSession).toHaveBeenCalledTimes(2);
  });
});
