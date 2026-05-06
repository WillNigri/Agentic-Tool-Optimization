import { lazy } from "react";
import { useTranslation } from "react-i18next";
import { Activity, BarChart3, Layers, Shield, Bot } from "lucide-react";
import SectionTabs, { type TabDef } from "./SectionTabs";

const AgentObservability = lazy(() => import("@/components/AgentObservability/Dashboard"));
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
      // v1.4.0 F6 — observability dashboard reads ~/.ato/agent-logs.jsonl.
      id: "agents",
      label: t("subnav.insightsAgents", "Agents"),
      icon: Bot,
      Component: AgentObservability,
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
  return <SectionTabs storageKey="ato.subtab.insights" tabs={tabs} defaultTab="agents" />;
}
