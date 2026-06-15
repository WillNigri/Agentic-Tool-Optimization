import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Loader2, Share2, ShieldCheck } from "lucide-react";
import { useTranslation } from "react-i18next";
import FlipToE2eModal from "./FlipToE2eModal";
import type { SharedResourceKind } from "@/lib/cloud-api";

import { cn } from "@/lib/utils";
import { useFeatureFlag, useTier } from "@/lib/tier";
import {
  getSharedChats,
  getSharedLoops,
  getSharedMissions,
  getSharedSessions,
  getSharedWarRooms,
  getTeamSharedAgents,
  getTeamSharedMethodologies,
  getTeams,
  shareAgentWithTeam,
  shareChatWithTeam,
  shareLoopWithTeam,
  shareMethodologyWithTeam,
  shareMissionWithTeam,
  shareSessionWithTeam,
  shareWarRoomWithTeam,
  unshareAgentFromTeam,
  unshareChatFromTeam,
  unshareLoopFromTeam,
  unshareMethodologyFromTeam,
  unshareMissionFromTeam,
  unshareSessionFromTeam,
  unshareWarRoomFromTeam,
  type Team,
} from "@/lib/cloud-api";

type ShareableResourceKind =
  | "session"
  | "war_room"
  | "chat"
  | "agent"
  | "methodology"
  | "loop"
  | "mission";

interface ShareWithTeamButtonProps {
  resourceKind: ShareableResourceKind;
  resourceId: string;
  getSnapshot?: () => Promise<unknown>;
  className?: string;
}

async function getSharedIdsForTeam(
  resourceKind: ShareableResourceKind,
  teamId: string,
): Promise<string[]> {
  switch (resourceKind) {
    case "session":
      return (await getSharedSessions(teamId)).map((row) => row.session_id);
    case "war_room":
      return (await getSharedWarRooms(teamId)).map((row) => row.war_room_id);
    case "chat":
      return (await getSharedChats(teamId)).map((row) => row.chat_thread_id);
    case "agent":
      return (await getTeamSharedAgents(teamId)).map((row) => row.agent_id);
    case "methodology":
      return (await getTeamSharedMethodologies(teamId)).map(
        (row) => row.methodology_id,
      );
    case "loop":
      return (await getSharedLoops(teamId)).map((row) => row.loop_id);
    case "mission":
      return (await getSharedMissions(teamId)).map((row) => row.mission_id);
    default:
      return [];
  }
}

async function shareResourceWithTeam(
  resourceKind: ShareableResourceKind,
  teamId: string,
  resourceId: string,
  getSnapshot?: () => Promise<unknown>,
): Promise<void> {
  const payload = getSnapshot ? await getSnapshot() : undefined;
  switch (resourceKind) {
    case "session":
      await shareSessionWithTeam(teamId, resourceId, payload ?? { snapshot: null });
      return;
    case "war_room":
      await shareWarRoomWithTeam(teamId, resourceId, payload ?? { snapshot: null });
      return;
    case "chat":
      await shareChatWithTeam(teamId, resourceId, payload ?? { snapshot: null });
      return;
    case "agent":
      await shareAgentWithTeam(teamId, resourceId);
      return;
    case "methodology":
      await shareMethodologyWithTeam(teamId, {
        methodology_id: resourceId,
        ...((payload as Record<string, unknown>) ?? {}),
      } as {
        methodology_id: string;
        slug: string;
        name: string;
        description?: string;
        config: unknown;
      });
      return;
    case "loop":
      await shareLoopWithTeam(teamId, resourceId, payload ?? { snapshot: null });
      return;
    case "mission":
      await shareMissionWithTeam(teamId, resourceId, payload ?? { snapshot: null });
      return;
  }
}

async function unshareResourceFromTeam(
  resourceKind: ShareableResourceKind,
  teamId: string,
  resourceId: string,
): Promise<void> {
  switch (resourceKind) {
    case "session":
      await unshareSessionFromTeam(teamId, resourceId);
      return;
    case "war_room":
      await unshareWarRoomFromTeam(teamId, resourceId);
      return;
    case "chat":
      await unshareChatFromTeam(teamId, resourceId);
      return;
    case "agent":
      await unshareAgentFromTeam(teamId, resourceId);
      return;
    case "methodology":
      await unshareMethodologyFromTeam(teamId, resourceId);
      return;
    case "loop":
      await unshareLoopFromTeam(teamId, resourceId);
      return;
    case "mission":
      await unshareMissionFromTeam(teamId, resourceId);
      return;
  }
}

