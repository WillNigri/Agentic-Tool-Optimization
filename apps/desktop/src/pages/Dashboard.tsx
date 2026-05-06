import { useState, lazy, Suspense } from "react";
import { Loader2 } from "lucide-react";
import Sidebar, { type Section } from "@/components/Sidebar";
import TerminalPane from "@/components/Terminal/TerminalPane";
import CommandPalette from "@/components/CommandPalette";
import ErrorBoundary from "@/components/ErrorBoundary";

// v1.3.0 — IA collapse: 6 top-level sections (T1).
// Each section owns its sub-tabs in pages/sections/*.
// SetupWizard retired in T9 — first-run UX is Home with a runtime empty state.

const HomePage = lazy(() => import("@/pages/Home"));
const AgentsSection = lazy(() => import("@/pages/sections/AgentsSection"));
const SkillsSection = lazy(() => import("@/pages/sections/SkillsSection"));
const RunsSection = lazy(() => import("@/pages/sections/RunsSection"));
const InsightsSection = lazy(() => import("@/pages/sections/InsightsSection"));
const SettingsSection = lazy(() => import("@/pages/sections/SettingsSection"));

export default function Dashboard() {
  const [section, setSection] = useState<Section>("home");

  const renderSection = () => {
    switch (section) {
      case "home":
        return <HomePage onOpenSettings={() => setSection("settings")} />;
      case "agents":
        return <AgentsSection />;
      case "skills":
        return <SkillsSection />;
      case "runs":
        return <RunsSection />;
      case "insights":
        return <InsightsSection />;
      case "settings":
        return <SettingsSection />;
    }
  };

  return (
    <div className="flex h-screen overflow-hidden">
      <Sidebar active={section} onNavigate={setSection} />
      <div className="flex-1 flex flex-col overflow-hidden">
        <main className="flex-1 overflow-y-auto p-6">
          <ErrorBoundary key={section}>
            <Suspense
              fallback={
                <div className="flex items-center justify-center h-32">
                  <Loader2 size={24} className="animate-spin text-cs-muted" />
                </div>
              }
            >
              {renderSection()}
            </Suspense>
          </ErrorBoundary>
        </main>
        <TerminalPane />
      </div>
      <CommandPalette onNavigate={setSection} />
    </div>
  );
}
