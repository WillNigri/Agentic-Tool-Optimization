import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Copy, Check, ExternalLink, Cloud, Server, Box, Layers } from "lucide-react";
import type { Agent } from "@/lib/agents";
import {
  generateCloudflareWorker,
  DEFAULT_DEPLOY_CONFIG,
  type DeployBundleConfig,
  type DeployProvider,
  type GeneratedBundle,
} from "@/lib/deployBundleGenerators/cloudflare";
import { cn } from "@/lib/utils";

// v2.0.0 Wave 1 — Deploy tab.
//
// Shows up only for agents with kind === 'external'. Lets the user pick a
// deploy target + provider, configure CORS allowlist + trace forwarding,
// then preview / copy the generated files. Cloudflare Worker is the only
// target wired in v2.0.0; Vercel / Docker / Node ship in Wave 3.

type Target = "cloudflare" | "vercel" | "docker" | "node";

interface Props {
  agent: Agent;
}

const TARGETS: { id: Target; label: string; icon: typeof Cloud; status: "ready" | "soon" }[] = [
  { id: "cloudflare", label: "Cloudflare Worker", icon: Cloud,  status: "ready" },
  { id: "vercel",     label: "Vercel Edge",       icon: Layers, status: "soon"  },
  { id: "docker",     label: "Docker",            icon: Box,    status: "soon"  },
  { id: "node",       label: "Node script",       icon: Server, status: "soon"  },
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

  const bundle: GeneratedBundle | null = useMemo(() => {
    if (target !== "cloudflare") return null;
    return generateCloudflareWorker(agent, config);
  }, [agent, config, target]);

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
            const disabled = tgt.status === "soon";
            return (
              <button
                key={tgt.id}
                type="button"
                disabled={disabled}
                onClick={() => setTarget(tgt.id)}
                className={cn(
                  "rounded-lg border px-3 py-3 text-left text-xs transition-colors",
                  active
                    ? "border-cs-accent bg-cs-accent/10 text-cs-text"
                    : "border-cs-border bg-cs-bg text-cs-muted hover:border-cs-accent/40",
                  disabled && "cursor-not-allowed opacity-50",
                )}
              >
                <div className="flex items-center gap-2">
                  <Icon size={14} />
                  <span className="font-medium text-cs-text">{tgt.label}</span>
                </div>
                {disabled && (
                  <span className="mt-1 inline-block rounded bg-cs-border/40 px-1.5 py-0.5 text-[10px] text-cs-muted">
                    {t("agentDetail.deploy.soon", "v2.0.x")}
                  </span>
                )}
              </button>
            );
          })}
        </div>
      </section>

      {/* Provider + config */}
      {target === "cloudflare" && (
        <>
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
                  "Customer brings their own API key — set as PROVIDER_API_KEY Worker secret post-deploy.",
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
                "One per line. The Worker rejects requests from any other origin.",
              )}
            </p>
          </section>

          <section className="flex items-center gap-3">
            <input
              id="forward-traces"
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
                  "Pro+ — needs ATO_TRACE_KEY Worker secret",
                )}
              </span>
            </label>
          </section>

          {/* File preview */}
          {bundle && (
            <section className="rounded-lg border border-cs-border bg-cs-bg/40 overflow-hidden">
              <div className="flex items-center justify-between border-b border-cs-border bg-cs-bg/60 px-3 py-2">
                <div className="flex gap-1">
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
                {currentFile && (
                  <CopyButton value={bundle.files[currentFile]} />
                )}
              </div>
              <pre className="max-h-[420px] overflow-auto p-3 text-[11px] text-cs-text font-mono whitespace-pre">
                {currentFile ? bundle.files[currentFile] : ""}
              </pre>
            </section>
          )}

          {/* Post-install commands */}
          {bundle && bundle.postInstall.length > 0 && (
            <section>
              <SectionHeader
                title={t("agentDetail.deploy.postInstall", "Run after writing the files")}
                hint={t(
                  "agentDetail.deploy.postInstallHint",
                  "Wrangler commands to set secrets and deploy. Run these from your project directory.",
                )}
              />
              <pre className="rounded-lg border border-cs-border bg-cs-bg p-3 text-[11px] text-cs-text font-mono whitespace-pre-wrap">
                {bundle.postInstall.join("\n")}
              </pre>
              <a
                href="https://developers.cloudflare.com/workers/wrangler/install-and-update/"
                target="_blank"
                rel="noreferrer"
                className="mt-2 inline-flex items-center gap-1 text-[11px] text-cs-accent hover:underline"
              >
                <ExternalLink size={11} />
                {t("agentDetail.deploy.wranglerDocs", "Install Wrangler")}
              </a>
            </section>
          )}
        </>
      )}

      {target !== "cloudflare" && (
        <div className="rounded-lg border border-cs-border bg-cs-bg/40 p-6 text-sm text-cs-muted">
          {t(
            "agentDetail.deploy.targetSoon",
            "This deploy target ships in v2.0.x. Pick Cloudflare Worker for now.",
          )}
        </div>
      )}
    </div>
  );
}

function SectionHeader({ title, hint }: { title: string; hint?: string }) {
  return (
    <div className="mb-2">
      <div className="text-[11px] font-semibold uppercase tracking-wide text-cs-muted">{title}</div>
      {hint && <p className="mt-1 text-[11px] text-cs-muted">{hint}</p>}
    </div>
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
