import { create } from "zustand";

// v1.3.0 — Tiny global state for the bottom Terminal pane (T5).
// Lets components elsewhere (e.g., the agent wizard's Connect step) request
// that the user be sent to an interactive Claude session — the pane subscribes
// and acts on the request when mounted.

export type TerminalRequest = {
  kind: "open-shell";
  /** Sent into the PTY with a trailing newline. */
  initialCommand?: string;
  /** Sent without a newline a moment AFTER `initialCommand`. Use for
   *  "queue this @mention so the user can finish typing" flows. */
  followUpKeys?: string;
  /** Delay between initialCommand and followUpKeys. Defaults to 1500ms so
   *  long-booting CLIs (claude) have time to print their prompt. */
  followUpDelayMs?: number;
  id: string;
};

interface TerminalState {
  open: boolean;
  pendingRequest: TerminalRequest | null;
  setOpen: (open: boolean) => void;
  requestShell: (
    initialCommand?: string,
    options?: { followUpKeys?: string; followUpDelayMs?: number }
  ) => void;
  clearRequest: () => void;
}

export const useTerminalStore = create<TerminalState>((set) => ({
  open: false,
  pendingRequest: null,
  setOpen: (open) => set({ open }),
  requestShell: (initialCommand, options) =>
    set({
      open: true,
      pendingRequest: {
        kind: "open-shell",
        initialCommand,
        followUpKeys: options?.followUpKeys,
        followUpDelayMs: options?.followUpDelayMs,
        id: `${Date.now()}`,
      },
    }),
  clearRequest: () => set({ pendingRequest: null }),
}));
