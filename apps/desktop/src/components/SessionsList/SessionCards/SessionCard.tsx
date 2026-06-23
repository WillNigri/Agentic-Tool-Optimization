// SessionsList/SessionCards/SessionCard.tsx — real multi-turn session card.
//
// Extracted from SessionsList.tsx (2026-05-19 frontend elegance push).
// Used for rows with `rowKind === "session"` — the default branch,
// covering both open + closed multi-turn sessions from the `sessions`
// table. Carries the full coordinator/+ runtime split, persona cluster,
// category/team/project metadata line, and the clickable tag chips
// (only card variant with tag interaction).

import { Lock, Tag, Users } from "lucide-react";

import { cn } from "@/lib/utils";
import {
  runtimeBadge,
  runtimeDisplay,
  personaBadge,
  personaDisplay,
  formatTime,
  avatarInitials,
} from "../_helpers";
import type { SessionListRow } from "../_helpers";

interface TeamShareAnnotation {
  teamName: string | null;
  sharedByLabel: string | null;
  isOwner: boolean;
  members?: { userId: string; name: string | null; email: string }[];
  onManageAccess?: () => void;
}

interface Props {
  session: SessionListRow;
  onOpen: () => void;
  tagFilter: string | null;
  setTagFilter: (tag: string | null) => void;
  teamShare?: TeamShareAnnotation;
}

