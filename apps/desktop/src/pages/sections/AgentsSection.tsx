import { lazy, useState } from "react";
import { useTranslation } from "react-i18next";
import { Bot, Terminal, FileCode, Plus, Network } from "lucide-react";
import SectionTabs, { type TabDef } from "./SectionTabs";
import CreateAgentWizard from "@/components/CreateAgentWizard";

// User-created agents (from the Create Agent wizard, stored in SQLite).
const MyAgentsList = lazy(() => import("@/components/MyAgentsList/MyAgentsList"));
// v1.4.0 F4 — Multi-agent groups (router + children).
const GroupsList = lazy(() => import("@/components/AgentGroups/GroupsList"));
// Runtime-discovered "built-in" agents (Claude Code subagents, OpenClaw gateway, etc).
const SubagentsList = lazy(() => import("@/components/SubagentsManager"));
const AgentDetail = lazy(() =>
  import("@/components/AgentManager").then((m) => ({ default: m.AgentManager }))
);

// "+ New" tab opens the wizard; render a lightweight stub that shows the trigger.
function NewAgentLauncher() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(true);
  return (
    <div className="rounded-lg border border-cs-border bg-cs-card p-6">
      <h3 className="text-sm font-medium text-cs-text">
        {t("agents.newTitle", "Create a new agent")}
      </h3>
      <p className="mt-1 text-xs text-cs-muted">
        {t(
          "agents.newSubtitle",
          "Two paths: a chat that suggests a stack, or a one-page form for power users."
        )}
      </p>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="mt-4 inline-flex items-center gap-2 rounded-lg bg-cs-accent px-4 py-2 text-sm font-medium text-cs-bg hover:bg-cs-accent-hover"
      >
        <Plus size={14} />
        {t("agents.openWizard", "Open Create Agent")}
      </button>
      <CreateAgentWizard open={open} onClose={() => setOpen(false)} />
    </div>
  );
}

export default function AgentsSection() {
  const { t } = useTranslation();
  const tabs: TabDef[] = [
    {
      id: "mine",
      label: t("subnav.agentsMine", "My Agents"),
      icon: Bot,
      Component: MyAgentsList,
    },
    {
      id: "groups",
      label: t("subnav.agentsGroups", "Groups"),
      icon: Network,
      Component: GroupsList,
    },
    {
      id: "builtin",
      label: t("subnav.agentsBuiltin", "Built-in"),
      icon: Terminal,
      Component: SubagentsList,
    },
    {
      id: "detail",
      label: t("subnav.agentsDetail", "Detail"),
      icon: FileCode,
      Component: AgentDetail,
    },
    {
      id: "new",
      label: t("subnav.agentsCreate", "+ New"),
      icon: Plus,
      Component: NewAgentLauncher,
    },
  ];
  return <SectionTabs storageKey="ato.subtab.agents" tabs={tabs} defaultTab="mine" />;
}
