import { useState, useRef, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Send, Bot, X, Loader2, ChevronUp, ChevronDown, Sparkles, Terminal, AlertCircle } from "lucide-react";
import { cn } from "@/lib/utils";

const isTauri = typeof window !== 'undefined' && '__TAURI__' in window;

interface Message {
  id: string;
  role: "user" | "assistant" | "error";
  content: string;
  timestamp: Date;
}

/** Call claude CLI via Tauri backend */
async function promptClaude(prompt: string): Promise<string> {
  if (isTauri) {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke<string>('prompt_claude', { prompt });
  }
  // Browser fallback — simulated
  return simulateMock(prompt);
}

function simulateMock(prompt: string): string {
  const lower = prompt.toLowerCase();
  if (lower.includes("skill")) return "I can help you create a skill! To create one, I'd need:\n\n1. A name (e.g., `my-skill`)\n2. A description (what triggers it)\n3. The skill content (markdown instructions)\n\nTell me what you want the skill to do and I'll generate it for you.\n\n(Note: This is a simulated response. In the desktop app, this connects to your real Claude Code subscription.)";
  if (lower.includes("context") || lower.includes("usage")) return "Context usage info would appear here from your real Claude Code session.\n\n(Simulated — run in the desktop app to connect to Claude Code.)";
  return "I'm Claude, running through Claude Code. Ask me anything — create skills, review code, manage configs.\n\n(Simulated — install the desktop app to use your Claude Code subscription.)";
}

export default function PromptBar() {
  const { t } = useTranslation();
  const [input, setInput] = useState("");
  const [messages, setMessages] = useState<Message[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (expanded && messagesEndRef.current) {
      messagesEndRef.current.scrollIntoView({ behavior: "smooth" });
    }
  }, [messages, expanded]);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!input.trim() || isLoading) return;

    const userMsg: Message = {
      id: String(Date.now()),
      role: "user",
      content: input.trim(),
      timestamp: new Date(),
    };

    setMessages((prev) => [...prev, userMsg]);
    setInput("");
    setIsLoading(true);
    setExpanded(true);

    try {
      const response = await promptClaude(userMsg.content);
      setMessages((prev) => [...prev, {
        id: String(Date.now()),
        role: "assistant",
        content: response,
        timestamp: new Date(),
      }]);
    } catch (err) {
      setMessages((prev) => [...prev, {
        id: String(Date.now()),
        role: "error",
        content: String(err),
        timestamp: new Date(),
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
            {messages.map((msg) => (
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
                      : "bg-cs-accent/10 border-cs-accent/20"
                  )}>
                    {msg.role === "error"
                      ? <AlertCircle size={12} className="text-red-400" />
                      : <Bot size={12} className="text-cs-accent" />
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
                      <Terminal size={10} className="text-cs-accent" />
                      <span className="text-[9px] font-mono text-cs-accent">claude --print</span>
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
            ))}
            {isLoading && (
              <div className="flex gap-2.5">
                <div className="w-6 h-6 rounded-md bg-cs-accent/10 border border-cs-accent/20 flex items-center justify-center shrink-0">
                  <Loader2 size={12} className="text-cs-accent animate-spin" />
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
          className="p-2 rounded-lg bg-cs-accent text-cs-bg hover:bg-cs-accent/90 transition-colors disabled:opacity-30 disabled:cursor-not-allowed shrink-0"
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
