import { useState } from "react";
import Sidebar, { type Section } from "@/components/Sidebar";
import ContextVisualizer from "@/components/ContextVisualizer";
import SkillsManager from "@/components/SkillsManager";
import UsageAnalytics from "@/components/UsageAnalytics";
import McpDashboard from "@/components/McpDashboard";
import ConfigEditor from "@/components/ConfigEditor";

const PANELS: Record<Section, React.ComponentType> = {
  context: ContextVisualizer,
  skills: SkillsManager,
  analytics: UsageAnalytics,
  mcp: McpDashboard,
  config: ConfigEditor,
};

export default function Dashboard() {
  const [section, setSection] = useState<Section>("context");
  const Panel = PANELS[section];

  return (
    <div className="flex h-screen overflow-hidden">
      <Sidebar active={section} onNavigate={setSection} />
      <main className="flex-1 overflow-y-auto p-6">
        <Panel />
      </main>
    </div>
  );
}
