import { UPGRADE_URL } from "@/lib/constants";
import { useAuthStore } from "@/hooks/useAuth";

// Self-serve Stripe checkout. Pre-PR, "Upgrade" pointed at a Calendly
// link (cal.com/willnigri/ato-onboarding) and every conversion required
// a sales call. This helper hits POST /api/billing/checkout on the
// authenticated cloud and opens the returned Stripe session URL in the
// customer's browser. Stripe session creation, webhook verification,
// and tier mutation all stay server-side in ato-cloud — this file is
// just a thin HTTP client + browser-open, safe to ship in OSS.
//
// Redirects use https://agentictool.ai instead of the ato:// deep-link
// scheme because the desktop doesn't register a URL scheme yet (no
// tauri-plugin-deep-link). Web-side redirect works on day 1; the
// landing page tells the user to return to the desktop, which picks
// up the new tier on its next /auth/me probe (24h cache or manual
// refresh from the Settings panel).

const CLOUD_API_URL =
  import.meta.env.VITE_CLOUD_API_URL || "https://api.agentictool.ai";

const SUCCESS_URL =
  "https://agentictool.ai/billing/success?session_id={CHECKOUT_SESSION_ID}";
const CANCEL_URL =
  "https://agentictool.ai/billing/cancel?session_id={CHECKOUT_SESSION_ID}";

// Only "pro" has a wired call site today. Team self-serve requires a
// 5-seat minimum on the cloud side (services/billing/src/checkout.ts:139)
// and a Team-pricing UI that hasn't shipped — keep this union honest.
export type CheckoutTier = "pro";

export type CheckoutResult =
  | { kind: "stripe-opened" }
  | { kind: "calendly-fallback"; reason: CalendlyReason; notice: string };

export type CalendlyReason =
  | "no_jwt"
  | "pro_required"
  | "network_error";

export class CheckoutError extends Error {
  code: string;
  constructor(code: string, message: string) {
    super(message);
    this.name = "CheckoutError";
    this.code = code;
  }
}

function inTauri(): boolean {
  if (typeof window === "undefined") return false;
  const w = window as unknown as {
    __TAURI__?: unknown;
    __TAURI_INTERNALS__?: unknown;
  };
  return Boolean(w.__TAURI__ || w.__TAURI_INTERNALS__);
}

async function openExternal(url: string): Promise<void> {
  if (inTauri()) {
    const { open } = await import("@tauri-apps/plugin-shell");
    await open(url);
    return;
  }
  // inTauri() returned false, which means window exists — guard above
  // covers SSR. No second window check needed.
  window.open(url, "_blank", "noreferrer,noopener");
}

type CheckoutResponseBody = {
  success: boolean;
  data?: { url?: string; sessionId?: string };
  error?: { code?: string; message?: string };
};

async function postCheckout(
  jwt: string,
  tier: CheckoutTier,
): Promise<Response> {
  return fetch(`${CLOUD_API_URL}/api/billing/checkout`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${jwt}`,
    },
    body: JSON.stringify({
      tier,
      successUrl: SUCCESS_URL,
      cancelUrl: CANCEL_URL,
    }),
  });
}

async function postCheckoutWithRefresh(
  jwt: string,
  tier: CheckoutTier,
): Promise<Response> {
  // First attempt with the JWT the caller had at click time. If the
  // access token has expired (common when the trial banner sits idle
  // for >access-token-TTL before the user clicks Upgrade), the cloud
  // returns 401; we ask the auth store to refresh and retry once with
  // the fresh token from the store.
  const initial = await postCheckout(jwt, tier);
  if (initial.status !== 401) return initial;

  const refreshed = await useAuthStore.getState().refreshAccessToken();
  if (!refreshed) return initial;
  const next = useAuthStore.getState().accessToken;
  if (!next || next === jwt) return initial;
  return postCheckout(next, tier);
}

/** Open a Stripe Checkout session for the given tier in the customer's
 *  browser. Returns once the URL has been opened — completion of the
 *  payment lands later via the webhook + an https://agentictool.ai
 *  landing page that tells the user to return to the desktop. Throws
 *  CheckoutError for transient failures the UI should surface (5xx,
 *  malformed cloud response). Returns a `calendly-fallback` result
 *  when the no-JWT / 402-PRO_REQUIRED / network-down branches fire,
 *  so the UI can render a one-line notice next to the Calendly redirect. */
export async function startCheckout(
  tier: CheckoutTier,
  jwt: string,
): Promise<CheckoutResult> {
  if (!jwt) {
    await openExternal(UPGRADE_URL);
    return {
      kind: "calendly-fallback",
      reason: "no_jwt",
      notice: "Opening founder-led onboarding — sign in to use self-serve checkout.",
    };
  }

  let response: Response;
  try {
    response = await postCheckoutWithRefresh(jwt, tier);
  } catch (err) {
    // Network down or DNS dead — Calendly is still reachable for most users.
    await openExternal(UPGRADE_URL);
    return {
      kind: "calendly-fallback",
      reason: "network_error",
      notice:
        err instanceof Error && err.message
          ? `Couldn't reach billing (${err.message}). Opening onboarding instead.`
          : "Couldn't reach billing. Opening onboarding instead.",
    };
  }

  if (response.status === 402) {
    // PRO_REQUIRED — account isn't eligible for self-serve (already on
    // a paid tier, or grandfathered into a sales-only plan). Tell the
    // user before the Calendly redirect happens so they don't think
    // their click went into a black hole.
    await openExternal(UPGRADE_URL);
    return {
      kind: "calendly-fallback",
      reason: "pro_required",
      notice:
        "Your account uses founder-led onboarding for upgrades — opening the booking page.",
    };
  }

  // response.ok responses must carry a parseable JSON body with the
  // Stripe URL; anything else is a cloud bug worth surfacing. For
  // non-ok responses we still try to parse to extract the structured
  // error code, but a parse failure there is non-fatal.
  let body: CheckoutResponseBody | null = null;
  if (response.ok) {
    try {
      body = (await response.json()) as CheckoutResponseBody;
    } catch (err) {
      throw new CheckoutError(
        "MALFORMED_RESPONSE",
        err instanceof Error
          ? `Billing service returned an unreadable response (${err.message}).`
          : "Billing service returned an unreadable response.",
      );
    }
  } else {
    try {
      body = (await response.json()) as CheckoutResponseBody;
    } catch {
      body = null;
    }
  }

  if (!response.ok || !body?.success || !body.data?.url) {
    const code = body?.error?.code || `HTTP_${response.status}`;
    const message =
      body?.error?.message || `Checkout failed (HTTP ${response.status}).`;
    throw new CheckoutError(code, message);
  }

  await openExternal(body.data.url);
  return { kind: "stripe-opened" };
}
