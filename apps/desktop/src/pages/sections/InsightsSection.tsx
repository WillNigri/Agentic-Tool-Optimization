import { lazy } from "react";
import { useTranslation } from "react-i18next";
import { Activity, BarChart3, Layers, Shield, Bot, Globe, Zap, GitCommit, DollarSign, Sparkles, ArrowLeftRight, Lock } from "lucide-react";
import SectionTabs, { type TabDef } from "./SectionTabs";

const AgentObservability = lazy(() => import("@/components/AgentObservability/Dashboard"));
const PipelinesPanel = lazy(() => import("@/components/PipelinesPanel"));
const CompareTracesPanel = lazy(() => import("@/components/CompareTracesPanel"));
const ExternalAgentsInsights = lazy(() => import("@/components/ExternalAgentsInsights"));
const LiveRuns = lazy(() => import("@/components/LiveRuns"));
const RegressionsPanel = lazy(() => import("@/components/RegressionsPanel"));
const CostBenchmarksPanel = lazy(() => import("@/components/CostBenchmarksPanel"));
const HealthDashboard = lazy(() =>
  import("@/components/HealthDashboard").then((m) => ({ default: m.HealthDashboard }))
);
const UsageAnalytics = lazy(() => import("@/components/UsageAnalytics"));
const ContextVisualizer = lazy(() => import("@/components/ContextVisualizer"));
const AuditLog = lazy(() => import("@/components/AuditLog/AuditLog"));
// v2.3.45 — Phase 6.x-K eval-score ratchet visualization.
const RatchetPanel = lazy(() => import("@/components/RatchetPanel"));

export default function InsightsSection() {
  const { t } = useTranslation();
  const tabs: TabDef[] = [
    {
      // v2.1.0 Phase 4 — Live runs registry. The "missing ops layer"
      // (Twitter feedback): which runtime is in which workspace, what's
      // running right now, kill button per row.
      id: "live",
      label: t("subnav.insightsLive", "Live"),
      icon: Zap,
      Component: LiveRuns,
    },
    {
      // v1.4.0 F6 — observability dashboard reads ~/.ato/agent-logs.jsonl.
      id: "agents",
      label: t("subnav.insightsAgents", "Agents"),
      icon: Bot,
      Component: AgentObservability,
    },
    {
      // v2.0.0 — Multi-stage dispatches grouped by parent_run_id.
      // Sequential groups (writer → reviewer), routed groups, anything
      // that fans out across runtimes lands here regardless of agent
      // kind. Bridge between Agents (single-agent local view) and
      // External (deployed bundles only).
      id: "pipelines",
      label: t("subnav.insightsPipelines", "Pipelines"),
      icon: Sparkles,
      Component: PipelinesPanel,
    },
    {
      // v2.0.0 — Eval workbench. Diffs any two cloud traces of the same
      // agent regardless of kind. Lives outside External so internal
      // CLI dispatches don't have to be tagged kind=external just to
      // be comparable (the prior dishonest pattern Beatriz flagged).
      id: "compare",
      label: t("subnav.insightsCompare", "Compare"),
      icon: ArrowLeftRight,
      Component: CompareTracesPanel,
    },
    {
      // v2.0.0 Wave 5 — traces from deployed Cloudflare/Vercel/Docker/Node bundles.
      id: "external",
      label: t("subnav.insightsExternal", "External"),
      icon: Globe,
      Component: ExternalAgentsInsights,
    },
    {
      // v2.1.0 Phase 5 — Cross-runtime regression detection.
      // Joins the config-change ledger with trace windows to flag
      // "this model swap dropped success rate by 17pp."
      id: "regressions",
      label: t("subnav.insightsRegressions", "Regressions"),
      icon: GitCommit,
      Component: RegressionsPanel,
    },
    {
      // v2.3.45 Phase 6.x-K — eval-score ratchet visualization.
      // Locked floors per target + breach history from the events bus.
      // Complements Regressions: ratchet = explicit floor I locked;
      // Regressions = "you didn't lock anything but quality dropped
      // after a config change anyway."
      id: "ratchet",
      label: t("subnav.insightsRatchet", "Ratchet"),
      icon: Lock,
      Component: RatchetPanel,
    },
    {
      // v2.1.0 Phase 8 — Usage benchmarks. Always-shown calls + p50 +
      // OK rate, plus per-(agent, runtime) cost when API dispatches
      // reported it. Subscription runs get a "subscription" badge —
      // we don't fake costs we don't have. Beatriz feedback 2026-05-09.
      id: "cost",
      label: t("subnav.insightsCost", "Usage"),
      icon: DollarSign,
      Component: CostBenchmarksPanel,
    },
    {
      id: "health",
      label: t("subnav.insightsHealth", "Health"),
      icon: Activity,
      Component: HealthDashboard,
    },
    {
      id: "analytics",
      label: t("subnav.insightsAnalytics", "Analytics"),
      icon: BarChart3,
      Component: UsageAnalytics,
    },
    {
      id: "context",
      label: t("subnav.insightsContext", "Context"),
      icon: Layers,
      Component: ContextVisualizer,
    },
    {
      id: "audit",
      label: t("subnav.insightsAudit", "Audit Log"),
      icon: Shield,
      Component: AuditLog,
    },
  ];
  // PR-D (2026-05-21) — Regressions promoted to default landing tab.
  // R4 competitive research confirmed cross-runtime regression detection is
  // unique to ATO across Langfuse / Helicone / LangSmith / Braintrust /
  // Promptfoo / Phoenix; making it the first thing a new install sees is
  // the cheapest moat move available. SectionTabs reads storageKey first,
  // so existing users keep their saved tab — the default only affects
  // fresh installs and users who haven't picked a tab yet. War-room
  // de8ffb6d-8b39-4b5c-a2e9-6665e6e7e9f3, R1 3/3 LOCK.
  return <SectionTabs storageKey="ato.subtab.insights" tabs={tabs} defaultTab="regressions" />;
}
