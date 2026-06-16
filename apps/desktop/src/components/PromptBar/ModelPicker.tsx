// PromptBar/ModelPicker.tsx — per-chat model selector for API-provider
// runtimes. v2.15.0 Slice C (war_room 0D398F74).
//
// Why this exists: pre-2.15.0 every provider had a hardcoded
// default_model in the registry. The user could override per-runtime
// in Settings → Models, but the chat picker UI never exposed model
// choice. Codex flagged this as the critical user-facing gap. This
// component adds a chip next to the Runtime picker that:
//   - Only renders for API providers (CLI runtimes pick models in
//     their own CLI UX; surfacing a duplicate here would be confusing)
//   - Reads the user's saved model_configs override as the "current"
//   - On open, fetches the live model list via useProviderModels
//   - On select, persists to model_configs via saveModelConfig
//   - Shows a "live" or "curated" badge so users know provenance
//   - Falls back gracefully (loading state, error state) so the picker
//     never blocks a dispatch — if the live fetch errors, the user can
//     still type and the dispatch will use whatever's saved (or the
//     registry default)

import { useState, useMemo } from "react";
import { ChevronDown, ChevronUp, Sparkles, RefreshCw } from "lucide-react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { cn } from "@/lib/utils";
import {
  useProviderModels,
  useRefreshProviderModels,
} from "@/lib/useProviderModels";
import {
  getModelConfig,
  saveModelConfig,
  type ProviderModelInfo,
} from "@/lib/tauri-api";
import { CLI_RUNTIME_MODELS, type RuntimeId } from "@/lib/runtimes";

interface Props {
  /** Provider/runtime slug — works for BOTH API providers (live model
   *  list from the provider) and CLI runtimes (curated list from
   *  CLI_RUNTIME_MODELS). The runtimeKind discriminates. */
  providerSlug: string;
  /**
   * #82 — discriminator between "api" (live fetch via useProviderModels)
   * and "cli" (curated hardcoded list from runtimes.ts). The backend
   * already pipes `--model <id>` for both paths; this just makes the
   * picker show for CLI runtimes too.
   */
  runtimeKind: "api" | "cli";
  /** Active project's id for per-project model overrides (optional). */
  projectId?: string;
  open: boolean;
  setOpen: (b: boolean) => void;
}

