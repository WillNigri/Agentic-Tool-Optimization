import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import "../i18n";

// Felipe P4 (S9) — Run button now dispatches via RunAgentDialog;
// the interactive shell moves to a secondary "Interactive session"
// link. Auto-fire kicks in when the agent has a stored
// default_prompt. These tests pin the swap so a future "make Run a
// shell again" refactor trips the suite immediately.

// vi.mock calls are hoisted to the top of the file; nothing
// referenced from the factories may be declared in module scope.
vi.mock("@/lib/agents", async (orig) => {
  const actual = await orig<typeof import("@/lib/agents")>();
  return {
    ...actual,
    listAgents: vi.fn(),
    deleteAgent: vi.fn().mockResolvedValue(undefined),
    touchAgentLastUsed: vi.fn().mockResolvedValue(undefined),
    getAgentDefaultPrompt: vi.fn(),
  };
});

vi.mock("@/lib/agentVariables", () => ({
  promptAgentWithContext: vi.fn(),
}));

import * as agentsLib from "@/lib/agents";
import * as agentVariables from "@/lib/agentVariables";
import { useTerminalStore } from "@/stores/useTerminalStore";
import MyAgentsList from "@/components/MyAgentsList/MyAgentsList";

const baseAgent: agentsLib.Agent = {
  id: "agent-1",
  slug: "reviewer",
  displayName: "Reviewer",
  description: "Code reviewer",
  runtime: "claude",
  model: "sonnet",
  projectId: null,
  systemPrompt: null,
  permissions: null,
  skills: null,
  mcps: null,
  goal: null,
  filePath: null,
  createdAt: "2026-05-01T00:00:00Z",
  lastUsedAt: null,
};

function renderList() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <MyAgentsList />
    </QueryClientProvider>,
  );
}

describe("MyAgentsList — Run vs Interactive session swap (Felipe P4)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(agentsLib.listAgents).mockResolvedValue([baseAgent]);
    vi.mocked(agentsLib.getAgentDefaultPrompt).mockResolvedValue(null);
    vi.mocked(agentVariables.promptAgentWithContext).mockResolvedValue(
      "dispatched response",
    );
    // jsdom doesn't implement scrollTo; RunAgentDialog's auto-scroll
    // effect calls it on every turn render and throws otherwise.
    if (!("scrollTo" in Element.prototype)) {
      Object.defineProperty(Element.prototype, "scrollTo", {
        value: vi.fn(),
        writable: true,
        configurable: true,
      });
    }
    // Reset the terminal store between tests so a prior requestShell
    // call doesn't bleed into the "Run doesn't open shell" assertion.
    useTerminalStore.setState({ open: false, pendingRequest: null });
  });

  it("Run opens the dispatch dialog (does not request a shell)", async () => {
    renderList();
    const runBtn = await screen.findByTestId("agent-run-reviewer");

    fireEvent.click(runBtn);

    // The dispatch dialog mounts (Send button is its tell).
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: /send/i }),
      ).toBeInTheDocument(),
    );
    // And no shell was requested.
    expect(useTerminalStore.getState().pendingRequest).toBeNull();
  });

  it("Run auto-fires the dispatch when the agent has a default_prompt", async () => {
    vi.mocked(agentsLib.getAgentDefaultPrompt).mockResolvedValue(
      "Review my latest PR",
    );

    renderList();
    const runBtn = await screen.findByTestId("agent-run-reviewer");
    fireEvent.click(runBtn);

    await waitFor(() => {
      expect(agentVariables.promptAgentWithContext).toHaveBeenCalledTimes(1);
    });
    const callArg = vi.mocked(agentVariables.promptAgentWithContext).mock
      .calls[0][0];
    expect(callArg.agentId).toBe("agent-1");
    expect(callArg.runtime).toBe("claude");
    // The Claude prompt builder wraps the user text with an @-mention
    // (@reviewer). What matters here is that the auto-fired text from
    // default_prompt made it through, not the exact wrapper shape.
    expect(callArg.prompt).toContain("Review my latest PR");
  });

  it("Run does NOT auto-fire when no default_prompt is set", async () => {
    vi.mocked(agentsLib.getAgentDefaultPrompt).mockResolvedValue(null);

    renderList();
    const runBtn = await screen.findByTestId("agent-run-reviewer");
    fireEvent.click(runBtn);

    // The dialog opens and waits for manual input.
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /send/i })).toBeInTheDocument(),
    );
    // Give any pending auto-fire microtask a chance to run, then
    // assert nothing dispatched.
    await new Promise((r) => setTimeout(r, 20));
    expect(agentVariables.promptAgentWithContext).not.toHaveBeenCalled();
  });

  it("Interactive session opens the embedded shell (and not the dispatch dialog)", async () => {
    renderList();

    // Expand the row to expose the secondary "Interactive session"
    // link (it lives under the chevron, not on the row header). The
    // row toggle uses data-demo-id, not data-testid.
    await screen.findByTestId("agent-run-reviewer"); // wait for list to render
    const rowToggle = document.querySelector(
      "[data-demo-id='agent-row-reviewer']",
    ) as HTMLElement | null;
    expect(rowToggle).not.toBeNull();
    fireEvent.click(rowToggle!);
    const interactiveLink = await screen.findByTestId(
      "agent-interactive-reviewer",
    );

    fireEvent.click(interactiveLink);

    // Terminal store should now hold a pending shell command for
    // claude (the per-runtime capability matrix decides the exact
    // string; we just assert the shape).
    await waitFor(() => {
      const req = useTerminalStore.getState().pendingRequest;
      expect(req).not.toBeNull();
      expect(req?.initialCommand ?? "").toMatch(/claude/i);
    });
    // And no dispatch dialog appeared.
    expect(
      screen.queryByRole("button", { name: /send/i }),
    ).not.toBeInTheDocument();
  });
});
