import { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Loader2,
  Save,
  AlertCircle,
  CheckCircle2,
  FileCode,
  Database,
  Copy,
  Check,
} from "lucide-react";
import type { Agent } from "@/lib/agents";
import { readAgentConfigFile, writeAgentConfigFile } from "@/lib/tauri-api";
import { listAgentVariables } from "@/lib/agentVariables";
import { listAgentHooks } from "@/lib/agentHooks";
import { listAgentKnowledge } from "@/lib/agentKnowledge";
import { cn } from "@/lib/utils";

// v2.0.0 — Advanced "Raw" tab for the agent detail view.
//
// The other tabs (Variables / Context / Memory / Models / Knowledge /
// Deploy) are structured editors — they make the right decisions easy.
// But power users sometimes want to see the WHOLE thing in one place
// and just edit the source. That's what this tab is for.
//
// Two views:
//   - File: the on-disk agent file (e.g., ~/.claude/agents/<slug>.md).
//     Internal agents only — external agents don't have an on-disk
//     representation. Editable, save writes back via the same
//     hash-checked / auto-backup path the structured editors use.
//   - Database: the agent's full SQLite state aggregated into one JSON
//     blob — record + variables + hooks + memory policy + role models +
//     knowledge sources. Read-only for v2.0 alpha; editing this from
//     here would require a careful "atomic update" command in Rust
//     (separate concerns to do this right). v2.0.x will make it
//     editable per-section.

interface Props {
  agent: Agent;
}

type View = "file" | "db";

export default function RawTab({ agent }: Props) {
  const { t } = useTranslation();
  const isExternal = agent.kind === "external";
  // Default view: external agents have no file → land on DB; internal
  // agents land on File since that's the more familiar "raw" surface.
  const [view, setView] = useState<View>(isExternal ? "db" : "file");

  return (
    <div className="space-y-3">
      <div className="inline-flex rounded-md border border-cs-border bg-cs-bg-raised p-0.5">
        {!isExternal && (
          <ViewToggle
            active={view === "file"}
            onClick={() => setView("file")}
            icon={<FileCode size={11} />}
            label={t("agentDetail.raw.file", "File on disk")}
          />
        )}
        <ViewToggle
          active={view === "db"}
          onClick={() => setView("db")}
          icon={<Database size={11} />}
          label={t("agentDetail.raw.db", "Full state (JSON)")}
        />
      </div>

      {view === "file" && !isExternal && <FileEditor agent={agent} />}
      {view === "db" && <DbStateView agent={agent} />}
    </div>
  );
}

function ViewToggle({
  active,
  onClick,
  icon,
  label,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  label: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "inline-flex items-center gap-1.5 rounded px-3 py-1.5 text-[11px] font-medium transition",
        active
          ? "bg-cs-accent/15 text-cs-accent"
          : "text-cs-muted hover:text-cs-text",
      )}
    >
      {icon}
      {label}
    </button>
  );
}