export default function ModelPicker({
  providerSlug,
  runtimeKind,
  projectId,
  open,
  setOpen,
}: Props) {
  const qc = useQueryClient();

  // The user's currently-saved override for this runtime. The dispatch
  // wrapper reads the same row to decide which model to send.
  const { data: savedConfig } = useQuery({
    queryKey: ["model-config", providerSlug, projectId ?? null],
    queryFn: () => getModelConfig(providerSlug, projectId),
    staleTime: 30_000,
  });

  // Live model list — only fetched for API providers when the picker is
  // open. CLI runtimes use the curated CLI_RUNTIME_MODELS list (see
  // below) since the binaries don't expose a list endpoint.
  const { data: liveApi, isLoading: liveLoading, error: liveError } =
    useProviderModels(
      runtimeKind === "api" && open ? providerSlug : null
    );

  // #82 — for CLI runtimes, synthesize a `live`-shaped object from the
  // curated list. Source = "curated" so the existing badge UI honestly
  // labels it. liveLoading / liveError stay false for CLI since there's
  // nothing to fetch.
  const cliCurated = useMemo(() => {
    if (runtimeKind !== "cli") return null;
    const models = CLI_RUNTIME_MODELS[providerSlug as RuntimeId];
    if (!models || models.length === 0) return null;
    return {
      source: "curated" as const,
      models: models.map((m) => ({
        id: m.id,
        display_name: m.display,
        owned_by: providerSlug,
      })) as ProviderModelInfo[],
      fallback_reason:
        "Curated CLI model list — binaries don't expose live list endpoints",
    };
  }, [runtimeKind, providerSlug]);

  const live = liveApi ?? cliCurated;
  const isLoading = runtimeKind === "api" ? liveLoading : false;
  const error = runtimeKind === "api" ? liveError : null;

  // Refresh only meaningful for API providers (force-refetch the live
  // list). For CLI we keep the curated list as-is.
  const refresh = useRefreshProviderModels(providerSlug);

  const setMutation = useMutation({
    mutationFn: (modelId: string) =>
      saveModelConfig(providerSlug, modelId, projectId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["model-config", providerSlug, projectId ?? null] });
      setOpen(false);
    },
  });

  const currentId = savedConfig?.modelId ?? null;

  return (
    <div className="relative shrink-0">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        className="flex items-center gap-1 px-2 py-1.5 rounded-lg border border-cs-border hover:border-opacity-60 transition-colors text-xs"
        title="Pick the model this provider will use for chat dispatches"
      >
        <Sparkles size={12} className="text-cs-muted" />
        <span className="font-mono text-cs-text">
          {currentId ?? "default"}
        </span>
        {open ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
      </button>

      {open && (
        <>
          {/* Backdrop to close on outside-click — mirrors RuntimePicker. */}
          <div
            className="fixed inset-0 z-40"
            onClick={() => setOpen(false)}
          />
          <div className="absolute right-0 bottom-full mb-1 z-50 w-72 rounded-lg border border-cs-border bg-cs-card shadow-xl overflow-hidden">
            <div className="flex items-center justify-between px-3 py-2 border-b border-cs-border bg-cs-bg-raised">
              <span className="text-[11px] font-medium text-cs-muted uppercase tracking-wide">
                {providerSlug} models
              </span>
              <button
                type="button"
                onClick={() => refresh.mutate()}
                disabled={refresh.isPending}
                hidden={runtimeKind === "cli"}
                className="flex items-center gap-1 text-[10px] text-cs-muted hover:text-cs-text disabled:opacity-50"
                title="Bypass the cache and re-fetch live from the provider"
              >
                <RefreshCw
                  size={10}
                  className={cn(refresh.isPending && "animate-spin")}
                />
                {refresh.isPending ? "fetching..." : "pull live"}
              </button>
            </div>

            {/* Source badge — honest provenance per codex's verdict */}
            {live && (
              <div className="px-3 py-1.5 text-[10px] border-b border-cs-border bg-cs-bg/50">
                <span
                  className={cn(
                    "inline-block px-1.5 py-0.5 rounded font-mono",
                    live.source === "live"
                      ? "bg-emerald-500/15 text-emerald-300"
                      : "bg-amber-500/15 text-amber-300"
                  )}
                >
                  {live.source === "live" ? "live" : "curated"}
                </span>
                {live.fallback_reason && (
                  <span className="ml-2 text-cs-muted/80">
                    {live.fallback_reason}
                  </span>
                )}
              </div>
            )}

            <div className="max-h-72 overflow-y-auto py-1">
              {isLoading && (
                <div className="px-3 py-3 text-xs text-cs-muted">
                  Fetching live model list…
                </div>
              )}
              {error && (
                <div className="px-3 py-3 text-xs text-red-400">
                  Couldn't fetch live models: {(error as Error).message}
                </div>
              )}
              {live &&
                live.models.map((m: ProviderModelInfo) => {
                  const isSelected = m.id === currentId;
                  // v2.15.0 codex nit (war_room E15DEA48): provider-curated
                  // display_name (e.g. Google's "Gemini 3 Flash Preview")
                  // is the user-facing label; m.id stays as monospace
                  // metadata so users can still see the wire identifier
                  // they'd pass via --model.
                  const hasDistinctDisplayName =
                    m.display_name && m.display_name !== m.id;
                  return (
                    <button
                      key={m.id}
                      type="button"
                      onClick={() => setMutation.mutate(m.id)}
                      disabled={setMutation.isPending}
                      className={cn(
                        "w-full flex flex-col items-start gap-0.5 px-3 py-2 text-xs text-left transition-colors disabled:opacity-50",
                        isSelected
                          ? "bg-cs-accent/10 text-cs-accent"
                          : "hover:bg-cs-bg text-cs-text"
                      )}
                    >
                      <div className="w-full flex items-center justify-between gap-2">
                        <span className="truncate font-medium">
                          {hasDistinctDisplayName ? m.display_name : m.id}
                        </span>
                        {m.owned_by && (
                          <span className="text-[10px] text-cs-muted/80 shrink-0">
                            {m.owned_by}
                          </span>
                        )}
                      </div>
                      {hasDistinctDisplayName && (
                        <span className="font-mono text-[10px] text-cs-muted/70 truncate">
                          {m.id}
                        </span>
                      )}
                    </button>
                  );
                })}
              {live && live.models.length === 0 && (
                <div className="px-3 py-3 text-xs text-cs-muted">
                  No models returned for this key.
                </div>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  );
}
