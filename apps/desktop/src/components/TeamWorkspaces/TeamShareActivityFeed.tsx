// v2.14 — Share activity feed for TeamWorkspaces.
//
// Surfaces recent share / unshare / refresh actions across all teams
// the user belongs to. Lets users discover what teammates shared
// recently without polling each detail view.
//
// Polls the cloud /teams/:id/activity endpoint every 30s with a stale
// time of 15s; filters for the v2.14 share actions (shared_resource /
// unshared_resource / refreshed_share). The full activity log carries
// every team action — we render only the share-related ones here.

import { useMemo } from "react";
import { useQueries } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { History, Share2, Trash2, RotateCw } from "lucide-react";

import { cn } from "@/lib/utils";
import { useCloudStore } from "@/stores/useCloudStore";
import {
  getTeamActivity,
  getTeams,
  type TeamActivityEntry,
} from "@/lib/cloud-api";
import { formatTime } from "@/components/SessionsList/_helpers";
import { useQuery } from "@tanstack/react-query";

const SHARE_ACTIONS = new Set([
  "shared_resource",
  "unshared_resource",
  "refreshed_share",
]);

function actionIcon(action: string) {
  if (action === "unshared_resource") return Trash2;
  if (action === "refreshed_share") return RotateCw;
  return Share2;
}

function actionTone(action: string): string {
  if (action === "unshared_resource") return "text-cs-danger";
  if (action === "refreshed_share") return "text-cs-muted";
  return "text-cs-accent";
}

export default function TeamShareActivityFeed() {
  const { t } = useTranslation();
  const { isAuthenticated } = useCloudStore();

  const teamsQuery = useQuery({
    queryKey: ["teams"],
    queryFn: getTeams,
    enabled: isAuthenticated,
  });

  const teams = teamsQuery.data ?? [];

  const activityQueries = useQueries({
    queries: teams.map((team) => ({
      queryKey: ["team-activity", team.id],
      queryFn: () => getTeamActivity(team.id, 100),
      enabled: isAuthenticated && teams.length > 0,
      staleTime: 15_000,
      refetchInterval: 30_000,
    })),
  });

  const isLoading = teamsQuery.isLoading || activityQueries.some((q) => q.isLoading);

  // Flatten + filter + sort across all teams.
  const entries = useMemo<TeamActivityEntry[]>(() => {
    const all: TeamActivityEntry[] = [];
    for (const q of activityQueries) {
      if (q.data) all.push(...q.data);
    }
    return all
      .filter((e) => SHARE_ACTIONS.has(e.action))
      .sort((a, b) => (a.created_at < b.created_at ? 1 : -1))
      .slice(0, 50);
  }, [activityQueries]);

  if (!isAuthenticated) return null;
  if (isLoading && entries.length === 0) {
    return (
      <div className="rounded-md border border-cs-border bg-cs-card p-3 text-xs text-cs-muted">
        {t("teamShareFeed.loading", { defaultValue: "Loading team share activity…" })}
      </div>
    );
  }
  if (entries.length === 0) {
    return (
      <div className="rounded-md border border-cs-border/40 bg-cs-bg-raised/30 p-3 text-xs text-cs-muted">
        <div className="flex items-center gap-2 mb-1">
          <History size={12} />
          <span>{t("teamShareFeed.title", { defaultValue: "Share activity" })}</span>
        </div>
        {t("teamShareFeed.empty", { defaultValue: "No recent share activity in your teams." })}
      </div>
    );
  }

  return (
    <div className="rounded-md border border-cs-border bg-cs-bg-raised/30 p-3 text-xs">
      <div className="flex items-center gap-2 mb-2 text-cs-muted">
        <History size={12} />
        <span>{t("teamShareFeed.title", { defaultValue: "Share activity" })}</span>
        <span className="ml-auto text-[10px]">{entries.length}</span>
      </div>
      <ul className="space-y-1.5">
        {entries.map((entry) => {
          const Icon = actionIcon(entry.action);
          const tone = actionTone(entry.action);
          const actor = entry.user_name || entry.user_email || entry.user_id.slice(0, 8);
          const resourceLabel = entry.resource_name
            ? `"${entry.resource_name}"`
            : entry.resource_type
              ? entry.resource_type
              : t("teamShareFeed.resourceFallback", { defaultValue: "a resource" });
          const verb =
            entry.action === "unshared_resource"
              ? t("teamShareFeed.verb.unshared", { defaultValue: "unshared" })
              : entry.action === "refreshed_share"
                ? t("teamShareFeed.verb.refreshed", { defaultValue: "refreshed" })
                : t("teamShareFeed.verb.shared", { defaultValue: "shared" });
          return (
            <li
              key={entry.id}
              className="flex items-start gap-2 rounded-md border border-cs-border/40 bg-cs-card/30 px-2 py-1.5"
            >
              <Icon size={12} className={cn("mt-0.5 shrink-0", tone)} />
              <div className="flex-1 min-w-0">
                <div className="text-cs-text">
                  <span className="font-medium">{actor}</span>{" "}
                  <span className="text-cs-muted">{verb}</span>{" "}
                  <span className="font-medium">{resourceLabel}</span>
                </div>
                <div className="text-[10px] text-cs-muted">
                  {formatTime(entry.created_at)}
                </div>
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
