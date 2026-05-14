import { useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  listRuntimePreferences,
  setRuntimeMonitored,
  type RuntimePreference,
} from "@/lib/tauri-api";

// v2.5.1 — Settings → Runtimes → Monitoring.
//
// Per-runtime toggle for the Insights → Health panel. Will surfaced
// (2026-05-14): Hermes shown as red "Down" though he never installed
// it; OpenClaw lingered from a long-uninstalled state. The Health
// panel should only render cards for runtimes the user actually uses.
//
// First-launch seed: the backend's ensure_runtime_preferences_seeded
// auto-sets monitored=true for runtimes detected via which_cli, false
// for the rest. So a fresh ATO install lands with sensible defaults.

const RUNTIME_LABELS: Record<string, string> = {
  claude: "Claude Code",
  codex: "Codex / OpenAI CLI",
  gemini: "Gemini CLI",
  openclaw: "OpenClaw",
  hermes: "Hermes",
};

export default function MonitoringToggles() {
  const queryClient = useQueryClient();
  const { data: prefs = [], isLoading } = useQuery({
    queryKey: ["runtime-preferences"],
    queryFn: listRuntimePreferences,
  });

  // Optimistic local state so the toggle feels instant. Reconciled
  // back to query data on every refetch.
  const [pending, setPending] = useState<Record<string, boolean>>({});
  useEffect(() => {
    setPending({});
  }, [prefs]);

  const onToggle = async (runtime: string, next: boolean) => {
    setPending((p) => ({ ...p, [runtime]: next }));
    try {
      await setRuntimeMonitored(runtime, next);
      // Invalidate both the preference list AND the health panel so
      // a card appears/disappears immediately.
      await queryClient.invalidateQueries({ queryKey: ["runtime-preferences"] });
      await queryClient.invalidateQueries({ queryKey: ["health-status"] });
    } catch (err) {
      console.error("[MonitoringToggles] set_runtime_monitored failed:", err);
      // Rollback the optimistic update.
      setPending((p) => {
        const { [runtime]: _, ...rest } = p;
        return rest;
      });
    }
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-32">
        <Loader2 size={20} className="animate-spin text-cs-muted" />
      </div>
    );
  }

  return (
    <div className="space-y-3 rounded-lg border border-cs-border/60 p-4">
      <div>
        <h3 className="text-sm font-semibold text-cs-text">Monitored runtimes</h3>
        <p className="mt-1 text-xs text-cs-muted">
          The Insights → Health panel only shows cards for runtimes you monitor here.
          Turn off the ones you don't use; the panel won't probe them or report them as "Down."
        </p>
      </div>
      <ul className="space-y-2">
        {prefs.map((pref: RuntimePreference) => {
          const isOn = pending[pref.runtime] ?? pref.monitored;
          return (
            <li
              key={pref.runtime}
              className="flex items-center justify-between rounded-md border border-cs-border/40 px-3 py-2"
            >
              <span className="text-sm text-cs-text">
                {RUNTIME_LABELS[pref.runtime] ?? pref.runtime}
              </span>
              <button
                role="switch"
                aria-checked={isOn}
                onClick={() => onToggle(pref.runtime, !isOn)}
                className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${
                  isOn ? "bg-cs-accent" : "bg-cs-border"
                }`}
              >
                <span
                  className={`inline-block h-3 w-3 transform rounded-full bg-white transition-transform ${
                    isOn ? "translate-x-5" : "translate-x-1"
                  }`}
                />
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
