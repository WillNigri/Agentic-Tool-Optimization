import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import "../i18n";
import EmptyMcpsWarning from "@/components/CreateAgentWizard/EmptyMcpsWarning";
import QuickPath from "@/components/CreateAgentWizard/QuickPath";
import { useUiStore } from "@/stores/useUiStore";
import * as api from "@/lib/api";

// Felipe P6 — the warning + Browse-catalog link must appear when the user
// reaches the MCP-selection field on an empty MCP list. The wizard's MCP
// MultiSelect collapses the empty state into a "No MCP servers configured"
// hint that's only visible after a chevron click — too easy for a new user
// to miss. The warning surfaces the same fact up front and gives them a
// path forward.

vi.mock("@/lib/api", () => ({
  getMcpServers: vi.fn().mockResolvedValue([]),
  getSkills: vi.fn().mockResolvedValue([]),
  listProjects: vi.fn().mockResolvedValue([]),
}));

function renderWarning(onClose = vi.fn()) {
  return render(<EmptyMcpsWarning onClose={onClose} />);
}

function renderQuickPath() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onCancel = vi.fn();
  const utils = render(
    <QueryClientProvider client={qc}>
      <QuickPath onCancel={onCancel} />
    </QueryClientProvider>,
  );
  return { ...utils, onCancel };
}

describe("EmptyMcpsWarning (Felipe P6)", () => {
  beforeEach(() => {
    // Reset the UI store so cross-test setSection calls don't leak.
    useUiStore.setState({ section: "home", subTabs: {} });
  });

  it("renders title and explainer copy", () => {
    renderWarning();
    expect(screen.getByText("No MCPs registered")).toBeInTheDocument();
    expect(screen.getByText(/gmail, github, filesystem/i)).toBeInTheDocument();
  });

  it("Browse catalog routes to Skills → MCPs and closes the wizard", () => {
    const onClose = vi.fn();
    renderWarning(onClose);
    fireEvent.click(screen.getByRole("button", { name: /Browse catalog/i }));
    expect(useUiStore.getState().section).toBe("skills");
    expect(useUiStore.getState().subTabs["ato.subtab.skills"]).toBe("mcps");
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("Skip for now dismisses the warning", () => {
    renderWarning();
    expect(screen.getByTestId("empty-mcps-warning")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Skip for now/i }));
    expect(screen.queryByTestId("empty-mcps-warning")).not.toBeInTheDocument();
  });
});

describe("QuickPath empty-MCPs integration (Felipe P6)", () => {
  beforeEach(() => {
    useUiStore.setState({ section: "home", subTabs: {} });
    vi.mocked(api.getMcpServers).mockResolvedValue([]);
  });

  it("renders the warning and the Browse catalog link when no MCPs exist", async () => {
    renderQuickPath();
    // The MCP query resolves async; wait for the warning to land.
    expect(await screen.findByTestId("empty-mcps-warning")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Browse catalog/i })).toBeInTheDocument();
  });

  it("does not render the warning when at least one MCP is registered", async () => {
    vi.mocked(api.getMcpServers).mockResolvedValue([
      // Shape matches McpServer enough for the MultiSelect's `m.name` / `m.transport` reads.
      { name: "gmail", transport: "stdio" } as never,
    ]);
    renderQuickPath();
    // Wait for the form to settle, then assert no warning rendered.
    await waitFor(() =>
      expect(screen.queryByTestId("empty-mcps-warning")).not.toBeInTheDocument(),
    );
  });
});
