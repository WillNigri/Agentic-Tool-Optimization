import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { AlertTriangle, X } from "lucide-react";
import { countUnmigratedAgents } from "@/lib/agents";

// S11 (v2.7.11) — pre-v2.7.8 agents have `permissions_migrated_at` NULL,
// which means the new permission DSL is RECORDED in the agents table but
// the dispatcher falls back to pre-PR-2 defaults when enforcing claude /
// codex flags. Result: a v2.7.6 agent that the user thinks is locked to
// "read_file, grep" still gets the full default set on every dispatch.
//
// This toast surfaces the count so the user knows there's work to do.
// Re-saving each agent via the existing AgentDetail save flow stamps
// `permissions_migrated_at = now` (see create_agent / update_*  paths in
// commands/mod.rs) and the count drops to zero.
//
// Read-only on purpose — a bulk-stamp button would mask the user's
// chance to review each agent's permissions while migrating. The toast
// is dismissable per-session so it doesn't nag once the user has seen it.

const DISMISS_STORAGE_KEY = "ato.migrationToast.dismissedAt";

export default function MigrationToast() {
  const { data: count } = useQuery({
    queryKey: ["unmigratedAgentCount"],
    queryFn: countUnmigratedAgents,
    staleTime: 60_000,
    // Failures here aren't worth surfacing — the toast just won't render.
    // The dispatcher's enforcement-vs-defaults logic is independent.
    retry: false,
  });

  const [dismissed, setDismissed] = useState<boolean>(() => {
    try {
      return !!localStorage.getItem(DISMISS_STORAGE_KEY);
    } catch {
      return false;
    }
  });

  if (dismissed || !count || count <= 0) return null;

  const onDismiss = () => {
    try {
      localStorage.setItem(DISMISS_STORAGE_KEY, new Date().toISOString());
    } catch {
      // localStorage may be unavailable (private browsing, quota); just
      // dismiss for this session.
    }
    setDismissed(true);
  };

  return (
    <div
      role="alert"
      data-testid="migration-toast"
      className="rounded-lg border border-cs-warning/40 bg-cs-warning/10 p-3 flex items-start gap-3"
    >
      <AlertTriangle size={16} className="text-cs-warning shrink-0 mt-0.5" />
      <div className="flex-1 min-w-0 space-y-1">
        <p className="text-xs font-semibold text-cs-text">
          {count === 1
            ? "1 agent needs permission migration"
            : `${count} agents need permission migration`}
        </p>
        <p className="text-[11px] text-cs-muted leading-relaxed">
          {count === 1 ? "This agent was" : "These agents were"} created before
          v2.7.8. Their permission DSL is recorded but not enforced —
          dispatches still use the pre-v2.7.8 defaults. Open each agent and
          click Save to engage enforcement.
        </p>
      </div>
      <button
        type="button"
        onClick={onDismiss}
        aria-label="Dismiss"
        className="text-cs-muted hover:text-cs-text"
      >
        <X size={14} />
      </button>
    </div>
  );
}
