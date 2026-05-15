import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Plus,
  Trash2,
  Loader2,
  AlertCircle,
  CheckCircle2,
  XCircle,
  Clock,
  Key as KeyIcon,
  ExternalLink,
  Eye,
  EyeOff,
  Shield,
  ThumbsUp,
} from "lucide-react";
import {
  listProviderKeys,
  createProviderKey,
  revokeProviderKey,
  listProviderPriorityVotes,
  voteProviderPriority,
  PROVIDER_CATALOG,
  type ProviderKey,
  type ProviderSlug,
  type LastPollStatus,
} from "@/lib/cloudProviderKeys";
import { useAuthStore } from "@/hooks/useAuth";
import TierGate from "@/components/Tier/TierGate";
import { cn } from "@/lib/utils";

// v2.6 PR-B follow-up — Provider Keys settings page.
//
// Lets Pro users register encrypted provider API keys that the cloud
// usage-poller cron reads daily. The plaintext key is typed once,
// POSTed once, and never returned by any subsequent endpoint — the list
// view only shows the key_prefix sigil (e.g. "ato_9SBE") plus audit
// metadata (last_polled_at, last_poll_status, revoked_at).
//
// IA: Settings → Cloud → Provider Keys. Referenced by name from the
// CostBenchmarksPanel empty state.

export default function ProviderKeys() {
  return (
    <TierGate feature="provider-keys">
      <ProviderKeysEditor />
    </TierGate>
  );
}

