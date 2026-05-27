import { useEffect, useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Loader2, Share2, Trash2, AlertCircle, Sparkles } from 'lucide-react';
import {
  getTeamSharedAgents,
  shareAgentWithTeam,
  unshareAgentFromTeam,
  type SharedTeamAgent,
} from '@/lib/cloud-api';

interface SharedAgentsPanelProps {
  teamId: string;
  // Agents the current user owns and could share. Caller (parent panel)
  // passes this in — typically the result of the existing
  // `getAgents()` cloud-sync query. id is the cloud-side agents.id UUID.
  ownedAgents?: Array<{ id: string; slug: string; display_name: string; runtime: string }>;
}

export default function SharedAgentsPanel({ teamId, ownedAgents = [] }: SharedAgentsPanelProps) {
  const queryClient = useQueryClient();
  const [shareAgentId, setShareAgentId] = useState<string>('');
  // Two-step confirm for destructive unshare. The first click on a row's
  // trash button parks the row id here; only the second click (the explicit
  // "Confirm" button) fires the mutation. Cleared on success, error, or
  // explicit cancel. Prevents accidental audit-row loss (docs/tiers.md).
  const [confirmAgentId, setConfirmAgentId] = useState<string | null>(null);

  const { data: shared = [], isLoading, error } = useQuery({
    queryKey: ['team-shared-agents', teamId],
    queryFn: () => getTeamSharedAgents(teamId),
    enabled: !!teamId,
  });

  const shareMutation = useMutation({
    mutationFn: (agentId: string) => shareAgentWithTeam(teamId, agentId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['team-shared-agents', teamId] });
      setShareAgentId('');
    },
  });

  const unshareMutation = useMutation({
    mutationFn: (agentId: string) => unshareAgentFromTeam(teamId, agentId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['team-shared-agents', teamId] });
      setConfirmAgentId(null);
    },
    onError: () => setConfirmAgentId(null),
  });

  // Auto-dismiss the confirm-armed state 10s after the user clicks Unshare.
  // The timer starts when confirmAgentId is set, not on any subsequent
  // activity — a stale armed state past 10s clears so a careless return-click
  // can't trip a destructive second click. (review L5)
  useEffect(() => {
    if (!confirmAgentId) return;
    const timer = setTimeout(() => setConfirmAgentId(null), 10_000);
    return () => clearTimeout(timer);
  }, [confirmAgentId]);

  const sharedIds = new Set(shared.map((s: SharedTeamAgent) => s.agent_id));
  const shareableAgents = ownedAgents.filter((a) => !sharedIds.has(a.id));

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <h3 className="text-base font-semibold text-foreground flex items-center gap-2">
          <Sparkles className="h-4 w-4 text-mint" />
          Shared agents
        </h3>
        <span className="text-xs text-muted-foreground">{shared.length} shared</span>
      </div>

      {shareableAgents.length > 0 && (
        <div className="flex items-center gap-2 rounded-md border border-border bg-surface-2 p-3">
          <select
            className="flex-1 rounded border border-border bg-background px-2 py-1 text-sm"
            value={shareAgentId}
            onChange={(e) => setShareAgentId(e.target.value)}
          >
            <option value="">Select an agent to share…</option>
            {shareableAgents.map((a) => (
              <option key={a.id} value={a.id}>
                {a.display_name} ({a.runtime} · {a.slug})
              </option>
            ))}
          </select>
          <button
            type="button"
            disabled={!shareAgentId || shareMutation.isPending}
            onClick={() => shareAgentId && shareMutation.mutate(shareAgentId)}
            className="inline-flex items-center gap-1.5 rounded bg-mint px-3 py-1.5 text-sm font-medium text-background hover:bg-mint/90 disabled:opacity-50"
          >
            {shareMutation.isPending ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Share2 className="h-3.5 w-3.5" />
            )}
            Share
          </button>
        </div>
      )}

      {shareMutation.error && (
        <div className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/5 p-3 text-sm text-destructive">
          <AlertCircle className="h-4 w-4 shrink-0 mt-0.5" />
          <span>{(shareMutation.error as Error).message}</span>
        </div>
      )}

      {isLoading ? (
        <div className="flex items-center justify-center py-6">
          <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
        </div>
      ) : error ? (
        <div className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/5 p-3 text-sm text-destructive">
          <AlertCircle className="h-4 w-4 shrink-0 mt-0.5" />
          <span>{(error as Error).message}</span>
        </div>
      ) : shared.length === 0 ? (
        <p className="text-sm text-muted-foreground italic py-4">
          No agents shared in this team yet. Pick one above to share it with teammates.
        </p>
      ) : (
        <ul className="flex flex-col gap-2">
          {shared.map((row: SharedTeamAgent) => (
            <li
              key={row.agent_id}
              className="flex items-center justify-between gap-3 rounded-md border border-border bg-surface-2 p-3"
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="font-medium text-foreground truncate">
                    {row.display_name}
                  </span>
                  <span className="text-xs text-muted-foreground">
                    {row.runtime}
                    {row.model ? ` · ${row.model}` : ''}
                  </span>
                </div>
                <div className="text-xs text-muted-foreground">
                  {row.slug} · shared by {row.shared_by_email ?? 'unknown'} ·{' '}
                  {new Date(row.shared_at).toLocaleString()}
                </div>
              </div>
              {confirmAgentId === row.agent_id ? (
                <div className="flex items-center gap-1">
                  <button
                    type="button"
                    onClick={() => unshareMutation.mutate(row.agent_id)}
                    disabled={
                      unshareMutation.isPending &&
                      unshareMutation.variables === row.agent_id
                    }
                    className="inline-flex items-center gap-1 rounded bg-destructive px-2 py-1 text-xs font-medium text-background hover:bg-destructive/90 disabled:opacity-50"
                  >
                    {unshareMutation.isPending &&
                    unshareMutation.variables === row.agent_id ? (
                      <Loader2 className="h-3 w-3 animate-spin" />
                    ) : (
                      <Trash2 className="h-3 w-3" />
                    )}
                    Confirm unshare
                  </button>
                  <button
                    type="button"
                    onClick={() => setConfirmAgentId(null)}
                    disabled={
                      unshareMutation.isPending &&
                      unshareMutation.variables === row.agent_id
                    }
                    className="rounded border border-border px-2 py-1 text-xs text-muted-foreground hover:bg-surface-2 disabled:opacity-50"
                  >
                    Cancel
                  </button>
                </div>
              ) : (
                <button
                  type="button"
                  onClick={() => setConfirmAgentId(row.agent_id)}
                  className="inline-flex items-center gap-1 rounded border border-border px-2 py-1 text-xs text-muted-foreground hover:text-destructive hover:border-destructive/40"
                >
                  <Trash2 className="h-3 w-3" />
                  Unshare
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
