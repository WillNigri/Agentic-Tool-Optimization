// v2.16 PR-7 — MissionBoard unit test.
//
// Tests the board with mocked Tauri invoke (global mock set up in
// src/test/setup.ts). Follows the same pattern as CloseConversationModal.test.tsx.

import { render, screen, waitFor } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import MissionBoard from "@/components/Missions/MissionBoard";
import type { MissionSummary } from "@/lib/missions";

// i18n is not set up in tests — components fall back to t("key", "fallback").
// vi.mock for react-i18next to return a simple pass-through t fn.
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_key: string, fallback?: string) => fallback ?? _key,
  }),
  initReactI18next: { type: "3rdParty", init: () => {} },
}));

const mockMissions: MissionSummary[] = [
  {
    id: "aaaaaaaa-0000-0000-0000-000000000001",
    slug: "improve-search",
    name: "Improve Search",
    goal: "Make search 3x faster",
    state: "open",
    category: "autonomous",
    workspaceStrategy: "single_cwd",
    mergeStrategy: "human_approves_each",
    maxLoops: 5,
    tokenBudgetUsd: 2.0,
    spentUsd: 0.45,
    dispatchCount: 1,
    updatedAt: new Date(Date.now() - 3600_000).toISOString(),
  },
  {
    id: "aaaaaaaa-0000-0000-0000-000000000002",
    slug: "fix-auth-bug",
    name: "Fix Auth Bug",
    goal: "Fix the login loop",
    state: "in_progress",
    category: "needs_owner",
    workspaceStrategy: "single_cwd",
    mergeStrategy: "human_approves_each",
    maxLoops: null,
    tokenBudgetUsd: null,
    spentUsd: 1.2,
    dispatchCount: 3,
    updatedAt: new Date(Date.now() - 7200_000).toISOString(),
  },
];

function makeWrapper() {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
  };
}

describe("<MissionBoard />", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("renders the board title when missions load", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(mockMissions);
    render(<MissionBoard />, { wrapper: makeWrapper() });

    // Title is rendered from the fallback string
    expect(screen.getByText("Missions")).toBeInTheDocument();

    // Wait for the data to load and cards to appear
    await waitFor(() => {
      expect(screen.getByText("Improve Search")).toBeInTheDocument();
    });
    expect(screen.getByText("Fix Auth Bug")).toBeInTheDocument();
  });

  it("calls missions_list invoke command", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(mockMissions);
    render(<MissionBoard />, { wrapper: makeWrapper() });

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("missions_list", {
        stateFilter: null,
        categoryFilter: null,
      });
    });
  });

  it("renders column headers for all four states", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(mockMissions);
    render(<MissionBoard />, { wrapper: makeWrapper() });

    await waitFor(() => {
      expect(screen.getByText("Open")).toBeInTheDocument();
    });
    expect(screen.getByText("In Progress")).toBeInTheDocument();
    expect(screen.getByText("Blocked")).toBeInTheDocument();
    expect(screen.getByText("Complete")).toBeInTheDocument();
  });

  it("shows empty state when no missions returned", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([]);
    render(<MissionBoard />, { wrapper: makeWrapper() });

    await waitFor(() => {
      expect(screen.getByText("No missions yet.")).toBeInTheDocument();
    });
  });

  it("shows error state when invoke rejects", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("DB locked"));
    render(<MissionBoard />, { wrapper: makeWrapper() });

    await waitFor(() => {
      expect(
        screen.getByText(
          "Failed to load missions — is the ATO DB available?"
        )
      ).toBeInTheDocument();
    });
  });

  it("renders category chip for autonomous missions", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(mockMissions);
    render(<MissionBoard />, { wrapper: makeWrapper() });

    await waitFor(() => {
      const chips = screen.getAllByText("autonomous");
      expect(chips.length).toBeGreaterThan(0);
    });
  });

  it("renders budget bar when tokenBudgetUsd is set", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(mockMissions);
    render(<MissionBoard />, { wrapper: makeWrapper() });

    await waitFor(() => {
      // "Improve Search" has a $2.00 budget and $0.45 spent
      expect(screen.getByText(/\$0\.450 \/ \$2\.00/)).toBeInTheDocument();
    });
  });
});