function ProviderKeysEditor() {
  const { t } = useTranslation();
  const accessToken = useAuthStore((s) => s.accessToken);
  const queryClient = useQueryClient();
  const [showAddForm, setShowAddForm] = useState(false);

  const { data: keys = [], isLoading, error } = useQuery({
    queryKey: ["provider-keys"],
    queryFn: () => listProviderKeys(accessToken),
    staleTime: 10_000,
    enabled: !!accessToken,
  });

  // Votes are a separate fetch; the page renders without waiting on them.
  // Empty fallback if the endpoint isn't deployed yet means the vote
  // button just doesn't appear (graceful degradation).
  const { data: votes = [] } = useQuery({
    queryKey: ["provider-priority-votes"],
    queryFn: () => listProviderPriorityVotes(accessToken),
    staleTime: 30_000,
    enabled: !!accessToken,
    // If the endpoint 404s on an older cloud build, fall through to
    // an empty list instead of breaking the page.
    retry: false,
  });
  const votedProviders = new Set(votes.map((v) => v.provider));

  const revokeMutation = useMutation({
    mutationFn: (id: string) => revokeProviderKey(accessToken, id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["provider-keys"] });
    },
  });

  const voteMutation = useMutation({
    mutationFn: (provider: ProviderSlug) => voteProviderPriority(accessToken, provider),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["provider-priority-votes"] });
    },
  });

  const activeKeys = keys.filter((k) => !k.revokedAt);
  const revokedKeys = keys.filter((k) => k.revokedAt);

  return (
    <div className="space-y-5">
      <header>
        <div className="flex items-center gap-2">
          <Shield size={16} className="text-cs-accent" />
          <h3 className="text-sm font-medium text-cs-text">
            {t("providerKeys.title", "Provider Keys")}
          </h3>
        </div>
        <p className="mt-1 text-xs text-cs-muted max-w-2xl">
          {t(
            "providerKeys.intro",
            "Register API keys for the providers you use. The cloud poller fetches your daily usage so the Usage tab shows authoritative totals — including activity from phone apps and web UIs the local watcher can't see. Keys are encrypted at rest under AES-256-GCM with per-user AAD; the plaintext is sent only on registration and never returned."
          )}
        </p>
      </header>

      {error ? (
        <div className="flex items-start gap-2 rounded-md border border-red-500/40 bg-red-500/5 p-3 text-xs">
          <AlertCircle size={14} className="mt-0.5 shrink-0 text-red-400" />
          <div className="text-red-300">
            {t("providerKeys.loadError", "Couldn't load your provider keys.")}{" "}
            <span className="text-red-400">
              {error instanceof Error ? error.message : String(error)}
            </span>
          </div>
        </div>
      ) : null}

      {!showAddForm ? (
        <button
          type="button"
          onClick={() => setShowAddForm(true)}
          className="inline-flex items-center gap-1.5 rounded-md bg-cs-accent/10 px-3 py-1.5 text-xs font-medium text-cs-accent hover:bg-cs-accent/20 transition-colors"
        >
          <Plus size={14} />
          {t("providerKeys.addButton", "Add a provider key")}
        </button>
      ) : (
        <AddProviderKeyForm
          onCancel={() => setShowAddForm(false)}
          onSuccess={() => {
            setShowAddForm(false);
            void queryClient.invalidateQueries({ queryKey: ["provider-keys"] });
          }}
        />
      )}

      {isLoading ? (
        <div className="flex items-center justify-center py-8 text-cs-muted">
          <Loader2 size={20} className="animate-spin" />
        </div>
      ) : activeKeys.length === 0 && revokedKeys.length === 0 ? (
        <EmptyState />
      ) : (
        <div className="space-y-4">
          {activeKeys.length > 0 ? (
            <section>
              <h4 className="mb-2 text-xs font-medium uppercase tracking-wide text-cs-muted">
                {t("providerKeys.activeSection", "Active")}
              </h4>
              <ul className="space-y-2">
                {activeKeys.map((key) => (
                  <ProviderKeyRow
                    key={key.id}
                    keyRow={key}
                    onRevoke={() => revokeMutation.mutate(key.id)}
                    isRevoking={
                      revokeMutation.isPending && revokeMutation.variables === key.id
                    }
                    hasVoted={votedProviders.has(key.provider)}
                    onVote={() => voteMutation.mutate(key.provider)}
                    isVoting={
                      voteMutation.isPending && voteMutation.variables === key.provider
                    }
                  />
                ))}
              </ul>
            </section>
          ) : null}

          {revokedKeys.length > 0 ? (
            <section>
              <h4 className="mb-2 text-xs font-medium uppercase tracking-wide text-cs-muted">
                {t("providerKeys.revokedSection", "Revoked")}
              </h4>
              <ul className="space-y-2 opacity-60">
                {revokedKeys.map((key) => (
                  <ProviderKeyRow
                    key={key.id}
                    keyRow={key}
                    onRevoke={null}
                    isRevoking={false}
                    hasVoted={votedProviders.has(key.provider)}
                    onVote={null}
                    isVoting={false}
                  />
                ))}
              </ul>
            </section>
          ) : null}
        </div>
      )}
    </div>
  );
}

function EmptyState() {
  const { t } = useTranslation();
  return (
    <div className="rounded-md border border-dashed border-cs-border/60 p-6 text-center">
      <KeyIcon size={20} className="mx-auto text-cs-muted/60" />
      <p className="mt-2 text-xs text-cs-muted">
        {t(
          "providerKeys.empty",
          "No provider keys yet. Add one above to start tracking cross-device usage."
        )}
      </p>
    </div>
  );
}

interface AddFormProps {
  onCancel: () => void;
  onSuccess: () => void;
}

