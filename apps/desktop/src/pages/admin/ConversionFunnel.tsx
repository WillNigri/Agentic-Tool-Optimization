// Internal-only WTP funnel dashboard. Strategy PR-B (2026-05-21).
//
// Reads from `conversion_events` (populated by `lib/conversionTelemetry`
// every 60s) and groups by (feature, tier_at_event, trial_cohort). The
// page itself short-circuits to a 404 in production builds because
// `import.meta.env.DEV` is dead-code-eliminated by Vite — even strings
// in the bundle disappear, which is the 2026-05-21 architecture
// war-room's CSO ruling.
//
// Route wiring (one-line follow-up — outside this PR's write set):
//   <Route path="/admin/conversion-funnel" element={<ConversionFunnel />} />
//   inside `App.tsx`, optionally guarded by `{import.meta.env.DEV && …}`.

import { useEffect, useState } from "react";

interface FunnelRow {
  feature: string;
  tierAtEvent: string;
  trialCohort: string | null;
  totalCount: number;
  sessionCount: number;
  firstSeenAt: string;
  lastSeenAt: string;
}

export default function ConversionFunnel() {
  if (!import.meta.env.DEV) {
    return <div className="p-6 text-sm text-gray-400">Not found.</div>;
  }
  return <ConversionFunnelDev />;
}

function ConversionFunnelDev() {
  const [rows, setRows] = useState<FunnelRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const data = await invoke<FunnelRow[]>("get_conversion_funnel", {
          since: null,
        });
        if (!cancelled) setRows(data);
      } catch (err) {
        if (!cancelled) setError(String(err));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  if (loading) {
    return <div className="p-6 text-sm text-gray-400">Loading funnel…</div>;
  }
  if (error) {
    return (
      <div className="p-6 text-sm text-red-400">
        Failed to load funnel: {error}
      </div>
    );
  }
  if (rows.length === 0) {
    return (
      <div className="p-6 text-sm text-gray-400">
        No conversion events recorded yet. Render a gated UI and wait 60s for
        the first flush.
      </div>
    );
  }

  return (
    <div className="p-6 space-y-4">
      <header>
        <h1 className="text-xl font-semibold text-white">
          Conversion funnel (internal)
        </h1>
        <p className="text-xs text-gray-400 mt-1">
          One row per (feature, tier_at_event, trial_cohort). Local only —
          never forwarded to ato-cloud. Sorted by total count.
        </p>
      </header>
      <table className="w-full text-sm border-collapse">
        <thead>
          <tr className="text-left text-gray-400 border-b border-gray-800">
            <th className="py-2 pr-4">Feature</th>
            <th className="py-2 pr-4">Tier @ event</th>
            <th className="py-2 pr-4">Trial cohort</th>
            <th className="py-2 pr-4 text-right">Total calls</th>
            <th className="py-2 pr-4 text-right">Sessions</th>
            <th className="py-2 pr-4">First seen</th>
            <th className="py-2 pr-4">Last seen</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row, i) => (
            <tr
              key={`${row.feature}-${row.tierAtEvent}-${row.trialCohort ?? "null"}-${i}`}
              className="border-b border-gray-900 text-gray-200"
            >
              <td className="py-2 pr-4 font-mono">{row.feature}</td>
              <td className="py-2 pr-4">{row.tierAtEvent}</td>
              <td className="py-2 pr-4">{row.trialCohort ?? "—"}</td>
              <td className="py-2 pr-4 text-right tabular-nums">
                {row.totalCount.toLocaleString()}
              </td>
              <td className="py-2 pr-4 text-right tabular-nums">
                {row.sessionCount.toLocaleString()}
              </td>
              <td className="py-2 pr-4 text-xs text-gray-500">
                {row.firstSeenAt}
              </td>
              <td className="py-2 pr-4 text-xs text-gray-500">
                {row.lastSeenAt}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
