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

  // v2.18 Wave 2 — dispatch_cancel honors the request via the
  // existing active_runs::kill_active_run Tauri command. The map
  // from request_id → run_id is populated by the StreamEvent::Started
  // event that spawn_streaming_dispatch emits before its first chunk.
  // If the request_id isn't in the map yet (cancel raced the Started
  // event, or the dispatch already completed), we just log and skip —
  // no harm; the dispatch either never started or already ended.
  if (raw.kind === "dispatch_cancel") {
    await handleDispatchCancel(plain_request_json);
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

// ── v2.18 Wave 2 — dispatch_request streaming handler ────────────────────

/** Wave 2 enabled runtimes — CLI runtimes only. API providers
 *  (minimax/grok/deepseek/qwen/openrouter) require different billing
 *  semantics + budget caps; deferred to Wave 2.2. */
const WAVE2_ENABLED_RUNTIMES: ReadonlySet<DispatchRuntime> = new Set([
  "claude",
  "codex",
  "gemini",
  "openclaw",
  "hermes",
]);

/** request_id → active_run_id. Populated by StreamEvent::Started; consumed
 *  by handleDispatchCancel. Entries are removed when dispatch terminates
 *  (Done or Error) so the map doesn't leak across a long-lived session.
 *  Per-process state — there is only ever one tether host process per
 *  desktop install, so we don't need a per-session prefix here. */
const requestIdToRunId = new Map<string, string>();

/** StreamEvent shape from Rust apps/desktop/src-tauri/src/commands/mod.rs.
 *  Keep in sync with the `pub enum StreamEvent` definition there. */
type StreamEvent =
  | { kind: "started"; runId: string }
  | { kind: "chunk"; text: string }
  | {
      kind: "done";
      full: string;
      receipt?: {
        executionLogId: string;
        status: string;
        model?: string | null;
        costUsd?: number | null;
        tokensIn?: number | null;
        tokensOut?: number | null;
        durationMs: number;
      };
    }
  | { kind: "error"; message: string };

async function makeChannel<T>(): Promise<{
  channel: unknown;
  onMessage: (cb: (event: T) => void) => void;
}> {
  // The Tauri Channel API is constructed once and exposes `onmessage`.
  // We wrap it so the dispatch handler can register a typed callback
  // before passing the channel into `invoke('prompt_agent_stream', …)`.
  const { Channel } = await import("@tauri-apps/api/core");
  const channel = new Channel<T>();
  return {
    channel,
    onMessage: (cb) => {
      channel.onmessage = cb;
    },
  };
}

/** v2.18 Wave 2 — workspace_root validation against the registered
 *  projects table. Strict equality (per codex pre-war finding):
 *  silently allowing descendants weakens the trust boundary because
 *  a tampered browser could escape into sibling directories via
 *  symlinks or unicode tricks. If you want descendant matching, do
 *  it explicitly with path normalization in Wave 2.x. */
async function validateWorkspaceRoot(
  workspaceRoot: string | null | undefined,
): Promise<{ ok: true; resolved: string | null } | { ok: false; error: string }> {
  if (!workspaceRoot) {
    // Omitted → run against the desktop's CWD (Wave 1 behavior). The
    // dispatch_request frame is allowed to omit workspace_root; the
    // browser UI just doesn't surface a project picker yet.
    return { ok: true, resolved: null };
  }
  try {
    const projects = await tauriInvokeResult<Array<{ path: string }>>(
      "list_projects",
    );
    const match = projects.find((p) => p.path === workspaceRoot);
    if (!match) {
      return {
        ok: false,
        error: `workspace_root ${JSON.stringify(workspaceRoot)} is not a registered project on this desktop. Add it via Settings → Projects first.`,
      };
    }
    return { ok: true, resolved: workspaceRoot };
  } catch (err) {
    return {
      ok: false,
      error: `workspace_root validation failed: ${err instanceof Error ? err.message : String(err)}`,
    };
  }
}

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

  // v2.18 Wave 2 gate — CLI runtimes only. API providers come in 2.2.
  if (!WAVE2_ENABLED_RUNTIMES.has(req.runtime)) {
    await sendCompleteFrame(sessionId, {
      kind: "dispatch_complete",
      request_id: req.request_id,
      status: "denied",
      error: `Wave 2 supports ${[...WAVE2_ENABLED_RUNTIMES].join(", ")}; got ${req.runtime}. API providers ship in next wave.`,
    });
    return;
  }

  // v2.18 Wave 2 (codex pre-war #E) — validate workspace_root against
  // the registered projects table. Strict equality. A tampered browser
  // cannot make the desktop dispatch run against an arbitrary path.
  const wsCheck = await validateWorkspaceRoot(req.workspace_root);
  if (!wsCheck.ok) {
    await sendCompleteFrame(sessionId, {
      kind: "dispatch_complete",
      request_id: req.request_id,
      status: "denied",
      error: wsCheck.error,
    });
    return;
  }

  const startedAt = performance.now();
  let chunkIndex = 0;
  // Set when we get StreamEvent::Started; cleared when the dispatch
  // terminates. handleDispatchCancel looks here for the run_id.
  let activeRunId: string | null = null;

  const config = req.model ? JSON.stringify({ model: req.model }) : undefined;
  const { channel, onMessage } = await makeChannel<StreamEvent>();

  // We need a Promise that resolves when the dispatch terminates so
  // handleDispatchRequest doesn't return before the final frame is
  // sealed. The channel callback drives resolution.
  let resolveTerminate: () => void = () => {};
  const terminated = new Promise<void>((res) => {
    resolveTerminate = res;
  });

  onMessage((event) => {
    void (async () => {
      try {
        switch (event.kind) {
          case "started":
            activeRunId = event.runId;
            requestIdToRunId.set(req.request_id, event.runId);
            break;
          case "chunk":
            await sendChunkFrame(sessionId, {
              kind: "dispatch_chunk",
              request_id: req.request_id,
              chunk_index: chunkIndex++,
              text: event.text,
            });
            break;
          case "done": {
            if (activeRunId) requestIdToRunId.delete(req.request_id);
            const durationMs = event.receipt?.durationMs
              ?? Math.round(performance.now() - startedAt);
            await sendCompleteFrame(sessionId, {
              kind: "dispatch_complete",
              request_id: req.request_id,
              status: "success",
              execution_log_id: event.receipt?.executionLogId ?? null,
              cost_usd: event.receipt?.costUsd ?? null,
              tokens_in: event.receipt?.tokensIn ?? null,
              tokens_out: event.receipt?.tokensOut ?? null,
              model: event.receipt?.model ?? req.model ?? null,
              duration_ms: durationMs,
            });
            resolveTerminate();
            break;
          }
          case "error": {
            if (activeRunId) requestIdToRunId.delete(req.request_id);
            await sendCompleteFrame(sessionId, {
              kind: "dispatch_complete",
              request_id: req.request_id,
              status: "failed",
              duration_ms: Math.round(performance.now() - startedAt),
              error: event.message,
            });
            resolveTerminate();
            break;
          }
        }
      } catch (innerErr) {
        console.error("[tether/host.ts] dispatch event-handler failed:", innerErr);
        // Don't leak the inner error to the browser — the dispatch
        // itself may have completed; just log.
      }
    })();
  });

  try {
    // prompt_agent_stream resolves AFTER the stream loop ends (when
    // the child process exits / stderr is drained). The channel
    // events are what actually drive the dispatch_complete frame —
    // the resolved value from invoke() is just () (no payload).
    await tauriInvokeResult<void>("prompt_agent_stream", {
      runtime: req.runtime,
      prompt: req.prompt,
      config,
      onEvent: channel,
    });
  } catch (err) {
    // The Rust side errored before the StreamEvent::Error path could
    // emit (e.g. shell-out failed, channel registration error). Emit
    // a final complete frame ourselves so the browser doesn't hang.
    if (requestIdToRunId.has(req.request_id)) {
      requestIdToRunId.delete(req.request_id);
    }
    const message = err instanceof Error ? err.message : String(err);
    await sendCompleteFrame(sessionId, {
      kind: "dispatch_complete",
      request_id: req.request_id,
      status: "failed",
      duration_ms: Math.round(performance.now() - startedAt),
      error: message,
    });
    resolveTerminate();
  }

  // Wait for the channel's terminal event (Done/Error) to be sealed
  // and forwarded before returning. If the channel never delivered a
  // terminal — defensive timeout — we still return after 5 minutes
  // so the bridge isn't held hostage. The dispatch's own process
  // already exited (invoke returned), so this is just channel drain.
  const timeout = new Promise<void>((res) => setTimeout(res, 5 * 60 * 1000));
  await Promise.race([terminated, timeout]);
}

// ── v2.18 Wave 2 — dispatch_cancel handler ───────────────────────────────

async function handleDispatchCancel(plainRequestJson: string): Promise<void> {
  interface DispatchCancelFrame {
    kind: "dispatch_cancel";
    request_id: string;
  }
  let req: DispatchCancelFrame;
  try {
    req = JSON.parse(plainRequestJson) as DispatchCancelFrame;
  } catch (err) {
    console.error("[tether/host.ts] dispatch_cancel parse failed:", err);
    return;
  }
  const runId = requestIdToRunId.get(req.request_id);
  if (!runId) {
    // Cancel raced the Started event, or dispatch already terminated.
    console.warn(
      `[tether/host.ts] dispatch_cancel: no active run for request ${req.request_id} (already completed or never started).`,
    );
    return;
  }
  try {
    await tauriInvokeResult<boolean>("kill_active_run", { runId });
    // The kill triggers the child process's terminate; the streaming
    // dispatch's Error path fires; the channel emits StreamEvent::Error,
    // which the request handler turns into a dispatch_complete frame
    // with status="failed" + an error mentioning the cancel.
  } catch (err) {
    console.error("[tether/host.ts] kill_active_run invoke failed:", err);
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
