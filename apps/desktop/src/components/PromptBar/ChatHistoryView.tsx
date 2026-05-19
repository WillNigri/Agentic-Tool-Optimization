// PromptBar/ChatHistoryView.tsx — scrolling message list + thinking
// indicator + multi-turn summary banner.
//
// Extracted from PromptBar/index.tsx 2026-05-19 (v2.7.7 frontend
// elegance push). The orchestrator passes the messages array + the
// streaming state + the runtime metadata; this component renders.
//
// Three coordinating pieces:
//   1. message list (one ChatRow per turn)
//   2. "Thinking…" pill while a dispatch is in flight, with the
//      streaming buffer when streaming is on
//   3. summary banner that warns when the next turn will trigger
//      the agent's memory-policy summarization

import { useEffect, useRef, type ComponentType, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, type LucideProps } from "lucide-react";

import { cn } from "@/lib/utils";
import { parseMemoryPolicy, type Agent } from "@/lib/agents";
import type { AgentGroup } from "@/lib/agentGroups";
import type { ChatMessage } from "@/lib/chatThreads";

import MarkdownContent from "../MarkdownContent";
import { ChatRow } from "./ChatRow";

interface RuntimeMeta {
  color: string;
}

interface Props {
  messages: ChatMessage[];
  isLoading: boolean;
  streamingText: string;
  selectedAgent: Agent | null;
  selectedGroup: AgentGroup | null;
  /** Used for the in-flight pill (color + icon). Pulled from the
   *  runtime registry on the orchestrator side. */
  currentRuntime: RuntimeMeta;
  RuntimeIcon: ComponentType<LucideProps>;
  /** Scroll target so new turns auto-scroll into view. Owned by
   *  the orchestrator (it's also the auto-scroll effect's anchor). */
  messagesEndRef: RefObject<HTMLDivElement>;
  /** When false, the history is collapsed and this component renders
   *  nothing for the list. The summary banner is independent — it
   *  renders whenever there's a selected agent + non-empty messages,
   *  regardless of expand state. */
  expanded: boolean;
}

export function ChatHistoryView({
  messages,
  isLoading,
  streamingText,
  selectedAgent,
  selectedGroup,
  currentRuntime,
  RuntimeIcon,
  messagesEndRef,
  expanded,
}: Props) {
  const { t } = useTranslation();

  // Auto-scroll the message list when new turns land or streaming
  // text grows. The ref is the anchor the orchestrator also passes
  // (single source of truth for the scroll target).
  const autoScrollRef = useRef(messagesEndRef);
  useEffect(() => {
    autoScrollRef.current = messagesEndRef;
  }, [messagesEndRef]);
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages.length, streamingText, messagesEndRef]);

  return (
    <>
      {/* Chat history — flex-1 with min-h-0 so it shares the parent's
          height with header + form instead of overflowing them. */}
      {expanded && messages.length > 0 && (
        <div className="flex-1 min-h-0 overflow-y-auto border-b border-cs-border">
          <div className="p-3 space-y-3">
            {messages.map((msg) => (
              <ChatRow key={msg.id} msg={msg} />
            ))}
            {isLoading && (
              <div className="flex gap-2.5">
                <div
                  className="w-6 h-6 rounded-md flex items-center justify-center shrink-0"
                  style={{
                    background: `${currentRuntime.color}15`,
                    border: `1px solid ${currentRuntime.color}30`,
                  }}
                >
                  {streamingText ? (
                    <RuntimeIcon
                      size={12}
                      style={{ color: currentRuntime.color }}
                    />
                  ) : (
                    <Loader2
                      size={12}
                      style={{ color: currentRuntime.color }}
                      className="animate-spin"
                    />
                  )}
                </div>
                <div className="rounded-lg px-3 py-2 bg-cs-bg border border-cs-border max-w-[85%]">
                  {streamingText ? (
                    <div className="relative">
                      <MarkdownContent content={streamingText} />
                      <span className="inline-block w-1.5 h-3 bg-cs-accent ml-0.5 animate-pulse align-middle" />
                    </div>
                  ) : (
                    <span className="text-xs text-cs-muted">
                      {selectedGroup
                        ? t(
                            "prompt.routingThroughGroup",
                            "Routing through {{group}}…",
                            { group: selectedGroup.slug },
                          )
                        : selectedAgent
                          ? t(
                              "prompt.thinkingWithAgent",
                              "Thinking — @{{agent}}…",
                              { agent: selectedAgent.slug },
                            )
                          : t("prompt.thinkingPlain", "Thinking…")}
                    </span>
                  )}
                </div>
              </div>
            )}
            <div ref={messagesEndRef} />
          </div>
        </div>
      )}

      {/* Multi-turn status banner — warns the user that the next
          dispatch will trigger their selected agent's memory-policy
          summarization (older turns collapsed into a summary; the
          last N kept verbatim). Only renders within 5 turns of the
          threshold OR already past it. */}
      {selectedAgent && messages.length > 0 && (() => {
        const policy = parseMemoryPolicy(selectedAgent);
        const willSummarize = messages.length > policy.summarizeAfter;
        const within = policy.summarizeAfter - messages.length;
        if (!willSummarize && within > 5) return null;
        return (
          <div
            className={cn(
              "px-3 py-1 text-[10px] border-t",
              willSummarize
                ? "border-cs-accent/30 bg-cs-accent/5 text-cs-accent"
                : "border-cs-border bg-cs-bg-raised text-cs-muted",
            )}
          >
            {willSummarize
              ? t(
                  "prompt.willSummarize",
                  "Next message: {{n}} prior turns will be summarized; last {{k}} kept verbatim.",
                  {
                    n: messages.length - policy.keepLastK,
                    k: policy.keepLastK,
                  },
                )
              : t(
                  "prompt.nearSummarize",
                  "{{n}} more turns until summarization fires.",
                  { n: within },
                )}
          </div>
        );
      })()}
    </>
  );
}
