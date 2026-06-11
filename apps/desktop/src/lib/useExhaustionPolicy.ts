// v2.15.3 — exhaustion-policy + fallback-order React Query hooks.
//
// Per war_room 27522371 (codex):
//   - One shared data hook; UI components consume the same source of truth.
//   - Two-stage consent: setExhaustionPolicy takes a `confirmAutoSwap`
//     boolean; the backend persists `authorized_auto_swap_at` only when
//     policy=fallback-chain AND the caller passed confirm=true.
//   - Fallback chain runtimes filtered to dispatchable (available=true)
//     by the consumer; this hook just returns the saved order.

import { invoke } from "@tauri-apps/api/core";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";

export type ExhaustionPolicy =
  | "ask"
  | "stop-and-notify"
  | "fallback-chain"
  | "pause-and-wake";

export interface ExhaustionPolicyState {
  policy: ExhaustionPolicy;
  /** RFC3339 timestamp when the user clicked through the consent prompt.
   *  NULL unless policy=fallback-chain AND consent was given. */
  authorized_auto_swap_at: string | null;
}

const POLICY_KEY = ["settings", "exhaustion_policy"] as const;
const ORDER_KEY = ["settings", "exhaustion_fallback_order"] as const;

export function useExhaustionPolicy() {
  return useQuery<ExhaustionPolicyState, Error>({
    queryKey: POLICY_KEY,
    queryFn: () => invoke<ExhaustionPolicyState>("get_exhaustion_policy"),
    staleTime: 30_000,
  });
}

export function useSetExhaustionPolicy() {
  const qc = useQueryClient();
  return useMutation<
    ExhaustionPolicyState,
    Error,
    { policy: ExhaustionPolicy; confirmAutoSwap: boolean }
  >({
    mutationFn: ({ policy, confirmAutoSwap }) =>
      invoke<ExhaustionPolicyState>("set_exhaustion_policy", {
        policy,
        confirmAutoSwap,
      }),
    onSuccess: (data) => {
      qc.setQueryData(POLICY_KEY, data);
    },
  });
}

export function useFallbackOrder() {
  return useQuery<string[], Error>({
    queryKey: ORDER_KEY,
    queryFn: () => invoke<string[]>("get_fallback_order"),
    staleTime: 30_000,
  });
}

export function useSetFallbackOrder() {
  const qc = useQueryClient();
  return useMutation<string[], Error, string[]>({
    mutationFn: (slugs) => invoke<string[]>("set_fallback_order", { slugs }),
    onSuccess: (data) => {
      qc.setQueryData(ORDER_KEY, data);
    },
  });
}
