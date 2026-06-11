// v2.15.0 Slice C — shared data hook for live model lists.
//
// Per war_room 0D398F74 codex verdict: "use one shared data hook and
// normalization layer, not one shared UI component. PromptBar, Settings,
// and agent config have different density and affordance needs."
//
// Backend caches in-process for 10 min (ato-list-models). React Query
// keeps a 1h UI cache in front of that. `noCache: true` bypasses both
// for explicit user-triggered refreshes (Settings → Models "Pull live"
// button + any agent-config refresh UI).

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  listProviderModels,
  type ProviderModelListResponse,
} from "./tauri-api";

const STALE_TIME_MS = 60 * 60 * 1000; // 1h — matches the codex-suggested UI cache window.

/**
 * Fetch the live model list for a provider. Enabled only when `slug` is
 * provided AND the user has a stored API key (the backend errors out
 * cleanly if the key isn't there; we surface that through React Query's
 * error state so the picker can show "add a key first").
 */
export function useProviderModels(
  slug: string | null | undefined,
  opts?: { enabled?: boolean }
) {
  const enabled = (opts?.enabled ?? true) && !!slug;
  return useQuery<ProviderModelListResponse, Error>({
    queryKey: ["provider-models", slug],
    queryFn: () => listProviderModels(slug as string),
    enabled,
    staleTime: STALE_TIME_MS,
    retry: 1, // The error is usually deterministic (no key / wrong key); don't hammer.
  });
}

/**
 * Force a fresh fetch from the provider, bypassing both the backend's
 * 10-min in-process cache and React Query's 1h UI cache. Used by the
 * "Pull live" button so a user-triggered refresh is honest about being
 * a real network call.
 */
export function useRefreshProviderModels(slug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => listProviderModels(slug, { noCache: true }),
    onSuccess: (data) => {
      // Replace the cached query data with the fresh result so the
      // dropdown re-renders without another round-trip.
      qc.setQueryData(["provider-models", slug], data);
    },
  });
}
