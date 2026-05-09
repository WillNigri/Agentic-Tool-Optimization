import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation } from "@tanstack/react-query";
import { Copy, Check, ExternalLink, Cloud, Server, Box, Layers, FolderDown, Loader2, BookOpen, Key, RefreshCw } from "lucide-react";
import type { Agent } from "@/lib/agents";
import { generateCloudflareWorker } from "@/lib/deployBundleGenerators/cloudflare";
import { generateVercelEdge } from "@/lib/deployBundleGenerators/vercel";
import { generateDocker } from "@/lib/deployBundleGenerators/docker";
import { generateNodeScript } from "@/lib/deployBundleGenerators/node";
import {
  DEFAULT_DEPLOY_CONFIG,
  type DeployBundleConfig,
  type DeployProvider,
  type GeneratedBundle,
  type InlineKnowledgeChunk,
} from "@/lib/deployBundleGenerators/shared";
import { listAgentKnowledge } from "@/lib/agentKnowledge";
import { getEmbedKey, rotateEmbedKey, CloudApiError } from "@/lib/cloud-api";
import { useAuthStore } from "@/hooks/useAuth";
import { cn } from "@/lib/utils";

// v2.0.0 Wave 1 + Wave 3 — Deploy tab.
//
// Shows up only for agents with kind === 'external'. Lets the user pick a
// deploy target + provider, configure CORS allowlist + trace forwarding,
// then preview / copy the generated files. All four targets land in v2.0.0.

type Target = "cloudflare" | "vercel" | "docker" | "node";

interface Props {
  agent: Agent;
}

const TARGETS: {
  id: Target;
  label: string;
  icon: typeof Cloud;
  hint: string;
}[] = [
  { id: "cloudflare", label: "Cloudflare Worker", icon: Cloud,  hint: "Fastest cold start, generous free tier" },
  { id: "vercel",     label: "Vercel Edge",       icon: Layers, hint: "Pairs naturally with a Next.js site" },
  { id: "docker",     label: "Docker",            icon: Box,    hint: "Deploy anywhere — Railway, Fly, ECS, k8s" },
  { id: "node",       label: "Node script",       icon: Server, hint: "Single file, no build step, runs on any host" },
];

const PROVIDERS: { id: DeployProvider; label: string }[] = [
  { id: "anthropic", label: "Anthropic Claude" },
  { id: "openai",    label: "OpenAI GPT" },
  { id: "gemini",    label: "Google Gemini" },
  { id: "groq",      label: "Groq" },
  { id: "mistral",   label: "Mistral" },
  { id: "deepseek",  label: "DeepSeek" },
  { id: "xai",       label: "xAI Grok" },
  { id: "together",  label: "Together AI" },
  { id: "fireworks", label: "Fireworks" },
];

