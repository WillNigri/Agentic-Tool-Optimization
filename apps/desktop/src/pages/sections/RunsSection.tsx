import { lazy } from "react";
import { useTranslation } from "react-i18next";
import {
  MonitorDot,
  ScrollText,
  Clock,
  Workflow,
  Webhook,
  MessagesSquare,
} from "lucide-react";
import SectionTabs, { type TabDef } from "./SectionTabs";

// AgentMonitor uses `export default`; importing directly gives the right shape.
const AgentMonitor = lazy(() => import("@/components/AgentMonitor/AgentMonitor"));
const LogViewer = lazy(() =>
  import("@/components/LogViewer").then((m) => ({ default: m.LogViewer }))
);
const CronDashboard = lazy(() => import("@/components/cron/CronDashboard"));
const AutomationFlow = lazy(() => import("@/components/AutomationFlow"));
const HooksManager = lazy(() => import("@/components/HooksManager"));
// v2.3.20 Phase 5.5 — Activity feed lives under Runs since it's the
// "what's happening between humans and agents" surface, adjacent to
// the existing Live + Automations tabs.
const ActivityFeed = lazy(() => import("@/components/ActivityFeed"));

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
      id: "history",
      label: t("subnav.runsHistory", "History"),
      icon: ScrollText,
      Component: LogViewer,
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
