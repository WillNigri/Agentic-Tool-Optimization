// PromptBar/ChatRow.tsx — single chat-row renderer for the bottom-pane
// Chat tab. Extracted from PromptBar.tsx (2026-05-19) so the orchestrator
// file doesn't carry ~150 lines of JSX for a single row. ChatRow has no
// state of its own — it renders one ChatMessage with runtime-themed
// styling, optional approval dialog for skill-creation responses, and
// the metadata badge row (routing, stage, tools-used).

import {
  Sparkles,
  AlertCircle,
  Bot,
  Paperclip,
  Network,
  ArrowRight,
} from "lucide-react";

import { cn } from "@/lib/utils";
import type { ChatMessage } from "@/lib/chatThreads";
import type { AgentRuntime } from "@/components/cron/types";
import ApprovalDialog, { extractSkillFromResponse } from "../ApprovalDialog";
import MarkdownContent from "../MarkdownContent";

import { RUNTIME_OPTIONS } from "./_helpers";

export function ChatRow({ msg }: { msg: ChatMessage }) {
  const runtime = msg.runtime
    ? RUNTIME_OPTIONS.find((r) => r.id === msg.runtime) ?? null
    : null;
  const Icon = runtime?.icon ?? Sparkles;
  const color = runtime?.color ?? "#888";
  const justifyEnd = msg.role === "user";

  if (msg.role === "attachment") {
    return (
      <div className="flex items-start gap-2 rounded-lg border border-cs-border bg-cs-bg-raised/60 px-3 py-2 text-xs">
        <Paperclip size={12} className="text-cs-accent shrink-0 mt-0.5" />
        <pre className="text-[11px] text-cs-text font-mono whitespace-pre-wrap line-clamp-3 flex-1">
          {msg.content}
        </pre>
      </div>
    );
  }

  return (
    <div
      data-message-id={msg.id}
      className={cn("flex gap-2.5", justifyEnd ? "justify-end" : "justify-start")}
    >
      {msg.role !== "user" && (
        <div
          className={cn(
            "w-6 h-6 rounded-md border flex items-center justify-center shrink-0 mt-0.5",
            msg.role === "error" ? "bg-red-500/10 border-red-500/20" : "",
          )}
          style={
            msg.role !== "error"
              ? { background: `${color}15`, borderColor: `${color}30` }
              : undefined
          }
        >
          {msg.role === "error" ? (
            <AlertCircle size={12} className="text-red-400" />
          ) : (
            <Icon size={12} style={{ color }} />
          )}
        </div>
      )}
      <div
        className={cn(
          "rounded-lg px-3 py-2 max-w-[85%]",
          msg.role === "user"
            ? "bg-cs-accent/10 border border-cs-accent/20"
            : msg.role === "error"
            ? "bg-red-500/5 border border-red-500/20"
            : "bg-cs-bg border border-cs-border",
        )}
      >
        {msg.role === "assistant" && runtime && (() => {
          // Parse metadata once per render — small JSON, cheap.
          let meta: {
            routedTo?: string;
            routingReason?: string;
            viaGroup?: string;
            toolsUsed?: string[];
            stageOf?: number;
            stageIndex?: number;
            stagedFrom?: string;
          } = {};
          if (msg.metadata) {
            try {
              meta = JSON.parse(msg.metadata);
            } catch {
              // ignore
            }
          }
          return (
            <div className="flex flex-wrap items-center gap-x-1 gap-y-1 mb-1.5">
              <span
                className="inline-flex items-center gap-1 text-[9px] font-mono"
                style={{ color }}
              >
                <Icon size={10} />
                {runtime.label}
              </span>
              {msg.agentSlug && (
                <span className="inline-flex items-center gap-1 text-[9px] font-mono text-cs-muted">
                  <span>·</span>
                  <Bot size={9} />
                  @{msg.agentSlug}
                </span>
              )}
              {meta.viaGroup && (
                <span className="inline-flex items-center gap-1 text-[9px] font-mono text-cs-accent">
                  <ArrowRight size={9} />
                  via <Network size={9} /> {meta.viaGroup}
                </span>
              )}
              {meta.stageOf && meta.stageOf > 1 && (
                <span
                  className="inline-flex items-center gap-1 text-[9px] font-mono font-semibold px-1.5 py-0.5 rounded bg-cs-accent/15 text-cs-accent"
                  title="One step in a sequential pipeline"
                >
                  stage {(meta.stageIndex ?? 0) + 1} / {meta.stageOf}
                </span>
              )}
              {meta.routingReason && (
                <span
                  className="text-[9px] text-cs-muted italic truncate max-w-[180px]"
                  title={meta.routingReason}
                >
                  {meta.routingReason}
                </span>
              )}
              {meta.toolsUsed && meta.toolsUsed.length > 0 && (
                <span className="inline-flex items-center gap-1 text-[9px] font-mono text-cs-muted">
                  <span>·</span>
                  tools:{" "}
                  {meta.toolsUsed
                    .slice(0, 3)
                    .map((t) => t.replace(/^mcp__/, ""))
                    .join(", ")}
                  {meta.toolsUsed.length > 3 && ` +${meta.toolsUsed.length - 3}`}
                </span>
              )}
            </div>
          );
        })()}
        {msg.role === "assistant" ? (
          <MarkdownContent content={msg.content} />
        ) : (
          <pre
            className={cn(
              "text-xs whitespace-pre-wrap font-mono leading-relaxed",
              msg.role === "error" ? "text-red-400" : "text-cs-text",
            )}
          >
            {msg.content}
          </pre>
        )}
        <p className="text-[9px] text-cs-muted mt-1">
          {new Date(msg.createdAt).toLocaleTimeString([], {
            hour: "2-digit",
            minute: "2-digit",
          })}
        </p>
        {/* Approval dialog for skill creation in assistant responses */}
        {msg.role === "assistant" && (() => {
          const skill = extractSkillFromResponse(msg.content);
          if (!skill) return null;
          return (
            <ApprovalDialog
              content={skill.content}
              filePath={skill.path}
              skillName={skill.name}
              runtime={(msg.runtime as AgentRuntime) ?? "claude"}
              onApprove={() => {
                /* approval is one-shot; no state to clear since we no
                 * longer mutate the messages array — re-render will
                 * skip when the file is written. */
              }}
              onDeny={() => {}}
            />
          );
        })()}
      </div>
    </div>
  );
}