export default function DeployTab({ agent }: Props) {
  const { t } = useTranslation();
  const [target, setTarget] = useState<Target>("cloudflare");
  const [config, setConfig] = useState<DeployBundleConfig>({
    ...DEFAULT_DEPLOY_CONFIG,
    brandName: agent.displayName,
  });
  const [activeFile, setActiveFile] = useState<string | null>(null);

  // Pull chunks WITH embeddings if useKnowledge is on so generators can
  // inline them. Skipped when toggle is off — saves a 1536-floats-per-row
  // serialization round-trip.
  const { data: knowledgeChunks = [] } = useQuery({
    queryKey: ["agent-knowledge-with-emb", agent.id, config.useKnowledge],
    queryFn: () => listAgentKnowledge(agent.id, true),
    enabled: agent.kind === "external" && config.useKnowledge,
    staleTime: 30_000,
  });

  const inlineChunks: InlineKnowledgeChunk[] = useMemo(
    () =>
      knowledgeChunks
        .filter((c) => Array.isArray(c.embedding) && c.embedding.length > 0)
        .map((c) => ({ s: c.source, c: c.content, e: c.embedding ?? [] })),
    [knowledgeChunks],
  );

  const bundle: GeneratedBundle | null = useMemo(() => {
    switch (target) {
      case "cloudflare": return generateCloudflareWorker(agent, config, inlineChunks);
      case "vercel":     return generateVercelEdge(agent, config, inlineChunks);
      case "docker":     return generateDocker(agent, config, inlineChunks);
      case "node":       return generateNodeScript(agent, config, inlineChunks);
    }
  }, [agent, config, target, inlineChunks]);

  const bundleSizeKb = useMemo(() => {
    if (!bundle) return 0;
    return Math.round(
      Object.values(bundle.files).reduce((s, f) => s + f.length, 0) / 1024,
    );
  }, [bundle]);

  if (agent.kind !== "external") {
    return (
      <div className="rounded-lg border border-cs-border bg-cs-bg/40 p-6 text-sm text-cs-muted">
        {t(
          "agentDetail.deploy.internalOnly",
          "Deploy is only available for external agents. Flip this agent to External in the Overview tab to unlock deployable bundles.",
        )}
      </div>
    );
  }

  const fileNames = bundle ? Object.keys(bundle.files) : [];
  const currentFile = activeFile ?? fileNames[0] ?? null;

  return (
    <div className="space-y-6">
      {/* Target picker */}
      <section>
        <SectionHeader
          title={t("agentDetail.deploy.target", "Where to deploy?")}
          hint={t(
            "agentDetail.deploy.targetHint",
            "ATO never runs the inference. The bundle deploys to your account, talks to the LLM provider you choose, and your customer's keys never leave your infra.",
          )}
        />
        <div className="grid grid-cols-2 gap-2 md:grid-cols-4">
          {TARGETS.map((tgt) => {
            const Icon = tgt.icon;
            const active = target === tgt.id;
            return (
              <button
                key={tgt.id}
                type="button"
                onClick={() => setTarget(tgt.id)}
                className={cn(
                  "rounded-lg border px-3 py-3 text-left text-xs transition-colors",
                  active
                    ? "border-cs-accent bg-cs-accent/10 text-cs-text"
                    : "border-cs-border bg-cs-bg text-cs-muted hover:border-cs-accent/40",
                )}
              >
                <div className="flex items-center gap-2">
                  <Icon size={14} />
                  <span className="font-medium text-cs-text">{tgt.label}</span>
                </div>
                <div className="mt-1 text-[11px] leading-tight text-cs-muted">{tgt.hint}</div>
              </button>
            );
          })}
        </div>
      </section>

      {/* Provider + config */}
      <section className="grid gap-4 md:grid-cols-2">
        <div>
          <label className="text-[10px] uppercase tracking-wide text-cs-muted">
            {t("agentDetail.deploy.provider", "LLM provider")}
          </label>
          <select
            value={config.provider}
            onChange={(e) => setConfig((c) => ({ ...c, provider: e.target.value as DeployProvider }))}
            className="mt-1 w-full rounded-lg border border-cs-border bg-cs-bg px-3 py-2 text-sm text-cs-text"
          >
            {PROVIDERS.map((p) => (
              <option key={p.id} value={p.id}>{p.label}</option>
            ))}
          </select>
          <p className="mt-1 text-[11px] text-cs-muted">
            {t(
              "agentDetail.deploy.providerHint",
              "Customer brings their own API key — set as PROVIDER_API_KEY at deploy time.",
            )}
          </p>
        </div>
        <div>
          <label className="text-[10px] uppercase tracking-wide text-cs-muted">
            {t("agentDetail.deploy.model", "Model override (optional)")}
          </label>
          <input
            type="text"
            value={config.model ?? ""}
            onChange={(e) => setConfig((c) => ({ ...c, model: e.target.value || undefined }))}
            placeholder={agent.model ?? "claude-sonnet-4-6"}
            className="mt-1 w-full rounded-lg border border-cs-border bg-cs-bg px-3 py-2 text-sm text-cs-text font-mono"
          />
        </div>
      </section>

      <section>
        <label className="text-[10px] uppercase tracking-wide text-cs-muted">
          {t("agentDetail.deploy.allowedOrigins", "Embed allowed origins")}
        </label>
        <textarea
          rows={2}
          value={config.allowedOrigins.join("\n")}
          onChange={(e) =>
            setConfig((c) => ({
              ...c,
              allowedOrigins: e.target.value
                .split("\n")
                .map((s) => s.trim())
                .filter(Boolean),
            }))
          }
          placeholder="https://acme.com&#10;https://staging.acme.com"
          className="mt-1 w-full rounded-lg border border-cs-border bg-cs-bg px-3 py-2 text-xs text-cs-text font-mono"
        />
        <p className="mt-1 text-[11px] text-cs-muted">
          {t(
            "agentDetail.deploy.allowedOriginsHint",
            "One per line. The deployed bundle rejects requests from any other origin.",
          )}
        </p>
      </section>

      <section className="space-y-2">
        <div className="flex items-center gap-3">
          <input
            id="use-knowledge"
            type="checkbox"
            checked={config.useKnowledge}
            onChange={(e) => setConfig((c) => ({ ...c, useKnowledge: e.target.checked }))}
            className="h-4 w-4 rounded border-cs-border bg-cs-bg accent-cs-accent"
          />
          <label htmlFor="use-knowledge" className="text-sm text-cs-text">
            <span className="inline-flex items-center gap-1.5">
              <BookOpen size={11} />
              {t("agentDetail.deploy.useKnowledge", "Inline knowledge for RAG retrieval")}
            </span>
            <span className="ml-2 text-[11px] text-cs-muted">
              {config.useKnowledge && knowledgeChunks.length === 0
                ? t(
                    "agentDetail.deploy.useKnowledgeEmpty",
                    "No chunks yet — open the Knowledge tab to add some",
                  )
                : t(
                    "agentDetail.deploy.useKnowledgeHint",
                    "Bake chunks + embeddings into the bundle. Needs EMBED_API_KEY (OpenAI) at deploy time.",
                  )}
            </span>
          </label>
        </div>
        <div className="flex items-center gap-3">
          <input
            id="forward-traces"
            data-demo-id="deploy-forward-traces"
            type="checkbox"
            checked={config.forwardTraces}
            onChange={(e) => setConfig((c) => ({ ...c, forwardTraces: e.target.checked }))}
            className="h-4 w-4 rounded border-cs-border bg-cs-bg accent-cs-accent"
          />
          <label htmlFor="forward-traces" className="text-sm text-cs-text">
            {t("agentDetail.deploy.forwardTraces", "Stream traces to ATO Insights")}
            <span className="ml-2 text-[11px] text-cs-muted">
              {t(
                "agentDetail.deploy.forwardTracesHint",
                "Pro+ — needs ATO_TRACE_KEY env var on the deployed bundle",
              )}
            </span>
          </label>
        </div>
        {config.forwardTraces && <EmbedKeyPanel />}
      </section>

      {/* Cloudflare Worker hard-fails to deploy if the script exceeds 1MB.
          Warn at 800KB so the user has headroom — typical FAQ bundles
          land at 50-300KB so this only fires on really large knowledge sets. */}
      {target === "cloudflare" && bundleSizeKb > 800 && (
        <section className="rounded-md border border-cs-warn/40 bg-cs-warn/10 p-3 text-xs text-cs-text">
          ⚠️ {t(
            "agentDetail.deploy.bundleSizeWarn",
            "Bundle is {{size}} KB — Cloudflare Workers cap at 1 MB. Consider trimming knowledge or splitting the agent.",
            { size: bundleSizeKb },
          )}
        </section>
      )}

      {/* File preview */}
      {bundle && (
        <section className="rounded-lg border border-cs-border bg-cs-bg/40 overflow-hidden">
          <div className="flex items-center justify-between gap-2 border-b border-cs-border bg-cs-bg/60 px-3 py-2">
            <div className="flex flex-wrap gap-1">
              {fileNames.map((name) => (
                <button
                  key={name}
                  type="button"
                  onClick={() => setActiveFile(name)}
                  className={cn(
                    "rounded-md px-3 py-1 text-xs font-mono",
                    currentFile === name
                      ? "bg-cs-accent/10 text-cs-accent"
                      : "text-cs-muted hover:text-cs-text",
                  )}
                >
                  {name}
                </button>
              ))}
            </div>
            <div className="flex items-center gap-2">
              <SaveBundleButton bundle={bundle} agentSlug={agent.slug} />
              {currentFile && <CopyButton value={bundle.files[currentFile]} />}
            </div>
          </div>
          <pre className="max-h-[420px] overflow-auto p-3 text-[11px] text-cs-text font-mono whitespace-pre">
            {currentFile ? bundle.files[currentFile] : ""}
          </pre>
        </section>
      )}

      {/* v2.0.0 Wave 4 — embed widget snippet preview. Every bundle now
          ships `embed.html` (test page) + `embed.js` (the chat-bubble
          widget). Customer drops one <script> tag on their site after
          replacing data-endpoint with their deployed URL. */}
      {bundle && (bundle.files["embed.js"] || bundle.files["public/embed.js"] || bundle.files["embed/embed.js"]) && (
        <section className="rounded-lg border border-cs-border bg-cs-bg-raised/40 p-3 space-y-2">
          <SectionHeader
            title={t("agentDetail.deploy.embedTitle", "Embed widget — paste this on your site")}
            hint={t(
              "agentDetail.deploy.embedHint",
              "Bundle includes embed.html (a working test page) and embed.js (the chat-bubble widget). Host embed.js on your CDN, then paste the snippet below into any HTML page. Replace data-endpoint with your deployed URL after wrangler/vercel/docker deploy prints it.",
            )}
          />
          <pre className="rounded border border-cs-border bg-cs-bg p-3 text-[11px] text-cs-text font-mono whitespace-pre-wrap break-all">
            {`<script src="https://your-cdn.example.com/embed.js"
        data-endpoint="https://your-deployed-agent.example.com"
        data-brand=${JSON.stringify(config.brandName || agent.displayName)}
        data-color="#00FFB2"
        data-greeting="Hi! How can I help?"
        data-agent-slug=${JSON.stringify(agent.slug)}></script>`}
          </pre>
        </section>
      )}

      {/* Post-install commands */}
      {bundle && bundle.postInstall.length > 0 && (
        <section>
          <SectionHeader
            title={t("agentDetail.deploy.postInstall", "Run after writing the files")}
            hint={postInstallHint(target, t)}
          />
          <pre className="rounded-lg border border-cs-border bg-cs-bg p-3 text-[11px] text-cs-text font-mono whitespace-pre-wrap">
            {bundle.postInstall.join("\n")}
          </pre>
          {target === "cloudflare" && (
            <a
              href="https://developers.cloudflare.com/workers/wrangler/install-and-update/"
              target="_blank"
              rel="noreferrer"
              className="mt-2 inline-flex items-center gap-1 text-[11px] text-cs-accent hover:underline"
            >
              <ExternalLink size={11} />
              {t("agentDetail.deploy.wranglerDocs", "Install Wrangler")}
            </a>
          )}
          {target === "vercel" && (
            <a
              href="https://vercel.com/docs/cli"
              target="_blank"
              rel="noreferrer"
              className="mt-2 inline-flex items-center gap-1 text-[11px] text-cs-accent hover:underline"
            >
              <ExternalLink size={11} />
              {t("agentDetail.deploy.vercelDocs", "Install Vercel CLI")}
            </a>
          )}
        </section>
      )}
    </div>
  );
}

