// v2.7.7 — shared enabled-runtimes hook.
//
// Backed by the `list_available_runtimes` Tauri command (CLI runtimes +
// API providers with active keys, unified). Lives behind one React
// Query key so multiple surfaces (PromptBar, FirstChatWizard, any
// future picker) share a single fetch and stay in sync — adding a key
// in Settings triggers `queryClient.invalidateQueries(["enabled-
// runtimes"])` and every subscriber refetches.
//
// Replaces:
//   - PromptBar's useState + useEffect that called `list_available_
//     runtimes` directly with no cache (re-fetched on every PromptBar
//     mount).
//   - FirstChatWizard's two separate useQueries (`["agent-statuses"]`
//     + `["llm-api-keys"]`) + computeEnabledRuntimes composition (now
//     done backend-side by `list_available_runtimes`).
//
// 2026-05-19 war-room (claude + codex unanimous) called for this.

import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";

export interface EnabledRuntimeRow {
  slug: string;
  label: string;
  /** "cli" for binary-detected runtimes (claude/codex/gemini/openclaw/
   *  hermes), "api" for providers reached via stored API key (minimax/
   *  grok/deepseek/qwen/openrouter/anthropic/google). */
  kind: "cli" | "api";
  /** Whether the runtime is currently dispatchable. CLI: binary on
   *  PATH. API: at least one active key for the provider. */
  available: boolean;
  /** "ok" / "no_binary" / "no_key" — short token, displayable in a
   *  status pill if needed. */
  reason: string;
}

const isTauri =
  typeof window !== "undefined" &&
  ("__TAURI__" in window || "__TAURI_INTERNALS__" in window);

/** Subscribe to the unified runtime availability list.
 *
 *  Both PromptBar's runtime picker and FirstChatWizard's seat-counter
 *  read from this. The query is disabled in non-Tauri builds (test
 *  / Storybook / vite preview) — consumers should handle the empty
 *  array gracefully and fall back to a hardcoded list if they need
 *  one. */
export function useEnabledRuntimes() {
  return useQuery<EnabledRuntimeRow[]>({
    queryKey: ["enabled-runtimes"],
    queryFn: () => invoke<EnabledRuntimeRow[]>("list_available_runtimes"),
    enabled: isTauri,
    // Runtime availability rarely flips mid-session (a binary doesn't
    // appear/disappear; keys are added in Settings which already
    // invalidates this key). 60s stale window is generous.
    staleTime: 60_000,
  });
}
