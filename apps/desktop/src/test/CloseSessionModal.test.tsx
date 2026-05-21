import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import CloseSessionModal from "@/components/SessionsList/CloseSessionModal";
import * as tauriApi from "@/lib/tauri-api";

// v2.7.12 — pre-close modal. Pins the user-visible contract:
//   - renders nothing when open=false
//   - submit returns the trimmed coordinator + humanComment (null when empty)
//   - cancel closes without invoking onSubmit
//   - coordinator picker is populated from listLlmApiKeys() and only
//     shows providers the user has keys for

vi.mock("@/lib/tauri-api", async (orig) => {
  const actual = (await orig()) as Record<string, unknown>;
  return {
    ...actual,
    listLlmApiKeys: vi.fn(),
  };
});

function renderModal(props: Partial<React.ComponentProps<typeof CloseSessionModal>> = {}) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onCancel = props.onCancel ?? vi.fn();
  const onSubmit = props.onSubmit ?? vi.fn();
  const utils = render(
    <QueryClientProvider client={qc}>
      <CloseSessionModal open={true} onCancel={onCancel} onSubmit={onSubmit} {...props} />
    </QueryClientProvider>,
  );
  return { ...utils, onCancel, onSubmit };
}

describe("CloseSessionModal (S12 v2.7.12)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(tauriApi.listLlmApiKeys).mockResolvedValue([]);
  });

  it("renders nothing when open=false", () => {
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { container } = render(
      <QueryClientProvider client={qc}>
        <CloseSessionModal open={false} onCancel={vi.fn()} onSubmit={vi.fn()} />
      </QueryClientProvider>,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders the dialog when open=true with both fields visible", () => {
    renderModal();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByLabelText(/coordinator/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/your note/i)).toBeInTheDocument();
  });

  it("submit with empty fields passes null for both", () => {
    const { onSubmit } = renderModal();
    fireEvent.click(screen.getByRole("button", { name: /^close session$/i }));
    expect(onSubmit).toHaveBeenCalledOnce();
    expect(onSubmit).toHaveBeenCalledWith({
      coordinator: null,
      humanComment: null,
    });
  });

  it("submit trims the human comment and treats whitespace-only as null", () => {
    const { onSubmit } = renderModal();
    fireEvent.change(screen.getByLabelText(/your note/i), {
      target: { value: "   \n\t  " },
    });
    fireEvent.click(screen.getByRole("button", { name: /^close session$/i }));
    expect(onSubmit).toHaveBeenCalledWith({
      coordinator: null,
      humanComment: null,
    });
  });

  it("submit passes the trimmed non-empty human comment", () => {
    const { onSubmit } = renderModal();
    fireEvent.change(screen.getByLabelText(/your note/i), {
      target: { value: "  hi there  " },
    });
    fireEvent.click(screen.getByRole("button", { name: /^close session$/i }));
    expect(onSubmit).toHaveBeenCalledWith({
      coordinator: null,
      humanComment: "hi there",
    });
  });

  it("coordinator picker shows only providers with configured keys", async () => {
    vi.mocked(tauriApi.listLlmApiKeys).mockResolvedValue([
      // @ts-expect-error — partial shape is enough for the picker filter.
      { provider: "anthropic", name: "personal", isActive: true },
      // @ts-expect-error
      { provider: "minimax", name: "personal", isActive: true },
    ]);
    renderModal();
    const select = screen.getByLabelText(/coordinator/i) as HTMLSelectElement;
    await waitFor(() => {
      const options = Array.from(select.options).map((o) => o.value);
      // Default (empty value) + only the two providers with keys.
      expect(options).toContain("anthropic");
      expect(options).toContain("minimax");
      expect(options).not.toContain("google");
      expect(options).not.toContain("grok");
    });
  });

  it("cancel button calls onCancel and not onSubmit", () => {
    const { onCancel, onSubmit } = renderModal();
    // Two elements share an aria-label of "Cancel" (header X icon +
    // footer text button). Grab the text button explicitly.
    const buttons = screen.getAllByRole("button", { name: /^cancel$/i });
    const textBtn = buttons.find((b) => b.textContent?.trim() === "Cancel");
    expect(textBtn).toBeTruthy();
    fireEvent.click(textBtn!);
    expect(onCancel).toHaveBeenCalledOnce();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("submit button is disabled while busy=true", () => {
    renderModal({ busy: true });
    const submitBtn = screen.getByRole("button", { name: /closing/i });
    expect(submitBtn).toBeDisabled();
  });
});
