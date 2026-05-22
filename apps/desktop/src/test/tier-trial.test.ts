import { describe, it, expect } from "vitest";

import { resolveEffectiveTier } from "@/lib/tier";
import type { TrialStatus } from "@/lib/trial";

function trialStatus(state: TrialStatus["state"], daysRemaining = 0): TrialStatus {
  return {
    state,
    daysRemaining,
    showBanner: state === "active" && daysRemaining <= 7,
  };
}

describe("resolveEffectiveTier", () => {
  it("paid tiers never downgrade regardless of trial state", () => {
    for (const cached of ["pro", "team", "enterprise"] as const) {
      const out = resolveEffectiveTier(
        cached,
        false,
        trialStatus("expired"),
        false,
        false,
      );
      expect(out).toBe(cached);
    }
  });

  it("free + non-Tauri + non-cloud stays free even with active trial", () => {
    const out = resolveEffectiveTier(
      "free",
      false,
      trialStatus("active", 10),
      false,
      false,
    );
    expect(out).toBe("free");
  });

  it("free + Tauri + active trial → pro", () => {
    const out = resolveEffectiveTier(
      "free",
      false,
      trialStatus("active", 10),
      true,
      false,
    );
    expect(out).toBe("pro");
  });

  it("free + cloud + active trial → pro", () => {
    const out = resolveEffectiveTier(
      "free",
      true,
      trialStatus("active", 3),
      false,
      false,
    );
    expect(out).toBe("pro");
  });

  it("free + Tauri + expired trial → free (the conversion gate)", () => {
    const out = resolveEffectiveTier(
      "free",
      false,
      trialStatus("expired"),
      true,
      false,
    );
    expect(out).toBe("free");
  });

  it("everPaid flag blocks a fresh trial after sign-out", () => {
    // User: was paid Pro, signed out, cache resets to free.
    // Without the everPaid check, useTier would re-arm a 14-day
    // trial. With it, they stay free until they pay again.
    const out = resolveEffectiveTier(
      "free",
      false,
      trialStatus("active", 10),
      true,
      true, // everPaid
    );
    expect(out).toBe("free");
  });

  it("free + never-started + Tauri → free (trial hasn't begun yet)", () => {
    const out = resolveEffectiveTier(
      "free",
      false,
      trialStatus("never-started"),
      true,
      false,
    );
    expect(out).toBe("free");
  });
});
