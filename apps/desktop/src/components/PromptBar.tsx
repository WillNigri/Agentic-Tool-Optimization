import { useState, useRef, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Send, Bot, X, Loader2, ChevronUp, ChevronDown, Sparkles, Terminal, AlertCircle, Cpu, Server, Globe } from "lucide-react";
import { cn } from "@/lib/utils";
import { promptAgent } from "@/lib/tauri-api";
import type { AgentRuntime } from "@/components/cron/types";

const isTauri = typeof window !== 'undefined' && ('__TAURI__' in window || '__TAURI_INTERNALS__' in window);

interface Message {
  id: string;
  role: "user" | "assistant" | "error";
  content: string;
  timestamp: Date;
  runtime?: AgentRuntime;
}

const RUNTIME_OPTIONS: { id: AgentRuntime; label: string; icon: typeof Terminal; color: string }[] = [
  { id: "claude", label: "Claude", icon: Terminal, color: "#f97316" },
  { id: "codex", label: "Codex", icon: Cpu, color: "#22c55e" },
  { id: "openclaw", label: "OpenClaw", icon: Server, color: "#06b6d4" },
  { id: "hermes", label: "Hermes", icon: Globe, color: "#a855f7" },
];

function simulateMock(prompt: string): string {
  const lower = prompt.toLowerCase();
  if (lower.includes("skill")) return "I can help you create a skill! Tell me what you want it to do.\n\n(Simulated — install the desktop app to connect to your agents.)";
  if (lower.includes("context") || lower.includes("usage")) return "Context usage info would appear here from your real session.\n\n(Simulated — run in the desktop app to connect.)";
  return "Ask me anything — create skills, review code, manage configs.\n\n(Simulated — install the desktop app to use your agent subscriptions.)";
}

