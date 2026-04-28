import { useState, useEffect, useRef } from "react";
import { Search, Sparkles, Cpu, Server, Zap, X } from "lucide-react";
import { cn } from "@/lib/utils";

interface CommandItem {
  id: string;
  label: string;
  group: "canvas" | "action";
  icon: typeof Sparkles;
  color?: string;
  onSelect: () => void;
}

interface CommandPaletteProps {
  items: CommandItem[];
  onClose: () => void;
}

export default function CommandPalette({ items, onClose }: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [selectedIdx, setSelectedIdx] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => { inputRef.current?.focus(); }, []);

  const filtered = query
    ? items.filter((i) => i.label.toLowerCase().includes(query.toLowerCase()))
    : items;

  const canvasItems = filtered.filter((i) => i.group === "canvas");
  const actionItems = filtered.filter((i) => i.group === "action");
  const allFiltered = [...canvasItems, ...actionItems];

  useEffect(() => { setSelectedIdx(0); }, [query]);

  function handleKey(e: React.KeyboardEvent) {
    if (e.key === "ArrowDown") { e.preventDefault(); setSelectedIdx((i) => Math.min(i + 1, allFiltered.length - 1)); }
    if (e.key === "ArrowUp") { e.preventDefault(); setSelectedIdx((i) => Math.max(i - 1, 0)); }
    if (e.key === "Enter" && allFiltered[selectedIdx]) { allFiltered[selectedIdx].onSelect(); onClose(); }
    if (e.key === "Escape") onClose();
  }

  return (
    <div className="absolute inset-0 z-30 flex items-start justify-center pt-20" onClick={onClose}>
      <div
        className="w-full max-w-lg rounded-xl border border-cs-border bg-cs-card shadow-2xl overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2 px-4 py-3 border-b border-cs-border">
          <Search size={14} className="text-cs-muted shrink-0" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKey}
            placeholder="Search nodes, skills, actions..."
            className="flex-1 bg-transparent text-sm focus:outline-none"
          />
          <kbd className="text-[9px] text-cs-muted bg-cs-border/60 rounded px-1.5 py-0.5">ESC</kbd>
        </div>

        <div className="max-h-72 overflow-y-auto">
          {canvasItems.length > 0 && (
            <>
              <div className="px-4 py-1.5 text-[9px] text-cs-muted uppercase tracking-wide bg-cs-bg/50">On this canvas</div>
              {canvasItems.map((item, idx) => {
                const globalIdx = idx;
                return <PaletteRow key={item.id} item={item} selected={selectedIdx === globalIdx} onSelect={() => { item.onSelect(); onClose(); }} />;
              })}
            </>
          )}
          {actionItems.length > 0 && (
            <>
              <div className="px-4 py-1.5 text-[9px] text-cs-muted uppercase tracking-wide bg-cs-bg/50">Actions</div>
              {actionItems.map((item, idx) => {
                const globalIdx = canvasItems.length + idx;
                return <PaletteRow key={item.id} item={item} selected={selectedIdx === globalIdx} onSelect={() => { item.onSelect(); onClose(); }} />;
              })}
            </>
          )}
          {allFiltered.length === 0 && (
            <div className="px-4 py-6 text-center text-xs text-cs-muted">No results for "{query}"</div>
          )}
        </div>
      </div>
    </div>
  );
}

function PaletteRow({ item, selected, onSelect }: { item: CommandItem; selected: boolean; onSelect: () => void }) {
  const Icon = item.icon;
  return (
    <button
      onClick={onSelect}
      className={cn(
        "flex w-full items-center gap-3 px-4 py-2 text-left text-xs transition-colors",
        selected ? "bg-cs-accent/10 text-cs-accent" : "text-cs-text hover:bg-cs-border/30"
      )}
    >
      <Icon size={13} style={{ color: item.color }} />
      <span className="truncate">{item.label}</span>
    </button>
  );
}