function postInstallHint(target: Target, t: (k: string, fb: string) => string): string {
  switch (target) {
    case "cloudflare":
      return t("agentDetail.deploy.postInstallCloudflare", "Wrangler commands to set secrets and deploy. Run from your project directory.");
    case "vercel":
      return t("agentDetail.deploy.postInstallVercel", "Vercel CLI commands. Set local vars first, then deploy.");
    case "docker":
      return t("agentDetail.deploy.postInstallDocker", "Build the image, run with secrets injected via -e flags.");
    case "node":
      return t("agentDetail.deploy.postInstallNode", "Run locally first, then deploy to Railway / Render / Fly with the same env vars.");
  }
}

function SectionHeader({ title, hint }: { title: string; hint?: string }) {
  return (
    <div className="mb-2">
      <div className="text-[11px] font-semibold uppercase tracking-wide text-cs-muted">{title}</div>
      {hint && <p className="mt-1 text-[11px] text-cs-muted">{hint}</p>}
    </div>
  );
}

function SaveBundleButton({ bundle, agentSlug }: { bundle: GeneratedBundle; agentSlug: string }) {
  const [state, setState] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  // Save the entire bundle into a folder the user picks. Tauri only — falls
  // back gracefully when the dialog plugin import fails (e.g., browser dev
  // mode), and surfaces the error inline rather than blowing up the tab.
  const onSave = async () => {
    setState("saving");
    setErrorMsg(null);
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const { writeTextFile, mkdir } = await import("@tauri-apps/plugin-fs");
      const dir = await open({
        directory: true,
        multiple: false,
        title: "Pick a folder for the deploy bundle",
      });
      if (!dir || typeof dir !== "string") {
        setState("idle");
        return;
      }
      const root = `${dir}/${agentSlug}`;
      // Best-effort recursive mkdir so nested paths like
      // app/api/agent/route.ts (Vercel target) work.
      await mkdir(root, { recursive: true }).catch(() => undefined);
      for (const [relPath, contents] of Object.entries(bundle.files)) {
        const full = `${root}/${relPath}`;
        const parent = full.substring(0, full.lastIndexOf("/"));
        if (parent && parent !== root) {
          await mkdir(parent, { recursive: true }).catch(() => undefined);
        }
        await writeTextFile(full, contents);
      }
      setState("saved");
      setTimeout(() => setState("idle"), 1500);
    } catch (err) {
      setState("error");
      setErrorMsg(err instanceof Error ? err.message : String(err));
      setTimeout(() => setState("idle"), 3000);
    }
  };

  const label =
    state === "saving" ? "Saving…" :
    state === "saved"  ? "Saved" :
    state === "error"  ? errorMsg ?? "Error" :
    "Save bundle…";

  return (
    <button
      type="button"
      onClick={onSave}
      disabled={state === "saving"}
      className={cn(
        "inline-flex items-center gap-1 rounded-md border px-2 py-1 text-[11px] transition-colors",
        state === "saved" ? "border-cs-accent/40 text-cs-accent" : "border-cs-border bg-cs-bg text-cs-muted hover:text-cs-text",
        state === "error" && "border-red-500/40 text-red-400",
      )}
      title={errorMsg ?? undefined}
    >
      {state === "saving" ? <Loader2 size={11} className="animate-spin" /> : state === "saved" ? <Check size={11} /> : <FolderDown size={11} />}
      {label}
    </button>
  );
}

