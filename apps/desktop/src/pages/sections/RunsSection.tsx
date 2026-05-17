import { lazy } from "react";
import { useTranslation } from "react-i18next";
import {
  MonitorDot,
  Clock,
  Workflow,
  Webhook,
  MessagesSquare,
  MessageSquare,
} from "lucide-react";
import SectionTabs, { type TabDef } from "./SectionTabs";

// AgentMonitor uses `export default`; importing directly gives the right shape.
const AgentMonitor = lazy(() => import("@/components/AgentMonitor/AgentMonitor"));
const CronDashboard = lazy(() => import("@/components/cron/CronDashboard"));
const AutomationFlow = lazy(() => import("@/components/AutomationFlow"));
const HooksManager = lazy(() => import("@/components/HooksManager"));
// v2.3.20 Phase 5.5 — Activity feed lives under Runs since it's the
// "what's happening between humans and agents" surface, adjacent to
// the existing Live + Automations tabs.
const ActivityFeed = lazy(() => import("@/components/ActivityFeed"));
// v2.3.42 — Sessions tab surfaces Phase 6 Slice A/A.2/B conversations.
// PR 5c (2026-05-17) — Sessions tab absorbs the standalone dispatches
// the History tab used to show. WhatsApp-feed model: multi-turn rooms
// (sessions) and single chats (ephemerals) coexist in one inbox. The
// History tab + its `LogViewer` import are removed here, and the
// orphaned `apps/desktop/src/components/LogViewer/` directory has
// been deleted alongside (codex Round-1 #4: dead code dies with the
// feature removal, not "in a follow-up PR"). AgentDetail has its own
// per-agent HistoryTab which is unrelated and stays.
const SessionsList = lazy(() => import("@/components/SessionsList"));

export default function RunsSection() {
  const { t } = useTranslation();
  const tabs: TabDef[] = [
    {
      id: "live",
      label: t("subnav.runsLive", "Live"),
      icon: MonitorDot,
      Component: AgentMonitor,
    },
    {
      id: "sessions",
      label: t("subnav.runsSessions", "Sessions"),
      icon: MessageSquare,
      Component: SessionsList,
    },
    {
      id: "feed",
      label: t("subnav.runsFeed", "Feed"),
      icon: MessagesSquare,
      Component: ActivityFeed,
    },
    {
      id: "schedules",
      label: t("subnav.runsSchedules", "Schedules"),
      icon: Clock,
      Component: CronDashboard,
    },
    {
      id: "automations",
      label: t("subnav.runsAutomations", "Automations"),
      icon: Workflow,
      Component: AutomationFlow,
    },
    {
      id: "hooks",
      label: t("subnav.runsHooks", "Hooks"),
      icon: Webhook,
      Component: HooksManager,
    },
  ];
  return <SectionTabs storageKey="ato.subtab.runs" tabs={tabs} />;
}
