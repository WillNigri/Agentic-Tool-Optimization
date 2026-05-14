// Credit-burn meter — Settings → Runtimes top card.
//
// Surfaces estimated month-to-date dispatch cost split by auth path,
// derived from execution_logs aggregations. Both subscription and
// api_key rows have a cost_usd_estimated populated (we always compute
// the API-equivalent dollar amount) — the split lets users see:
//
//   - api_key total: what they actually got billed via their API account
//   - subscription total: what they *would* have been billed if BYOK
//     was on for those dispatches. After 2026-06-15 this maps roughly
//     to the Agent SDK credit pool consumption.
//
// Pre-migration rows (auth_mode IS NULL) sit in `unattributed` and
// don't influence the split, only the total — so the meter is honest
// about what it doesn't know.

import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { DollarSign, TrendingUp, Key, Terminal, Info } from "lucide-react";
import { cn } from "@/lib/utils";

interface RuntimeCostRow {
  runtime: string;
  authMode: string | null;
  dispatchCount: number;
  tokensIn: number;
  tokensOut: number;
  costUsdEstimated: number;
}

interface CreditBurnSummary {
  since: string;
  until: string;
  totalCostUsd: number;
  totalDispatches: number;
  apiKeyCostUsd: number;
  subscriptionCostUsd: number;
  rows: RuntimeCostRow[];
}

function formatUsd(n: number): string {
  if (n === 0) return "$0.00";
  if (n < 0.01) return "<$0.01";
  if (n < 1) return `$${n.toFixed(3)}`;
  if (n < 100) return `$${n.toFixed(2)}`;
  return `$${Math.round(n)}`;
}

function monthLabel(iso: string): string {
  try {
    return new Date(iso).toLocaleDateString(undefined, {
      month: "long",
      year: "numeric",
    });
  } catch {
    return iso;
  }
}

