import { useState, useRef, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Send, Bot, X, Loader2, ChevronUp, ChevronDown, Sparkles, Terminal } from "lucide-react";
import { cn } from "@/lib/utils";

interface Message {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  timestamp: Date;
  tool?: string; // MCP tool used
}

// Simulated MCP tool responses
function simulateMcpResponse(prompt: string): { content: string; tool?: string } {
  const lower = prompt.toLowerCase();

  if (lower.includes("context") || lower.includes("usage")) {
    return {
      tool: "get_context_usage",
      content: "Context Usage: 67,234 / 200,000 tokens (33.6%)\n\nBreakdown:\n- System Prompts: 28,450 tokens\n- Skills (4 active): 12,300 tokens\n- MCP Schemas (3): 8,200 tokens\n- CLAUDE.md: 2,100 tokens\n- Conversation: 14,184 tokens\n- File Reads: 2,000 tokens",
    };
  }

  if (lower.includes("skill") && (lower.includes("list") || lower.includes("show") || lower.includes("status"))) {
    return {
      tool: "list_skills",
      content: "Active Skills (5 enabled):\n\n1. typescript-expert (2,340 tokens) — Personal\n2. code-review (1,890 tokens) — Personal\n3. project-conventions (3,200 tokens) — Project\n4. api-guidelines (1,200 tokens) — Project\n5. security-policy (4,100 tokens) — Enterprise\n\nDisabled: deprecated-skill, docker-helper",
    };
  }

  if (lower.includes("mcp") || lower.includes("server")) {
    return {
      tool: "get_mcp_status",
      content: "MCP Servers:\n\n- filesystem: Running (12 tools)\n- github: Running (8 tools)\n- slack: Error — Connection failed\n- postgres: Running (6 tools)\n\nTotal: 3/4 servers healthy, 26 tools available",
    };
  }

  if (lower.includes("stats") || lower.includes("cost") || lower.includes("token")) {
    return {
      tool: "get_usage_stats",
      content: "Usage Stats:\n\nToday: 45,230 tokens ($0.68)\nThis Week: 312,450 tokens ($4.69)\nThis Month: 891,000 tokens ($18.67)\n\nBurn Rate: 12,340 tokens/hour ($0.19/hr)\nEstimated time to limit: 2.5 hours",
    };
  }

  if (lower.includes("toggle") && lower.includes("skill")) {
    return {
      tool: "toggle_skill",
      content: "Skill toggled successfully. Run `/list_skills` to see current state.",
    };
  }

  return {
    content: "Available MCP commands:\n\n- Ask about **context usage** — get_context_usage\n- Ask about **skills** — list_skills\n- Ask about **MCP servers** — get_mcp_status\n- Ask about **usage/costs** — get_usage_stats\n- **Toggle a skill** — toggle_skill\n\nTry: \"Show me my context usage\" or \"What skills are active?\"",
  };
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

    // Simulate MCP delay
    await new Promise((r) => setTimeout(r, 600 + Math.random() * 800));

    const response = simulateMcpResponse(userMsg.content);

    const assistantMsg: Message = {
      id: String(Date.now() + 1),
      role: "assistant",
      content: response.content,
      timestamp: new Date(),
      tool: response.tool,
    };

    setMessages((prev) => [...prev, assistantMsg]);
    setIsLoading(false);
  }

  function clearHistory() {
    setMessages([]);
    setExpanded(false);
  }

  return (
    <div className="border-t border-cs-border bg-cs-card">
      {/* Chat history (expandable) */}
      {expanded && messages.length > 0 && (
        <div className="max-h-64 overflow-y-auto border-b border-cs-border">
          <div className="p-3 space-y-3">
            {messages.map((msg) => (
              <div
                key={msg.id}
                className={cn(
                  "flex gap-2.5",
                  msg.role === "user" ? "justify-end" : "justify-start"
                )}
              >
                {msg.role === "assistant" && (
                  <div className="w-6 h-6 rounded-md bg-cs-accent/10 border border-cs-accent/20 flex items-center justify-center shrink-0 mt-0.5">
                    <Bot size={12} className="text-cs-accent" />
                  </div>
                )}
                <div
                  className={cn(
                    "rounded-lg px-3 py-2 max-w-[85%]",
                    msg.role === "user"
                      ? "bg-cs-accent/10 border border-cs-accent/20"
                      : "bg-cs-bg border border-cs-border"
                  )}
                >
                  {msg.tool && (
                    <div className="flex items-center gap-1 mb-1">
                      <Terminal size={10} className="text-cs-accent" />
                      <span className="text-[9px] font-mono text-cs-accent">{msg.tool}</span>
                    </div>
                  )}
                  <pre className="text-xs text-cs-text whitespace-pre-wrap font-mono leading-relaxed">
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
            placeholder={t("prompt.placeholder")}
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