export default function PromptBar() {
  const { t } = useTranslation();
  const [input, setInput] = useState("");
  const [messages, setMessages] = useState<Message[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [runtime, setRuntime] = useState<AgentRuntime>("claude");
  const [showRuntimePicker, setShowRuntimePicker] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (expanded && messagesEndRef.current) {
      messagesEndRef.current.scrollIntoView({ behavior: "smooth" });
    }
  }, [messages, expanded]);

  const currentRuntime = RUNTIME_OPTIONS.find((r) => r.id === runtime)!;
  const RuntimeIcon = currentRuntime.icon;

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!input.trim() || isLoading) return;

    const userMsg: Message = {
      id: String(Date.now()),
      role: "user",
      content: input.trim(),
      timestamp: new Date(),
      runtime,
    };

    setMessages((prev) => [...prev, userMsg]);
    setInput("");
    setIsLoading(true);
    setExpanded(true);

    try {
      let response: string;
      if (isTauri) {
        response = await promptAgent(runtime, userMsg.content);
      } else {
        response = simulateMock(userMsg.content);
      }
      setMessages((prev) => [...prev, {
        id: String(Date.now()),
        role: "assistant",
        content: response,
        timestamp: new Date(),
        runtime,
      }]);
    } catch (err) {
      setMessages((prev) => [...prev, {
        id: String(Date.now()),
        role: "error",
        content: String(err),
        timestamp: new Date(),
        runtime,
      }]);
    } finally {
      setIsLoading(false);
    }
  }

  function clearHistory() {
    setMessages([]);
    setExpanded(false);
  }

  return (
    <div className="border-t border-cs-border bg-cs-card">
      {/* Chat history */}
      {expanded && messages.length > 0 && (
        <div className="max-h-80 overflow-y-auto border-b border-cs-border">
          <div className="p-3 space-y-3">
            {messages.map((msg) => {
              const msgRuntime = RUNTIME_OPTIONS.find((r) => r.id === msg.runtime) || currentRuntime;
              const MsgIcon = msgRuntime.icon;
              return (
                <div
                  key={msg.id}
                  className={cn(
                    "flex gap-2.5",
                    msg.role === "user" ? "justify-end" : "justify-start"
                  )}
                >
                  {msg.role !== "user" && (
                    <div className={cn(
                      "w-6 h-6 rounded-md border flex items-center justify-center shrink-0 mt-0.5",
                      msg.role === "error"
                        ? "bg-red-500/10 border-red-500/20"
                        : "border-opacity-20"
                    )}
                    style={msg.role !== "error" ? {
                      background: `${msgRuntime.color}15`,
                      borderColor: `${msgRuntime.color}30`,
                    } : undefined}
                    >
                      {msg.role === "error"
                        ? <AlertCircle size={12} className="text-red-400" />
                        : <MsgIcon size={12} style={{ color: msgRuntime.color }} />
                      }
                    </div>
                  )}
                  <div
                    className={cn(
                      "rounded-lg px-3 py-2 max-w-[85%]",
                      msg.role === "user"
                        ? "bg-cs-accent/10 border border-cs-accent/20"
                        : msg.role === "error"
                          ? "bg-red-500/5 border border-red-500/20"
                          : "bg-cs-bg border border-cs-border"
                    )}
                  >
                    {msg.role === "assistant" && isTauri && (
                      <div className="flex items-center gap-1 mb-1">
                        <MsgIcon size={10} style={{ color: msgRuntime.color }} />
                        <span className="text-[9px] font-mono" style={{ color: msgRuntime.color }}>
                          {msgRuntime.label}
                        </span>
                      </div>
                    )}
                    <pre className={cn(
                      "text-xs whitespace-pre-wrap font-mono leading-relaxed",
                      msg.role === "error" ? "text-red-400" : "text-cs-text"
                    )}>
                      {msg.content}
                    </pre>
                    <p className="text-[9px] text-cs-muted mt-1">
                      {msg.timestamp.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
                    </p>
                  </div>
                </div>
              );
            })}
            {isLoading && (
              <div className="flex gap-2.5">
                <div className="w-6 h-6 rounded-md flex items-center justify-center shrink-0"
                  style={{ background: `${currentRuntime.color}15`, border: `1px solid ${currentRuntime.color}30` }}
                >
                  <Loader2 size={12} style={{ color: currentRuntime.color }} className="animate-spin" />
                </div>
                <div className="rounded-lg px-3 py-2 bg-cs-bg border border-cs-border">
                  <span className="text-xs text-cs-muted">{t("prompt.thinking")}</span>
                </div>
              </div>
            )}
            <div ref={messagesEndRef} />
          </div>
        </div>
      )}

      {/* Input bar */}
      <form onSubmit={handleSubmit} className="flex items-center gap-2 px-3 py-2.5">
        <button
          type="button"
          onClick={() => messages.length > 0 && setExpanded(!expanded)}
          className={cn(
            "p-1.5 rounded transition-colors shrink-0",
            messages.length > 0
              ? "text-cs-accent hover:bg-cs-accent/10"
              : "text-cs-muted/30 cursor-default"
          )}
        >
          {expanded ? <ChevronDown size={14} /> : <ChevronUp size={14} />}
        </button>

        {/* Runtime selector */}
        <div className="relative shrink-0">
          <button
            type="button"
            onClick={() => setShowRuntimePicker(!showRuntimePicker)}
            className="flex items-center gap-1 px-2 py-1.5 rounded-lg border border-cs-border hover:border-opacity-60 transition-colors"
            style={{ borderColor: `${currentRuntime.color}40` }}
          >
            <RuntimeIcon size={12} style={{ color: currentRuntime.color }} />
            <span className="text-[10px] font-medium" style={{ color: currentRuntime.color }}>
              {currentRuntime.label}
            </span>
          </button>

          {showRuntimePicker && (
            <>
              <div className="fixed inset-0 z-30" onClick={() => setShowRuntimePicker(false)} />
              <div className="absolute bottom-full left-0 mb-1 w-36 rounded-lg border border-cs-border bg-cs-card shadow-xl z-40 overflow-hidden">
                {RUNTIME_OPTIONS.map((rt) => {
                  const Icon = rt.icon;
                  return (
                    <button
                      key={rt.id}
                      type="button"
                      onClick={() => { setRuntime(rt.id); setShowRuntimePicker(false); }}
                      className={cn(
                        "w-full flex items-center gap-2 px-3 py-2 text-xs transition-colors",
                        runtime === rt.id ? "bg-cs-accent/5" : "hover:bg-cs-bg"
                      )}
                    >
                      <Icon size={12} style={{ color: rt.color }} />
                      <span style={{ color: runtime === rt.id ? rt.color : undefined }}>
                        {rt.label}
                      </span>
                    </button>
                  );
                })}
              </div>
            </>
          )}
        </div>

        <div className="flex-1 relative">
          <Sparkles size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted" />
          <input
            ref={inputRef}
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder={isTauri ? t("prompt.placeholderReal") : t("prompt.placeholder")}
            className="w-full bg-cs-bg border border-cs-border rounded-lg pl-9 pr-3 py-2 text-sm text-cs-text placeholder:text-cs-muted/60 focus:outline-none focus:border-cs-accent/50 font-mono"
            disabled={isLoading}
          />
        </div>

        <button
          type="submit"
          disabled={!input.trim() || isLoading}
          className="p-2 rounded-lg text-cs-bg hover:opacity-90 transition-colors disabled:opacity-30 disabled:cursor-not-allowed shrink-0"
          style={{ background: currentRuntime.color }}
        >
          <Send size={14} />
        </button>

        {messages.length > 0 && (
          <button
            type="button"
            onClick={clearHistory}
            className="p-1.5 rounded text-cs-muted hover:text-cs-text hover:bg-cs-border transition-colors shrink-0"
            title={t("prompt.clear")}
          >
            <X size={14} />
          </button>
        )}
      </form>
    </div>
  );
}
