import { cn } from "@/lib/utils";

const TOOL_ICONS: Record<string, string> = {
  Read: "\u{1F4D6}",
  Write: "\u270F\uFE0F",
  Bash: "\u26A1",
  Grep: "\u{1F50D}",
  Glob: "\u{1F4C2}",
  Edit: "\u270F\uFE0F",
  Agent: "\u{1F916}",
};

export function ToolTag({ tool }: { tool: string }) {
  return (
    <span className="inline-flex items-center gap-1 px-2 py-0.5 text-xs font-mono rounded-full border border-cs-accent/40 bg-cs-accent/10 text-cs-accent">
      {TOOL_ICONS[tool] && <span>{TOOL_ICONS[tool]}</span>}
      {tool}
    </span>
  );
}

export function ModelTag({ model }: { model: string }) {
  return (
    <span className="inline-flex items-center gap-1 px-2 py-0.5 text-xs font-mono rounded-full border border-purple-500/40 bg-purple-500/10 text-purple-400">
      <span>{"\u25C6"}</span>
      {model}
    </span>
  );
}

export function StatusTag({ enabled }: { enabled: boolean }) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 px-2 py-0.5 text-xs rounded-full border",
        enabled
          ? "border-green-500/40 bg-green-500/10 text-green-400"
          : "border-cs-border bg-cs-card text-cs-muted"
      )}
    >
      <span>{enabled ? "\u25CF" : "\u25CB"}</span>
      {enabled ? "Enabled" : "Disabled"}
    </span>
  );
}

export function TokenTag({ count }: { count: number }) {
  return (
    <span className="inline-flex items-center px-2 py-0.5 text-xs font-mono rounded-full border border-cs-border bg-cs-card text-cs-muted">
      {count.toLocaleString()} tokens
    </span>
  );
}
