/**
 * Tether decrypt bridge — v2.17 Wave 2
 *
 * Glue layer between the Rust tether_host task and the existing v2.15 JS
 * crypto stack. Listens for `tether_decrypt` Tauri events (emitted by
 * tether_host.rs when it receives an AEAD-decrypted browser request), runs
 * the per-event sig-verify + AEAD decrypt pipeline, and returns the
 * plaintext reply via the `tether_decrypt_response` Tauri command.
 *
 * The Team Key and sig-verification live here (JS), never in Rust, so
 * all existing v2.15 invariants are preserved:
 *   - Team Key never leaves desktop memory.
 *   - Signature is verified BEFORE content is surfaced to the caller.
 *   - Events with an invalid signature are returned with sig_valid=false;
 *     the browser sees only that bit, never the attempted plaintext.
 *
 * Lifecycle: call `startTetherDecryptBridge()` once after login, call
 * `stopTetherDecryptBridge()` on logout. Both are idempotent.
 */

import { decryptEventPayload, type DecryptContext } from "@/lib/cloud-api";
import { loadTeamKey } from "@/lib/e2e/teamKey";
import { getTeamMemberE2eKeys } from "@/lib/cloud-api";
import type { TeamEvent, SharedResourceKind } from "@/lib/cloud-api";

// ── Tauri wrappers ────────────────────────────────────────────────────────

async function tauriListen(
  event: string,
  handler: (payload: unknown) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return listen(event, (e) => handler(e.payload));
}

async function tauriInvoke(cmd: string, args?: Record<string, unknown>): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke(cmd, args);
}

async function tauriInvokeResult<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return (await invoke<T>(cmd, args));
}

// ── Request/reply shape (must match tether_host.rs + browser client) ─────

interface DecryptRequest {
  request_id: string;
  kind: "decrypt_events";
  team_id: string;
  resource_kind: SharedResourceKind;
  resource_id: string;
  /** Fetch events with seq_num > since. */
  since: number;
  limit: number;
}

interface DecryptedEvent extends TeamEvent {
  /** True iff the Ed25519 signature verified. False means redaction. */
  sig_valid: boolean;
}

interface DecryptReply {
  request_id: string;
  ok: boolean;
  events?: DecryptedEvent[];
  error?: string;
}

// ── v2.18 Wave 1 — browser-driven dispatch frames ─────────────────────────
//
// Shape mirrors apps/web/src/lib/tether/dispatchFrames.ts. The browser
// sends DispatchRequestFrame; the desktop bridge fires the dispatch
// locally via the existing prompt_agent Tauri command, then replies
// with one DispatchChunkFrame + one DispatchCompleteFrame. Wave 1 is
// claude-only and batch-flushes at end (no real streaming yet).
type DispatchRuntime =
  | "claude" | "codex" | "gemini" | "openclaw" | "hermes"
  | "minimax" | "grok" | "deepseek" | "qwen" | "openrouter";

interface DispatchRequestFrame {
  kind: "dispatch_request";
  request_id: string;
  runtime: DispatchRuntime;
  prompt: string;
  model?: string | null;
  agent_slug?: string | null;
  war_room_id?: string | null;
  war_room_round?: number | null;
  workspace_root?: string | null;
}

type DispatchStatus = "success" | "failed" | "denied" | "cancelled";

interface DispatchChunkFrame {
  kind: "dispatch_chunk";
  request_id: string;
  chunk_index: number;
  text: string;
}

interface DispatchCompleteFrame {
  kind: "dispatch_complete";
  request_id: string;
  status: DispatchStatus;
  execution_log_id?: string | null;
  cost_usd?: number | null;
  tokens_in?: number | null;
  tokens_out?: number | null;
  model?: string | null;
  duration_ms?: number | null;
  error?: string | null;
}

// ── Bridge state ──────────────────────────────────────────────────────────

let _unlisten: (() => void) | null = null;

// Cache of team member pubkeys: team_id → pubkey map.
// Refreshed per-request on cache miss; TTL managed by the existing
// getTeamMemberE2eKeys response (no local TTL needed here — the
// desktop process is long-lived and key rotations are rare).
const memberPubkeyCache = new Map<
  string,
  Record<string, { ed25519_pubkey: string; key_id: string }>
>();

// ── Public API ────────────────────────────────────────────────────────────

/**
 * Start listening for `tether_decrypt` events from the Rust host task.
 * Idempotent: calling twice replaces the old listener cleanly.
 */
export async function startTetherDecryptBridge(): Promise<void> {
  // Remove any existing listener before registering a new one.
  _unlisten?.();
  _unlisten = null;

  const unlisten = await tauriListen("tether_decrypt", (payload) => {
    // Fire-and-forget; errors are caught inside handleDecryptRequest.
    void handleDecryptRequest(
      payload as { session_id: string; request_id: string; plain_request_json: string },
    );
  });
  _unlisten = unlisten;
}