export function SessionCard({ session: s, onOpen, tagFilter, setTagFilter, teamShare }: Props) {
  // Prefer the coordinator-generated auto_title when present (distilled
  // from the actual conversation); fall back to the user-supplied title,
  // then to a muted "untitled".
  const displayTitle = s.autoTitle || s.title;
  // For closed sessions, the summary is a better preview than the last
  // assistant turn (which is often a tool result or mid-thought fragment).
  // For open sessions, keep the live last-turn preview.
  const previewText =
    s.status === "closed" && s.summary ? s.summary : s.lastAssistantPreview;

  return (
    <button
      onClick={onOpen}
      title={`Session ${s.id}`}
      className={cn(
        "w-full text-left border rounded-lg transition-colors p-4",
        s.status === "closed"
          ? "border-cs-border/60 bg-cs-card/60 hover:border-cs-accent/40"
          : "border-cs-border bg-cs-card hover:border-cs-accent/40 hover:bg-cs-border/20",
      )}
    >
      {/* Part D — team-share annotation banner */}
      {teamShare && (
        <div className="mb-3 flex items-center gap-2 flex-wrap pb-3 border-b border-cs-border/40">
          <span
            className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide bg-cs-accent/15 text-cs-accent ring-1 ring-cs-accent/40"
            title={`Shared into team: ${teamShare.teamName ?? "unknown"}`}
          >
            <Users size={10} />
            {teamShare.teamName ?? "Team"}
          </span>
          {!teamShare.isOwner && teamShare.sharedByLabel && (
            <span className="px-1.5 py-0.5 rounded text-[10px] font-medium bg-cs-accent/10 text-cs-accent">
              shared by {teamShare.sharedByLabel}
            </span>
          )}
          {teamShare.members && teamShare.members.length > 0 && (
            <div className="flex items-center gap-1">
              {teamShare.members.slice(0, 4).map((m) => (
                <span
                  key={m.userId}
                  title={m.name ?? m.email}
                  className="inline-flex items-center justify-center w-5 h-5 rounded-full bg-cs-accent/20 text-cs-accent text-[9px] font-bold uppercase"
                >
                  {avatarInitials(m.name ?? m.email)}
                </span>
              ))}
              {teamShare.members.length > 4 && (
                <span className="text-[10px] text-cs-muted">+{teamShare.members.length - 4}</span>
              )}
            </div>
          )}
          {teamShare.onManageAccess && (
            <button
              type="button"
              onClick={(e) => { e.stopPropagation(); teamShare.onManageAccess?.(); }}
              className="ml-auto text-[10px] text-cs-accent hover:underline"
            >
              Manage access
            </button>
          )}
        </div>
      )}
      <div className="flex items-center gap-3 flex-wrap">
        {/* PR 17 — kind marker for parity with war-room (⚔) + single-run
            (⚡) cards. The kind marker is the visual hook that says
            "I'm a session" at a 60px scan. */}
        <span
          aria-label="session"
          className="px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide bg-cs-muted/15 text-cs-muted"
          title="Sequential multi-turn conversation. Each new turn sees prior turns via history replay."
        >
          💬 session
        </span>
        {/* 2026-05-17 — coordinator vs participants split. "Coord" label +
            ring-accented coordinator badge, separator, "+" label + dimmed
            participant badges. The session's anchor runtime is always
            shown as coordinator even if no turns have been recorded yet. */}
        <div className="flex items-center gap-1">
          <span className="text-[10px] uppercase tracking-wider text-cs-muted font-medium">
            Coord
          </span>
          <span
            className={cn(runtimeBadge(s.runtime), "ring-1 ring-cs-accent/70")}
            title={`Coordinator runtime: ${runtimeDisplay(s.runtime)} — orchestrated this session`}
          >
            {s.runtime}
          </span>
        </div>
        {(() => {
          const participants = s.runtimesUsed.filter((r) => r !== s.runtime);
          if (participants.length === 0) return null;
          return (
            <div className="flex items-center gap-1">
              <span className="text-[10px] uppercase tracking-wider text-cs-muted font-medium">
                +
              </span>
              {participants.map((r) => (
                <span
                  key={r}
                  className={cn(runtimeBadge(r), "opacity-75")}
                  title={`Participant runtime: ${runtimeDisplay(r)} — contributed turns to this session`}
                >
                  {r}
                </span>
              ))}
            </div>
          );
        })()}
        {/* 2026-05-16 — persona cluster. Distinct seat slugs that spoke in
            this session, in first-spoken order. Empty (cluster hidden) for
            generalist-only sessions. */}
        {s.agentsUsed.length > 0 && (
          <div className="flex items-center gap-1">
            {s.agentsUsed.map((slug) => (
              <span
                key={slug}
                className={personaBadge()}
                title={`Persona seat: ${personaDisplay(slug)}`}
              >
                {personaDisplay(slug)}
              </span>
            ))}
          </div>
        )}
        {s.status === "closed" && (
          <span
            className="flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium uppercase bg-cs-muted/20 text-cs-muted"
            title={s.closedAt ? `Closed ${formatTime(s.closedAt)}` : "Closed"}
          >
            <Lock size={10} /> closed
          </span>
        )}
        {/* 2026-05-17 — Sessions UX polish PR 4. Category badge between
            the closed-lock and the title so the work-band reads at a
            glance (Dev / Marketing / Backend / etc.). Hidden when NULL. */}
        {s.category && (
          <span
            className="px-1.5 py-0.5 rounded text-[10px] font-medium uppercase bg-cs-accent/15 text-cs-accent"
            title={`Category: ${s.category} — populated by the coordinator at close`}
          >
            {s.category}
          </span>
        )}
        <div className="ml-auto inline-flex items-center gap-3 text-xs text-cs-muted">
          <span>
            {s.turnCount} turn{s.turnCount !== 1 ? "s" : ""}
          </span>
          {s.totalCostUsd !== null && s.totalCostUsd > 0 && (
            <span
              className="font-mono"
              title="Estimated session cost (sum of execution_logs). Open the session to see the per-runtime breakdown including which rows are metered API vs subscription-estimate."
            >
              ${s.totalCostUsd.toFixed(4)}
            </span>
          )}
          {s.unpricedCount > 0 && (
            <span
              className="font-mono text-amber-400"
              title={`${s.unpricedCount} dispatch(es) have no cost estimate (model missing from the pricing table). The cost shown counts only priced dispatches.`}
            >
              {s.unpricedCount} unpriced
            </span>
          )}
          <span>{formatTime(s.lastUsedAt)}</span>
        </div>
      </div>
      <div className="mt-2 text-sm font-medium text-cs-text truncate">
        {displayTitle || (
          <span className="text-cs-muted italic font-normal">
            untitled session
          </span>
        )}
      </div>
      {/* 2026-05-16 — coordinator + project line. */}
      <div className="mt-1 flex items-center flex-wrap gap-x-3 gap-y-1 text-[11px] text-cs-muted">
        <span>
          coordinator:{" "}
          <span className="text-cs-text">{runtimeDisplay(s.runtime)}</span>
          {s.agentSlug && (
            <>
              {" / "}
              <span className="text-cs-accent">
                {personaDisplay(s.agentSlug)}
              </span>
            </>
          )}
        </span>
        {s.projectId && (
          <span>
            project:{" "}
            <span
              className="text-cs-text font-mono"
              title={`project_id: ${s.projectId}`}
            >
              {s.projectName ?? s.projectId.slice(0, 8)}
            </span>
          </span>
        )}
        {s.team && (
          <span>
            team:{" "}
            <span className="text-cs-text font-mono">{s.team}</span>
          </span>
        )}
      </div>
      {previewText && (
        <div className="mt-2 text-xs text-cs-muted line-clamp-2">
          {previewText}
        </div>
      )}
      {s.tags.length > 0 && (
        <div className="mt-2 flex items-center gap-1 flex-wrap">
          <Tag size={10} className="text-cs-muted" />
          {s.tags.map((tag) => (
            // PR 6 — tag chips become click-to-filter. The outer card is
            // a button (opens the session), so a nested button is invalid
            // HTML; use a span with role=button + onClick.
            // stopPropagation so the click sets the filter without also
            // opening the session detail.
            <span
              key={tag}
              role="button"
              tabIndex={0}
              aria-pressed={tagFilter === tag}
              onClick={(e) => {
                e.stopPropagation();
                // Toggle: clicking the already-active tag clears the
                // filter (matches the aria-pressed toggle semantics).
                setTagFilter(tagFilter === tag ? null : tag);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  e.stopPropagation();
                  setTagFilter(tagFilter === tag ? null : tag);
                }
              }}
              title={
                tagFilter === tag
                  ? `Clear tag filter (currently "${tag}")`
                  : `Filter to sessions tagged "${tag}"`
              }
              className={cn(
                // PR 9 — pressed-state designer note: the color-only delta
                // (bg + text inversion) fails for users with color vision
                // deficiency. Add font-weight + letter-spacing so the
                // active tag also reads as "different" by typography.
                "px-1.5 py-0.5 rounded text-[10px] cursor-pointer transition-colors",
                "focus:outline-none focus-visible:ring-2 focus-visible:ring-cs-accent focus-visible:ring-offset-1 focus-visible:ring-offset-cs-bg",
                tagFilter === tag
                  ? "bg-cs-accent text-cs-bg ring-1 ring-cs-accent font-bold tracking-wide"
                  : "bg-cs-accent/10 text-cs-accent hover:bg-cs-accent/20 font-medium",
              )}
            >
              {tag}
            </span>
          ))}
        </div>
      )}
    </button>
  );
}
