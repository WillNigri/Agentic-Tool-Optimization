import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { startCheckout, CheckoutError } from "@/lib/billing";
import { UPGRADE_URL } from "@/lib/constants";
import { useAuthStore } from "@/hooks/useAuth";

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
    vi.spyOn(window, "open").mockImplementation(
      windowOpen as unknown as typeof window.open,
    );
    tauriOpen.mockReset();
    useAuthStore.getState().logout();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    setBrowserContext();
    useAuthStore.getState().logout();
  });

  it("posts to /api/billing/checkout with the JWT + opens the returned URL in the browser", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse({
        success: true,
        data: { sessionId: "cs_test_123", url: "https://checkout.stripe.com/c/cs_test_123" },
      }),
    );

    const result = await startCheckout("pro", "jwt-token-abc");
    expect(result).toEqual({ kind: "stripe-opened" });

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toMatch(/\/api\/billing\/checkout$/);
    expect((init as RequestInit).method).toBe("POST");
    const headers = (init as RequestInit).headers as Record<string, string>;
    expect(headers["Authorization"]).toBe("Bearer jwt-token-abc");
    const body = JSON.parse((init as RequestInit).body as string);
    expect(body).toMatchObject({
      tier: "pro",
      successUrl: expect.stringMatching(
        /^https:\/\/agentictool\.ai\/billing\/success\?session_id=\{CHECKOUT_SESSION_ID\}$/,
      ),
      cancelUrl: expect.stringMatching(
        /^https:\/\/agentictool\.ai\/billing\/cancel\?session_id=\{CHECKOUT_SESSION_ID\}$/,
      ),
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

    const result = await startCheckout("pro", "jwt-tauri");
    expect(result).toEqual({ kind: "stripe-opened" });
    expect(tauriOpen).toHaveBeenCalledWith("https://checkout.stripe.com/c/cs_test_tauri");
    expect(windowOpen).not.toHaveBeenCalled();
  });

  it("falls back to the Calendly UPGRADE_URL when called without a JWT", async () => {
    const result = await startCheckout("pro", "");
    expect(result).toMatchObject({ kind: "calendly-fallback", reason: "no_jwt" });
    expect(fetchMock).not.toHaveBeenCalled();
    expect(windowOpen).toHaveBeenCalledWith(UPGRADE_URL, "_blank", "noreferrer,noopener");
  });

  it("falls back to the Calendly UPGRADE_URL on 402 PRO_REQUIRED + surfaces a notice", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse(
        { success: false, error: { code: "PRO_REQUIRED", message: "Account is sales-only" } },
        { status: 402 },
      ),
    );

    const result = await startCheckout("pro", "jwt-token");
    expect(result).toMatchObject({ kind: "calendly-fallback", reason: "pro_required" });
    expect((result as { notice: string }).notice).toMatch(/founder-led onboarding/i);
    expect(windowOpen).toHaveBeenCalledWith(UPGRADE_URL, "_blank", "noreferrer,noopener");
  });

  it("retries with a refreshed token on 401", async () => {
    useAuthStore.getState().setAuth(
      { id: "u1", email: "u@example.com", name: "U" },
      "stale-token",
      "refresh-token-value",
      "free",
    );
    // refreshAccessToken's real impl hits the API; stub it to flip the token.
    vi.spyOn(useAuthStore.getState(), "refreshAccessToken").mockImplementation(async () => {
      useAuthStore.setState({ accessToken: "fresh-token" });
      return true;
    });

    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ success: false, error: { code: "UNAUTHORIZED" } }, { status: 401 }),
      )
      .mockResolvedValueOnce(
        jsonResponse({
          success: true,
          data: { url: "https://checkout.stripe.com/c/cs_retry" },
        }),
      );

    const result = await startCheckout("pro", "stale-token");

    expect(result).toEqual({ kind: "stripe-opened" });
    expect(fetchMock).toHaveBeenCalledTimes(2);
    const retryHeaders = (fetchMock.mock.calls[1][1] as RequestInit).headers as Record<string, string>;
    expect(retryHeaders["Authorization"]).toBe("Bearer fresh-token");
    expect(windowOpen).toHaveBeenCalledWith(
      "https://checkout.stripe.com/c/cs_retry",
      "_blank",
      "noreferrer,noopener",
    );
  });

  it("throws CheckoutError on a non-2xx response that isn't 402 or 401", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse(
        { success: false, error: { code: "STRIPE_INVALID_REQUEST", message: "Price archived" } },
        { status: 502 },
      ),
    );

    await expect(startCheckout("pro", "jwt-token")).rejects.toBeInstanceOf(CheckoutError);
    expect(windowOpen).not.toHaveBeenCalled();
  });

  it("throws SESSION_EXPIRED when both the initial 401 and the refreshed retry return 401", async () => {
    useAuthStore.getState().setAuth(
      { id: "u1", email: "u@example.com", name: "U" },
      "stale-token",
      "refresh-token-value",
      "free",
    );
    vi.spyOn(useAuthStore.getState(), "refreshAccessToken").mockImplementation(async () => {
      // Simulate refresh succeeding but token still stale (e.g. backend
      // rejected the refresh JWT, useAuth swallows the error and returns
      // true). The store's accessToken doesn't actually change.
      return true;
    });

    fetchMock.mockResolvedValue(
      jsonResponse({ success: false, error: { code: "UNAUTHORIZED" } }, { status: 401 }),
    );

    await expect(startCheckout("pro", "stale-token")).rejects.toMatchObject({
      code: "SESSION_EXPIRED",
    });
  });

  it("throws INVALID_REDIRECT when the cloud returns a non-Stripe URL", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse({
        success: true,
        data: { url: "https://attacker.example.com/phish" },
      }),
    );

    await expect(startCheckout("pro", "jwt-token")).rejects.toMatchObject({
      code: "INVALID_REDIRECT",
    });
    expect(windowOpen).not.toHaveBeenCalled();
    expect(tauriOpen).not.toHaveBeenCalled();
  });

  it("throws INVALID_REDIRECT when the cloud returns an http (non-https) Stripe-host URL", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse({
        success: true,
        data: { url: "http://checkout.stripe.com/c/cs_test_downgrade" },
      }),
    );

    await expect(startCheckout("pro", "jwt-token")).rejects.toMatchObject({
      code: "INVALID_REDIRECT",
    });
  });

  it("throws MALFORMED_RESPONSE when a 200 returns a body that isn't JSON", async () => {
    fetchMock.mockResolvedValueOnce(
      new Response("not-json-at-all", {
        status: 200,
        headers: { "Content-Type": "text/plain" },
      }),
    );

    await expect(startCheckout("pro", "jwt-token")).rejects.toMatchObject({
      code: "MALFORMED_RESPONSE",
    });
  });

  it("opens Calendly + returns a network_error result when fetch itself rejects", async () => {
    fetchMock.mockRejectedValueOnce(new TypeError("Load failed"));

    const result = await startCheckout("pro", "jwt-token");
    expect(result).toMatchObject({ kind: "calendly-fallback", reason: "network_error" });
    expect(windowOpen).toHaveBeenCalledWith(UPGRADE_URL, "_blank", "noreferrer,noopener");
  });
});
