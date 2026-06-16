// #86 — Surface embed key on Settings → Cloud.
//
// The Onboarding flow in apps/web tells new users to put their key in
// ATO_API_KEY=... in a .env file. Until now the desktop only exposed
// the key from a deep path (open an Agent → Deploy tab → Reveal).
// This card puts it on the top-level Cloud panel so users coming
// from Onboarding can find it immediately.
//
// Naming alignment: the SDK + DeployTab call the env var ATO_TRACE_KEY
// and the cloud route uses /embed-key. The web Onboarding documents
// ATO_API_KEY. We surface both labels here so users who came from
// either path recognize what they're looking at — and the snippet
// reads ATO_API_KEY since that's what the Onboarding asks them to
// type into their .env.

import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Key,
  Copy,
  Eye,
  EyeOff,
  RefreshCw,
  Loader2,
  AlertTriangle,
  Check,
  ExternalLink,
} from "lucide-react";
import { getEmbedKey, rotateEmbedKey } from "@/lib/cloud-api";

interface EmbedKeyCardProps {
  /** Tier from cloud user record. embed_key is Pro+ only. */
  subscriptionTier: "free" | "pro" | "team" | "enterprise" | null | undefined;
  /**
   * Cloud user id. R1 fix — the query key MUST be scoped per account so
   * that logout+login as a different cloud account never surfaces the
   * previous account's key. useCloudStore.logout() does not clear the
   * react-query cache, so a plain ["embed-key"] would leak across
   * accounts in the same desktop session.
   */
  userId: string | null | undefined;
}

