// v2.16 Wave 1 — read-only Team Workspaces: teams list.

import { useQuery } from '@tanstack/react-query';
import { Users, ChevronRight, AlertCircle } from 'lucide-react';
import { listTeams, type TeamRow } from '../lib/api';

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

  return (
    <div className="space-y-6">
      {/* Header */}
      <div>
        <h2 className="text-xl font-semibold text-white flex items-center gap-2">
          <Users className="w-5 h-5 text-[#00FFB2]" />
          Team Workspaces
        </h2>
        <p className="text-[#8888a0] text-sm mt-0.5">
          Browse shared sessions, war rooms, and missions across your teams.
        </p>
      </div>

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

      {/* Empty state */}
      {!isLoading && !error && teams?.length === 0 && (
        <div className="flex flex-col items-center justify-center py-16 bg-[#16161e] border border-[#2a2a3a] rounded-lg text-center">
          <Users className="w-10 h-10 text-[#2a2a3a] mb-4" />
          <p className="text-white text-sm font-medium">No teams yet</p>
          <p className="text-[#8888a0] text-xs mt-1">
            Create one from your desktop app.
          </p>
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