/**
 * Stop listening for `tether_decrypt` events.
 * Call on logout or when the tether host is stopped.
 */
export function stopTetherDecryptBridge(): void {
  _unlisten?.();
  _unlisten = null;
  memberPubkeyCache.clear();
}

// ── Core handler ──────────────────────────────────────────────────────────

async function handleDecryptRequest(payload: {
  session_id: string;
  request_id: string;
  plain_request_json: string;
}): Promise<void> {
  const { session_id, request_id, plain_request_json } = payload;

  // Parse the request.
  let raw: { kind?: string };
  try {
    raw = JSON.parse(plain_request_json) as { kind?: string };
  } catch (err) {
    await replyError(session_id, request_id, `request JSON parse failed: ${String(err)}`);
    return;
  }

  // v2.18 Wave 1 — route dispatch_request frames to the dispatch handler.
  // The browser sends these via sendTetherFrame (NOT tetherRpc), so there's
  // no pending RPC on the browser side waiting for a request_id-matched
  // reply — we respond with separate chunk + complete frames that the
  // browser routes through hostFrameListeners.
  if (raw.kind === "dispatch_request") {
    await handleDispatchRequest(session_id, plain_request_json);
    return;
  }

  // v2.18 Wave 1 R1 (codex #1) — dispatch_cancel is in the published
  // contract but the runtime hook isn't wired yet (Wave 2 work — the
  // Tauri-side prompt_agent has no abort handle today). The browser
  // surface that ships in Wave 1 doesn't actually send this frame yet,
  // but if a future client or replay does, we don't want to log it as
  // "unknown request kind" — that would mask the real Wave 2 gap.
  // For now, accept-and-ignore; Wave 2 will wire it through to
  // prompt_agent_cancel.
  if (raw.kind === "dispatch_cancel") {
    console.warn(
      "[tether/host.ts] dispatch_cancel received but not yet honored (Wave 2 feature). Ignoring.",
    );
    return;
  }

  if (raw.kind !== "decrypt_events") {
    await replyError(session_id, request_id, `unknown request kind: ${raw.kind}`);
    return;
  }
  const req = raw as DecryptRequest;

  try {
    // 1. Load the Team Key (from cache or keychain+cloud envelope).
    const teamKey = await loadTeamKey(req.team_id);

    // 2. Load or refresh member pubkeys for sig verification.
    let memberPubkeys = memberPubkeyCache.get(req.team_id);
    if (!memberPubkeys) {
      const keyList = await getTeamMemberE2eKeys(req.team_id);
      const map: Record<string, { ed25519_pubkey: string; key_id: string }> = {};
      for (const k of keyList) {
        map[k.member_user_id] = {
          ed25519_pubkey: k.ed25519_pubkey,
          key_id: k.key_id,
        };
      }
      memberPubkeyCache.set(req.team_id, map);
      memberPubkeys = map;
    }

    // 3. Fetch the raw events from the cloud for the requested range.
    //    We call backfillTeamEvents directly; the bridge owns the cloud fetch.
    const { backfillTeamEvents } = await import("@/lib/cloud-api");
    const rawEvents = await backfillTeamEvents(
      req.team_id,
      req.resource_kind,
      req.resource_id,
      req.since,
      Math.min(req.limit, 200), // cap at 200 per the cloud default
    );

    // 4. For each event: run v2.15 sig-verify + AEAD decrypt.
    const ctx: DecryptContext = { teamId: req.team_id, resourceId: req.resource_id };
    const decryptedEvents: DecryptedEvent[] = [];

    for (const raw of rawEvents) {
      // decryptEventPayload never throws; it returns __decrypt_error sentinel on failure.
      const decrypted = await decryptEventPayload(raw, teamKey, memberPubkeys, ctx);

      const payloadObj = decrypted.payload_json as Record<string, unknown> | null;
      const hasDecryptError =
        payloadObj !== null &&
        typeof payloadObj === "object" &&
        payloadObj.__decrypt_error === true;

      // Determine sig_valid.
      // - If there's no signature_b64, we consider it not-signed (sig_valid = true
      //   for plaintext-mode events, consistent with the v2.15 contract).
      // - If signature was present and decryptEventPayload returned __decrypt_error,
      //   sig_valid = false.
      const sig_valid = !hasDecryptError;

      if (hasDecryptError) {
        // Return the event with redacted payload + sig_valid=false.
        // Browser renders a redaction UI; it must NOT see the original ciphertext.
        decryptedEvents.push({
          ...raw,
          payload_json: null,
          sig_valid: false,
        });
      } else {
        decryptedEvents.push({ ...decrypted, sig_valid });
      }
    }

    // 5. Reply via tether_decrypt_response Tauri command.
    const reply: DecryptReply = {
      request_id,
      ok: true,
      events: decryptedEvents,
    };
    await tauriInvoke("tether_decrypt_response", {
      sessionId: session_id,
      requestId: request_id,
      plainReplyJson: JSON.stringify(reply),
    });
  } catch (err) {
    await replyError(session_id, request_id, String(err));
  }
}

