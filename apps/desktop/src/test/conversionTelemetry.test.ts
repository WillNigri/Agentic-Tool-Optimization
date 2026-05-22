import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  __peekCountersForTests,
  __resetForTests,
  flushConversionEvents,
  getSessionId,
  getTrialCohort,
  recordFeatureUse,
} from "@/lib/conversionTelemetry";

const invokeMock = vi.mocked(invoke);

beforeEach(() => {
  __resetForTests();
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
  window.localStorage.clear();
  window.sessionStorage.clear();
});

afterEach(() => {
  __resetForTests();
});

describe("recordFeatureUse", () => {
  it("aggregates repeat invocations into a single counter", () => {
    recordFeatureUse("cloud-traces", "pro");
    recordFeatureUse("cloud-traces", "pro");
    recordFeatureUse("cloud-traces", "pro");
    const snapshot = __peekCountersForTests();
    expect(snapshot).toHaveLength(1);
    expect(snapshot[0].feature).toBe("cloud-traces");
    expect(snapshot[0].counter.count).toBe(3);
  });

  it("keeps separate counters for different tiers (mid-session tier change)", () => {
    recordFeatureUse("cloud-traces", "free");
    recordFeatureUse("cloud-traces", "pro");
    const snapshot = __peekCountersForTests();
    expect(snapshot).toHaveLength(2);
    expect(snapshot.map((s) => s.counter.tier).sort()).toEqual(["free", "pro"]);
  });

  it("snapshots trial_cohort at first-seen time", () => {
    window.localStorage.setItem("ato.trialCohort", "A5");
    recordFeatureUse("evaluators", "pro");
    window.localStorage.setItem("ato.trialCohort", "control");
    recordFeatureUse("evaluators", "pro");
    const snapshot = __peekCountersForTests();
    // Same feature+tier → same counter; cohort frozen from first call.
    expect(snapshot).toHaveLength(1);
    expect(snapshot[0].counter.trialCohort).toBe("A5");
  });
});

describe("flushConversionEvents", () => {
  it("is a no-op when there is nothing to flush", async () => {
    await flushConversionEvents();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("drains counters and invokes the Rust command with batched events", async () => {
    recordFeatureUse("cloud-traces", "pro");
    recordFeatureUse("cloud-traces", "pro");
    recordFeatureUse("evaluators", "free");
    await flushConversionEvents();

    expect(invokeMock).toHaveBeenCalledTimes(1);
    const [cmd, args] = invokeMock.mock.calls[0];
    expect(cmd).toBe("record_conversion_events");
    const events = (args as { events: Array<{ feature: string; count: number }> }).events;
    expect(events).toHaveLength(2);
    const byFeature = Object.fromEntries(events.map((e) => [e.feature, e.count]));
    expect(byFeature["cloud-traces"]).toBe(2);
    expect(byFeature["evaluators"]).toBe(1);

    // Counters cleared after flush.
    expect(__peekCountersForTests()).toHaveLength(0);
  });

  it("carries tierAtEvent and trialCohort onto the flushed payload (regression)", async () => {
    window.localStorage.setItem("ato.trialCohort", "A5");
    recordFeatureUse("cloud-traces", "pro");
    // Cohort changes mid-session; existing counter must keep the first-seen value.
    window.localStorage.setItem("ato.trialCohort", "control");
    recordFeatureUse("cloud-traces", "pro");
    await flushConversionEvents();

    const args = invokeMock.mock.calls[0][1] as {
      events: Array<{ feature: string; tierAtEvent: string; trialCohort: string | null }>;
    };
    const cloud = args.events.find((e) => e.feature === "cloud-traces");
    expect(cloud).toBeDefined();
    expect(cloud?.tierAtEvent).toBe("pro");
    // first-seen wins — flushed cohort is the value when the counter was created,
    // not the value at flush time.
    expect(cloud?.trialCohort).toBe("A5");
  });

  it("swallows invoke failures so telemetry can never break the app", async () => {
    invokeMock.mockRejectedValueOnce(new Error("rusqlite locked"));
    recordFeatureUse("cloud-traces", "pro");
    await expect(flushConversionEvents()).resolves.toBeUndefined();
    // Batch is dropped, not re-queued (we accept ≤60s data loss by design).
    expect(__peekCountersForTests()).toHaveLength(0);
  });
});

describe("getSessionId", () => {
  it("mints a UUID once and reuses it within the session", () => {
    const first = getSessionId();
    const second = getSessionId();
    expect(first).toBe(second);
    expect(first.length).toBeGreaterThan(8);
  });
});

describe("getTrialCohort", () => {
  it("returns null when no cohort assignment is stored", () => {
    expect(getTrialCohort()).toBeNull();
  });

  it("returns the stored cohort when localStorage carries one", () => {
    window.localStorage.setItem("ato.trialCohort", "A5");
    expect(getTrialCohort()).toBe("A5");
  });

  it("ignores unknown cohort labels (defends against typos / stale keys)", () => {
    window.localStorage.setItem("ato.trialCohort", "Z9-bogus");
    expect(getTrialCohort()).toBeNull();
  });
});
