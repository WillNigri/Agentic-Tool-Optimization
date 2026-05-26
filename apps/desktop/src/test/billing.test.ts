import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { startCheckout, CheckoutError } from "@/lib/billing";
import { UPGRADE_URL } from "@/lib/constants";

const tauriOpen = vi.fn();
vi.mock("@tauri-apps/plugin-shell", () => ({
  open: (url: string) => tauriOpen(url),
}));

type FetchMock = ReturnType<typeof vi.fn>;

function setBrowserContext() {
  const w = window as unknown as { __TAURI__?: unknown; __TAURI_INTERNALS__?: unknown };
  delete w.__TAURI__;
  delete w.__TAURI_INTERNALS__;
}

function setTauriContext() {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
}

function jsonResponse(body: unknown, init: ResponseInit = {}): Response {
  return new Response(JSON.stringify(body), {
    status: init.status ?? 200,
    headers: { "Content-Type": "application/json", ...init.headers },
  });
}

describe("startCheckout", () => {
  let fetchMock: FetchMock;
  let windowOpen: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    setBrowserContext();
    fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    windowOpen = vi.fn();
    vi.stubGlobal("open", windowOpen);
    tauriOpen.mockReset();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    setBrowserContext();
  });

  it("posts to /api/billing/checkout with the JWT + opens the returned URL in the browser", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse({
        success: true,
        data: { sessionId: "cs_test_123", url: "https://checkout.stripe.com/c/cs_test_123" },
      }),
    );

    await startCheckout("pro", "jwt-token-abc");

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toMatch(/\/api\/billing\/checkout$/);
    expect((init as RequestInit).method).toBe("POST");
    const headers = (init as RequestInit).headers as Record<string, string>;
    expect(headers["Authorization"]).toBe("Bearer jwt-token-abc");
    const body = JSON.parse((init as RequestInit).body as string);
    expect(body).toMatchObject({
      tier: "pro",
      successUrl: expect.stringMatching(/^ato:\/\/billing\/success/),
      cancelUrl: "ato://billing/cancel",
    });

    expect(windowOpen).toHaveBeenCalledWith(
      "https://checkout.stripe.com/c/cs_test_123",
      "_blank",
      "noreferrer,noopener",
    );
    expect(tauriOpen).not.toHaveBeenCalled();
  });

  it("uses the Tauri shell plugin when running inside the desktop app", async () => {
    setTauriContext();
    fetchMock.mockResolvedValueOnce(
      jsonResponse({
        success: true,
        data: { url: "https://checkout.stripe.com/c/cs_test_tauri" },
      }),
    );

    await startCheckout("pro", "jwt-tauri");

    expect(tauriOpen).toHaveBeenCalledWith("https://checkout.stripe.com/c/cs_test_tauri");
    expect(windowOpen).not.toHaveBeenCalled();
  });

  it("falls back to the Calendly UPGRADE_URL when called without a JWT", async () => {
    await startCheckout("pro", "");
    expect(fetchMock).not.toHaveBeenCalled();
    expect(windowOpen).toHaveBeenCalledWith(UPGRADE_URL, "_blank", "noreferrer,noopener");
  });

  it("falls back to the Calendly UPGRADE_URL on 402 PRO_REQUIRED", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse(
        { success: false, error: { code: "PRO_REQUIRED", message: "Account is sales-only" } },
        { status: 402 },
      ),
    );

    await startCheckout("pro", "jwt-token");

    expect(windowOpen).toHaveBeenCalledWith(UPGRADE_URL, "_blank", "noreferrer,noopener");
  });

  it("throws CheckoutError on a non-2xx response that isn't 402", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse(
        { success: false, error: { code: "STRIPE_INVALID_REQUEST", message: "Price archived" } },
        { status: 502 },
      ),
    );

    await expect(startCheckout("pro", "jwt-token")).rejects.toBeInstanceOf(CheckoutError);
    expect(windowOpen).not.toHaveBeenCalled();
  });

  it("opens Calendly and throws when fetch itself rejects (network down)", async () => {
    fetchMock.mockRejectedValueOnce(new TypeError("Load failed"));

    await expect(startCheckout("pro", "jwt-token")).rejects.toMatchObject({
      name: "CheckoutError",
      code: "NETWORK_ERROR",
    });
    expect(windowOpen).toHaveBeenCalledWith(UPGRADE_URL, "_blank", "noreferrer,noopener");
  });
});
