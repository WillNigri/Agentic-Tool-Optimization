// v2.16 Wave 1 — read-only Team Workspaces: shared resources per team.

import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { ArrowLeft, AlertCircle, Clock, Cpu } from 'lucide-react';
import {
  listSharedResources,
  RESOURCE_KIND_META,
  type SharedResourceKind,
  type SharedRow,
} from '../lib/api';

const KINDS: SharedResourceKind[] = ['session', 'war-room', 'chat', 'loop', 'mission'];

/** Relative time string ("3h ago", "2d ago", …). */
function timeAgo(isoStr: string): string {
  const seconds = Math.floor((Date.now() - new Date(isoStr).getTime()) / 1000);
  if (seconds < 60) return 'just now';
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return `${Math.floor(seconds / 86400)}d ago`;
}

interface Props {
  teamId: string;
  teamName: string;
  onOpenDetail(kind: SharedResourceKind, resourceId: string): void;
  onBack(): void;
  onOpenSettings?(): void;
}

export default function TeamWorkspacePage({ teamId, teamName, onOpenDetail, onBack, onOpenSettings }: Props) {
  const [activeKind, setActiveKind] = useState<SharedResourceKind>('session');

  const { data: rows, isLoading, error } = useQuery({
    queryKey: ['shared', teamId, activeKind],
    queryFn: () => listSharedResources(teamId, activeKind),
  });

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center gap-3">
        <button
          onClick={onBack}
          className="p-1.5 rounded-md hover:bg-[#2a2a3a]/60 text-[#8888a0] hover:text-white transition-colors"
          aria-label="Back to teams"
        >
          <ArrowLeft className="w-4 h-4" />
        </button>
        <div className="flex-1 min-w-0">
          <h2 className="text-xl font-semibold text-white">{teamName}</h2>
          <p className="text-[#8888a0] text-xs mt-0.5">Team workspace — read-only view</p>
        </div>
        {onOpenSettings && (
          <button
            onClick={onOpenSettings}
            className="px-3 py-1.5 rounded-md border border-[#2a2a3a] bg-[#16161e] text-[#aaaab8] hover:text-white hover:border-[#3a3a4a] text-xs transition-colors"
          >
            Settings
          </button>
        )}
      </div>

      {/* Kind tabs */}
      <div className="flex gap-1 border-b border-[#2a2a3a]">
        {KINDS.map((kind) => (
          <button
            key={kind}
            onClick={() => setActiveKind(kind)}
            className={`px-4 py-2 text-sm font-medium transition-colors border-b-2 -mb-px ${
              activeKind === kind
                ? 'text-[#00FFB2] border-[#00FFB2]'
                : 'text-[#8888a0] border-transparent hover:text-white hover:border-[#2a2a3a]'
            }`}
          >
            {RESOURCE_KIND_META[kind].label}
          </button>
        ))}
      </div>

      {/* Loading */}
      {isLoading && (
        <div className="space-y-2 animate-pulse">
          {[1, 2, 3].map((i) => (
            <div key={i} className="h-[68px] bg-[#16161e] rounded-lg border border-[#2a2a3a]" />
          ))}
        </div>
      )}

      {/* Error */}
      {error && (
        <div className="flex items-start gap-3 bg-red-500/10 border border-red-500/30 rounded-lg p-4">
          <AlertCircle className="w-5 h-5 text-red-400 shrink-0 mt-0.5" />
          <div>
            <p className="text-sm text-red-400 font-medium">Failed to load shared resources</p>
            <p className="text-xs text-[#8888a0] mt-1">
              {error instanceof Error ? error.message : 'Unknown error'}
            </p>
          </div>
        </div>
      )}

      {/* Empty state */}
      {!isLoading && !error && rows?.length === 0 && (
        <div className="flex flex-col items-center justify-center py-14 bg-[#16161e] border border-[#2a2a3a] rounded-lg text-center">
          <p className="text-white text-sm font-medium">Nothing shared with this team yet</p>
          <p className="text-[#8888a0] text-xs mt-1">
            Share {RESOURCE_KIND_META[activeKind].label.toLowerCase()} from your desktop app.
          </p>
        </div>
      )}

      {/* Row cards */}
      {!isLoading && rows && rows.length > 0 && (
        <div className="space-y-2">
          {rows.map((row: SharedRow) => (
            <SharedRowCard
              key={row.resource_id}
              row={row}
              onClick={() => onOpenDetail(activeKind, row.resource_id)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function SharedRowCard({ row, onClick }: { row: SharedRow; onClick(): void }) {
  const title = row.title ?? row.resource_id.slice(0, 12) + '…';

  return (
    <button
      onClick={onClick}
      className="w-full bg-[#16161e] border border-[#2a2a3a] rounded-lg px-4 py-3 flex items-center gap-3 hover:border-[#00FFB2]/25 transition-colors text-left group"
    >
      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium text-white truncate">{title}</p>

        <div className="flex items-center gap-3 mt-1 text-xs text-[#8888a0]">
          {/* Shared at */}
          <span className="flex items-center gap-1">
            <Clock className="w-3 h-3" />
            {timeAgo(row.shared_at)}
          </span>

          {/* Runtime + agent slug */}
          {(row.runtime || row.agent_slug) && (
            <span className="flex items-center gap-1">
              <Cpu className="w-3 h-3" />
              {[row.runtime, row.agent_slug].filter(Boolean).join(' / ')}
            </span>
          )}
        </div>
      </div>

      {/* Turn count badge */}
      {typeof row.turn_count === 'number' && (
        <span className="text-[11px] font-medium px-2 py-0.5 rounded-full bg-[#2a2a3a]/60 text-[#8888a0] border border-[#2a2a3a] shrink-0">
          {row.turn_count} turns
        </span>
      )}

      <span className="text-[#2a2a3a] group-hover:text-[#8888a0] transition-colors text-xs shrink-0">→</span>
    </button>
  );
}
