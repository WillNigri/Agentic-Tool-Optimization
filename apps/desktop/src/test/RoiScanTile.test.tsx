import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import "../i18n";
import RoiScanTile from "@/components/RoiScanTile/RoiScanTile";
import * as localInsights from "@/lib/localInsights";

// PR-C (2026-05-21) — RoiScanTile component tests.
// The tile's three states (loading, empty, summary) each get a render-level
// check. requestIdleCallback is mocked synchronously so the queries enable
// immediately under jsdom; in the real Safari fallback path the setTimeout(0)
// branch handles it.

interface IdleHost {
  requestIdleCallback?: (cb: () => void, opts?: { timeout: number }) => number;
  cancelIdleCallback?: (id: number) => void;
}

beforeEach(() => {
  const host = window as unknown as IdleHost;
  host.requestIdleCallback = (cb: () => void) => {
    cb();
    return 1;
  };
  host.cancelIdleCallback = () => {};
});

afterEach(() => {
  vi.restoreAllMocks();
});

function renderTile(onOpenInsights = vi.fn()) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return {
    ...render(
      <QueryClientProvider client={qc}>
        <RoiScanTile onOpenInsights={onOpenInsights} />
      </QueryClientProvider>
    ),
    onOpenInsights,
  };
}

describe("RoiScanTile", () => {
  it("renders the empty-state CTA when no recs and no regressions", async () => {
    vi.spyOn(localInsights, "getCostRecommendationsLocal").mockResolvedValue({
      recommendations: [],
      days: 30,
      min_runs: 10,
      source: "local",
    });
    vi.spyOn(localInsights, "getRegressionsLocal").mockResolvedValue({
      regressions: [],
      window_hours: 24,
      min_samples: 10,
      days: 30,
      source: "local",
    });

    renderTile();
    await waitFor(() => {
      expect(screen.getByTestId("roi-scan-empty")).toBeInTheDocument();
    });
    // Locked copy from war-room Q2 — no $ in the empty state.
    expect(screen.getByText(/Run an agent to see your day-1 ROI scan/i)).toBeInTheDocument();
  });

  it("renders the savings headline with the top recommendation", async () => {
    vi.spyOn(localInsights, "getCostRecommendationsLocal").mockResolvedValue({
      recommendations: [
        {
          agent_slug: "writer",
          current_runtime: "claude",
          current_runs: 100,
          current_cost_per_run: 0.01,
          current_ok_rate: 0.92,
          current_eval_score: null,
          suggested_runtime: "minimax",
          suggested_runs: 80,
          suggested_cost_per_run: 0.003,
          suggested_ok_rate: 0.88,
          suggested_eval_score: null,
          savings_per_run_usd: 0.007,
          savings_window_usd: 0.7,
          savings_pct: 70,
          projected_monthly_usd: 4.2,
        },
      ],
      days: 30,
      min_runs: 10,
      source: "local",
    });
    vi.spyOn(localInsights, "getRegressionsLocal").mockResolvedValue({
      regressions: [],
      window_hours: 24,
      min_samples: 10,
      days: 30,
      source: "local",
    });

    renderTile();
    await waitFor(() => {
      expect(screen.getByTestId("roi-scan-savings")).toBeInTheDocument();
    });
    expect(screen.getByText(/\$4\/mo of cost wins/i)).toBeInTheDocument();
    expect(screen.getByText(/@writer claude → minimax/i)).toBeInTheDocument();
  });

  it("renders the distinct error chip when a Tauri command fails", async () => {
    // PR-C review round 2 (gemini MAJOR) — a failed invoke must not fall
    // through to the empty-state CTA; the user needs to know the scan
    // itself errored, not that they have no dispatches yet.
    vi.spyOn(localInsights, "getCostRecommendationsLocal").mockRejectedValue(
      new Error("Tauri command failed: read_local_db")
    );
    vi.spyOn(localInsights, "getRegressionsLocal").mockRejectedValue(
      new Error("Tauri command failed: read_local_db")
    );

    renderTile();
    await waitFor(() => {
      expect(screen.getByTestId("roi-scan-error")).toBeInTheDocument();
    });
    expect(screen.queryByTestId("roi-scan-empty")).toBeNull();
    expect(screen.queryByTestId("roi-scan-savings")).toBeNull();
  });

  it("renders the regression line and routes clicks to onOpenInsights", async () => {
    vi.spyOn(localInsights, "getCostRecommendationsLocal").mockResolvedValue({
      recommendations: [],
      days: 30,
      min_runs: 10,
      source: "local",
    });
    vi.spyOn(localInsights, "getRegressionsLocal").mockResolvedValue({
      regressions: [
        {
          change_id: "c1",
          agent_slug: "reviewer",
          field: "model",
          old_value: "sonnet-4-6",
          new_value: "sonnet-4-7",
          changed_at: "2026-05-15T00:00:00Z",
          before_runs: 50,
          before_ok_rate: 0.91,
          before_p95_ms: 1200,
          before_cost_per_run: 0.01,
          before_eval_score: null,
          before_eval_count: 0,
          after_runs: 50,
          after_ok_rate: 0.74,
          after_p95_ms: 1500,
          after_cost_per_run: 0.012,
          after_eval_score: null,
          after_eval_count: 0,
          ok_delta_pp: -17,
          p95_delta_pct: 25,
          cost_delta_pct: 20,
          eval_delta_pp: null,
          severity: "regression",
          failing_trace_ids: [],
        },
      ],
      window_hours: 24,
      min_samples: 10,
      days: 30,
      source: "local",
    });

    const { onOpenInsights } = renderTile();
    await waitFor(() => {
      expect(screen.getByTestId("roi-scan-regression")).toBeInTheDocument();
    });
    expect(screen.getByText(/1 config change hurt @reviewer/i)).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("roi-scan-open-insights"));
    expect(onOpenInsights).toHaveBeenCalledOnce();
  });
});
