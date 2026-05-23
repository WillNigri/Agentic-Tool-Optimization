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
    // Backend sends UTC midnight of the first-of-month
    // (e.g. "2026-05-01T00:00:00Z"). In any timezone west of UTC,
    // toLocaleDateString without `timeZone: 'UTC'` rolls back to
    // April 30 local + renders "April" — the BRT-dogfood "abril"
    // bug (Will 2026-05-22 screenshot). Force UTC interpretation
    // so we always render the same month the backend selected.
    return new Date(iso).toLocaleDateString(undefined, {
      month: "long",
      year: "numeric",
      timeZone: "UTC",
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
  // Unattributed slice = pre-migration rows + rows from runtimes
  // without a BYOK mapping (hermes/openclaw). Surfacing this stops
  // the % labels from silently failing to add up to 100. (claude #3)
  const unattributedCost = Math.max(
    0,
    data.totalCostUsd - data.subscriptionCostUsd - data.apiKeyCostUsd,
  );
  const unattributedPct = data.totalCostUsd > 0
    ? Math.max(0, 100 - subPct - keyPct)
    : 0;
  const showUnattributed = unattributedCost > 0.0001;

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
          <div
            className={cn(
              "grid grid-cols-1 gap-3 mb-4",
              showUnattributed ? "md:grid-cols-4" : "md:grid-cols-3",
            )}
          >
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
            {showUnattributed && (
              <CostStat
                label={t("runtimes.burnUnattributed", "Unattributed")}
                value={formatUsd(unattributedCost)}
                icon={<Info size={14} />}
                tone="muted"
                detail={`${unattributedPct}% · pre-migration or non-BYOK runtime`}
              />
            )}
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
                    <RowCost row={r} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          <p className="text-[10px] text-cs-muted mt-3">
            {t(
              "runtimes.burnDisclaimer",
              "BYOK API rows (Google / Anthropic / OpenAI / MiniMax etc. with an API key via `ato dispatch`) use the EXACT token counts the provider returned + the published per-million rates in pricing.ts — should match your provider billing portal within rounding. Subscription rows (claude / codex / gemini CLI) estimate tokens with a 4-chars-per-token heuristic because the CLIs don't return token counts; expect ±15% on those. Subscription rows are the API-equivalent dollar amount — your actual subscription pool consumption isn't tracked here.",
            )}
          </p>

          {/* v2.7.15 — Cost scope disclosure (Will dogfood 2026-05-22):
              users were reading this card as their TOTAL LLM spend.
              It's not — it's only what ATO sees. Spell that out so the
              gap between this number and the user's provider billing
              portal is intelligible instead of looking like an
              undercount bug. */}
          <details className="mt-3 group">
            <summary className="text-[11px] text-cs-muted cursor-pointer hover:text-cs-text inline-flex items-center gap-1.5">
              <Info size={11} />
              {t(
                "runtimes.burnScopeSummary",
                "What ATO tracks vs. what's missing →",
              )}
            </summary>
            <div className="mt-2 grid grid-cols-1 md:grid-cols-2 gap-3 text-[11px]">
              <div className="rounded border border-emerald-500/20 bg-emerald-500/5 p-3">
                <div className="font-medium text-emerald-300 mb-1">
                  {t("runtimes.burnScopeTracked", "✓ Tracked")}
                </div>
                <ul className="text-cs-muted space-y-0.5 list-none">
                  <li>{t("runtimes.burnScopeT1", "ATO-dispatched calls (war-rooms, sessions, chats, agents)")}</li>
                  <li>{t("runtimes.burnScopeT2", "External Claude Code + Codex CLI sessions (via passive observer)")}</li>
                </ul>
              </div>
              <div className="rounded border border-amber-500/20 bg-amber-500/5 p-3">
                <div className="font-medium text-amber-300 mb-1">
                  {t("runtimes.burnScopeNotTracked", "✗ NOT tracked")}
                </div>
                <ul className="text-cs-muted space-y-0.5 list-none">
                  <li>{t("runtimes.burnScopeN1", "Browser sessions: claude.ai, chatgpt.com, gemini.google.com, aistudio.google.com")}</li>
                  <li>{t("runtimes.burnScopeN2", "IDE integrations: Cursor, Continue.dev, GitHub Copilot in VS Code")}</li>
                  <li>{t("runtimes.burnScopeN3", "Other terminal tools (aider, etc.) unless we add a watcher")}</li>
                  <li>{t("runtimes.burnScopeN4", "Direct API calls from your own scripts that bypass ato dispatch")}</li>
                </ul>
              </div>
            </div>
            <p className="text-[10px] text-cs-muted mt-2">
              {t(
                "runtimes.burnScopeFooter",
                "For your total LLM spend, check each provider's billing portal directly. The gap between that number and the total above is your non-ATO usage.",
              )}
            </p>
          </details>
        </>
      )}
    </section>
  );
}

/** Distinguish "$0 because the model is unpriced in our table" from
 *  "$0 because there was no work this period." The Rust aggregator
 *  COALESCEs NULL cost_usd_estimated to 0, so a row with tokens > 0
 *  but cost = 0 means at least some dispatches used a model that
 *  `pricing_for_model` returned None for. Render an em dash with a
 *  hover hint pointing at the fix (add the model to pricing.ts) — much
 *  more honest than reporting "$0.00" on an actually-billed Gemini /
 *  GPT-5 / o3 / etc. session. (2026-05-15 user bug report; pricing.ts
 *  has an `isModelPriced` helper for future per-row use when the
 *  aggregator surfaces the model name.) */
function RowCost({ row }: { row: RuntimeCostRow }) {
  if (row.costUsdEstimated > 0) {
    return <>{formatUsd(row.costUsdEstimated)}</>;
  }
  const totalTokens = row.tokensIn + row.tokensOut;
  if (totalTokens === 0) {
    // Genuinely nothing happened in this slice.
    return <>{formatUsd(0)}</>;
  }
  // Tokens but no priced cost → some/all dispatches used a model not in
  // pricing.ts. Surface the gap rather than imply "$0 for 2.5M tokens."
  return (
    <span
      className="text-cs-muted italic cursor-help"
      title="At least one dispatch used a model not in our pricing table — actual cost is non-zero but unknown to ATO. Add the model to apps/desktop/src/lib/pricing.ts (and the Rust mirror in apps/desktop/src-tauri/src/commands.rs::pricing_for_model) to make this row priced."
    >
      —
    </span>
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
  tone: "accent" | "emerald" | "sky" | "muted";
  detail?: string;
}) {
  const toneCls = {
    accent: "bg-cs-accent/5 border-cs-accent/30 text-cs-accent",
    emerald: "bg-emerald-500/5 border-emerald-500/30 text-emerald-300",
    sky: "bg-sky-500/5 border-sky-500/30 text-sky-300",
    muted: "bg-cs-muted/5 border-cs-border text-cs-muted",
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