export default function EmbedKeyCard({ subscriptionTier, userId }: EmbedKeyCardProps) {
  const queryClient = useQueryClient();
  const [revealed, setRevealed] = useState(false);
  const [copied, setCopied] = useState(false);
  const [confirmingRotate, setConfirmingRotate] = useState(false);
  // R1 fix — typed-confirm for the rotate destructive action. Users
  // must type "rotate" (case-insensitive) before the button arms.
  const [rotateConfirmText, setRotateConfirmText] = useState("");

  // R1 fix — explicit boolean for readability + correctness on null/undefined.
  const hasAccess: boolean =
    !!subscriptionTier && subscriptionTier !== "free";

  const keyQuery = useQuery<string, Error>({
    // R1 fix — per-account scope; never leaks across logout+login.
    queryKey: ["embed-key", userId ?? "anon"],
    queryFn: getEmbedKey,
    enabled: hasAccess && !!userId,
    staleTime: Infinity, // Won't change unless rotated.
  });

  const rotateMutation = useMutation<string, Error>({
    mutationFn: rotateEmbedKey,
    onSuccess: (fresh) => {
      // R1 fix — write under the per-account scope key.
      queryClient.setQueryData(["embed-key", userId ?? "anon"], fresh);
      // Auto-reveal after rotate: the user JUST clicked Rotate; they
      // need the fresh value visible to copy + redeploy. Defensible
      // trade-off vs always-masked default.
      setRevealed(true);
      setConfirmingRotate(false);
      setRotateConfirmText("");
    },
  });

  const copyKey = () => {
    if (!keyQuery.data) return;
    navigator.clipboard.writeText(keyQuery.data);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  if (!hasAccess) {
    return (
      <div className="card">
        <div className="flex items-start gap-3">
          <div className="w-9 h-9 rounded-lg bg-cs-border/30 flex items-center justify-center shrink-0">
            <Key size={16} className="text-cs-muted" />
          </div>
          <div>
            <h3 className="text-sm font-medium">SDK trace key</h3>
            <p className="text-xs text-cs-muted mt-1 leading-relaxed">
              The SDK trace key is a Pro feature — it tells the ATO SDK which
              account to attribute traces to when your apps post receipts to
              the cloud. Upgrade to Pro to get a key.
            </p>
            <a
              href="https://agentictool.ai/#pricing"
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-1 text-xs text-cs-accent hover:underline mt-2"
            >
              See pricing <ExternalLink size={12} />
            </a>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="card space-y-3">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h3 className="text-sm font-medium flex items-center gap-2">
            <Key size={16} className="text-cs-accent" />
            SDK trace key
          </h3>
          <p className="text-xs text-cs-muted mt-1">
            Paste this into your app's <code className="text-cs-text bg-cs-border/30 px-1.5 py-0.5 rounded">
              .env
            </code> as <code className="text-cs-text bg-cs-border/30 px-1.5 py-0.5 rounded">
              ATO_API_KEY
            </code>. The ATO SDK reads it to attribute every trace your code
            sends to <strong>this</strong> account.
          </p>
        </div>
      </div>

      {/* Key row */}
      <div className="bg-cs-bg border border-cs-border rounded-md p-3">
        {keyQuery.isLoading && (
          <p className="text-xs text-cs-muted flex items-center gap-2">
            <Loader2 size={12} className="animate-spin" />
            Loading your key…
          </p>
        )}
        {keyQuery.isError && (
          <p className="text-xs text-red-400">
            Couldn't load embed key: {keyQuery.error?.message}
          </p>
        )}
        {keyQuery.data && (
          <div className="flex items-center gap-2">
            <code className="flex-1 font-mono text-sm text-cs-accent break-all">
              {revealed ? keyQuery.data : maskKey(keyQuery.data)}
            </code>
            <button
              onClick={() => setRevealed((r) => !r)}
              className="p-1.5 rounded-md text-cs-muted hover:text-cs-text hover:bg-cs-border/30 transition-colors shrink-0"
              aria-label={revealed ? "Hide key" : "Reveal key"}
            >
              {revealed ? <EyeOff size={14} /> : <Eye size={14} />}
            </button>
            <button
              onClick={copyKey}
              className="p-1.5 rounded-md text-cs-muted hover:text-cs-text hover:bg-cs-border/30 transition-colors shrink-0"
              aria-label="Copy key"
            >
              {copied ? <Check size={14} className="text-cs-accent" /> : <Copy size={14} />}
            </button>
          </div>
        )}
      </div>

      {/* Use in code snippet */}
      <details className="text-xs">
        <summary className="cursor-pointer text-cs-muted hover:text-cs-text">
          How to use it
        </summary>
        <div className="mt-2 space-y-2 pl-3">
          <p className="text-cs-muted leading-relaxed">
            1. Add to your project's <code className="text-cs-text">.env</code>:
          </p>
          <pre className="bg-cs-bg border border-cs-border rounded-md p-2 font-mono text-[11px] overflow-x-auto">
            ATO_API_KEY={keyQuery.data ? (revealed ? keyQuery.data : maskKey(keyQuery.data)) : "ato_..."}
          </pre>
          <p className="text-cs-muted leading-relaxed mt-2">
            2. Initialize the SDK in your code:
          </p>
          <pre className="bg-cs-bg border border-cs-border rounded-md p-2 font-mono text-[11px] overflow-x-auto">
{`import { init } from '@ato-sdk/js';
init({ apiKey: process.env.ATO_API_KEY });`}
          </pre>
          <p className="text-cs-muted leading-relaxed mt-2">
            Every LLM call after init is auto-traced. See{" "}
            <a
              href="https://github.com/WillNigri/Agentic-Tool-Optimization/blob/main/docs/SDK.md"
              target="_blank"
              rel="noreferrer"
              className="text-cs-accent hover:underline"
            >
              docs/SDK.md
            </a>{" "}for OpenAI / Claude Agent SDK wrappers + a non-technical
            setup guide.
          </p>
          <p className="text-[10px] text-cs-muted/70 leading-relaxed mt-2 italic">
            Note: ATO's external-agent bundle generator deploys the same key
            under the legacy env-var name <code className="not-italic">ATO_TRACE_KEY</code>. Both names refer to
            the same secret; the SDK accepts whichever your code passes to{" "}
            <code className="not-italic">init({"{ apiKey }"})</code>.
          </p>
        </div>
      </details>

      {/* Rotate — R1 fix: typed-confirm required before button arms */}
      <div className="pt-2 border-t border-cs-border/50">
        {!confirmingRotate ? (
          <button
            onClick={() => setConfirmingRotate(true)}
            className="text-xs text-cs-muted hover:text-cs-text inline-flex items-center gap-1 transition-colors"
          >
            <RefreshCw size={12} />
            Rotate key
          </button>
        ) : (
          <div className="space-y-2">
            <div className="flex items-start gap-2">
              <AlertTriangle size={14} className="text-amber-400 shrink-0 mt-0.5" />
              <p className="text-xs text-cs-text leading-relaxed">
                Rotating invalidates the current key immediately. Any deployed
                bundle still using the old value will stop sending traces.
                You'll need to redeploy with the new key.
              </p>
            </div>
            <div className="pl-6 space-y-2">
              <label className="block text-xs text-cs-muted">
                Type <code className="text-cs-text bg-cs-border/30 px-1.5 py-0.5 rounded font-mono">rotate</code> to confirm:
              </label>
              <input
                type="text"
                value={rotateConfirmText}
                onChange={(e) => setRotateConfirmText(e.target.value)}
                placeholder="rotate"
                autoFocus
                className="w-full max-w-xs px-3 py-1.5 bg-cs-bg border border-cs-border rounded-md text-xs font-mono text-cs-text placeholder:text-cs-muted/50 focus:outline-none focus:border-amber-500/50"
              />
            </div>
            <div className="flex items-center gap-2 pl-6">
              <button
                onClick={() => rotateMutation.mutate()}
                disabled={
                  rotateMutation.isPending ||
                  rotateConfirmText.trim().toLowerCase() !== "rotate"
                }
                className="px-3 py-1.5 rounded-md bg-amber-500/15 border border-amber-500/30 text-amber-300 text-xs font-medium hover:bg-amber-500/25 disabled:opacity-40 disabled:cursor-not-allowed inline-flex items-center gap-1.5 transition-colors"
              >
                {rotateMutation.isPending && <Loader2 size={12} className="animate-spin" />}
                Rotate now
              </button>
              <button
                onClick={() => {
                  setConfirmingRotate(false);
                  setRotateConfirmText("");
                }}
                className="text-xs text-cs-muted hover:text-cs-text transition-colors"
              >
                Cancel
              </button>
            </div>
            {rotateMutation.isError && (
              <p className="text-xs text-red-400 pl-6">
                Rotate failed: {rotateMutation.error?.message}
              </p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

/** Reveal first 7 chars + last 4; mask the middle. ato_xyz...AB34 */
function maskKey(k: string): string {
  if (k.length <= 14) return k.replace(/./g, "•");
  return `${k.slice(0, 7)}${"•".repeat(8)}${k.slice(-4)}`;
}