export default function CreditBurnCard() {
  const { t } = useTranslation();
  const { data, isLoading } = useQuery<CreditBurnSummary>({
    queryKey: ["credit-burn-summary"],
    queryFn: () => invoke<CreditBurnSummary>("get_credit_burn_summary"),
    refetchInterval: 60_000,
    staleTime: 30_000,
  });

  if (isLoading || !data) {
    return (
      <section className="rounded-xl border border-cs-border bg-cs-card p-5 mb-4">
        <div className="text-xs text-cs-muted">Loading dispatch cost…</div>
      </section>
    );
  }

  const empty = data.totalDispatches === 0;
  const subPct = data.totalCostUsd > 0
    ? Math.round((data.subscriptionCostUsd / data.totalCostUsd) * 100)
    : 0;
  const keyPct = data.totalCostUsd > 0
    ? Math.round((data.apiKeyCostUsd / data.totalCostUsd) * 100)
    : 0;

  return (
    <section className="rounded-xl border border-cs-border bg-cs-card p-5 mb-4">
      <header className="flex items-start justify-between mb-3">
        <div>
          <h3 className="text-sm font-semibold text-cs-text flex items-center gap-2">
            <DollarSign size={14} className="text-cs-accent" />
            {t("runtimes.burnTitle", "Dispatch cost — {{month}}", {
              month: monthLabel(data.since),
            })}
          </h3>
          <p className="text-xs text-cs-muted mt-1 max-w-2xl">
            {t(
              "runtimes.burnSubtitle",
              "Estimated cost of every dispatch this month. Subscription rows show the API-equivalent dollar amount Anthropic / OpenAI / Google would have charged at API rates — useful for sizing whether BYOK would save you money.",
            )}
          </p>
        </div>
      </header>

      {empty ? (
        <div className="rounded-lg border border-cs-border bg-cs-bg-raised p-4 text-sm text-cs-muted flex items-center gap-2">
          <Info size={14} />
          {t(
            "runtimes.burnEmpty",
            "No dispatches recorded this month yet. Run an agent and the cost split will show up here.",
          )}
        </div>
      ) : (
        <>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-3 mb-4">
            <CostStat
              label={t("runtimes.burnTotal", "Total this month")}
              value={formatUsd(data.totalCostUsd)}
              icon={<TrendingUp size={14} />}
              tone="accent"
              detail={t("runtimes.burnDispatches", "{{count}} dispatches", {
                count: data.totalDispatches,
              })}
            />
            <CostStat
              label={t("runtimes.burnApiKey", "API key (real billing)")}
              value={formatUsd(data.apiKeyCostUsd)}
              icon={<Key size={14} />}
              tone="emerald"
              detail={`${keyPct}% of total`}
            />
            <CostStat
              label={t("runtimes.burnSubscription", "Subscription equivalent")}
              value={formatUsd(data.subscriptionCostUsd)}
              icon={<Terminal size={14} />}
              tone="sky"
              detail={`${subPct}% of total · at API rates`}
            />
          </div>

          <table className="w-full text-xs">
            <thead className="text-cs-muted">
              <tr className="border-b border-cs-border/40">
                <th className="text-left py-1.5 pr-3 font-medium">Runtime</th>
                <th className="text-left py-1.5 px-3 font-medium">Mode</th>
                <th className="text-right py-1.5 px-3 font-medium">Dispatches</th>
                <th className="text-right py-1.5 px-3 font-medium">Tokens</th>
                <th className="text-right py-1.5 pl-3 font-medium">Cost (est.)</th>
              </tr>
            </thead>
            <tbody>
              {data.rows.map((r, i) => (
                <tr
                  key={`${r.runtime}-${r.authMode ?? "unknown"}-${i}`}
                  className="border-b border-cs-border/20 last:border-0"
                >
                  <td className="py-1.5 pr-3 text-cs-text">{r.runtime}</td>
                  <td className="py-1.5 px-3">
                    <ModeChip mode={r.authMode} />
                  </td>
                  <td className="py-1.5 px-3 text-right text-cs-muted">
                    {r.dispatchCount.toLocaleString()}
                  </td>
                  <td className="py-1.5 px-3 text-right text-cs-muted">
                    {(r.tokensIn + r.tokensOut).toLocaleString()}
                  </td>
                  <td className="py-1.5 pl-3 text-right font-medium text-cs-text">
                    {formatUsd(r.costUsdEstimated)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          <p className="text-[10px] text-cs-muted mt-3">
            {t(
              "runtimes.burnDisclaimer",
              "Estimates use a 4-chars-per-token heuristic + the per-million-token rates in pricing.ts. Real billing may differ by ±15%. Subscription rows are the API-equivalent — your actual subscription pool consumption isn't tracked here.",
            )}
          </p>
        </>
      )}
    </section>
  );
}

function CostStat({
  label,
  value,
  icon,
  tone,
  detail,
}: {
  label: string;
  value: string;
  icon: React.ReactNode;
  tone: "accent" | "emerald" | "sky";
  detail?: string;
}) {
  const toneCls = {
    accent: "bg-cs-accent/5 border-cs-accent/30 text-cs-accent",
    emerald: "bg-emerald-500/5 border-emerald-500/30 text-emerald-300",
    sky: "bg-sky-500/5 border-sky-500/30 text-sky-300",
  }[tone];
  return (
    <div className={cn("rounded-lg border p-3", toneCls)}>
      <div className="flex items-center gap-1.5 text-[10px] font-medium uppercase tracking-wide opacity-80">
        {icon}
        {label}
      </div>
      <div className="mt-1 text-xl font-semibold text-cs-text tabular-nums">{value}</div>
      {detail && <div className="text-[10px] text-cs-muted mt-0.5">{detail}</div>}
    </div>
  );
}

function ModeChip({ mode }: { mode: string | null }) {
  if (mode === "api_key") {
    return (
      <span className="inline-flex items-center gap-1 rounded-full bg-emerald-500/10 px-2 py-0.5 text-[10px] font-medium text-emerald-300 border border-emerald-500/30">
        <Key size={9} />
        API key
      </span>
    );
  }
  if (mode === "subscription") {
    return (
      <span className="inline-flex items-center gap-1 rounded-full bg-sky-500/10 px-2 py-0.5 text-[10px] font-medium text-sky-300 border border-sky-500/30">
        <Terminal size={9} />
        Subscription
      </span>
    );
  }
  return (
    <span className="inline-flex items-center gap-1 rounded-full bg-cs-muted/10 px-2 py-0.5 text-[10px] font-medium text-cs-muted border border-cs-border">
      Pre-migration
    </span>
  );
}
