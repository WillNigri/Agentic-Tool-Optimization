// PromptBar/RuntimePicker.tsx — runtime selector popover.
//
// Extracted from PromptBar/index.tsx 2026-05-19 (v2.7.7 frontend
// elegance push). Owns the trigger button, popover JSX, and the
// runtime list rendering. The orchestrator passes:
//
//   - runtime / setRuntime: current selection + setter
//   - availableRuntimes:    unified CLI + API list from
//                           useEnabledRuntimes(); null in non-Tauri
//                           contexts (we fall back to the legacy
//                           hardcoded CLI list).
//   - open / setOpen:       picker popover state lives in the
//                           orchestrator's `openPicker` union so only
//                           one popover can be open at a time.
//
// Why not own `open` internally (like RoomTypePicker): the runtime
// picker shares the `openPicker` mutex with the agent + thread
// pickers. Internal-state would re-introduce the latent backdrop-
// stacking bug the 2026-05-19 war-room called out.

import { Globe } from "lucide-react";

import { cn } from "@/lib/utils";

import type { AvailableRuntimeRow } from "./_helpers";
import { RUNTIME_META, RUNTIME_OPTIONS } from "./_helpers";
import { PROVIDER_TO_RUNTIME } from "@/lib/runtimes";
import type { AgentRuntime } from "@/components/cron/types";

interface Props {
  runtime: AgentRuntime;
  setRuntime: (rt: AgentRuntime) => void;
  /** From `useEnabledRuntimes()` (shared cache). `null` outside Tauri
   *  builds — we fall back to the legacy CLI-only hardcoded list. */
  availableRuntimes: AvailableRuntimeRow[] | null;
  open: boolean;
  setOpen: (next: boolean) => void;
}

export function RuntimePicker({
  runtime,
  setRuntime,
  availableRuntimes,
  open,
  setOpen,
}: Props) {
  const currentRuntime = RUNTIME_OPTIONS.find((r) => r.id === runtime)!;
  const RuntimeIcon = currentRuntime.icon;

  return (
    <div className="relative shrink-0">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        data-demo-id="runtime-picker"
        className="flex items-center gap-1 px-2 py-1.5 rounded-lg border border-cs-border hover:border-opacity-60 transition-colors"
        style={{ borderColor: `${currentRuntime.color}40` }}
      >
        <RuntimeIcon size={12} style={{ color: currentRuntime.color }} />
        <span
          className="text-[10px] font-medium"
          style={{ color: currentRuntime.color }}
        >
          {currentRuntime.label}
        </span>
      </button>

      {open && (
        <>
          <div
            className="fixed inset-0 z-30"
            onClick={() => setOpen(false)}
          />
          <div className="absolute bottom-full left-0 mb-1 w-44 rounded-lg border border-cs-border bg-cs-card shadow-xl z-40 overflow-hidden">
            {(() => {
              // Use the queried list when available, else the legacy
              // 4-CLI hardcoded list. Filter to available rows; render
              // API providers as a separate group with a clear
              // "subscription via API key" tooltip.
              const rows: AvailableRuntimeRow[] = availableRuntimes
                ? availableRuntimes.filter((r) => r.available)
                : RUNTIME_OPTIONS.map((o) => ({
                    slug: o.id,
                    label: o.label,
                    kind: "cli" as const,
                    available: true,
                    reason: "ok",
                  }));
              const cliRows = rows.filter((r) => r.kind === "cli");
              const apiRows = rows.filter((r) => r.kind === "api");
              const renderRow = (r: AvailableRuntimeRow) => {
                const meta = RUNTIME_META[r.slug] ?? {
                  label: r.label,
                  icon: Globe,
                  color: "#888",
                };
                const Icon = meta.icon;
                const isApi = r.kind === "api";
                return (
                  <button
                    key={r.slug}
                    type="button"
                    onClick={() => {
                      // v2.14.2 — API provider slugs ("google", "anthropic",
                      // "openai") aren't direct RUNTIME_OPTIONS keys (those
                      // are "gemini", "claude", "codex"). Alias before set
                      // so downstream consumers (PromptBar's currentRuntime
                      // lookup, chat history filters) get a known id.
                      const aliased = PROVIDER_TO_RUNTIME[r.slug] ?? r.slug;
                      setRuntime(aliased as AgentRuntime);
                      setOpen(false);
                    }}
                    title={
                      isApi
                        ? `${meta.label} — API provider (subscription via API key)`
                        : meta.label
                    }
                    className={cn(
                      "w-full flex items-center gap-2 px-3 py-2 text-xs transition-colors",
                      runtime === r.slug ? "bg-cs-accent/5" : "hover:bg-cs-bg",
                    )}
                  >
                    <Icon size={12} style={{ color: meta.color }} />
                    <span
                      className="flex-1 text-left"
                      style={{ color: runtime === r.slug ? meta.color : undefined }}
                    >
                      {meta.label}
                    </span>
                    {isApi ? (
                      <span className="text-[9px] uppercase tracking-wide text-cs-muted">
                        API
                      </span>
                    ) : null}
                  </button>
                );
              };
              return (
                <>
                  {cliRows.map(renderRow)}
                  {apiRows.length > 0 ? (
                    <div className="px-3 pt-2 pb-1 text-[9px] uppercase tracking-wide text-cs-muted border-t border-cs-border">
                      API providers
                    </div>
                  ) : null}
                  {apiRows.map(renderRow)}
                </>
              );
            })()}
          </div>
        </>
      )}
    </div>
  );
}