async function replyError(
  sessionId: string,
  requestId: string,
  error: string,
): Promise<void> {
  console.error("[tether/host.ts] decrypt error:", error);
  const reply: DecryptReply = { request_id: requestId, ok: false, error };
  try {
    await tauriInvoke("tether_decrypt_response", {
      sessionId,
      requestId,
      plainReplyJson: JSON.stringify(reply),
    });
  } catch (e) {
    console.error("[tether/host.ts] failed to send error reply:", e);
  }
}

// ── v2.18 Wave 1 — dispatch_request handler ──────────────────────────────

async function handleDispatchRequest(
  sessionId: string,
  plainRequestJson: string,
): Promise<void> {
  let req: DispatchRequestFrame;
  try {
    req = JSON.parse(plainRequestJson) as DispatchRequestFrame;
  } catch (err) {
    console.error("[tether/host.ts] dispatch_request parse failed:", err);
    return;
  }

  // Wave 1 gate: claude only. Anything else replies denied immediately.
  if (req.runtime !== "claude") {
    await sendCompleteFrame(sessionId, {
      kind: "dispatch_complete",
      request_id: req.request_id,
      status: "denied",
      error: `Wave 1 supports claude only; got ${req.runtime}. More runtimes ship in next wave.`,
    });
    return;
  }

  const startedAt = performance.now();

  try {
    // Build the optional config payload — model override only for now.
    // Matches the existing prompt_agent contract (commands/mod.rs:1224).
    const config = req.model
      ? JSON.stringify({ model: req.model })
      : undefined;

    const output = await tauriInvokeResult<string>("prompt_agent", {
      runtime: req.runtime,
      prompt: req.prompt,
      config,
      agentSlug: req.agent_slug ?? null,
      // v2.18 Wave 1 R1 (codex #3) — workspace_root is in the published
      // frame contract but Wave 1 intentionally drops it on both sides.
      // Wave 2 adds the validation step (workspace_root MUST match one
      // of the user's registered project roots on the desktop) before
      // surfacing it to prompt_agent. Until then, defense in depth: even
      // if a tampered browser includes workspace_root in the frame, we
      // ignore it here so the dispatch runs against the desktop's CWD
      // — never an attacker-controlled path.
      workspace: null,
    });

    const durationMs = Math.round(performance.now() - startedAt);

    // CLI runtimes batch-flush at end (per [[codex-dispatch-no-stream]]).
    // Wave 1 sends the entire output as a single chunk, then completes.
    // Wave 2 will swap this for the actual streaming hook so the browser
    // sees tokens as they're generated.
    await sendChunkFrame(sessionId, {
      kind: "dispatch_chunk",
      request_id: req.request_id,
      chunk_index: 0,
      text: output,
    });

    await sendCompleteFrame(sessionId, {
      kind: "dispatch_complete",
      request_id: req.request_id,
      status: "success",
      model: req.model ?? null,
      duration_ms: durationMs,
      // Wave 1 doesn't surface execution_log_id / cost / tokens —
      // prompt_agent returns the raw output string today. Wave 2 will
      // either return a struct or query execution_logs by run_id.
    });
  } catch (err) {
    const durationMs = Math.round(performance.now() - startedAt);
    const message = err instanceof Error ? err.message : String(err);
    await sendCompleteFrame(sessionId, {
      kind: "dispatch_complete",
      request_id: req.request_id,
      status: "failed",
      duration_ms: durationMs,
      error: message,
    });
  }
}

async function sendChunkFrame(
  sessionId: string,
  frame: DispatchChunkFrame,
): Promise<void> {
  try {
    await tauriInvoke("tether_decrypt_response", {
      sessionId,
      // request_id on the wire is informational — the browser routes
      // chunk frames by kind, not by matching pending RPCs.
      requestId: frame.request_id,
      plainReplyJson: JSON.stringify(frame),
    });
  } catch (e) {
    console.error("[tether/host.ts] sendChunkFrame failed:", e);
  }
}

async function sendCompleteFrame(
  sessionId: string,
  frame: DispatchCompleteFrame,
): Promise<void> {
  try {
    await tauriInvoke("tether_decrypt_response", {
      sessionId,
      requestId: frame.request_id,
      plainReplyJson: JSON.stringify(frame),
    });
  } catch (e) {
    console.error("[tether/host.ts] sendCompleteFrame failed:", e);
  }
}
