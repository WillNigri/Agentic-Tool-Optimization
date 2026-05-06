import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Plus,
  Network,
  Loader2,
  AlertCircle,
  Trash2,
  ChevronRight,
  Crown,
} from "lucide-react";
import {
  listAgentGroups,
  deleteAgentGroup,
  type AgentGroup,
} from "@/lib/agentGroups";
import { useFeatureFlag } from "@/lib/tier";
import UpgradePrompt from "@/components/Tier/UpgradePrompt";
import { cn } from "@/lib/utils";
import GroupDetail from "./GroupDetail";

// v1.4.0 F4 — Multi-agent groups list.
//
// First-class object: a router + N specialized children. The article's
// biggest leverage point: one mega-agent with 30 tools is brittle; 4
// specialized agents with 6-8 tools each + a router is robust.

const RUNTIME_DOT: Record<AgentGroup["runtime"], string> = {
  claude: "bg-orange-500",
  codex: "bg-green-500",
  gemini: "bg-blue-500",
  openclaw: "bg-cyan-400",
  hermes: "bg-purple-500",
};

export default function GroupsList() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const unlimited = useFeatureFlag("groups.unlimited");
  const [editing, setEditing] = useState<AgentGroup | "new" | null>(null);
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);

  const { data: groups = [], isLoading, error } = useQuery({
    queryKey: ["agent-groups"],
    queryFn: () => listAgentGroups(),
    staleTime: 5_000,
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteAgentGroup(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["agent-groups"] });
      setPendingDelete(null);
    },
  });

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-32">
        <Loader2 size={20} className="animate-spin text-cs-muted" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-4 flex items-start gap-3">
        <AlertCircle size={16} className="text-cs-danger shrink-0 mt-0.5" />
        <div className="text-xs text-cs-text">
          {t("agentGroups.loadError", "Couldn't load groups.")}{" "}
          <span className="font-mono text-cs-muted">
            {error instanceof Error ? error.message : String(error)}
          </span>
        </div>
      </div>
    );
  }

  if (editing) {
    return (
      <GroupDetail
        existing={editing === "new" ? null : editing}
        onClose={() => setEditing(null)}
        onSaved={() => {
          void queryClient.invalidateQueries({ queryKey: ["agent-groups"] });
          setEditing(null);
        }}
      />
    );
  }

  return (
    <div className="space-y-4">
      <header className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <Network size={16} className="text-cs-accent" />
            <h3 className="text-sm font-medium text-cs-text">
              {t("agentGroups.title", "Multi-agent groups")}
            </h3>
          </div>
          <p className="mt-1 text-xs text-cs-muted">
            {t(
              "agentGroups.subtitle",
              "One router + N specialized children. The router decides who handles each prompt. Cheaper, more reliable, easier to debug than one mega-agent."
            )}
          </p>
        </div>
        <button
          type="button"
          onClick={() => setEditing("new")}
          className="inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover shrink-0"
        >
          <Plus size={12} />
          {t("agentGroups.new", "New group")}
        </button>
      </header>

      {!unlimited && (
        <div className="rounded-md border border-cs-warning/40 bg-cs-warning/10 p-3 flex items-start gap-2 text-xs">
          <Crown size={12} className="text-cs-accent shrink-0 mt-0.5" />
          <span className="text-cs-text">
            {t(
              "agentGroups.freeLimit",
              "Free tier supports up to 3 children per group. Upgrade to Pro for unlimited children."
            )}
          </span>
        </div>
      )}

      {groups.length === 0 ? (
        <EmptyState onAdd={() => setEditing("new")} />
      ) : (
        <div className="space-y-2">
          {groups.map((g) => {
            const childCount = g.members.filter((m) => m.role === "child").length;
            return (
              <div
                key={g.id}
                className="flex items-stretch rounded-lg border border-cs-border bg-cs-card overflow-hidden"
              >
                <button
                  type="button"
                  onClick={() => setEditing(g)}
                  className="flex-1 flex items-center gap-3 px-4 py-3 text-left hover:bg-cs-bg-raised transition min-w-0"
                >
                  <span
                    className={cn(
                      "inline-block w-2 h-2 rounded-full shrink-0",
                      RUNTIME_DOT[g.runtime]
                    )}
                  />
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-medium text-cs-text truncate">
                        {g.displayName}
                      </span>
                      <span className="text-[10px] uppercase tracking-wide text-cs-muted">
                        {childCount} {t("agentGroups.children", "children")}
                      </span>
                    </div>
                    {g.description && (
                      <p className="mt-0.5 text-xs text-cs-muted truncate">{g.description}</p>
                    )}
                  </div>
                  <ChevronRight size={14} className="text-cs-muted shrink-0" />
                </button>
                {pendingDelete === g.id ? (
                  <div className="flex items-center gap-2 px-3 border-l border-cs-border">
                    <span className="text-xs text-cs-danger">
                      {t("agentGroups.deleteConfirm", "Delete?")}
                    </span>
                    <button
                      type="button"
                      onClick={() => setPendingDelete(null)}
                      className="text-xs text-cs-muted hover:text-cs-text"
                    >
                      {t("common.cancel", "Cancel")}
                    </button>
                    <button
                      type="button"
                      onClick={() => deleteMutation.mutate(g.id)}
                      disabled={deleteMutation.isPending}
                      className="inline-flex items-center gap-1 rounded-md border border-cs-danger/40 bg-cs-danger/20 text-cs-danger px-2 py-1 text-xs"
                    >
                      {deleteMutation.isPending && deleteMutation.variables === g.id && (
                        <Loader2 size={10} className="animate-spin" />
                      )}
                      {t("agentGroups.deleteYes", "Yes")}
                    </button>
                  </div>
                ) : (
                  <button
                    type="button"
                    onClick={() => setPendingDelete(g.id)}
                    className="px-3 border-l border-cs-border bg-cs-bg-raised text-cs-muted hover:text-cs-danger flex items-center"
                    title={t("common.delete", "Delete")}
                  >
                    <Trash2 size={12} />
                  </button>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function EmptyState({ onAdd }: { onAdd: () => void }) {
  const { t } = useTranslation();
  const [proPrompt, setProPrompt] = useState(false);
  return (
    <>
      <div className="rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-8 text-center">
        <Network size={28} className="mx-auto text-cs-muted mb-3" />
        <h3 className="text-sm font-medium text-cs-text">
          {t("agentGroups.emptyTitle", "No groups yet")}
        </h3>
        <p className="mt-1 text-xs text-cs-muted max-w-md mx-auto">
          {t(
            "agentGroups.emptyBody",
            "Build a router + specialized children. e.g. customer support → (billing, technical, sales). Each child has its own scoped tools and prompt; the router picks who handles each prompt."
          )}
        </p>
        <div className="mt-4 flex items-center justify-center gap-2">
          <button
            type="button"
            onClick={onAdd}
            className="inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover"
          >
            <Plus size={12} />
            {t("agentGroups.new", "New group")}
          </button>
          <button
            type="button"
            onClick={() => setProPrompt(true)}
            className="text-xs text-cs-muted hover:text-cs-text inline-flex items-center gap-1"
          >
            <Crown size={11} className="text-cs-accent" />
            {t("agentGroups.learnPro", "What's in Pro?")}
          </button>
        </div>
      </div>
      <UpgradePrompt
        feature="groups.unlimited"
        open={proPrompt}
        onClose={() => setProPrompt(false)}
      />
    </>
  );
}