function AddProviderKeyForm({ onCancel, onSuccess }: AddFormProps) {
  const { t } = useTranslation();
  const accessToken = useAuthStore((s) => s.accessToken);
  const [provider, setProvider] = useState<ProviderSlug>("openai");
  const [label, setLabel] = useState("");
  const [keyValue, setKeyValue] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const createMutation = useMutation({
    mutationFn: () =>
      createProviderKey(accessToken, {
        provider,
        key: keyValue,
        label: label.trim() || undefined,
      }),
    onSuccess: () => {
      setProvider("openai");
      setLabel("");
      setKeyValue("");
      setSubmitError(null);
      onSuccess();
    },
    onError: (e) => {
      setSubmitError(e instanceof Error ? e.message : String(e));
    },
  });

  const selectedCatalog = PROVIDER_CATALOG.find((p) => p.slug === provider);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    setSubmitError(null);
    if (!keyValue.trim()) {
      setSubmitError(t("providerKeys.keyRequired", "Paste the API key first."));
      return;
    }
    createMutation.mutate();
  };

  return (
    <form
      onSubmit={handleSubmit}
      className="space-y-3 rounded-md border border-cs-border/60 bg-cs-bg2/40 p-4"
    >
      <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
        <label className="block">
          <span className="block text-xs font-medium text-cs-muted mb-1">
            {t("providerKeys.providerLabel", "Provider")}
          </span>
          <select
            value={provider}
            onChange={(e) => setProvider(e.target.value as ProviderSlug)}
            className="w-full rounded-md border border-cs-border/60 bg-cs-bg2 px-2 py-1.5 text-xs text-cs-text focus:outline-none focus:border-cs-accent"
          >
            {PROVIDER_CATALOG.map((p) => (
              <option key={p.slug} value={p.slug}>
                {p.displayName}
                {p.pollStatus === "viable" ? "" : " (limited — see below)"}
              </option>
            ))}
          </select>
        </label>

        <label className="block">
          <span className="block text-xs font-medium text-cs-muted mb-1">
            {t("providerKeys.labelLabel", "Label")}{" "}
            <span className="text-cs-muted/60">
              {t("providerKeys.labelOptional", "(optional)")}
            </span>
          </span>
          <input
            type="text"
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            maxLength={128}
            placeholder={t("providerKeys.labelPlaceholder", "e.g. work, personal")}
            className="w-full rounded-md border border-cs-border/60 bg-cs-bg2 px-2 py-1.5 text-xs text-cs-text placeholder:text-cs-muted/40 focus:outline-none focus:border-cs-accent"
          />
        </label>
      </div>

      <label className="block">
        <span className="block text-xs font-medium text-cs-muted mb-1">
          {t("providerKeys.keyLabel", "API key")}
        </span>
        <div className="relative">
          <input
            type={showKey ? "text" : "password"}
            value={keyValue}
            onChange={(e) => setKeyValue(e.target.value)}
            autoComplete="off"
            spellCheck={false}
            placeholder={t("providerKeys.keyPlaceholder", "Paste the key — it's encrypted server-side and never echoed back.")}
            className="w-full rounded-md border border-cs-border/60 bg-cs-bg2 px-2 py-1.5 pr-8 text-xs font-mono text-cs-text placeholder:text-cs-muted/40 focus:outline-none focus:border-cs-accent"
          />
          <button
            type="button"
            onClick={() => setShowKey((v) => !v)}
            aria-label={showKey ? "Hide" : "Show"}
            className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded p-1 text-cs-muted hover:text-cs-text"
          >
            {showKey ? <EyeOff size={14} /> : <Eye size={14} />}
          </button>
        </div>
      </label>

      {selectedCatalog ? (
        <div
          className={cn(
            "rounded-md border px-3 py-2 text-[11px] leading-relaxed",
            selectedCatalog.pollStatus === "viable"
              ? "border-cs-accent/30 bg-cs-accent/5 text-cs-muted"
              : "border-amber-500/30 bg-amber-500/5 text-amber-200/80"
          )}
        >
          {selectedCatalog.pollStatus === "viable" ? (
            <>
              {t(
                "providerKeys.providerNote.viable",
                "Daily cron polls this provider's usage API and writes authoritative totals to your dashboard."
              )}
            </>
          ) : selectedCatalog.pollStatus === "balance-only" ? (
            <>
              {t(
                "providerKeys.providerNote.balanceOnly",
                "{{provider}} only exposes a balance endpoint, not historical aggregates. The key is stored for when the upcoming client-side capture path (PR-D, Pro feature) lands. Your dashboard will show no rows for this provider until then.",
                { provider: selectedCatalog.displayName }
              )}
            </>
          ) : (
            <>
              {t(
                "providerKeys.providerNote.noAggregate",
                "{{provider}} doesn't expose an aggregate usage endpoint to third parties. The key is stored for when the upcoming client-side capture path (PR-D, Pro feature) lands. Your dashboard will show no rows for this provider until then.",
                { provider: selectedCatalog.displayName }
              )}
            </>
          )}
          {" — "}
          <a
            href={selectedCatalog.signupUrl}
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-0.5 text-cs-accent hover:underline"
          >
            {t("providerKeys.findKeyLink", "Get a key")}
            <ExternalLink size={10} />
          </a>
        </div>
      ) : null}

      {submitError ? (
        <div className="flex items-start gap-2 rounded-md border border-red-500/40 bg-red-500/5 p-2 text-[11px] text-red-300">
          <AlertCircle size={12} className="mt-0.5 shrink-0" />
          <div>{submitError}</div>
        </div>
      ) : null}

      <div className="flex items-center gap-2 pt-1">
        <button
          type="submit"
          disabled={createMutation.isPending || !keyValue.trim()}
          className="inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg disabled:opacity-50 disabled:cursor-not-allowed hover:bg-cs-accent/80 transition-colors"
        >
          {createMutation.isPending ? (
            <Loader2 size={12} className="animate-spin" />
          ) : (
            <Plus size={12} />
          )}
          {t("providerKeys.submitButton", "Register key")}
        </button>
        <button
          type="button"
          onClick={onCancel}
          disabled={createMutation.isPending}
          className="text-xs text-cs-muted hover:text-cs-text px-2 py-1.5"
        >
          {t("providerKeys.cancelButton", "Cancel")}
        </button>
      </div>
    </form>
  );
}

