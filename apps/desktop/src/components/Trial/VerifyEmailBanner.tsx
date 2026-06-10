import { useEffect, useState } from "react";
import { Mail, X } from "lucide-react";

import { useAuthStore } from "@/hooks/useAuth";

// PR-pricing-visibility (2026-05-26) — email-verification reminder.
//
// Renders when:
//   - The user signed in via cloud (isCloudUser = true)
//   - The /auth/me response said email_verified === false
//   - The user hasn't dismissed it this session
//
// Hidden for local-only users (no email anyway) and for cloud users
// who already verified. Dismissal is per-session so users who close
// once still see it next launch — same logic as TrialBanner.
//
// "Resend" hits POST /api/auth/resend-verification with the user's
// email. The endpoint is intentionally idempotent + anti-enumeration
// (always returns 200), so a successful POST tells us nothing about
// whether the email was actually sent — we surface a generic "Sent
// (check your spam folder)" toast either way.

const SESSION_KEY = "ato.verify-email.bannerDismissed";

const CLOUD_API_URL =
  import.meta.env.VITE_CLOUD_API_URL || "https://api.agentictool.ai";

export default function VerifyEmailBanner() {
  const isCloudUser = useAuthStore((s) => s.isCloudUser);
  const user = useAuthStore((s) => s.user);
  const accessToken = useAuthStore((s) => s.accessToken);
  // The `User` shape in useAuth doesn't carry email_verified yet (it
  // lives on the cloud-api `CloudUser`); we read it from a refetch.
  const [emailVerified, setEmailVerified] = useState<boolean | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [resendState, setResendState] = useState<
    "idle" | "sending" | "sent" | "error"
  >("idle");

  // Read sessionStorage dismissal once on mount.
  useEffect(() => {
    if (typeof window === "undefined") return;
    try {
      if (window.sessionStorage.getItem(SESSION_KEY) === "1") {
        setDismissed(true);
      }
    } catch {
      /* sessionStorage unavailable — banner just stays visible */
    }
  }, []);

  // Fetch /auth/me on mount, on token change, AND on window focus —
  // the focus trigger catches the case where the user clicks the
  // verify-email link in a separate browser tab. Without it, the
  // banner persists until the user logs out or the desktop process
  // restarts. (Caught live by Will 2026-05-27 on willnigri+4.)
  //
  // v2.13.1 — Tauri's webview doesn't fire DOM `window.focus` reliably
  // when the macOS app switches to foreground from another app. Subscribe
  // to Tauri's native `onFocusChanged` instead (still keep the DOM
  // listener as a browser-build fallback). Caught live by Will
  // 2026-05-27 on willnigri+3 — Cmd+R cleared the banner; pure DOM
  // focus did not.
  useEffect(() => {
    if (!isCloudUser || !accessToken) {
      setEmailVerified(null);
      return;
    }
    let cancelled = false;
    let tauriUnlisten: (() => void) | null = null;
    const fetchVerified = async () => {
      try {
        const resp = await fetch(`${CLOUD_API_URL}/api/auth/me`, {
          headers: { Authorization: `Bearer ${accessToken}` },
        });
        if (!resp.ok) {
          if (!cancelled) setEmailVerified(null);
          return;
        }
        const body = (await resp.json()) as {
          data?: { user?: { email_verified?: boolean } };
        };
        const verified = body.data?.user?.email_verified;
        if (cancelled) return;
        // Treat undefined as "verified" so older backends + already-
        // verified users don't trip the banner. Only an explicit
        // `false` should render.
        setEmailVerified(verified === false ? false : true);
      } catch {
        if (!cancelled) setEmailVerified(null);
      }
    };
    fetchVerified();
    const onFocus = () => {
      // Re-fetch when the window regains focus. Customer clicked the
      // email link in a separate tab → desktop refocus picks up the
      // new email_verified=true on the next /auth/me poll.
      fetchVerified();
    };
    window.addEventListener('focus', onFocus);
    // Tauri-native focus subscription (load lazily so browser builds
    // of the same component don't crash on the missing module).
    if (typeof window !== 'undefined' && '__TAURI__' in window) {
      import('@tauri-apps/api/window')
        .then(({ getCurrentWindow }) => {
          if (cancelled) return;
          return getCurrentWindow().onFocusChanged(({ payload: focused }) => {
            if (focused) fetchVerified();
          });
        })
        .then((unlisten) => {
          if (cancelled) {
            unlisten?.();
            return;
          }
          tauriUnlisten = unlisten ?? null;
        })
        .catch(() => {
          /* not a Tauri context, or webview API missing — DOM fallback covers it */
        });
    }
    return () => {
      cancelled = true;
      window.removeEventListener('focus', onFocus);
      tauriUnlisten?.();
    };
  }, [isCloudUser, accessToken]);

  if (!isCloudUser || emailVerified !== false || dismissed) return null;

  const dismiss = () => {
    setDismissed(true);
    try {
      window.sessionStorage.setItem(SESSION_KEY, "1");
    } catch {
      /* best-effort */
    }
  };

  const resend = async () => {
    if (!user?.email || resendState === "sending") return;
    setResendState("sending");
    try {
      const resp = await fetch(
        `${CLOUD_API_URL}/api/auth/resend-verification`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ email: user.email }),
        },
      );
      // Anti-enumeration endpoint — always 200. Surface whichever
      // friendly string the backend gave us, fall back to a stable one.
      if (resp.ok) {
        setResendState("sent");
      } else {
        setResendState("error");
      }
    } catch {
      setResendState("error");
    }
  };

  return (
    <div
      role="status"
      aria-live="polite"
      className="flex items-center gap-3 border-b border-cs-border bg-amber-500/10 px-4 py-2 text-xs"
    >
      <Mail size={14} className="text-amber-500 shrink-0" aria-hidden />
      <span className="text-cs-text">
        Please verify your email (<strong>{user?.email}</strong>) so we can keep
        your trial + billing in sync.
      </span>
      <button
        type="button"
        onClick={resend}
        disabled={resendState === "sending"}
        className="ml-auto inline-flex items-center gap-1 text-amber-600 hover:underline disabled:opacity-50"
      >
        {resendState === "idle" && "Resend verification"}
        {resendState === "sending" && "Sending..."}
        {resendState === "sent" && "Sent — check your spam folder"}
        {resendState === "error" && "Send failed — try again"}
      </button>
      <button
        type="button"
        aria-label="Dismiss verification banner"
        onClick={dismiss}
        className="text-cs-muted hover:text-cs-text shrink-0"
      >
        <X size={14} />
      </button>
    </div>
  );
}
