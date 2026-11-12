/**
 * HttpClient tests (spec 001c-2): the fetch shape (URL, method,
 * token header, snake_case args), the `{code, message}` error mapping,
 * and the documented no-op `subscribe()`. The fetch itself is stubbed
 * — the live round-trip is the Playwright test (CP5).
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { HttpClient } from "./http-client";

const BASE = "http://127.0.0.1:54321";
const TOKEN = "test-token";

function stubFetch(response: unknown, ok = true, status = 200): ReturnType<typeof vi.fn> {
  const fetchMock = vi.fn(() =>
    Promise.resolve({
      ok,
      status,
      json: () => Promise.resolve(response),
    }),
  );
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("HttpClient", () => {
  it("posts the command with the token header and returns the JSON", async () => {
    const fetchMock = stubFetch({ message: "pong" });
    const client = new HttpClient(BASE, TOKEN);
    await expect(client.ping()).resolves.toEqual({ message: "pong" });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe(`${BASE}/__e2e/ping`);
    expect(init.method).toBe("POST");
    expect((init.headers as Record<string, string>)["x-shopcore-e2e-token"]).toBe(TOKEN);
    expect(init.body).toBe("{}");
  });

  it("sends snake_case args, with nulls for a missing manager pair", async () => {
    const fetchMock = stubFetch({ draft_id: "draft-1", lines: [], totals: {} });
    const client = new HttpClient(`${BASE}/`, TOKEN);
    await client.draftApplyDiscount("draft-1", "line-1", "percent", 500);
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe(`${BASE}/__e2e/draft_apply_discount`);
    expect(JSON.parse(init.body as string)).toEqual({
      draft_id: "draft-1",
      line_id: "line-1",
      kind: "percent",
      value: 500,
      manager_username: null,
      manager_pin: null,
    });
  });

  it("maps a non-2xx {code, message} body onto the error", async () => {
    stubFetch({ code: "DISCOUNT_NEEDS_APPROVAL", message: "Manager approval required" }, false, 400);
    const client = new HttpClient(BASE, TOKEN);
    const error = await client
      .draftApplyDiscount("draft-1", "line-1", "percent", 1000)
      .then(
        () => null,
        (caught: unknown) => caught as Error & { code?: string },
      );
    expect(error?.code).toBe("DISCOUNT_NEEDS_APPROVAL");
    expect(error?.message).toBe("Manager approval required");
  });

  it("falls back to HTTP_<status> when the error body has no code", async () => {
    stubFetch(null, false, 401);
    const client = new HttpClient(BASE, TOKEN);
    const error = await client.ping().then(
      () => null,
      (caught: unknown) => caught as Error & { code?: string },
    );
    expect(error?.code).toBe("HTTP_401");
  });

  it("subscribe is a no-op returning an unsubscribe function", () => {
    const client = new HttpClient(BASE, TOKEN);
    const unsubscribe = client.subscribe();
    expect(() => {
      unsubscribe();
    }).not.toThrow();
  });
});
