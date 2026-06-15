// Unit tests for AppendTurnComposer.
//
// Uses @testing-library/react with fireEvent (no user-event; package
// not installed in this repo — mirrors MissionBoard.test.tsx pattern).
//
// Mocks:
//   • react-i18next → pass-through t()
//   • @/lib/cloud-api → spy on appendTeamEvent

import { render, screen, fireEvent, waitFor, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import AppendTurnComposer from "./AppendTurnComposer";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_key: string, opts?: { defaultValue?: string }) =>
      opts?.defaultValue ?? _key,
  }),
  initReactI18next: { type: "3rdParty", init: () => {} },
}));

// Spy on appendTeamEvent. Default: resolves with seq_num=1.
const mockAppendTeamEvent = vi.fn().mockResolvedValue({
  seq_num: 1,
  created_at: new Date().toISOString(),
});

vi.mock("@/lib/cloud-api", async (importOriginal) => {
  const original = await importOriginal<typeof import("@/lib/cloud-api")>();
  return {
    ...original,
    appendTeamEvent: (...args: unknown[]) => mockAppendTeamEvent(...args),
  };
});

const defaultProps = {
  teamId: "team-abc",
  kind: "session" as const,
  resourceId: "res-123",
  isE2e: false,
};

describe("<AppendTurnComposer />", () => {
  beforeEach(() => {
    mockAppendTeamEvent.mockClear();
  });

  it("renders textarea enabled when isE2e=false", () => {
    render(<AppendTurnComposer {...defaultProps} />);
    const textarea = screen.getByTestId("append-turn-textarea");
    expect(textarea).not.toBeDisabled();
  });

  it("renders textarea disabled when isE2e=true", () => {
    render(<AppendTurnComposer {...defaultProps} isE2e />);
    const textarea = screen.getByTestId("append-turn-textarea");
    expect(textarea).toBeDisabled();
  });

  it("shows the E2E hint banner when isE2e=true", () => {
    render(<AppendTurnComposer {...defaultProps} isE2e />);
    expect(
      screen.getByText(/Encrypted append shipping in Wave 3/i),
    ).toBeInTheDocument();
  });

  it("does not show E2E hint when isE2e=false", () => {
    render(<AppendTurnComposer {...defaultProps} />);
    expect(
      screen.queryByText(/Encrypted append shipping in Wave 3/i),
    ).not.toBeInTheDocument();
  });

  it("calls appendTeamEvent with correct args on Send click", async () => {
    render(<AppendTurnComposer {...defaultProps} />);

    const textarea = screen.getByTestId("append-turn-textarea");
    fireEvent.change(textarea, { target: { value: "Hello team" } });

    const button = screen.getByRole("button", { name: /Send turn/i });
    await act(async () => {
      fireEvent.click(button);
    });

    await waitFor(() => {
      expect(mockAppendTeamEvent).toHaveBeenCalledOnce();
    });

    const [teamId, kind, resourceId, body] = mockAppendTeamEvent.mock.calls[0];
    expect(teamId).toBe("team-abc");
    expect(kind).toBe("session");
    expect(resourceId).toBe("res-123");
    expect(body).toMatchObject({
      event_kind: "turn_appended",
      payload_json: { role: "user", text: "Hello team" },
      surface: "desktop",
    });
  });

  it("clears textarea after successful send", async () => {
    render(<AppendTurnComposer {...defaultProps} />);

    const textarea = screen.getByTestId("append-turn-textarea");
    fireEvent.change(textarea, { target: { value: "Test message" } });

    const button = screen.getByRole("button", { name: /Send turn/i });
    await act(async () => {
      fireEvent.click(button);
    });

    await waitFor(() => {
      expect((textarea as HTMLTextAreaElement).value).toBe("");
    });
  });

  it("does not call appendTeamEvent when isE2e=true", async () => {
    render(<AppendTurnComposer {...defaultProps} isE2e />);

    const button = screen.getByRole("button", { name: /Send turn/i });
    await act(async () => {
      fireEvent.click(button);
    });

    expect(mockAppendTeamEvent).not.toHaveBeenCalled();
  });

  it("shows an error message when appendTeamEvent rejects", async () => {
    mockAppendTeamEvent.mockRejectedValueOnce(new Error("Network error"));
    render(<AppendTurnComposer {...defaultProps} />);

    const textarea = screen.getByTestId("append-turn-textarea");
    fireEvent.change(textarea, { target: { value: "Failing message" } });

    const button = screen.getByRole("button", { name: /Send turn/i });
    await act(async () => {
      fireEvent.click(button);
    });

    await waitFor(() => {
      expect(screen.getByText("Network error")).toBeInTheDocument();
    });
  });

  it("sends on Cmd+Enter keyboard shortcut", async () => {
    render(<AppendTurnComposer {...defaultProps} />);

    const textarea = screen.getByTestId("append-turn-textarea");
    fireEvent.change(textarea, { target: { value: "Keyboard send" } });

    await act(async () => {
      fireEvent.keyDown(textarea, { key: "Enter", metaKey: true });
    });

    await waitFor(() => {
      expect(mockAppendTeamEvent).toHaveBeenCalledOnce();
    });
  });
});
