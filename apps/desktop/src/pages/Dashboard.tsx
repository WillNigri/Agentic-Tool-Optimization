import { useState } from "react";
import Sidebar, { type Section } from "@/components/Sidebar";
import ContextVisualizer from "@/components/ContextVisualizer";
import SkillsManager from "@/components/SkillsManager";
import UsageAnalytics from "@/components/UsageAnalytics";
import McpDashboard from "@/components/McpDashboard";
import RuntimeSettings from "@/components/RuntimeSettings";
import SubagentsManager from "@/components/SubagentsManager";
import HooksManager from "@/components/HooksManager";
import AutomationFlow from "@/components/AutomationFlow";
import CronDashboard from "@/components/cron/CronDashboard";
import PromptBar from "@/components/PromptBar";
import SetupWizard from "@/components/SetupWizard";
import { AgentManager } from "@/components/AgentManager";

const PANELS: Record<Section, React.ComponentType> = {
  context: ContextVisualizer,
  skills: SkillsManager,
  subagents: SubagentsManager,
  hooks: HooksManager,
  automation: AutomationFlow,
  cron: CronDashboard,
  analytics: UsageAnalytics,
  mcp: McpDashboard,
  agents: AgentManager,
  config: RuntimeSettings,
};

function isSetupComplete(): boolean {
  const setup = localStorage.getItem("ato-setup");
  if (!setup) return false;
  try {
    const data = JSON.parse(setup);
    return !!data.completedAt;
  } catch {
    return false;
  }
}

export default function Dashboard() {
  const [section, setSection] = useState<Section>("context");
  const [showSetup, setShowSetup] = useState(!isSetupComplete());
  const Panel = PANELS[section];

  // Automation flow needs full width with no padding
  const isFullWidth = section === "automation";

  if (showSetup) {
    return <SetupWizard onComplete={() => setShowSetup(false)} />;
  }

  return (
    <div className="flex h-screen overflow-hidden">
      <Sidebar active={section} onNavigate={setSection} />
      <div className="flex-1 flex flex-col overflow-hidden">
        <main className={isFullWidth ? "flex-1 overflow-hidden" : "flex-1 overflow-y-auto p-6"}>
          <Panel />
        </main>
        <PromptBar />
      </div>
    </div>
  );
}