interface RowProps {
  keyRow: ProviderKey;
  onRevoke: (() => void) | null;
  isRevoking: boolean;
  hasVoted: boolean;
  onVote: (() => void) | null;
  isVoting: boolean;
}

function ProviderKeyRow({
  keyRow,
  onRevoke,
  isRevoking,
  hasVoted,
  onVote,
  isVoting,
}: RowProps) {
  const { t } = useTranslation();
  const catalog = PROVIDER_CATALOG.find((p) => p.slug === keyRow.provider);
  const isRevoked = !!keyRow.revokedAt;
  // Vote affordance applies to providers that aren't poll-viable today:
  // balance-only + no-aggregate. Renders next to the status badge.
  const showVoteCta =
    !isRevoked &&
    !!catalog &&
    catalog.pollStatus !== "viable" &&
    !!onVote;

  return (
    <li className="flex items-center gap-3 rounded-md border border-cs-border/40 bg-cs-bg2/30 px-3 py-2.5">
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 flex-wrap">
          <span className="text-xs font-medium text-cs-text">
            {catalog?.displayName ?? keyRow.provider}
          </span>
          {keyRow.label ? (
            <span className="text-[11px] text-cs-muted">· {keyRow.label}</span>
          ) : null}
          <code className="text-[11px] font-mono text-cs-muted/70">{keyRow.keyPrefix}</code>
        </div>
        <div className="mt-1 flex items-center gap-3 flex-wrap text-[11px] text-cs-muted">
          <StatusBadge status={keyRow.lastPollStatus} isRevoked={isRevoked} />
          <LastPolledHint lastPolledAt={keyRow.lastPolledAt} isRevoked={isRevoked} />
          {showVoteCta ? (
            hasVoted ? (
              <span className="inline-flex items-center gap-1 text-cs-accent/80">
                <ThumbsUp size={11} />
                {t("providerKeys.voted", "Voted")}
              </span>
            ) : (
              <button
                type="button"
                onClick={onVote}
                disabled={isVoting}
                className="inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-cs-accent hover:bg-cs-accent/10 disabled:opacity-50"
                aria-label={t(
                  "providerKeys.voteAria",
                  "Vote to prioritize {{provider}} support",
                  { provider: catalog?.displayName ?? keyRow.provider }
                )}
              >
                {isVoting ? (
                  <Loader2 size={11} className="animate-spin" />
                ) : (
                  <ThumbsUp size={11} />
                )}
                {t("providerKeys.voteCta", "Vote to prioritize")}
              </button>
            )
          ) : null}
        </div>
      </div>
      {onRevoke ? (
        <button
          type="button"
          onClick={() => {
            if (window.confirm(t("providerKeys.confirmRevoke", "Revoke this key? The cron will stop polling it, but its audit row stays for forensics."))) {
              onRevoke();
            }
          }}
          disabled={isRevoking}
          aria-label={t("providerKeys.revokeAria", "Revoke key")}
          className="rounded-md p-1.5 text-cs-muted hover:text-red-400 hover:bg-red-500/5 disabled:opacity-50"
        >
          {isRevoking ? <Loader2 size={14} className="animate-spin" /> : <Trash2 size={14} />}
        </button>
      ) : null}
    </li>
  );
}

