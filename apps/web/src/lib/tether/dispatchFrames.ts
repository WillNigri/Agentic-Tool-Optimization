// #81 — v2.18 Wave 1 — browser-driven dispatch over the tether channel.
//
// Shared frame schema between the Rust desktop host and the browser
// client. Both sides serialize/deserialize against THIS file's types;
// adding a new field means editing once and both sides typecheck.
//
// The frames live INSIDE the AEAD payload of the existing v2.17 tether
// channel — the cloud relay never sees plaintext. See
// docs/v2.18-active-workstation.md for the security model.

import type { AgentRuntime } from "@/components/cron/types";

/** Browser → Desktop. Request the desktop to fire a dispatch. */
export interface DispatchRequestFrame {
  kind: "dispatch_request";
  /** UUID minted by the browser; echoed back on every chunk + complete. */
  request_id: string;
  /** Which runtime to use. Initially Wave 1 only supports "claude". */
  runtime: AgentRuntime;
  /** The prompt to send. */
  prompt: string;
  /** Optional explicit model override; defaults to the agent's stored
   *  model or the runtime's default. */
  model?: string | null;
  /** Optional agent slug to attribute the dispatch to. */
  agent_slug?: string | null;
  /** Optional war-room grouping (for parallel fan-outs). */
  war_room_id?: string | null;
  /** Round number for multi-round war-rooms. */
  war_room_round?: number | null;
  /** Optional registered workspace root; MUST match one of the user's
   *  registered project roots on the desktop (server-side validated). */
  workspace_root?: string | null;
}

/** Desktop → Browser. Streams partial output back as the runtime
 *  emits it. Wave 1: CLI runtimes batch-flush at end (one chunk at
 *  completion). API providers stream natively. */
export interface DispatchChunkFrame {
  kind: "dispatch_chunk";
  request_id: string;
  /** Monotonic 0-indexed chunk counter. The browser MAY treat gaps as
   *  protocol failures (the AEAD seq order is enforced by the
   *  underlying tether channel, so an out-of-order chunk_index is a
   *  bug). */
  chunk_index: number;
  /** UTF-8 text payload. */
  text: string;
}

/** Desktop → Browser. Exactly one per request_id. Signals the dispatch
 *  ended for any reason. */
export interface DispatchCompleteFrame {
  kind: "dispatch_complete";
  request_id: string;
  /** Terminal status. */
  status: "success" | "failed" | "denied" | "cancelled";
  /** UUID of the execution_logs row written for this dispatch. */
  execution_log_id?: string | null;
  /** USD cost as recorded (may be 0 for subscription-billed runtimes). */
  cost_usd?: number | null;
  tokens_in?: number | null;
  tokens_out?: number | null;
  /** The model that actually ran (may differ from the requested model
   *  if the runtime fell back). */
  model?: string | null;
  duration_ms?: number | null;
  /** Populated when status !== "success". */
  error?: string | null;
}

/** Browser → Desktop. Optional cancel signal. If received before the
 *  runtime completes, the dispatch is aborted and dispatch_complete
 *  fires with status="cancelled". After completion, cancel is a no-op
 *  (the spend already happened). */
export interface DispatchCancelFrame {
  kind: "dispatch_cancel";
  request_id: string;
}

/** Union of all Wave 1 dispatch frame kinds. */
export type DispatchFrame =
  | DispatchRequestFrame
  | DispatchChunkFrame
  | DispatchCompleteFrame
  | DispatchCancelFrame;

/** Type-guard against the discriminator. */
export function isDispatchFrame(v: unknown): v is DispatchFrame {
  if (!v || typeof v !== "object") return false;
  const k = (v as { kind?: unknown }).kind;
  return (
    k === "dispatch_request" ||
    k === "dispatch_chunk" ||
    k === "dispatch_complete" ||
    k === "dispatch_cancel"
  );
}
