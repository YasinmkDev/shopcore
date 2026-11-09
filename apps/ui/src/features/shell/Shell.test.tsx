import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Pong } from "@shopcore/protocol-ts";
import { Shell } from "./Shell";
import type { CoreClient } from "../../lib/core-client";

function makeClient(outcome: Pong | Error): CoreClient {
  return {
    ping: () =>
      outcome instanceof Error ? Promise.reject(outcome) : Promise.resolve(outcome),
    subscribe: () => () => {
      /* no events in spec 000 */
    },
  };
}

describe("Shell", () => {
  it("shows the pong message after ping", async () => {
    render(<Shell client={makeClient({ message: "pong" })} />);
    fireEvent.click(screen.getByRole("button", { name: "Ping" }));
    await waitFor(() => {
      expect(screen.getByText(/Core replied/)).toBeInTheDocument();
    });
  });

  it("shows the error state when ping fails", async () => {
    render(<Shell client={makeClient(new Error("boom"))} />);
    fireEvent.click(screen.getByRole("button", { name: "Ping" }));
    await waitFor(() => {
      expect(screen.getByRole("alert")).toBeInTheDocument();
    });
  });

  it("disables the button while pinging", async () => {
    let resolvePing!: (pong: Pong) => void;
    const client: CoreClient = {
      ping: () =>
        new Promise<Pong>((resolve) => {
          resolvePing = resolve;
        }),
      subscribe: () => () => {
        /* no events in spec 000 */
      },
    };
    render(<Shell client={client} />);
    fireEvent.click(screen.getByRole("button", { name: "Ping" }));
    expect(screen.getByRole("button", { name: "Pinging…" })).toBeDisabled();
    resolvePing({ message: "pong" });
    await waitFor(() => {
      expect(screen.getByText(/Core replied/)).toBeInTheDocument();
    });
  });
});
