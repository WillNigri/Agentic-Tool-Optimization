// v2.16 Wave 1 — read-only Team Workspaces: teams list.
// v2.18.1 — adds "+ New team" button + CreateTeamModal wiring.
// #89 — gates "+ New team" visibility on subscription_tier !== 'free'
// so free-tier users don't get a dead-end 403 from cloud when they click.

import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { Users, ChevronRight, AlertCircle, Plus, Sparkles } from 'lucide-react';
import { listTeams, getMe, type TeamRow } from '../lib/api';
import CreateTeamModal from './CreateTeamModal';

const ROLE_PILL: Record<TeamRow['role'], { label: string; classes: string }> = {
  owner: { label: 'Owner', classes: 'bg-[#00FFB2]/15 text-[#00FFB2] border border-[#00FFB2]/30' },
  admin: { label: 'Admin', classes: 'bg-purple-500/15 text-purple-400 border border-purple-500/30' },
  member: { label: 'Member', classes: 'bg-[#2a2a3a]/60 text-[#8888a0] border border-[#2a2a3a]' },
};

interface Props {
  onSelectTeam(team: TeamRow): void;
}

export default function TeamsListPage({ onSelectTeam }: Props) {
  const { data: teams, isLoading, error } = useQuery({
    queryKey: ['teams'],
    queryFn: listTeams,
  });
  // #89 — fetch the current user so we can gate team-creation on plan tier.
  // Same ['me'] cache key UserSettingsPage uses, so React Query dedupes
  // the request across the app.
  const meQuery = useQuery({ queryKey: ['me'], queryFn: getMe });
  const canCreateTeam =
    !!meQuery.data && meQuery.data.subscription_tier !== 'free';

  const [showCreate, setShowCreate] = useState(false);

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold text-white flex items-center gap-2">
            <Users className="w-5 h-5 text-[#00FFB2]" />
            Team Workspaces
          </h2>
          <p className="text-[#8888a0] text-sm mt-0.5">
            Browse shared sessions, war rooms, and missions across your teams.
          </p>
        </div>
        {canCreateTeam ? (
          <button
            onClick={() => setShowCreate(true)}
            className="shrink-0 px-3 py-2 rounded-md bg-[#00FFB2] text-black text-sm font-semibold hover:bg-[#00FFB2]/90 transition-colors inline-flex items-center gap-1.5"
          >
            <Plus className="w-3.5 h-3.5" /> New team
          </button>
        ) : null}
      </div>

      <CreateTeamModal
        open={showCreate}
        onClose={() => setShowCreate(false)}
      />

      {/* Loading */}
      {isLoading && (
        <div className="space-y-2 animate-pulse">
          {[1, 2, 3].map((i) => (
            <div key={i} className="h-[72px] bg-[#16161e] rounded-lg border border-[#2a2a3a]" />
          ))}
        </div>
      )}

      {/* Error */}
      {error && (
        <div className="flex items-start gap-3 bg-red-500/10 border border-red-500/30 rounded-lg p-4">
          <AlertCircle className="w-5 h-5 text-red-400 shrink-0 mt-0.5" />
          <div>
            <p className="text-sm text-red-400 font-medium">Failed to load teams</p>
            <p className="text-xs text-[#8888a0] mt-1">
              {error instanceof Error ? error.message : 'Unknown error'}
            </p>
          </div>
        </div>
      )}

      {/* Empty state — R1 fix: gate the upsell on meQuery.isSuccess +
          confirmed free tier, so a transient /me fetch failure doesn't
          mistakenly show "upgrade to Pro" to a paid user. Also living
          INSIDE the empty state (not above the team list) so free users
          with existing memberships in Pro teams don't get nag content
          pushed at them every visit. */}
      {!isLoading && !error && teams?.length === 0 && (
        <div className="flex flex-col items-center justify-center py-16 bg-[#16161e] border border-[#2a2a3a] rounded-lg text-center px-6">
          {meQuery.isSuccess && !canCreateTeam ? (
            <>
              <div className="w-12 h-12 rounded-xl bg-[#00FFB2]/15 flex items-center justify-center mb-4">
                <Sparkles className="w-5 h-5 text-[#00FFB2]" />
              </div>
              <p className="text-white text-sm font-medium">
                Team workspaces are a Pro feature
              </p>
              <p className="text-[#8888a0] text-xs mt-2 max-w-sm leading-relaxed">
                Share sessions, war-rooms, and missions with teammates. Upgrade
                to create your first team and invite members.
              </p>
              <a
                href="https://agentictool.ai/#pricing"
                target="_blank"
                rel="noreferrer"
                className="mt-4 px-3 py-2 rounded-md bg-[#00FFB2] text-black text-sm font-semibold hover:bg-[#00FFB2]/90 transition-colors inline-flex items-center gap-1.5"
              >
                See plans →
              </a>
            </>
          ) : (
            <>
              <Users className="w-10 h-10 text-[#2a2a3a] mb-4" />
              <p className="text-white text-sm font-medium">No teams yet</p>
              <p className="text-[#8888a0] text-xs mt-1">
                Create one to share sessions, war rooms, and missions with teammates.
              </p>
              {canCreateTeam && (
                <button
                  onClick={() => setShowCreate(true)}
                  className="mt-4 px-3 py-2 rounded-md bg-[#00FFB2] text-black text-sm font-semibold hover:bg-[#00FFB2]/90 transition-colors inline-flex items-center gap-1.5"
                >
                  <Plus className="w-3.5 h-3.5" /> Create your first team
                </button>
              )}
            </>
          )}
        </div>
      )}

      {/* Team cards */}
      {!isLoading && teams && teams.length > 0 && (
        <div className="space-y-2">
          {teams.map((team) => {
            const pill = ROLE_PILL[team.role];
            return (
              <button
                key={team.id}
                onClick={() => onSelectTeam(team)}
                className="w-full bg-[#16161e] border border-[#2a2a3a] rounded-lg px-4 py-4 flex items-center gap-4 hover:border-[#00FFB2]/25 hover:bg-[#16161e]/80 transition-colors text-left group"
              >
                {/* Avatar */}
                <div className="w-9 h-9 rounded-md bg-[#00FFB2]/10 border border-[#00FFB2]/20 flex items-center justify-center shrink-0">
                  <Users className="w-4 h-4 text-[#00FFB2]" />
                </div>

                {/* Name + slug */}
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium text-white truncate">{team.name}</p>
                  <p className="text-xs text-[#8888a0] font-mono mt-0.5">{team.slug}</p>
                </div>

                {/* Role pill */}
                <span className={`text-[11px] font-medium px-2 py-0.5 rounded-full shrink-0 ${pill.classes}`}>
                  {pill.label}
                </span>

                <ChevronRight className="w-4 h-4 text-[#2a2a3a] group-hover:text-[#8888a0] transition-colors shrink-0" />
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
