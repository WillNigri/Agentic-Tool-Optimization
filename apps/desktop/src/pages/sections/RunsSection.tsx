import { lazy, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  MonitorDot,
  ChevronLeft,
  Clock,
  Plus,
  Workflow,
  Webhook,
  MessagesSquare,
  MessageSquare,
  Target,
} from "lucide-react";
import SectionTabs, { type TabDef } from "./SectionTabs";
import { useAutomationStore } from "@/stores/useLoopStore";
import type { Loop } from "@/lib/loops-api";

// AgentMonitor uses `export default`; importing directly gives the right shape.
const AgentMonitor = lazy(() => import("@/components/AgentMonitor/AgentMonitor"));
const CronDashboard = lazy(() => import("@/components/cron/CronDashboard"));
const LoopComposer = lazy(() => import("@/components/LoopComposer"));
const LoopsListPage = lazy(() => import("@/components/loops/LoopsListPage"));
const HooksManager = lazy(() => import("@/components/HooksManager"));
// v2.3.20 Phase 5.5 — Activity feed lives under Runs since it's the
// "what's happening between humans and agents" surface, adjacent to
// the existing Live + Automations tabs.
const ActivityFeed = lazy(() => import("@/components/ActivityFeed"));
// v2.3.42 — Sessions tab surfaces Phase 6 Slice A/A.2/B conversations.
// PR 5c (2026-05-17) — Sessions tab absorbs the standalone dispatches
// the History tab used to show. WhatsApp-feed model: multi-turn rooms
// (sessions) and single chats (single-runs) coexist in one inbox. The
// History tab + its `LogViewer` import are removed here, and the
// orphaned `apps/desktop/src/components/LogViewer/` directory has
// been deleted alongside (codex Round-1 #4: dead code dies with the
// feature removal, not "in a follow-up PR"). AgentDetail has its own
// per-agent HistoryTab which is unrelated and stays.
const SessionsList = lazy(() => import("@/components/SessionsList"));
// v2.16 PR-7 — local Mission-control board (OSS single-machine view).
const MissionBoard = lazy(() => import("@/components/Missions/MissionBoard"));

function RunsLoopsTab() {
  const { t } = useTranslation();
  const [showComposer, setShowComposer] = useState(false);
  const [selectedLoopId, setSelectedLoopId] = useState<string | null>(null);
  const workflows = useAutomationStore((s) => s.workflows);

  useEffect(() => {
    if (!showComposer || !selectedLoopId) return;
    if (!workflows.some((workflow) => workflow.id === selectedLoopId)) return;
    useAutomationStore.getState().setActiveWorkflowId(selectedLoopId);
  }, [selectedLoopId, showComposer, workflows]);

  return showComposer ? (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <div className="flex items-center justify-between gap-3 rounded-xl border border-cs-border bg-cs-card p-3">
        <button
          type="button"
          onClick={() => setShowComposer(false)}
          className="inline-flex items-center gap-2 rounded-md border border-cs-border px-3 py-2 text-sm text-cs-text transition-colors hover:bg-cs-bg-raised"
        >
          <ChevronLeft size={14} />
          {t("loops.shell.back", "Back to loops")}
        </button>
        <button
          type="button"
          onClick={() => {
            const store = useAutomationStore.getState();
            store.createWorkflow("Untitled Loop");
            store.setMode("edit");
            setSelectedLoopId(null);
          }}
          className="inline-flex items-center gap-2 rounded-md border border-cs-border px-3 py-2 text-sm text-cs-text transition-colors hover:bg-cs-bg-raised"
        >
          <Plus size={14} />
          {t("loops.shell.newLoop", "New loop")}
        </button>
      </div>
      <div className="min-h-0 flex-1">
        <LoopComposer />
      </div>
    </div>
  ) : (
    <LoopsListPage
      onCreateLoop={() => {
        const store = useAutomationStore.getState();
        store.createWorkflow("Untitled Loop");
        store.setMode("edit");
        setSelectedLoopId(null);
        setShowComposer(true);
      }}
      onSelectLoop={(loop: Loop) => {
        const store = useAutomationStore.getState();
        store.setActiveWorkflowId(loop.id);
        store.setMode("view");
        setSelectedLoopId(loop.id);
        setShowComposer(true);
      }}
    />
  );
}

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
      label: t("subnav.runsLoops", "Loops"),
      icon: Workflow,
      Component: RunsLoopsTab,
    },
    {
      id: "hooks",
      label: t("subnav.runsHooks", "Hooks"),
      icon: Webhook,
      Component: HooksManager,
    },
    {
      id: "missions",
      label: t("subnav.runsMissions", "Missions"),
      icon: Target,
      Component: MissionBoard,
    },
  ];
  return <SectionTabs storageKey="ato.subtab.runs" tabs={tabs} />;
}
