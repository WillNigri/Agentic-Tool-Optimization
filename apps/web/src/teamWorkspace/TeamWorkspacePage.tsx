// v2.16 Wave 1 — read-only Team Workspaces: shared resources per team.

import { useState, useEffect } from 'react';
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
}

export default function TeamWorkspacePage({ teamId, teamName, onOpenDetail, onBack }: Props) {
  const [activeKind, setActiveKind] = useState<SharedResourceKind>('session');
  // v2.16 Wave 3 — paginated shares. Reset to page 0 whenever the
  // active kind tab changes so a teammate switching tabs doesn't
  // land mid-tail of the previous kind.
  const [page, setPage] = useState(0);
  // Wrap setActiveKind so the page resets on kind change without
  // making both effectful state-updates the responsibility of
  // every tab onClick.
  const switchKind = (k: SharedResourceKind) => {
    setActiveKind(k);
    setPage(0);
  };

  // Codex R1 #2 — isFetching covers background refetches the pager
  // also needs to guard against (not just first-load isLoading).
  // placeholderData: keepPrevious avoids flashing skeletons on page
  // change but does NOT change the contract that buttons disable
  // during the fetch.
  const { data, isLoading, isFetching, error } = useQuery({
    queryKey: ['shared', teamId, activeKind, page],
    queryFn: () => listSharedResources(teamId, activeKind, page),
    placeholderData: (prev) => prev,
  });
  const rows = data?.rows;
  const total = data?.total ?? 0;
  const hasMore = data?.hasMore ?? false;
  // Codex R1 #2 — clamp page against total. Without this, a rapid
  // Next click after a fast Prev (or a query that returned a shorter
  // total than the previous fetch) lands the user on a page past
  // the end and renders bogus "Showing 151–120 of 120".
  const maxPage = Math.max(0, Math.ceil(total / 50) - 1);
  const safePage = Math.min(page, maxPage);
  // If the safe page differs from the stored page (post-shrink),
  // realign before render so we never compute negative window starts.
  useEffect(() => {
    if (page !== safePage) setPage(safePage);
  }, [page, safePage]);

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
        <div>
          <h2 className="text-xl font-semibold text-white">{teamName}</h2>
          <p className="text-[#8888a0] text-xs mt-0.5">Team workspace — read-only view</p>
        </div>
      </div>

      {/* Kind tabs */}
      <div className="flex gap-1 border-b border-[#2a2a3a]">
        {KINDS.map((kind) => (
          <button
            key={kind}
            onClick={() => switchKind(kind)}
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

      {/* v2.16 Wave 3 — pagination controls. R1 #2 fix: clamp the
          window-start with safePage so a rapid double-click can't
          overflow; disable both pager buttons while isFetching so a
          stale `hasMore` from the previous page can't be acted on
          mid-load. */}
      {!isLoading && total > 0 && (safePage > 0 || hasMore) && (
        <div className="flex items-center justify-between gap-3 pt-2 border-t border-[#2a2a3a]">
          <div className="text-xs text-[#8888a0]">
            Showing {safePage * 50 + 1}–{Math.min((safePage + 1) * 50, total)} of {total}
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={() => setPage((p) => Math.max(0, p - 1))}
              disabled={safePage === 0 || isFetching}
              className="px-2.5 py-1 text-xs text-[#8888a0] hover:text-white border border-[#2a2a3a] rounded-md hover:border-[#00FFB2]/25 transition-colors disabled:opacity-30 disabled:cursor-not-allowed disabled:hover:text-[#8888a0] disabled:hover:border-[#2a2a3a]"
            >
              ← Prev
            </button>
            <button
              onClick={() => setPage((p) => Math.min(maxPage, p + 1))}
              disabled={!hasMore || isFetching}
              className="px-2.5 py-1 text-xs text-[#8888a0] hover:text-white border border-[#2a2a3a] rounded-md hover:border-[#00FFB2]/25 transition-colors disabled:opacity-30 disabled:cursor-not-allowed disabled:hover:text-[#8888a0] disabled:hover:border-[#2a2a3a]"
            >
              Next →
            </button>
          </div>
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
