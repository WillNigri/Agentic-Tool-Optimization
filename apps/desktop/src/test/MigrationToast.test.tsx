import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import MigrationToast from "@/components/MyAgentsList/MigrationToast";
import * as agents from "@/lib/agents";

// S11 (v2.7.11) — Migration toast must:
//   - render nothing on count=0 (the migrated steady state)
//   - render the banner with the count when count > 0
//   - stay dismissed across the same session via localStorage
// These three pin the user-visible contract; the dispatcher's
// enforcement-vs-defaults logic is tested separately in Rust.

vi.mock("@/lib/agents", () => ({
  countUnmigratedAgents: vi.fn(),
}));

function renderToast() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <MigrationToast />
    </QueryClientProvider>,
  );
}

describe("MigrationToast (S11)", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
  });

  it("renders nothing when count is 0", async () => {
    vi.mocked(agents.countUnmigratedAgents).mockResolvedValue(0);
    const { container } = renderToast();
    await waitFor(() => {
      // Query resolves and component re-renders; assert empty.
      expect(container.firstChild).toBeNull();
    });
  });

  it("renders the banner with a singular label when count is 1", async () => {
    vi.mocked(agents.countUnmigratedAgents).mockResolvedValue(1);
    renderToast();
    expect(
      await screen.findByText(/1 agent needs permission migration/i),
    ).toBeInTheDocument();
  });

  it("renders the banner with a plural label when count is > 1", async () => {
    vi.mocked(agents.countUnmigratedAgents).mockResolvedValue(3);
    renderToast();
    expect(
      await screen.findByText(/3 agents need permission migration/i),
    ).toBeInTheDocument();
  });

  it("dismiss button hides the banner and persists to localStorage", async () => {
    vi.mocked(agents.countUnmigratedAgents).mockResolvedValue(2);
    renderToast();
    const banner = await screen.findByTestId("migration-toast");
    expect(banner).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Dismiss/i }));
    expect(screen.queryByTestId("migration-toast")).not.toBeInTheDocument();
    expect(localStorage.getItem("ato.migrationToast.dismissedAt")).toBeTruthy();
  });

  it("stays dismissed across re-renders when localStorage flag is set", async () => {
    localStorage.setItem("ato.migrationToast.dismissedAt", "2026-05-20T00:00:00Z");
    vi.mocked(agents.countUnmigratedAgents).mockResolvedValue(5);
    const { container } = renderToast();
    // Even with a positive count, the localStorage dismissal short-circuits
    // before the query result paints anything.
    await waitFor(() => {
      expect(container.firstChild).toBeNull();
    });
  });

  it("renders nothing when the query throws", async () => {
    vi.mocked(agents.countUnmigratedAgents).mockRejectedValue(new Error("oops"));
    const { container } = renderToast();
    await waitFor(() => {
      expect(container.firstChild).toBeNull();
    });
  });
});
