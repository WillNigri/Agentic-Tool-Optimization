// v2.3.36 Phase 6.x-I.2 — Runtime health banner.
//
// Polls the Tauri runtime_health_check command once on mount + once
// every five minutes. Renders nothing when every detected runtime is
// `ok` or `missing` (missing isn't a "broken" state — the user just
// hasn't installed that runtime). When any row has `revoked` /
// `quarantined` / `unsigned`, the banner appears with the specific
// reason and a one-click fix button when a canned command exists.
//
// Triggered by Will's codex install hitting CSSMERR_TP_CERT_REVOKED
// on 2026-05-11. CLI command shipped v2.3.34; this is the GUI half.

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, ShieldAlert, Wrench, CheckCircle2 } from "lucide-react";

interface RuntimeHealthRow {
  runtime: string;
  binary_path: string | null;
  status: "ok" | "missing" | "revoked" | "quarantined" | "unsigned" | "unknown";
  detail: string | null;
  fix_command: string | null;
}

// Statuses that justify a banner. `missing` is intentionally excluded
// — Home already has a "no runtime detected" prompt for first-run,
// and we don't want to nag for runtimes the user never installed.
const BROKEN_STATUSES = new Set(["revoked", "quarantined", "unsigned", "unknown"]);

const STATUS_LABEL: Record<string, string> = {
  revoked: "Developer cert revoked",
  quarantined: "Quarantined by Gatekeeper",
  unsigned: "Not signed",
  unknown: "Verification failed",
};

export default function RuntimeHealthBanner() {
  const { data: rows = [] } = useQuery<RuntimeHealthRow[]>({
    queryKey: ["runtime-health"],
    queryFn: () => invoke<RuntimeHealthRow[]>("runtime_health_check"),
    staleTime: 5 * 60_000,
    refetchInterval: 5 * 60_000,
  });
  const [fixingRuntime, setFixingRuntime] = useState<string | null>(null);
  const [fixResult, setFixResult] = useState<{ runtime: string; ok: boolean; message: string } | null>(null);

  const broken = rows.filter((r) => BROKEN_STATUSES.has(r.status));
  if (broken.length === 0) {
    if (fixResult?.ok) {
      // Confirmation toast after a successful fix — only renders while
      // the next health-check refetch is pending. Once the refetch
      // clears `broken`, the toast keeps showing until cleared so the
      // user gets feedback that the action took.
      return (
        <section className="flex items-start gap-3 rounded-lg border border-cs-accent/40 bg-cs-accent/10 p-4">
          <CheckCircle2 size={18} className="text-cs-accent shrink-0" />
          <div className="flex-1 text-sm text-cs-text">
            Fixed <span className="font-medium">@{fixResult.runtime}</span>. {fixResult.message}
          </div>
          <button
            type="button"
            onClick={() => setFixResult(null)}
            className="text-xs text-cs-muted hover:text-cs-text"
          >
            dismiss
          </button>
        </section>
      );
    }
    return null;
  }

  const runFix = async (row: RuntimeHealthRow) => {
    if (!row.fix_command) return;
    setFixingRuntime(row.runtime);
    setFixResult(null);
    try {
      const out = await invoke<string>("runtime_health_run_fix", {
        fixCommand: row.fix_command,
      });
      setFixResult({
        runtime: row.runtime,
        ok: true,
        message: out.split("\n").slice(-3).join(" ") || "Done.",
      });
    } catch (e) {
      setFixResult({
        runtime: row.runtime,
        ok: false,
        message: String(e),
      });
    } finally {
      setFixingRuntime(null);
    }
  };

  return (
    <section className="flex items-start gap-3 rounded-lg border border-cs-danger/40 bg-cs-danger/10 p-4">
      <ShieldAlert size={18} className="text-cs-danger shrink-0 mt-0.5" />
      <div className="flex-1 space-y-3">
        <div>
          <h3 className="text-sm font-medium text-cs-text">
            {broken.length === 1
              ? `Runtime issue: @${broken[0].runtime}`
              : `${broken.length} runtimes need attention`}
          </h3>
          <p className="mt-1 text-xs text-cs-muted">
            macOS won't let ATO spawn a runtime whose signature is invalid
            or whose Developer ID cert was revoked. Fix below before
            dispatching.
          </p>
        </div>
        <ul className="space-y-2">
          {broken.map((row) => (
            <li
              key={row.runtime}
              className="flex items-start gap-3 rounded-md border border-cs-border bg-cs-bg-raised p-3"
            >
              <AlertTriangle size={14} className="text-cs-danger shrink-0 mt-0.5" />
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 flex-wrap">
                  <span className="text-sm font-medium text-cs-text">@{row.runtime}</span>
                  <span className="text-xs rounded bg-cs-danger/20 px-1.5 py-0.5 text-cs-danger">
                    {STATUS_LABEL[row.status] ?? row.status}
                  </span>
                </div>
                {row.binary_path && (
                  <div className="mt-1 text-xs font-mono text-cs-muted truncate">
                    {row.binary_path}
                  </div>
                )}
                {row.detail && (
                  <div className="mt-1 text-xs text-cs-muted">{row.detail}</div>
                )}
                {row.fix_command && (
                  <div className="mt-2 flex items-center gap-2 flex-wrap">
                    <code className="text-xs rounded bg-cs-bg px-2 py-1 text-cs-text font-mono">
                      {row.fix_command}
                    </code>
                    <button
                      type="button"
                      onClick={() => runFix(row)}
                      disabled={fixingRuntime === row.runtime}
                      className="inline-flex items-center gap-1 rounded-md border border-cs-accent/60 bg-cs-accent/10 px-2.5 py-1 text-xs font-medium text-cs-accent hover:bg-cs-accent/20 disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                      <Wrench size={12} />
                      {fixingRuntime === row.runtime ? "Running…" : "Run fix"}
                    </button>
                  </div>
                )}
                {fixResult?.runtime === row.runtime && !fixResult.ok && (
                  <div className="mt-2 text-xs text-cs-danger">
                    Failed: {fixResult.message}
                  </div>
                )}
              </div>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
