import { describe, it, expect } from "vitest";

import {
  TRIAL_DURATION_DAYS,
  TRIAL_STARTED_AT_KEY,
  TRIAL_EVER_PAID_KEY,
  deriveTrialStatus,
  readTrialStartedAt,
  startTrialIfUnset,
  hasEverPaid,
  markEverPaid,
  type TrialStorage,
} from "@/lib/trial";

// In-memory TrialStorage stub — gives each test an isolated bag so
// they can run in parallel without touching window.localStorage.
function memoryStorage(initial: Record<string, string> = {}): TrialStorage & { snapshot: () => Record<string, string> } {
  const data = new Map<string, string>(Object.entries(initial));
  return {
    getItem: (key) => (data.has(key) ? (data.get(key) as string) : null),
    setItem: (key, value) => {
      data.set(key, value);
    },
    removeItem: (key) => {
      data.delete(key);
    },
    snapshot: () => Object.fromEntries(data),
  };
}

describe("deriveTrialStatus", () => {
  it("returns never-started when no timestamp is stored", () => {
    const out = deriveTrialStatus(null);
    expect(out.state).toBe("never-started");
    expect(out.daysRemaining).toBe(0);
    expect(out.showBanner).toBe(false);
  });

  it("returns active with full duration on day 0", () => {
    const startedAt = "2026-05-21T00:00:00.000Z";
    const now = new Date("2026-05-21T00:00:00.000Z");
    const out = deriveTrialStatus(startedAt, now);
    expect(out.state).toBe("active");
    expect(out.daysRemaining).toBe(TRIAL_DURATION_DAYS);
    // Banner only fires at day 7 onward — full duration is too early.
    expect(out.showBanner).toBe(false);
  });

  it("fires the banner from day 7 of remaining (= day 7 elapsed)", () => {
    const startedAt = "2026-05-01T00:00:00.000Z";
    const now = new Date("2026-05-08T00:00:00.000Z"); // 7 days elapsed
    const out = deriveTrialStatus(startedAt, now);
    expect(out.daysRemaining).toBe(7);
    expect(out.state).toBe("active");
    expect(out.showBanner).toBe(true);
  });

  it("expires on exactly day 14 elapsed", () => {
    const startedAt = "2026-05-01T00:00:00.000Z";
    const now = new Date("2026-05-15T00:00:00.000Z"); // 14 days elapsed
    const out = deriveTrialStatus(startedAt, now);
    expect(out.state).toBe("expired");
    expect(out.daysRemaining).toBe(0);
    expect(out.showBanner).toBe(false);
  });

  it("stays expired well past the window", () => {
    const startedAt = "2026-01-01T00:00:00.000Z";
    const now = new Date("2026-05-21T00:00:00.000Z");
    const out = deriveTrialStatus(startedAt, now);
    expect(out.state).toBe("expired");
    expect(out.daysRemaining).toBe(0);
  });

  it("treats a corrupt timestamp as never-started rather than wedging", () => {
    const out = deriveTrialStatus("not-a-date");
    expect(out.state).toBe("never-started");
    expect(out.daysRemaining).toBe(0);
  });

  it("never returns negative daysRemaining when the system clock is set back", () => {
    const startedAt = "2026-05-21T00:00:00.000Z";
    const now = new Date("2025-01-01T00:00:00.000Z"); // before start
    const out = deriveTrialStatus(startedAt, now);
    expect(out.daysRemaining).toBeGreaterThanOrEqual(0);
    expect(out.daysRemaining).toBeLessThanOrEqual(TRIAL_DURATION_DAYS);
  });
});

describe("startTrialIfUnset", () => {
  it("writes a fresh timestamp when storage is empty", () => {
    const s = memoryStorage();
    const now = new Date("2026-05-21T12:00:00.000Z");
    const got = startTrialIfUnset(s, now);
    expect(got).toBe(now.toISOString());
    expect(s.snapshot()[TRIAL_STARTED_AT_KEY]).toBe(now.toISOString());
  });

  it("returns the existing timestamp when one is already set", () => {
    const existing = "2026-05-15T00:00:00.000Z";
    const s = memoryStorage({ [TRIAL_STARTED_AT_KEY]: existing });
    const got = startTrialIfUnset(s, new Date("2026-05-21T00:00:00.000Z"));
    expect(got).toBe(existing);
    // Storage NOT overwritten.
    expect(s.snapshot()[TRIAL_STARTED_AT_KEY]).toBe(existing);
  });

  it("returns null when storage is unavailable", () => {
    expect(startTrialIfUnset(null)).toBeNull();
  });
});

describe("readTrialStartedAt", () => {
  it("reads the stored value", () => {
    const s = memoryStorage({ [TRIAL_STARTED_AT_KEY]: "2026-05-21T00:00:00.000Z" });
    expect(readTrialStartedAt(s)).toBe("2026-05-21T00:00:00.000Z");
  });

  it("returns null for empty storage", () => {
    expect(readTrialStartedAt(memoryStorage())).toBeNull();
  });

  it("returns null when storage is unavailable", () => {
    expect(readTrialStartedAt(null)).toBeNull();
  });
});

describe("hasEverPaid + markEverPaid", () => {
  it("returns false when the flag has never been set", () => {
    expect(hasEverPaid(memoryStorage())).toBe(false);
  });

  it("returns true after markEverPaid", () => {
    const s = memoryStorage();
    markEverPaid(s);
    expect(hasEverPaid(s)).toBe(true);
    expect(s.snapshot()[TRIAL_EVER_PAID_KEY]).toBe("1");
  });

  it("is idempotent", () => {
    const s = memoryStorage();
    markEverPaid(s);
    markEverPaid(s);
    expect(s.snapshot()[TRIAL_EVER_PAID_KEY]).toBe("1");
  });

  it("returns false when storage is unavailable", () => {
    expect(hasEverPaid(null)).toBe(false);
  });
});
