import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import "../i18n";
import Home from "@/pages/Home";

// Render Home with a fresh QueryClient and the runtime ready flag overridden
// so the empty-state banner doesn't fire (we test that case separately).
function renderHome(props: Parameters<typeof Home>[0] = {}) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Home runtimeReady={true} {...props} />
    </QueryClientProvider>
  );
}

describe("Home (T1+T9)", () => {
  it("renders the Create Agent CTAs in the hero", () => {
    renderHome();
    expect(screen.getByRole("button", { name: /Start with chat/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Quick setup/i })).toBeInTheDocument();
  });

  it("shows empty states when no agents and no runs are passed", () => {
    renderHome();
    expect(screen.getByText(/No agents yet/i)).toBeInTheDocument();
    expect(screen.getByText(/No runs yet/i)).toBeInTheDocument();
  });

  it("shows the connect-runtime banner when no runtime is ready", () => {
    const onOpenSettings = vi.fn();
    renderHome({ runtimeReady: false, onOpenSettings });
    expect(screen.getByText(/Connect a runtime to get started/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Open Settings/i })).toBeInTheDocument();
  });

  it("calls onOpenSettings when the banner button is clicked", () => {
    const onOpenSettings = vi.fn();
    renderHome({ runtimeReady: false, onOpenSettings });
    fireEvent.click(screen.getByRole("button", { name: /Open Settings/i }));
    expect(onOpenSettings).toHaveBeenCalledOnce();
  });

  it("renders recent agents and runs when provided", () => {
    renderHome({
      recentAgents: [
        { id: "a1", displayName: "PR Reviewer", runtime: "claude", lastUsedAt: Date.now() },
      ],
      recentRuns: [
        { id: "r1", agentName: "PR Reviewer", runtime: "claude", status: "ok", startedAt: Date.now() - 60_000 },
      ],
    });
    // Both the agent card and the run row render the same name.
    expect(screen.getAllByText("PR Reviewer").length).toBe(2);
  });
});
