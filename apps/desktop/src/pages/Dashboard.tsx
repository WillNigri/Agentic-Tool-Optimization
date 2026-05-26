import { lazy, Suspense } from "react";
import { Loader2 } from "lucide-react";
import Sidebar from "@/components/Sidebar";
import TerminalPane from "@/components/Terminal/TerminalPane";
import CommandPalette from "@/components/CommandPalette";
import ErrorBoundary from "@/components/ErrorBoundary";
import DemoOverlay from "@/components/DemoOverlay";
import WelcomeTour from "@/components/WelcomeTour";
import TrialBanner from "@/components/Trial/TrialBanner";
import VerifyEmailBanner from "@/components/Trial/VerifyEmailBanner";
import { useUiStore } from "@/stores/useUiStore";

const CreateAgentWizard = lazy(() => import("@/components/CreateAgentWizard"));
const FirstChatWizard = lazy(() => import("@/components/FirstChatWizard"));

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
  const section = useUiStore((s) => s.section);
  const setSection = useUiStore((s) => s.setSection);
  const createAgentOpen = useUiStore((s) => s.createAgentOpen);
  const createAgentPath = useUiStore((s) => s.createAgentPath);
  const closeCreateAgent = useUiStore((s) => s.closeCreateAgent);
  // 2026-05-19 — FirstChatWizard must live at Dashboard scope so the
  // bottom-pane "War room" launcher works from any section. Previously
  // mounted only in Home.tsx; clicking War room from Sessions or
  // Settings flipped firstChatOpen=true in Zustand with no listener.
  const firstChatOpen = useUiStore((s) => s.firstChatOpen);
  const closeFirstChat = useUiStore((s) => s.closeFirstChat);

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
        {/* 2026-05-26 (Will): two stacked banners, each self-gating.
            VerifyEmailBanner: shows when /auth/me returns
              email_verified === false. Hidden for verified users +
              local-only mode.
            TrialBanner: shows for the entire active trial window
              (Phase 1 PR-A threshold flipped from day 7+ to day 1+ in
              this PR so users see the $29/mo price-after-trial from
              day one). Hidden for paid users + before trial start.
            Both mounted ABOVE the scroll container so they stay pinned
            across section scrolls. */}
        <VerifyEmailBanner />
        <TrialBanner />
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
      <DemoOverlay />
      <WelcomeTour />
      {/* Globally-mounted Create Agent wizard so the demo runner (and any
          coordinator) can open it from any section. */}
      <Suspense fallback={null}>
        <CreateAgentWizard
          open={createAgentOpen}
          initialPath={createAgentPath}
          onClose={closeCreateAgent}
        />
      </Suspense>
      <Suspense fallback={null}>
        <FirstChatWizard
          open={firstChatOpen}
          onClose={closeFirstChat}
          onOpenSettings={() => setSection("settings")}
        />
      </Suspense>
    </div>
  );
}
