import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";

import TrialBanner from "@/components/Trial/TrialBanner";
import {
  TRIAL_STARTED_AT_KEY,
  TRIAL_BANNER_DISMISSED_KEY,
} from "@/lib/trial";

// Helper: stash a startedAt N days in the past so deriveTrialStatus
// yields a specific daysRemaining at render time.
function setTrialStartedDaysAgo(days: number) {
  const past = new Date(Date.now() - days * 24 * 60 * 60 * 1000);
  window.localStorage.setItem(TRIAL_STARTED_AT_KEY, past.toISOString());
}

describe("<TrialBanner />", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
  });

  it("renders nothing before day 7 of the trial", () => {
    setTrialStartedDaysAgo(2);
    const { container } = render(<TrialBanner />);
    expect(container).toBeEmptyDOMElement();
  });

  it("renders the countdown from day 7 onward", () => {
    setTrialStartedDaysAgo(8); // 6 days remaining
    render(<TrialBanner />);
    expect(screen.getByRole("status")).toBeInTheDocument();
    expect(screen.getByText(/6 days/)).toBeInTheDocument();
  });

  it("uses singular 'day' when 1 day remains", () => {
    setTrialStartedDaysAgo(13); // 1 day remaining
    render(<TrialBanner />);
    expect(screen.getByText(/1 day\b/)).toBeInTheDocument();
  });

  it("disappears once dismissed", () => {
    setTrialStartedDaysAgo(10);
    render(<TrialBanner />);
    expect(screen.getByRole("status")).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText(/dismiss/i));
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("stays hidden when sessionStorage marker is set on mount", () => {
    setTrialStartedDaysAgo(10);
    window.sessionStorage.setItem(TRIAL_BANNER_DISMISSED_KEY, "1");
    const { container } = render(<TrialBanner />);
    // The banner mounts, the effect reads sessionStorage, the
    // component unmounts itself. Net: no status element survives.
    expect(container.querySelector('[role="status"]')).toBeNull();
  });

  it("renders nothing once the trial has expired", () => {
    setTrialStartedDaysAgo(20);
    const { container } = render(<TrialBanner />);
    expect(container).toBeEmptyDOMElement();
  });
});