function FileEditor({ agent }: { agent: Agent }) {
  const { t } = useTranslation();
  const filePath = agent.filePath;

  if (!filePath) {
    return (
      <div className="rounded-lg border border-cs-border bg-cs-bg/40 p-4 text-xs text-cs-muted">
        {t(
          "agentDetail.raw.noFilePath",
          "This agent has no on-disk file (was created without a write-to-disk step). Switch to the DB view to inspect its full state.",
        )}
      </div>
    );
  }

  const { data, isLoading, error, refetch } = useQuery({
    queryKey: ["agent-config-file", filePath],
    queryFn: () => readAgentConfigFile(filePath),
    staleTime: 0,
  });

  const [edited, setEdited] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveErr, setSaveErr] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);

  const original = data?.raw ?? "";
  const current = edited ?? original;
  const dirty = edited !== null && edited !== original;

  const onSave = async () => {
    if (!dirty || !filePath) return;
    setSaving(true);
    setSaveErr(null);
    try {
      await writeAgentConfigFile(filePath, current, { expectedHash: data?.contentHash });
      setEdited(null);
      setSavedAt(Date.now());
      await refetch();
    } catch (err) {
      setSaveErr(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  if (isLoading) {
    return (
      <div className="text-xs text-cs-muted">
        <Loader2 size={11} className="inline animate-spin mr-1" />
        {t("agentDetail.raw.reading", "Reading file…")}
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex items-start gap-2 rounded-md border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
        <AlertCircle size={12} className="text-cs-danger shrink-0 mt-0.5" />
        <span>
          {t("agentDetail.raw.readErr", "Couldn't read {{path}}", { path: filePath })}
          {": "}
          {error instanceof Error ? error.message : String(error)}
        </span>
      </div>
    );
  }

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-2 text-[11px] text-cs-muted">
        <code className="font-mono truncate">{filePath}</code>
        <span>
          {data?.format} · {data?.sizeBytes ?? 0} bytes
        </span>
      </div>
      <textarea
        value={current}
        onChange={(e) => setEdited(e.target.value)}
        spellCheck={false}
        rows={20}
        className="w-full rounded-md border border-cs-border bg-cs-bg p-3 font-mono text-[11px] text-cs-text leading-relaxed focus:border-cs-accent focus:outline-none"
      />
      {saveErr && (
        <div className="flex items-start gap-2 rounded-md border border-cs-danger/40 bg-cs-danger/10 p-2 text-xs text-cs-text">
          <AlertCircle size={12} className="text-cs-danger shrink-0 mt-0.5" />
          <span>{saveErr}</span>
        </div>
      )}
      <div className="flex items-center justify-between gap-2">
        <span className="text-[11px] text-cs-muted">
          {dirty
            ? t("agentDetail.raw.unsaved", "Unsaved changes")
            : savedAt
            ? t("agentDetail.raw.saved", "Saved")
            : t(
                "agentDetail.raw.editHint",
                "Edits write back to disk via the same hash-checked path the structured editors use. A backup is created automatically.",
              )}
        </span>
        <div className="flex items-center gap-2">
          {dirty && (
            <button
              type="button"
              onClick={() => setEdited(null)}
              className="inline-flex items-center gap-1 rounded-md border border-cs-border bg-cs-bg-raised px-2.5 py-1 text-[11px] text-cs-muted hover:text-cs-text"
            >
              {t("common.discard", "Discard")}
            </button>
          )}
          <button
            type="button"
            onClick={onSave}
            disabled={!dirty || saving}
            className="inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover disabled:opacity-50"
          >
            {saving ? <Loader2 size={11} className="animate-spin" /> : savedAt && !dirty ? <CheckCircle2 size={11} /> : <Save size={11} />}
            {t("common.save", "Save")}
          </button>
        </div>
      </div>
    </div>
  );
}

function DbStateView({ agent }: { agent: Agent }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  const { data: variables } = useQuery({
    queryKey: ["agent-vars-raw", agent.id],
    queryFn: () => listAgentVariables(agent.id),
    staleTime: 5_000,
  });
  const { data: hooks } = useQuery({
    queryKey: ["agent-hooks-raw", agent.id],
    queryFn: () => listAgentHooks(agent.id),
    staleTime: 5_000,
  });
  const { data: knowledge } = useQuery({
    queryKey: ["agent-knowledge-raw", agent.id],
    queryFn: () => listAgentKnowledge(agent.id, false),
    staleTime: 5_000,
    enabled: agent.kind === "external",
  });

  const json = useMemo(() => {
    return JSON.stringify(
      {
        agent: {
          id: agent.id,
          slug: agent.slug,
          displayName: agent.displayName,
          description: agent.description,
          runtime: agent.runtime,
          model: agent.model,
          projectId: agent.projectId,
          kind: agent.kind ?? "internal",
          systemPrompt: agent.systemPrompt,
          permissions: tryParse(agent.permissions),
          skills: tryParse(agent.skills),
          mcps: tryParse(agent.mcps),
          goal: agent.goal,
          filePath: agent.filePath,
          roleModels: tryParse(agent.roleModels ?? null),
          memoryPolicy: tryParse(agent.memoryPolicy ?? null),
          createdAt: agent.createdAt,
          lastUsedAt: agent.lastUsedAt,
        },
        variables: (variables ?? []).map((v) => ({
          name: v.name,
          kind: v.kind,
          enabled: v.enabled,
          config: tryParse(v.configJson),
        })),
        hooks: (hooks ?? []).map((h) => ({
          name: h.name,
          kind: h.kind,
          enabled: h.enabled,
          fireMode: h.fireMode,
          config: tryParse(h.configJson),
        })),
        knowledge:
          agent.kind === "external" && knowledge
            ? (() => {
                const bySource = new Map<string, { chunks: number; tokens: number; embedModel: string }>();
                for (const c of knowledge) {
                  const cur = bySource.get(c.source) ?? { chunks: 0, tokens: 0, embedModel: c.embedModel };
                  cur.chunks += 1;
                  cur.tokens += c.tokens;
                  bySource.set(c.source, cur);
                }
                return Array.from(bySource.entries()).map(([source, stats]) => ({ source, ...stats }));
              })()
            : undefined,
      },
      null,
      2,
    );
  }, [agent, variables, hooks, knowledge]);

  const onCopy = async () => {
    try {
      await navigator.clipboard.writeText(json);
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch { /* clipboard blocked — silent */ }
  };

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-2">
        <span className="text-[11px] text-cs-muted">
          {t(
            "agentDetail.raw.dbHint",
            "Read-only aggregate of the agent's full SQLite state (record + variables + hooks + knowledge + memory + role models). Edit these via the structured tabs above.",
          )}
        </span>
        <button
          type="button"
          onClick={onCopy}
          className="inline-flex items-center gap-1 rounded-md border border-cs-border bg-cs-bg-raised px-2 py-1 text-[11px] text-cs-muted hover:text-cs-text"
        >
          {copied ? <Check size={11} className="text-cs-accent" /> : <Copy size={11} />}
          {copied ? t("common.copied", "Copied") : t("common.copy", "Copy JSON")}
        </button>
      </div>
      <pre className="max-h-[600px] overflow-auto rounded-md border border-cs-border bg-cs-bg p-3 font-mono text-[11px] text-cs-text leading-relaxed">
        {json}
      </pre>
    </div>
  );
}

function tryParse(s: string | null | undefined): unknown {
  if (!s) return null;
  try {
    return JSON.parse(s);
  } catch {
    return s; // fall back to the raw string for non-JSON columns
  }
}
