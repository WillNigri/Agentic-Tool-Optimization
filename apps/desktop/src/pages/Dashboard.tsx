import { useState, lazy, Suspense } from "react";
import { Loader2 } from "lucide-react";
import Sidebar, { type Section } from "@/components/Sidebar";
import PromptBar from "@/components/PromptBar";
import SetupWizard from "@/components/SetupWizard";

const ContextVisualizer = lazy(() => import("@/components/ContextVisualizer"));
const SkillsManager = lazy(() => import("@/components/SkillsManager"));
const UsageAnalytics = lazy(() => import("@/components/UsageAnalytics"));
const McpDashboard = lazy(() => import("@/components/McpDashboard"));
const RuntimeSettings = lazy(() => import("@/components/RuntimeSettings"));
const SubagentsManager = lazy(() => import("@/components/SubagentsManager"));
const HooksManager = lazy(() => import("@/components/HooksManager"));
const AutomationFlow = lazy(() => import("@/components/AutomationFlow"));
const CronDashboard = lazy(() => import("@/components/cron/CronDashboard"));
const AgentManager = lazy(() => import("@/components/AgentManager").then(m => ({ default: m.AgentManager })));
const ProjectManager = lazy(() => import("@/components/AgentManager").then(m => ({ default: m.ProjectManager })));
const SecretsManager = lazy(() => import("@/components/SecretsManager").then(m => ({ default: m.SecretsManager })));
const EnvManager = lazy(() => import("@/components/EnvManager").then(m => ({ default: m.EnvManager })));
const ModelConfig = lazy(() => import("@/components/ModelConfig").then(m => ({ default: m.ModelConfig })));
const LogViewer = lazy(() => import("@/components/LogViewer").then(m => ({ default: m.LogViewer })));
const HealthDashboard = lazy(() => import("@/components/HealthDashboard").then(m => ({ default: m.HealthDashboard })));
const CloudAuth = lazy(() => import("@/components/CloudAuth"));
const TeamWorkspaces = lazy(() => import("@/components/TeamWorkspaces"));
const SkillSync = lazy(() => import("@/components/SkillSync"));
const NotificationsSettings = lazy(() => import("@/components/NotificationsSettings"));
const AuditLog = lazy(() => import("@/components/AuditLog/AuditLog"));
const LlmApiKeys = lazy(() => import("@/components/LlmApiKeys/LlmApiKeys"));
const AgentMonitor = lazy(() => import("@/components/AgentMonitor/AgentMonitor"));
const WorkspaceView = lazy(() => import("@/components/workspace/WorkspaceView"));

const PANELS: Record<Section, React.ComponentType> = {
  context: ContextVisualizer,
  skills: SkillsManager,
  projects: ProjectManager,
  subagents: SubagentsManager,
  hooks: HooksManager,
  automation: AutomationFlow,
  workspace: WorkspaceView,
  cron: CronDashboard,
  analytics: UsageAnalytics,
  logs: LogViewer,
  health: HealthDashboard,
  mcp: McpDashboard,
  agents: AgentManager,
  cloud: CloudAuth,
  teams: TeamWorkspaces,
  sync: SkillSync,
  notifications: NotificationsSettings,
  audit: AuditLog,
  "llm-keys": LlmApiKeys,
  "agent-monitor": AgentMonitor,
  secrets: SecretsManager,
  env: EnvManager,
  models: ModelConfig,
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
  const isFullWidth = section === "automation" || section === "workspace";

  if (showSetup) {
    return <SetupWizard onComplete={() => setShowSetup(false)} />;
  }

  return (
    <div className="flex h-screen overflow-hidden">
      <Sidebar active={section} onNavigate={setSection} />
      <div className="flex-1 flex flex-col overflow-hidden">
        <main className={isFullWidth ? "flex-1 overflow-hidden" : "flex-1 overflow-y-auto p-6"}>
          <Suspense fallback={<div className="flex items-center justify-center h-32"><Loader2 size={24} className="animate-spin text-cs-muted" /></div>}>
            <Panel />
          </Suspense>
        </main>
        <PromptBar />
      </div>
    </div>
  );
}