function StatusBadge({
  status,
  isRevoked,
}: {
  status: LastPollStatus | null;
  isRevoked: boolean;
}) {
  const { t } = useTranslation();
  if (isRevoked) {
    return (
      <span className="inline-flex items-center gap-1 text-cs-muted/60">
        <XCircle size={11} />
        {t("providerKeys.status.revoked", "Revoked")}
      </span>
    );
  }
  if (status === null) {
    return (
      <span className="inline-flex items-center gap-1 text-cs-muted/60">
        <Clock size={11} />
        {t("providerKeys.status.pending", "Pending first poll")}
      </span>
    );
  }
  const map: Record<LastPollStatus, { icon: typeof CheckCircle2; label: string; cls: string }> = {
    ok: {
      icon: CheckCircle2,
      label: t("providerKeys.status.ok", "Polling OK"),
      cls: "text-emerald-400",
    },
    auth_failed: {
      icon: XCircle,
      label: t("providerKeys.status.authFailed", "Auth failed"),
      cls: "text-red-400",
    },
    rate_limited: {
      icon: AlertCircle,
      label: t("providerKeys.status.rateLimited", "Rate-limited"),
      cls: "text-amber-400",
    },
    provider_error: {
      icon: AlertCircle,
      label: t("providerKeys.status.providerError", "Provider error"),
      cls: "text-amber-400",
    },
    unsupported_provider: {
      icon: Clock,
      label: t("providerKeys.status.unsupported", "Awaiting client-capture path"),
      cls: "text-cs-muted/70",
    },
    timeout: {
      icon: Clock,
      label: t("providerKeys.status.timeout", "Timed out"),
      cls: "text-amber-400",
    },
  };
  const entry = map[status];
  const Icon = entry.icon;
  return (
    <span className={cn("inline-flex items-center gap-1", entry.cls)}>
      <Icon size={11} />
      {entry.label}
    </span>
  );
}

function LastPolledHint({
  lastPolledAt,
  isRevoked,
}: {
  lastPolledAt: string | null;
  isRevoked: boolean;
}) {
  const { t } = useTranslation();
  if (isRevoked || lastPolledAt === null) return null;
  const ms = Date.now() - new Date(lastPolledAt).getTime();
  const hours = Math.floor(ms / 3_600_000);
  const days = Math.floor(hours / 24);
  const label =
    days >= 1
      ? t("providerKeys.lastPolled.days", "polled {{n}}d ago", { n: days })
      : hours >= 1
      ? t("providerKeys.lastPolled.hours", "polled {{n}}h ago", { n: hours })
      : t("providerKeys.lastPolled.recent", "polled <1h ago");
  return <span className="text-cs-muted/60">· {label}</span>;
}
