import { useEffect, useRef, useState } from "react";
import type { AgentRuntime } from "@/components/cron/types";
import {
  isDispatchFrame,
  type DispatchCancelFrame,
  type DispatchChunkFrame,
  type DispatchCompleteFrame,
  type DispatchRequestFrame,
} from "./dispatchFrames";

export type DispatchStatus =
  | "running"
  | "cancelling"
  | "success"
  | "failed"
  | "denied"
  | "cancelled";

export interface DispatchRunState {
  requestId: string;
  runtime: AgentRuntime;
  prompt: string;
  chunks: string[];
  status: DispatchStatus;
  error?: string;
  model?: string | null;
  costUsd?: number | null;
  tokensIn?: number | null;
  tokensOut?: number | null;
  durationMs?: number | null;
  executionLogId?: string | null;
}

type DispatchSendInput =
  | Omit<DispatchRequestFrame, "kind" | "request_id" | "workspace_root">
  | DispatchCancelFrame;

interface DispatchTransport {
  sendTetherFrame(payload: Record<string, unknown>): void;
  subscribeHostFrames(cb: (frame: Record<string, unknown>) => void): () => void;
}

function assertNever(x: never): never {
  throw new Error(`Unhandled dispatch frame: ${String(x)}`);
}

function applyChunk(
  prev: DispatchRunState | null,
  frame: DispatchChunkFrame,
): DispatchRunState | null {
  if (!prev || prev.requestId !== frame.request_id) return prev;
  return {
    ...prev,
    chunks: [...prev.chunks, frame.text],
    status: prev.status === "cancelling" ? "cancelling" : "running",
  };
}

function applyComplete(
  prev: DispatchRunState | null,
  frame: DispatchCompleteFrame,
): DispatchRunState | null {
  if (!prev || prev.requestId !== frame.request_id) return prev;
  return {
    ...prev,
    status: frame.status,
    error: frame.error ?? undefined,
    model: frame.model ?? null,
    costUsd: frame.cost_usd ?? null,
    tokensIn: frame.tokens_in ?? null,
    tokensOut: frame.tokens_out ?? null,
    durationMs: frame.duration_ms ?? null,
    executionLogId: frame.execution_log_id ?? null,
  };
}

export function useDispatchRequest(tetherClient: DispatchTransport) {
  const [current, setCurrent] = useState<DispatchRunState | null>(null);
  const [history, setHistory] = useState<DispatchRunState[]>([]);
  const lastPromotedRequestIdRef = useRef<string | null>(null);

  useEffect(() => {
    if (!current) return;
    if (!["success", "failed", "denied", "cancelled"].includes(current.status)) return;
    if (lastPromotedRequestIdRef.current === current.requestId) return;

    lastPromotedRequestIdRef.current = current.requestId;
    setHistory((existing) => [
      current,
      ...existing.filter((run) => run.requestId !== current.requestId),
    ]);
  }, [current]);

  useEffect(() => {
    return tetherClient.subscribeHostFrames((rawFrame) => {
      if (!isDispatchFrame(rawFrame)) return;

      switch (rawFrame.kind) {
        case "dispatch_request":
          break;
        case "dispatch_cancel":
          setCurrent((prev) => {
            if (!prev || prev.requestId !== rawFrame.request_id) return prev;
            return { ...prev, status: "cancelling" };
          });
          break;
        case "dispatch_chunk":
          setCurrent((prev) => applyChunk(prev, rawFrame));
          break;
        case "dispatch_complete":
          // R2 codex fix: use a functional updater (same as chunk above)
          // instead of reading currentRef. The useEffect that mirrors
          // current → currentRef runs AFTER React commits, so if a
          // dispatch_chunk and dispatch_complete arrive in the same
          // render batch (or back-to-back before the mirror catches up),
          // currentRef.current would be stale and applyComplete would
          // drop the latest chunk's text. setCurrent(prev => …) reads
          // the latest queued state, no staleness window.
          setCurrent((prev) => applyComplete(prev, rawFrame));
          break;
        default:
          assertNever(rawFrame);
      }
    });
  }, [tetherClient]);

  function send(input: DispatchSendInput): string {
    if ("kind" in input && input.kind === "dispatch_cancel") {
      tetherClient.sendTetherFrame(input);
      setCurrent((prev) => {
        if (!prev || prev.requestId !== input.request_id) return prev;
        return { ...prev, status: "cancelling" };
      });
      return input.request_id;
    }

    const requestId = crypto.randomUUID();
    const frame: DispatchRequestFrame = {
      kind: "dispatch_request",
      request_id: requestId,
      runtime: input.runtime,
      prompt: input.prompt,
      model: input.model ?? null,
      agent_slug: input.agent_slug ?? null,
      war_room_id: input.war_room_id ?? null,
      war_room_round: input.war_room_round ?? null,
    };

    // Only mark a run as in-flight after the transport accepts the frame.
    // If sendTetherFrame throws, leaving local state untouched avoids a phantom
    // "running" dispatch that the host never actually received.
    try {
      tetherClient.sendTetherFrame(frame);
    } catch (error) {
      throw error;
    }

    setCurrent({
      requestId,
      runtime: frame.runtime,
      prompt: frame.prompt,
      chunks: [],
      status: "running",
      model: frame.model ?? null,
    });
    return requestId;
  }

  return { send, current, history };
}
