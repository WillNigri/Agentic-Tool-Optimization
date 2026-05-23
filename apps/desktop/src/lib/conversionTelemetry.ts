// Strategy PR-B (2026-05-21) — willingness-to-pay (WTP) counter.
//
// Every `useFeatureFlag(feature)` call goes through `recordFeatureUse` so
// the funnel dashboard can answer "which gated features do today's Free
// users actually touch" BEFORE we re-introduce paid Pro (currently every
// Tauri user is silently auto-promoted in `tier.ts`).
//
// Architecture war-room (2026-05-21) constraints baked in:
//   - In-memory `Map` per (session_id, feature). NO SQLite write per
//     event. Flush every 60s + on visibilitychange→hidden + on unload.
//   - tier + trial_cohort SNAPSHOTTED at first-seen time. Mid-session
//     tier change → next flush window starts a new row.
//   - session_id is a renderer-minted UUID, NOT joined to sessions.id
//     (CSO seat: prevents joinable behavioral profile).
//   - Rows stay LOCAL. No cloud-forward in this PR. The A5 trial_cohort
//     piggybacks on `agent_traces` uploads via `metadata.trial_cohort`
//     instead — separate consent surface.

import type { Tier } from "@/hooks/useAuth";

/** A5 trial cohort labels. Use a stable enum, not free-form strings —
 *  prevents `null` / `"none"` / `"control"` drift across the codebase
 *  (called out by minimax in the 2026-05-21 architecture war-room).
 *  Extend this union when new cohorts launch. */
export type TrialCohort = "A5" | "control" | null;

const STORAGE_KEY = "ato.trialCohort";
const SESSION_KEY = "ato.sessionId";
const FLUSH_INTERVAL_MS = 60_000;

interface Counter {
  count: number;
  firstSeenAt: string;
  lastSeenAt: string;
  tier: Tier;
  trialCohort: TrialCohort;
}

interface PendingEvent {
  sessionId: string;
  feature: string;
  tierAtEvent: Tier;
  trialCohort: TrialCohort;
  count: number;
  firstSeenAt: string;
  lastSeenAt: string;
}

const counters: Map<string, Counter> = new Map();
let flushTimer: ReturnType<typeof setInterval> | null = null;
let lifecycleWired = false;

/** Read the current trial cohort. Persisted via localStorage so the
 *  cohort is stable across reloads but writable from devtools / a
 *  future cohort-assignment hook. Returns null for users not enrolled.
 *
 *  Cohort is read at TWO different call sites and the readings can
 *  diverge if cohort changes mid-session — this is by design per the
 *  2026-05-21 architecture war-room:
 *   - `recordFeatureUse` snapshots cohort at first-seen for the local
 *     `conversion_events` row (immutable for the rest of that counter)
 *   - `agentTraceUpload.withTrialCohort` reads it live at upload time
 *     because the upload IS the per-trace snapshot
 *  Code-review war-room (2026-05-22) confirmed: harmless today because
 *  cohorts are assigned once at enrollment; revisit if a re-assignment
 *  hook ever lands. */
export function getTrialCohort(): TrialCohort {
  if (typeof window === "undefined" || !window.localStorage) return null;
  const raw = window.localStorage.getItem(STORAGE_KEY);
  if (raw === "A5" || raw === "control") return raw;
  return null;
}

/** Boot-time UUID. Per the war-room ruling this is a throwaway id —
 *  never linked to user, project, or runtime session. Survives a
 *  reload (`sessionStorage` lifetime) so counters in the same window
 *  aggregate, but does not persist across app restarts. */
