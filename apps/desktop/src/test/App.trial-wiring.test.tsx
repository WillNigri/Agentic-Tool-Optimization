import { render, screen, fireEvent, act } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";

import TierGate from "@/components/Tier/TierGate";
import { useAuthStore } from "@/hooks/useAuth";
import {
  TRIAL_STARTED_AT_KEY,
  TRIAL_EVER_PAID_KEY,
} from "@/lib/trial";
import { UPGRADE_URL } from "@/lib/constants";

// Phase 1 PR-B — integration tests for the wiring layer.
//
// These tests verify the CONNECTIONS between trial primitives and the
// existing tier/auth machinery — not the primitives themselves (those
// are covered in trial.test.ts, tier-trial.test.ts, TrialBanner.test.tsx,
// and TrialExpiredModal.test.tsx).
//
// Wiring under test:
//   1. TierGate swaps UpgradePrompt → TrialExpiredModal when the trial
//      is expired (war-room Q2 pick: in-TierGate swap).
//   2. useAuthStore latches everPaid via markEverPaid on every tier
//      transition to a paid value (war-room Q3 pick: setAuth + setTier
//      + refreshTier, guarded by newTier !== "free").
//   3. UPGRADE_URL is the single source of truth — three previously-
//      hardcoded call sites all import from lib/constants.ts.

function setTrialStartedDaysAgo(days: number) {
  const past = new Date(Date.now() - days * 24 * 60 * 60 * 1000);
  window.localStorage.setItem(TRIAL_STARTED_AT_KEY, past.toISOString());
}

function resetAuthStore() {
  useAuthStore.setState({
    user: { id: "local", name: "Local User", email: "" },
    accessToken: null,
    refreshTokenValue: null,
    isAuthenticated: true,
    isCloudUser: false,
    tier: "free",
  });
}

describe("TierGate trial-aware modal swap", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    resetAuthStore();
  });

  it("opens UpgradePrompt for a Free user when the trial has not started", () => {
    // Plain web user — no trial, no localStorage key.
    render(
      <TierGate feature="evaluators.scheduled">
        <div>evaluators panel</div>
      </TierGate>,
    );
    // mode="block" renders the upgrade button; click it.
    fireEvent.click(screen.getByRole("button"));
    // UpgradePrompt has the per-feature title for "evaluators".
    expect(screen.getByText(/Scheduled batch evaluators/i)).toBeInTheDocument();
    // TrialExpiredModal should NOT render in this branch.
    expect(screen.queryByText(/Your Pro trial has ended/i)).not.toBeInTheDocument();
  });

  it("opens TrialExpiredModal once the trial has expired", () => {
    setTrialStartedDaysAgo(20); // expired
    render(
      <TierGate feature="evaluators.scheduled">
        <div>evaluators panel</div>
      </TierGate>,
    );
    fireEvent.click(screen.getByRole("button"));
    expect(screen.getByText(/Your Pro trial has ended/i)).toBeInTheDocument();
    // Per-feature UpgradePrompt should NOT render in the expired branch.
    expect(screen.queryByText(/Scheduled batch evaluators/i)).not.toBeInTheDocument();
  });

  it("both trial-aware modal variants link to the shared UPGRADE_URL", () => {
    // Not-expired branch first.
    const { unmount } = render(
      <TierGate feature="evaluators.scheduled">
        <div>evaluators panel</div>
      </TierGate>,
    );
    fireEvent.click(screen.getByRole("button"));
    expect(screen.getByRole("link")).toHaveAttribute("href", UPGRADE_URL);
    unmount();

    // Expired branch.
    setTrialStartedDaysAgo(20);
    render(
      <TierGate feature="evaluators.scheduled">
        <div>evaluators panel</div>
      </TierGate>,
    );
    fireEvent.click(screen.getByRole("button"));
    expect(screen.getByRole("link")).toHaveAttribute("href", UPGRADE_URL);
  });
});

describe("useAuthStore everPaid latch", () => {
  beforeEach(() => {
    window.localStorage.clear();
    resetAuthStore();
  });

  it("setAuth with default tier (pro) latches everPaid", () => {
    act(() => {
      useAuthStore.getState().setAuth(
        { id: "u1", name: "U", email: "u@x.com" },
        "access",
        "refresh",
      );
    });
    expect(window.localStorage.getItem(TRIAL_EVER_PAID_KEY)).toBe("1");
  });

  it("setAuth with explicit paid tiers latches everPaid", () => {
    for (const tier of ["pro", "team", "enterprise"] as const) {
      window.localStorage.removeItem(TRIAL_EVER_PAID_KEY);
      act(() => {
        useAuthStore.getState().setAuth(
          { id: "u1", name: "U", email: "u@x.com" },
          "access",
          "refresh",
          tier,
        );
      });
      expect(window.localStorage.getItem(TRIAL_EVER_PAID_KEY)).toBe("1");
    }
  });

  it("setAuth with free tier does NOT latch everPaid", () => {
    act(() => {
      useAuthStore.getState().setAuth(
        { id: "u1", name: "U", email: "u@x.com" },
        "access",
        "refresh",
        "free",
      );
    });
    expect(window.localStorage.getItem(TRIAL_EVER_PAID_KEY)).toBeNull();
  });

  it("setTier to a paid tier latches everPaid", () => {
    act(() => {
      useAuthStore.getState().setTier("pro");
    });
    expect(window.localStorage.getItem(TRIAL_EVER_PAID_KEY)).toBe("1");
  });

  it("setTier to free does NOT latch everPaid", () => {
    act(() => {
      useAuthStore.getState().setTier("free");
    });
    expect(window.localStorage.getItem(TRIAL_EVER_PAID_KEY)).toBeNull();
  });
});
