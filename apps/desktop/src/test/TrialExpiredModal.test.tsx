import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";

import TrialExpiredModal from "@/components/Trial/TrialExpiredModal";
import { TRIAL_STARTED_AT_KEY } from "@/lib/trial";

function setTrialStartedDaysAgo(days: number) {
  const past = new Date(Date.now() - days * 24 * 60 * 60 * 1000);
  window.localStorage.setItem(TRIAL_STARTED_AT_KEY, past.toISOString());
}

describe("<TrialExpiredModal />", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("renders nothing when open=false", () => {
    setTrialStartedDaysAgo(20);
    const { container } = render(
      <TrialExpiredModal open={false} onClose={() => {}} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("renders nothing when the trial is still active", () => {
    setTrialStartedDaysAgo(3);
    const { container } = render(
      <TrialExpiredModal open={true} onClose={() => {}} />,
    );
    // Defensive: even if a caller opens us by mistake, we don't
    // confuse the user with an "expired" modal on day 3.
    expect(container).toBeEmptyDOMElement();
  });

  it("renders the dialog when open=true and the trial is expired", () => {
    setTrialStartedDaysAgo(20);
    render(<TrialExpiredModal open={true} onClose={() => {}} />);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText(/Pro trial has ended/i)).toBeInTheDocument();
  });

  it("calls onClose when the dismiss button is clicked", () => {
    setTrialStartedDaysAgo(20);
    const onClose = vi.fn();
    render(<TrialExpiredModal open={true} onClose={onClose} />);
    fireEvent.click(screen.getByRole("button", { name: /not now/i }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("calls onClose when the X button is clicked", () => {
    setTrialStartedDaysAgo(20);
    const onClose = vi.fn();
    render(<TrialExpiredModal open={true} onClose={onClose} />);
    fireEvent.click(screen.getByRole("button", { name: /close/i }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("does NOT close when the inner card is clicked", () => {
    setTrialStartedDaysAgo(20);
    const onClose = vi.fn();
    render(<TrialExpiredModal open={true} onClose={onClose} />);
    // Click inside the dialog content — backdrop handler should ignore.
    fireEvent.click(screen.getByText(/Pro trial has ended/i));
    expect(onClose).not.toHaveBeenCalled();
  });
});
