// PromptBar/_helpers.ts — pure helpers + runtime-picker constants
// extracted from PromptBar/index.tsx (2026-05-19 frontend elegance push).
//
// Lives next to index.tsx so the orchestrator file shrinks from ~1700
// lines to a manageable size while keeping the mental boundary tight
// (everything here is either constant data or a pure function). No JSX,
// no React state — anything stateful stays in the main component.

import { Terminal } from "lucide-react";

import type { AgentRuntime } from "@/components/cron/types";
import { RUNTIME_REGISTRY, type RuntimeId } from "@/lib/runtimes";
import type { AgentMessage } from "@/lib/agentVariables";
import type { ChatMessage } from "@/lib/chatThreads";

// v2.3.23 Phase 6.x-B — picker is data-driven from
// `list_available_runtimes` (CLI runtimes + API providers with active
// keys). The rendering metadata used to live here as a 10-entry local
// map; 2026-05-18 elegance push sources it from the canonical runtime
// registry (lib/runtimes.ts) so adding a new LLM doesn't require
// touching this file. RUNTIME_META is now a thin shape adapter that
// projects the registry into the {label, icon, color} tuple this
// component already consumed — keeps every existing call site stable.
export const RUNTIME_META: Record<
  string,
  { label: string; icon: typeof Terminal; color: string }
> = Object.fromEntries(
  (Object.keys(RUNTIME_REGISTRY) as RuntimeId[]).map((id) => {
    const m = RUNTIME_REGISTRY[id];
    return [id, { label: m.label, icon: m.icon, color: m.hex }];
  }),
);

// RUNTIME_OPTIONS is the dropdown source when the live availability
// query (`list_available_runtimes`) hasn't returned yet — historically
// this was a hand-maintained 4-entry list of CLI runtimes only, which
// meant the picker silently lost gemini / minimax / grok / etc. Now
// derived from the registry: every runtime is offered at startup, and
// the live query disables the ones that aren't ready.
export const RUNTIME_OPTIONS: {
  id: AgentRuntime;
  label: string;
  icon: typeof Terminal;
  color: string;
}[] = (Object.keys(RUNTIME_REGISTRY) as RuntimeId[]).map((id) => ({
  id,
  label: RUNTIME_REGISTRY[id].label,
  icon: RUNTIME_REGISTRY[id].icon,
  color: RUNTIME_REGISTRY[id].hex,
}));

export interface AvailableRuntimeRow {
  slug: string;
  label: string;
  kind: "cli" | "api";
  available: boolean;
  reason: string;
}

export const MAX_ATTACHMENT_BYTES = 32 * 1024;

export function simulateMock(prompt: string): string {
  const lower = prompt.toLowerCase();
  if (lower.includes("skill"))
    return "I can help you create a skill! Tell me what you want it to do.\n\n(Simulated — install the desktop app to connect to your agents.)";
  if (lower.includes("context") || lower.includes("usage"))
    return "Context usage info would appear here from your real session.\n\n(Simulated — run in the desktop app to connect.)";
  return "Ask me anything — create skills, review code, manage configs.\n\n(Simulated — install the desktop app to use your agent subscriptions.)";
}

export function isProbablyBinary(text: string): boolean {
  // Cheap heuristic: look for NUL bytes in the first 4KB.
  const chunk = text.slice(0, 4096);
  return chunk.includes("\0");
}

/** Map persisted ChatMessage history into the AgentMessage shape the
 *  dispatchers want. Attachments become "system" messages so the
 *  summarizer/judge see them. Errors are dropped. */
export function messagesToAgentHistory(messages: ChatMessage[]): AgentMessage[] {
  return messages
    .filter((m) => m.role !== "error")
    .map((m) => ({
      role:
        m.role === "user"
          ? "user"
          : m.role === "assistant"
          ? "assistant"
          : "system",
      content: m.content,
    }));
}

/** Stitch a thread's prior history into a single prompt the runtime will
 *  treat as one big request. Used for the no-agent path so cross-runtime
 *  swaps mid-thread still carry context. The framing instruction is short
 *  on purpose — telling the model "this is an ongoing conversation,
 *  respond to the last message" is enough; it'll figure out the rest. */
export function stitchThreadIntoPrompt(
  history: AgentMessage[],
  newPrompt: string,
): string {
  if (history.length === 0) return newPrompt;
  let out =
    "You are continuing an ongoing conversation. The previous turns are below; respond to the user's most recent message at the end.\n\n";
  for (const m of history) {
    out += `[${m.role}]: ${m.content}\n\n`;
  }
  out += `[user]: ${newPrompt}\n`;
  return out;
}