function CopyButton({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  const onCopy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch {
      /* clipboard blocked — silent */
    }
  };
  return (
    <button
      type="button"
      onClick={onCopy}
      className="inline-flex items-center gap-1 rounded-md border border-cs-border bg-cs-bg px-2 py-1 text-[11px] text-cs-muted hover:text-cs-text"
    >
      {copied ? <Check size={11} className="text-cs-accent" /> : <Copy size={11} />}
      {copied ? "Copied" : "Copy"}
    </button>
  );
}

/** v2.1.0 — Embed key surface for the trace-forwarding flow.
 *
 *  Shown only when "Stream traces to ATO Insights" is enabled. Mints
 *  the user's embed key on demand (server-side first-read), shows a
 *  masked + copyable preview, and offers a rotate button. Free-tier
 *  users see a friendly upgrade hint instead of the key. Local-only
 *  users (no cloud login) see a sign-in prompt. */
function EmbedKeyPanel() {
  const { t } = useTranslation();
  const isCloudUser = useAuthStore((s) => s.isCloudUser);
  const accessToken = useAuthStore((s) => s.accessToken);
  const mock = import.meta.env.VITE_USE_MOCK_CLOUD === "true";
  const canFetch = mock || (!!isCloudUser && !!accessToken);
  const [revealed, setRevealed] = useState(false);
  const [copied, setCopied] = useState(false);

  // Mint-on-read: the cloud creates the key the first time this fires.
  // Pro+ check happens server-side; free returns 403 / TIER_REQUIRED
  // which we surface as the upgrade hint.
  const keyQuery = useQuery({
    queryKey: ["embed-key", accessToken, mock],
    queryFn: getEmbedKey,
    enabled: canFetch,
    staleTime: Infinity,
    retry: false,
  });

  const rotateMut = useMutation({
    mutationFn: rotateEmbedKey,
    onSuccess: (newKey) => {
      keyQuery.refetch();
      // Reset reveal/copy state so the operator explicitly handles the
      // new value instead of trusting stale UI.
      setRevealed(true);
      setCopied(false);
      void newKey;
    },
  });

  if (!canFetch) {
    return (
      <div className="ml-7 mt-2 rounded-md border border-cs-border bg-cs-bg-raised/40 p-3 text-[11px] text-cs-muted">
        {t(
          "agentDetail.deploy.embedKeySignIn",
          "Sign in via Settings → Cloud to mint the embed key the deployed bundle needs.",
        )}
      </div>
    );
  }

  if (keyQuery.isLoading) {
    return (
      <div className="ml-7 mt-2 text-[11px] text-cs-muted">
        <Loader2 size={11} className="inline animate-spin mr-1" />
        {t("agentDetail.deploy.embedKeyLoading", "Fetching embed key…")}
      </div>
    );
  }

  if (keyQuery.error) {
    const err = keyQuery.error;
    if (err instanceof CloudApiError && err.code === "TIER_REQUIRED") {
      return (
        <div className="ml-7 mt-2 rounded-md border border-cs-warn/40 bg-cs-warn/10 p-3 text-[11px] text-cs-text">
          {t(
            "agentDetail.deploy.embedKeyTierRequired",
            "Embed keys (and the cloud Insights dashboard) require Pro tier. Upgrade in Settings → Cloud to enable trace forwarding.",
          )}
        </div>
      );
    }
    return (
      <div className="ml-7 mt-2 rounded-md border border-cs-danger/40 bg-cs-danger/10 p-3 text-[11px] text-cs-text">
        {t("agentDetail.deploy.embedKeyError", "Couldn't load embed key:")} {String(err)}
      </div>
    );
  }

  const key = keyQuery.data ?? "";
  // Show a masked preview by default — the key is plaintext-equivalent
  // to a long-lived bearer token and shouldn't be casually visible
  // over-the-shoulder during demos / screen shares.
  const masked = key ? `${key.slice(0, 8)}${"•".repeat(20)}${key.slice(-4)}` : "";

  const onCopy = async () => {
    try {
      await navigator.clipboard.writeText(key);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      /* clipboard blocked — silent */
    }
  };

  return (
    <div className="ml-7 mt-2 rounded-md border border-cs-border bg-cs-bg-raised/40 p-3 space-y-2">
      <div className="flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wide text-cs-muted">
        <Key size={11} />
        {t("agentDetail.deploy.embedKeyLabel", "ATO_TRACE_KEY for this account")}
      </div>
      <div className="flex items-center gap-2">
        <code className="flex-1 truncate rounded border border-cs-border bg-cs-bg px-2 py-1.5 font-mono text-[11px] text-cs-text">
          {revealed ? key : masked}
        </code>
        <button
          type="button"
          data-demo-id="embed-key-reveal"
          onClick={() => setRevealed((v) => !v)}
          className="rounded-md border border-cs-border bg-cs-bg px-2 py-1 text-[11px] text-cs-muted hover:text-cs-text"
        >
          {revealed
            ? t("agentDetail.deploy.embedKeyHide", "Hide")
            : t("agentDetail.deploy.embedKeyReveal", "Reveal")}
        </button>
        <button
          type="button"
          data-demo-id="embed-key-copy"
          onClick={onCopy}
          className="inline-flex items-center gap-1 rounded-md border border-cs-border bg-cs-bg px-2 py-1 text-[11px] text-cs-muted hover:text-cs-text"
        >
          {copied ? <Check size={11} className="text-cs-accent" /> : <Copy size={11} />}
          {copied
            ? t("common.copied", "Copied")
            : t("common.copy", "Copy")}
        </button>
      </div>
      <div className="flex items-center justify-between gap-2">
        <span className="text-[11px] text-cs-muted">
          {t(
            "agentDetail.deploy.embedKeyHint",
            "Set as ATO_TRACE_KEY env var on the deployed bundle. Same key for every external agent on this account.",
          )}
        </span>
        <button
          type="button"
          onClick={() => {
            if (
              confirm(
                t(
                  "agentDetail.deploy.embedKeyRotateConfirm",
                  "Rotate the key? The current key will stop working immediately — every deployed bundle using it will need to be re-deployed with the new value.",
                ),
              )
            ) {
              rotateMut.mutate();
            }
          }}
          disabled={rotateMut.isPending}
          className="inline-flex items-center gap-1 text-[11px] text-cs-muted hover:text-cs-accent disabled:opacity-50"
        >
          {rotateMut.isPending ? (
            <Loader2 size={11} className="animate-spin" />
          ) : (
            <RefreshCw size={11} />
          )}
          {t("agentDetail.deploy.embedKeyRotate", "Rotate key")}
        </button>
      </div>
    </div>
  );
}