export function getSessionId(): string {
  if (typeof window === "undefined" || !window.sessionStorage) {
    return "no-session";
  }
  let id = window.sessionStorage.getItem(SESSION_KEY);
  if (!id) {
    id =
      typeof crypto !== "undefined" && "randomUUID" in crypto
        ? crypto.randomUUID()
        : `s-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
    window.sessionStorage.setItem(SESSION_KEY, id);
  }
  return id;
}

/** Increment the in-memory counter for one feature-flag invocation.
 *  Sync + cheap — `useFeatureFlag` calls this from a `useEffect`, so
 *  one mount of a gated UI = one bump (plus dev StrictMode double-
 *  mount). Disk + IPC happen only at flush time. Lifecycle wiring
 *  (interval + listeners) is set up once at module load via
 *  `ensureLifecycle()` below, NOT per-call. */
export function recordFeatureUse(feature: string, tier: Tier): void {
  const key = `${feature}|${tier}`;
  const now = new Date().toISOString();
  const existing = counters.get(key);
  if (existing) {
    existing.count += 1;
    existing.lastSeenAt = now;
  } else {
    counters.set(key, {
      count: 1,
      firstSeenAt: now,
      lastSeenAt: now,
      tier,
      trialCohort: getTrialCohort(),
    });
  }
}

/** Drain the in-memory counters and push them to the Rust side via
 *  the `record_conversion_events` Tauri command. Safe to call when
 *  there's nothing to flush — returns early. Best-effort: failures
 *  are logged, not thrown, because we never want telemetry to break
 *  the app. */
export async function flushConversionEvents(): Promise<void> {
  if (counters.size === 0) return;
  const sessionId = getSessionId();
  const drained: PendingEvent[] = [];
  for (const [key, counter] of counters.entries()) {
    drained.push({
      sessionId,
      feature: key.split("|")[0],
      tierAtEvent: counter.tier,
      trialCohort: counter.trialCohort,
      count: counter.count,
      firstSeenAt: counter.firstSeenAt,
      lastSeenAt: counter.lastSeenAt,
    });
  }
  counters.clear();

  // Always persist locally first — local is our source of truth.
  // Cloud-forward is best-effort and only runs if the user opted
  // in via the upgrade consent screen.
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("record_conversion_events", { events: drained });
  } catch (err) {
    console.warn("[conversion-telemetry] local flush failed (dropping batch)", err, {
      batchSize: drained.length,
    });
    // Don't attempt cloud-forward if even local failed — something
    // bigger is broken.
    return;
  }

  // v2.8.x Phase A chunk 5 — cloud-forward (Will's "we have no
  // telemetry" correction, 2026-05-22). Only fires when the user
  // has explicitly consented via the Pro upgrade screen.
  if (isTelemetryConsented()) {
    void forwardEventsToCloud(drained);
  }
}

// ──────────────────────────────────────────────────────────────────────
// Cloud-forward (chunk 5)
// ──────────────────────────────────────────────────────────────────────
//
// The local `record_conversion_events` Tauri command writes to SQLite
// for the user's own dashboard. Cloud-forward POSTs the same events
// to ato-cloud so WE (ATO) can see aggregate usage — which features
// matter, which models dominate, which MCPs win, etc.
//
// Pseudonymized: tied to the user's account ID (not IP, not device
// fingerprint). Per the war-room 87E6CADF round 2 consent screen:
//   ✓ Features, models, MCPs (hashed), costs (aggregate), active days
//   ✗ NEVER: prompts, responses, tool results, API keys, raw MCP names
//
// Failure mode: silent + best-effort. If cloud is down or auth is
// missing, drop the cloud batch. Local was already persisted; the
// user's own dashboard is unaffected.

const CLOUD_TELEMETRY_URL = "https://ato.cloud/api/telemetry/events";
const TELEMETRY_CONSENT_KEY = "ato.telemetry.consent";

/** Read the telemetry-consent flag set by the upgrade consent screen. */
export function isTelemetryConsented(): boolean {
  if (typeof window === "undefined" || !window.localStorage) return false;
  return window.localStorage.getItem(TELEMETRY_CONSENT_KEY) === "true";
}

/** Set the consent flag — called by the Pro upgrade flow. */
export function setTelemetryConsent(consented: boolean): void {
  if (typeof window === "undefined" || !window.localStorage) return;
  window.localStorage.setItem(TELEMETRY_CONSENT_KEY, consented ? "true" : "false");
}

async function readAuthToken(): Promise<string | null> {
  // The auth store holds the bearer token in-memory after login.
  // Importing it lazily avoids a circular dep (useAuth → tier → telemetry).
  try {
    const { useAuthStore } = await import("@/hooks/useAuth");
    const token = useAuthStore.getState().accessToken;
    return token || null;
  } catch {
    return null;
  }
}

async function forwardEventsToCloud(events: PendingEvent[]): Promise<void> {
  if (events.length === 0) return;
  const token = await readAuthToken();
  if (!token) return; // pre-auth user; nothing to forward to
  try {
    const resp = await fetch(CLOUD_TELEMETRY_URL, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${token}`,
      },
      // Defensive: cap batch size at 500 to avoid pathological bursts
      // that would either OOM the gateway or get rejected by the WAF.
      body: JSON.stringify({ events: events.slice(0, 500) }),
      // Silent: short timeout so a slow cloud doesn't slow the local app.
      signal: AbortSignal.timeout(5000),
    });
    if (!resp.ok) {
      console.warn(
        "[conversion-telemetry] cloud-forward returned non-2xx; dropping batch",
        { status: resp.status, batchSize: events.length },
      );
    }
  } catch (err) {
    console.warn("[conversion-telemetry] cloud-forward failed (best-effort, dropping)", err);
  }
}

/** Reset all in-memory state. For tests. */
export function __resetForTests(): void {
  counters.clear();
  if (flushTimer) {
    clearInterval(flushTimer);
    flushTimer = null;
  }
  lifecycleWired = false;
}

/** Snapshot of the current in-memory counter set. For tests only —
 *  do not read this in production code (use the funnel command). */
export function __peekCountersForTests(): Array<{ feature: string; counter: Counter }> {
  return Array.from(counters.entries()).map(([key, counter]) => ({
    feature: key.split("|")[0],
    counter,
  }));
}

function ensureLifecycle(): void {
  // window-check FIRST — otherwise an SSR/edge import would latch
  // `lifecycleWired = true` and the real window context would never
  // get the interval. (code-review war-room 2026-05-22, claude #7.)
  if (typeof window === "undefined") return;
  if (lifecycleWired) return;
  lifecycleWired = true;
  flushTimer = setInterval(() => {
    void flushConversionEvents();
  }, FLUSH_INTERVAL_MS);
  window.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") {
      void flushConversionEvents();
    }
  });
  // beforeunload flush is best-effort — browsers terminate async work
  // immediately on tab/window close, so the last 60s window typically
  // does not reach SQLite. Accepted as the design tradeoff (≤60s loss).
  window.addEventListener("beforeunload", () => {
    void flushConversionEvents();
  });
}

// Wire the flush lifecycle once at module load — keeps `recordFeatureUse`
// allocation-free and free of branch-prediction noise on the render hot
// path. Guarded internally by the `typeof window` check above so a Node
// / test import is a no-op until a real browser context is present.
ensureLifecycle();
