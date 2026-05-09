import { lazy } from "react";
import { useTranslation } from "react-i18next";
import { Activity, BarChart3, Layers, Shield, Bot, Globe, Zap, GitCommit } from "lucide-react";
import SectionTabs, { type TabDef } from "./SectionTabs";

const AgentObservability = lazy(() => import("@/components/AgentObservability/Dashboard"));
const ExternalAgentsInsights = lazy(() => import("@/components/ExternalAgentsInsights"));
const LiveRuns = lazy(() => import("@/components/LiveRuns"));
const RegressionsPanel = lazy(() => import("@/components/RegressionsPanel"));
const HealthDashboard = lazy(() =>
  import("@/components/HealthDashboard").then((m) => ({ default: m.HealthDashboard }))
);
const UsageAnalytics = lazy(() => import("@/components/UsageAnalytics"));
const ContextVisualizer = lazy(() => import("@/components/ContextVisualizer"));
const AuditLog = lazy(() => import("@/components/AuditLog/AuditLog"));

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
  return <SectionTabs storageKey="ato.subtab.insights" tabs={tabs} defaultTab="live" />;
}
