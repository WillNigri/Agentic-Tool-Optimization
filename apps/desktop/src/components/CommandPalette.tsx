import { useEffect, useRef, useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  Home as HomeIcon,
  Bot,
  Sparkles,
  Activity,
  BarChart3,
  Settings,
  Search,
  CornerDownLeft,
  Plug,
  FolderGit2,
  type LucideIcon,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { Section } from "@/components/Sidebar";
import { listAgents } from "@/lib/agents";
import { getSkills, getMcpServers, listProjects } from "@/lib/api";

// T8 — Command palette (⌘K / Ctrl+K).
// v1.4.0 Polish-T3 — Global search: when the user types, we also surface
// matching agents / skills / MCPs / projects. Selecting any result navigates
// to its section. Deep-linking to the row/detail comes when each section
// gains a URL/state-driven selection (tracked separately).

type Command = {
  id: string;
  label: string;
  hint?: string;
  icon: LucideIcon;
  group: string;
  run: () => void;
};

interface Props {
  onNavigate: (section: Section) => void;
}

export default function CommandPalette({ onNavigate }: Props) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  // ⌘K / Ctrl+K listener
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen((v) => !v);
      } else if (e.key === "Escape" && open) {
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  // Reset on open
  useEffect(() => {
    if (open) {
      setQuery("");
      setActiveIndex(0);
      // Focus on next tick once the input has mounted
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  // Only fetch the searchable corpus while the palette is open. 60s stale —
  // the catalog doesn't change fast and ⌘K open/close happens often.
  const { data: agents = [] } = useQuery({
    queryKey: ["cmdk", "agents"],
    queryFn: () => listAgents(),
    enabled: open,
    staleTime: 60_000,
  });
  const { data: skills = [] } = useQuery({
    queryKey: ["cmdk", "skills"],
    queryFn: getSkills,
    enabled: open,
    staleTime: 60_000,
  });
  const { data: mcps = [] } = useQuery({
    queryKey: ["cmdk", "mcps"],
    queryFn: getMcpServers,
    enabled: open,
    staleTime: 60_000,
  });
  const { data: projects = [] } = useQuery({
    queryKey: ["cmdk", "projects"],
    queryFn: listProjects,
    enabled: open,
    staleTime: 60_000,
  });

  const commands: Command[] = useMemo(() => {
    const navGroup = t("cmdk.groupNavigate", "Navigate");
    const items: { id: string; section: Section; label: string; icon: LucideIcon }[] = [
      { id: "go.home",      section: "home",      label: t("nav.home", "Home"),                icon: HomeIcon },
      { id: "go.agents",    section: "agents",    label: t("nav.agents", "Agents"),            icon: Bot },
      { id: "go.skills",    section: "skills",    label: t("nav.skills", "Skills & MCPs"),     icon: Sparkles },
      { id: "go.runs",      section: "runs",      label: t("nav.runs", "Runs"),                icon: Activity },
      { id: "go.insights",  section: "insights",  label: t("nav.insights", "Insights"),        icon: BarChart3 },
      { id: "go.settings",  section: "settings",  label: t("nav.settings", "Settings"),        icon: Settings },
    ];
    const navCommands: Command[] = items.map((it) => ({
      id: it.id,
      label: it.label,
      hint: t("cmdk.goHint", "Go to {{label}}", { label: it.label }),
      icon: it.icon,
      group: navGroup,
      run: () => {
        onNavigate(it.section);
        setOpen(false);
      },
    }));

    const agentCommands: Command[] = agents.map((a) => ({
      id: `agent.${a.id}`,
      label: a.displayName,
      hint: `${a.runtime}${a.model ? ` · ${a.model}` : ""}`,
      icon: Bot,
      group: t("cmdk.groupAgents", "Agents"),
      run: () => {
        onNavigate("agents");
        setOpen(false);
      },
    }));

    const skillCommands: Command[] = skills.map((s) => ({
      id: `skill.${s.id}`,
      label: s.name,
      hint: s.description?.slice(0, 80) || s.runtime,
      icon: Sparkles,
      group: t("cmdk.groupSkills", "Skills"),
      run: () => {
        onNavigate("skills");
        setOpen(false);
      },
    }));

    const mcpCommands: Command[] = mcps.map((m) => ({
      id: `mcp.${m.id}`,
      label: m.name,
      hint: `${m.transport} · ${m.status}`,
      icon: Plug,
      group: t("cmdk.groupMcps", "MCPs"),
      run: () => {
        onNavigate("skills");
        setOpen(false);
      },
    }));

    const projectCommands: Command[] = projects.map((p) => ({
      id: `project.${p.id}`,
      label: p.name,
      hint: p.path,
      icon: FolderGit2,
      group: t("cmdk.groupProjects", "Projects"),
      run: () => {
        onNavigate("settings");
        setOpen(false);
      },
    }));

    return [
      ...navCommands,
      ...agentCommands,
      ...skillCommands,
      ...mcpCommands,
      ...projectCommands,
    ];
  }, [t, onNavigate, agents, skills, mcps, projects]);

  const filtered = useMemo(() => {
    // No query → only navigation commands. Showing every agent/skill on
    // first open would be a wall of text — search reveals them.
    if (!query.trim()) return commands.filter((c) => c.id.startsWith("go."));
    const q = query.toLowerCase();
    return commands.filter(
      (c) =>
        c.label.toLowerCase().includes(q) ||
        c.id.toLowerCase().includes(q) ||
        (c.hint && c.hint.toLowerCase().includes(q))
    );
  }, [query, commands]);

  // Keep activeIndex in range when filtered changes
  useEffect(() => {
    setActiveIndex((i) => Math.min(i, Math.max(0, filtered.length - 1)));
  }, [filtered.length]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIndex((i) => Math.min(i + 1, filtered.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIndex((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const cmd = filtered[activeIndex];
      if (cmd) cmd.run();
    }
  };

  if (!open) return null;

  // Group commands for display
  const groups = filtered.reduce<Record<string, Command[]>>((acc, c) => {
    (acc[c.group] ??= []).push(c);
    return acc;
  }, {});

  let renderedIndex = 0;

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={t("cmdk.label", "Command palette")}
      className="fixed inset-0 z-[60] flex items-start justify-center bg-black/60 backdrop-blur-sm pt-[10vh]"
      onClick={(e) => {
        if (e.target === e.currentTarget) setOpen(false);
      }}
    >
      <div className="w-full max-w-xl rounded-xl border border-cs-border bg-cs-card shadow-2xl overflow-hidden">
        <div className="flex items-center gap-2 px-4 border-b border-cs-border">
          <Search size={16} className="text-cs-muted shrink-0" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={t("cmdk.placeholder", "Type a command or search…")}
            className="flex-1 bg-transparent py-3.5 text-sm text-cs-text placeholder:text-cs-muted focus:outline-none"
          />
          <kbd className="hidden sm:inline-flex text-[10px] text-cs-muted border border-cs-border rounded px-1.5 py-0.5">
            ESC
          </kbd>
        </div>

        <div className="max-h-[50vh] overflow-y-auto py-2">
          {filtered.length === 0 ? (
            <p className="px-4 py-8 text-center text-sm text-cs-muted">
              {t("cmdk.noResults", "No commands match.")}
            </p>
          ) : (
            Object.entries(groups).map(([group, cmds]) => (
              <div key={group} className="mb-2">
                <div className="px-4 pb-1 text-[10px] uppercase tracking-wide text-cs-muted">
                  {group}
                </div>
                {cmds.map((cmd) => {
                  const Icon = cmd.icon;
                  const isActive = renderedIndex === activeIndex;
                  const myIndex = renderedIndex;
                  renderedIndex += 1;
                  return (
                    <button
                      key={cmd.id}
                      type="button"
                      onMouseEnter={() => setActiveIndex(myIndex)}
                      onClick={() => cmd.run()}
                      className={cn(
                        "w-full flex items-center gap-3 px-4 py-2 text-left text-sm transition-colors",
                        isActive
                          ? "bg-cs-accent/10 text-cs-accent"
                          : "text-cs-text hover:bg-cs-border/40"
                      )}
                    >
                      <Icon size={14} className="shrink-0" />
                      <span className="flex-1">{cmd.label}</span>
                      {isActive && (
                        <CornerDownLeft size={12} className="text-cs-muted" />
                      )}
                    </button>
                  );
                })}
              </div>
            ))
          )}
        </div>

        <div className="px-4 py-2 border-t border-cs-border flex items-center justify-between text-[10px] text-cs-muted">
          <span>
            <kbd className="border border-cs-border rounded px-1 py-0.5 mr-1">↑↓</kbd>
            {t("cmdk.hintNav", "navigate")}
            <kbd className="border border-cs-border rounded px-1 py-0.5 mx-1">↵</kbd>
            {t("cmdk.hintRun", "run")}
          </span>
          <span>
            <kbd className="border border-cs-border rounded px-1 py-0.5">⌘K</kbd>
            {" "}
            {t("cmdk.hintToggle", "toggle")}
          </span>
        </div>
      </div>
    </div>
  );
}
