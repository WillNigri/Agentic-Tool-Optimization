import { UPGRADE_URL } from "@/lib/constants";

// Self-serve Stripe checkout. Pre-PR, "Upgrade" pointed at a Calendly
// link (cal.com/willnigri/ato-onboarding) and every conversion required
// a sales call. This helper hits POST /api/billing/checkout on the
// authenticated cloud and opens the returned Stripe session URL in the
// customer's browser. Stripe session creation, webhook verification,
// and tier mutation all stay server-side in ato-cloud — this file is
// just a thin HTTP client + browser-open, safe to ship in OSS.

const CLOUD_API_URL =
  import.meta.env.VITE_CLOUD_API_URL || "https://api.agentictool.ai";

// Cloud caps a server-side allow-list of redirect protocols. ato://
// deep-links the desktop after success/cancel; the cloud rewrites
// {CHECKOUT_SESSION_ID} on success.
const SUCCESS_DEEP_LINK = "ato://billing/success?session_id={CHECKOUT_SESSION_ID}";
const CANCEL_DEEP_LINK = "ato://billing/cancel";

export type CheckoutTier = "pro" | "team";

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
  if (typeof window !== "undefined") {
    window.open(url, "_blank", "noreferrer,noopener");
  }
}

export class CheckoutError extends Error {
  code: string;
  constructor(code: string, message: string) {
    super(message);
    this.name = "CheckoutError";
    this.code = code;
  }
}

/** Open a Stripe Checkout session for the given tier in the customer's
 *  browser. Returns once the URL has been opened — completion of the
 *  payment lands later via the webhook + an `ato://billing/success`
 *  deep link. On 402 PRO_REQUIRED (server says this account can't
 *  self-serve), falls back to the Calendly link so the user still has
 *  a path forward. */
export async function startCheckout(
  tier: CheckoutTier,
  jwt: string,
): Promise<void> {
  if (!jwt) {
    await openExternal(UPGRADE_URL);
    return;
  }

  let response: Response;
  try {
    response = await fetch(`${CLOUD_API_URL}/api/billing/checkout`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${jwt}`,
      },
      body: JSON.stringify({
        tier,
        successUrl: SUCCESS_DEEP_LINK,
        cancelUrl: CANCEL_DEEP_LINK,
      }),
    });
  } catch (err) {
    // Network down or DNS dead — Calendly is still reachable for most users.
    await openExternal(UPGRADE_URL);
    throw new CheckoutError(
      "NETWORK_ERROR",
      err instanceof Error ? err.message : "Unable to reach billing service",
    );
  }

  if (response.status === 402) {
    // PRO_REQUIRED — account isn't eligible for self-serve (already
    // on a paid tier, or grandfathered into a sales-only plan).
    await openExternal(UPGRADE_URL);
    return;
  }

  const body = (await response.json().catch(() => null)) as
    | { success: boolean; data?: { url?: string }; error?: { code?: string; message?: string } }
    | null;

  if (!response.ok || !body?.success || !body.data?.url) {
    const code = body?.error?.code || `HTTP_${response.status}`;
    const message = body?.error?.message || `Checkout failed (HTTP ${response.status})`;
    throw new CheckoutError(code, message);
  }

  await openExternal(body.data.url);
}