export default function ShareWithTeamButton({
  resourceKind,
  resourceId,
  getSnapshot,
  className,
}: ShareWithTeamButtonProps) {
  const { t } = useTranslation();
  // Codex final-review F1: gate on the Team-tier capability rather
  // than `tier !== "free"`. The cloud routes are gated requireTeamTier
  // server-side, so a Pro user would see the dropdown and get a 402 on
  // every share attempt — confusing UX.
  const teamShareEnabled = useFeatureFlag("team-workspaces");
  const tier = useTier();
  const [open, setOpen] = useState(false);
  const [pendingTeamId, setPendingTeamId] = useState<string | null>(null);
  const [sharedTeamIds, setSharedTeamIds] = useState<Set<string>>(new Set());
  const [sharedStateLoading, setSharedStateLoading] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  // Wave 3: per-team E2E flip modal state.
  const [flipE2eTeam, setFlipE2eTeam] = useState<{
    teamId: string;
    kind: SharedResourceKind;
    resourceId: string;
  } | null>(null);

  const teamsQuery = useQuery<Team[]>({
    queryKey: ["teams"],
    queryFn: getTeams,
    enabled: tier !== "free",
  });

  const teams = teamsQuery.data ?? [];
  const sharedCount = sharedTeamIds.size;
  const heading = useMemo(
    () =>
      sharedCount > 0
        ? t("teamShare.sharedWithCount", { count: sharedCount, defaultValue: "Shared with {{count}} teams" })
        : t("teamShare.shareWithTeams", "Share with teams"),
    [sharedCount, t],
  );

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };

    if (open) {
      document.addEventListener("mousedown", handleClickOutside);
    }

    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, [open]);

  useEffect(() => {
    let cancelled = false;

    async function loadSharedState(currentTeams: Team[]) {
      if (!open || currentTeams.length === 0) {
        if (!open) setErrorMessage(null);
        return;
      }
      setSharedStateLoading(true);
      setErrorMessage(null);
      try {
        const results = await Promise.all(
          currentTeams.map(async (team) => ({
            teamId: team.id,
            sharedIds: await getSharedIdsForTeam(resourceKind, team.id),
          })),
        );
        if (cancelled) return;
        setSharedTeamIds(
          new Set(
            results
              .filter((row) => row.sharedIds.includes(resourceId))
              .map((row) => row.teamId),
          ),
        );
      } catch (error) {
        if (cancelled) return;
        setErrorMessage(
          error instanceof Error ? error.message : t("teamShare.share_failed", "Share failed"),
        );
      } finally {
        if (!cancelled) {
          setSharedStateLoading(false);
        }
      }
    }

    void loadSharedState(teams);

    return () => {
      cancelled = true;
    };
  }, [open, resourceId, resourceKind, t, teams]);

  if (!teamShareEnabled) {
    // Free / Pro users without Team-tier capability: hide the button
    // entirely. `tier` is kept in scope for the error-fallback path
    // below (subscription downgrade between mount + click).
    void tier;
    return null;
  }

  const handleToggleShare = async (teamId: string, alreadyShared: boolean) => {
    setPendingTeamId(teamId);
    setErrorMessage(null);
    try {
      if (alreadyShared) {
        await unshareResourceFromTeam(resourceKind, teamId, resourceId);
        setSharedTeamIds((prev) => {
          const next = new Set(prev);
          next.delete(teamId);
          return next;
        });
      } else {
        await shareResourceWithTeam(resourceKind, teamId, resourceId, getSnapshot);
        setSharedTeamIds((prev) => new Set(prev).add(teamId));
      }
    } catch (error) {
      // tier is narrowed to non-"free" by the early return above; if the
      // server still 402s (subscription downgraded between mount and
      // share), surface the same message.
      setErrorMessage(
        error instanceof Error
          ? error.message
          : t("teamShare.share_failed", "Share failed"),
      );
    } finally {
      setPendingTeamId(null);
    }
  };

  return (
    <>
    {/* Wave 3: FlipToE2eModal — mounted outside the dropdown so it's not
        clipped by overflow:hidden on the parent. */}
    {flipE2eTeam && (
      <FlipToE2eModal
        teamId={flipE2eTeam.teamId}
        kind={flipE2eTeam.kind}
        resourceId={flipE2eTeam.resourceId}
        onClose={() => setFlipE2eTeam(null)}
        onSuccess={() => {
          setFlipE2eTeam(null);
          // Reloading the shared state will re-fetch the updated encryption_mode.
          setOpen(false);
        }}
      />
    )}
    <div className={cn("relative", className)} ref={containerRef}>
      <button
        type="button"
        onClick={() => setOpen((prev) => !prev)}
        className="inline-flex items-center justify-center rounded-md border border-cs-border bg-cs-card px-2 py-1.5 text-cs-muted transition-colors hover:bg-cs-border/30 hover:text-cs-text"
        title={t("teamShare.title", "Share with team")}
        aria-label={t("teamShare.title", "Share with team")}
      >
        <Share2 size={14} />
      </button>

      {open && (
        <div className="absolute right-0 top-full z-50 mt-2 w-80 rounded-lg border border-cs-border bg-cs-card p-3 shadow-xl">
          <div className="mb-2 flex items-center justify-between gap-2">
            <div className="text-xs font-medium uppercase tracking-wide text-cs-muted">
              {heading}
            </div>
            {(teamsQuery.isLoading || sharedStateLoading) && (
              <Loader2 size={12} className="animate-spin text-cs-muted" />
            )}
          </div>

          {errorMessage && (
            <div className="mb-2 rounded-md border border-cs-danger/40 bg-cs-danger/10 px-2.5 py-2 text-xs text-cs-text">
              {errorMessage}
            </div>
          )}

          {teams.length === 0 && !teamsQuery.isLoading ? (
            <div className="rounded-md border border-cs-border bg-cs-bg px-2.5 py-2 text-xs text-cs-muted">
              {t("teamShare.noTeams", "No teams available")}
            </div>
          ) : (
            <div className="space-y-1">
              {teams.map((team) => {
                const alreadyShared = sharedTeamIds.has(team.id);
                const isPending = pendingTeamId === team.id;
                return (
                  <div
                    key={team.id}
                    className="flex items-center justify-between gap-3 rounded-md border border-cs-border bg-cs-bg px-2.5 py-2"
                  >
                    <div className="min-w-0">
                      <div className="truncate text-sm text-cs-text">{team.name}</div>
                    </div>
                    <div className="flex items-center gap-1.5">
                    {alreadyShared && (
                      <>
                      {/* v2.14 #7 — Refresh re-uploads the latest snapshot. */}
                      <button
                        type="button"
                        title={t("teamShare.refresh", "Refresh snapshot")}
                        onClick={() => void handleToggleShare(team.id, false)}
                        disabled={isPending || sharedStateLoading}
                        className="shrink-0 rounded-md border border-cs-border px-2 py-1 text-xs text-cs-muted hover:text-cs-text hover:bg-cs-border/30 transition-colors disabled:opacity-50"
                      >
                        {t("teamShare.refreshShort", "↻")}
                      </button>
                      {/* Wave 3 — Switch to E2E encryption. Only shown when the
                          resource kind has a SharedResourceKind mapping. */}
                      {(resourceKind === "session" ||
                        resourceKind === "war_room" ||
                        resourceKind === "chat" ||
                        resourceKind === "loop" ||
                        resourceKind === "mission") && (
                        <button
                          type="button"
                          title={t("teamShare.enableE2e", "Switch to End-to-End Encryption")}
                          onClick={() =>
                            setFlipE2eTeam({
                              teamId: team.id,
                              kind: (resourceKind === "war_room"
                                ? "war-room"
                                : resourceKind) as SharedResourceKind,
                              resourceId,
                            })
                          }
                          disabled={isPending || sharedStateLoading}
                          className="shrink-0 rounded-md border border-cs-accent/40 px-2 py-1 text-xs text-cs-accent hover:bg-cs-accent/10 transition-colors disabled:opacity-50"
                        >
                          <ShieldCheck size={12} />
                        </button>
                      )}
                      </>
                    )}
                    <button
                      type="button"
                      onClick={() => void handleToggleShare(team.id, alreadyShared)}
                      disabled={isPending || sharedStateLoading}
                      className={cn(
                        "shrink-0 rounded-md px-2.5 py-1 text-xs font-medium transition-colors disabled:opacity-50",
                        alreadyShared
                          ? "border border-cs-border text-cs-text hover:bg-cs-border/30"
                          : "bg-cs-accent text-cs-bg hover:bg-cs-accent/90",
                      )}
                    >
                      {isPending ? (
                        <span className="inline-flex items-center gap-1">
                          <Loader2 size={12} className="animate-spin" />
                          {alreadyShared
                            ? t("teamShare.unshare", "Unshare")
                            : t("teamShare.share", "Share")}
                        </span>
                      ) : alreadyShared ? (
                        t("teamShare.unshare", "Unshare")
                      ) : (
                        t("teamShare.share", "Share")
                      )}
                    </button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}
    </div>
    </>
  );
}
