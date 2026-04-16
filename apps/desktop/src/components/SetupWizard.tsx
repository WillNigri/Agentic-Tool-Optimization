import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import {
  Terminal,
  Cpu,
  Server,
  Globe,
  CheckCircle,
  XCircle,
  Loader2,
  ArrowRight,
  ArrowLeft,
  Zap,
  Shield,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { queryAgentStatus, detectAgentRuntimes, setRuntimePath } from "@/lib/api";
import type { AgentRuntime, OpenClawConfig } from "@/components/cron/types";
import type { AgentStatus } from "@/lib/api";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface RuntimeSetup {
  runtime: AgentRuntime;
  label: string;
  icon: typeof Terminal;
  color: string;
  description: string;
  enabled: boolean;
  status: AgentStatus | null;
  checking: boolean;
  config: Record<string, string>;
}

const INITIAL_RUNTIMES: RuntimeSetup[] = [
  {
    runtime: "claude",
    label: "Claude Code",
    icon: Terminal,
    color: "#f97316",
    description: "Anthropic's CLI for AI-assisted coding. Uses your Claude Code subscription.",
    enabled: true,
    status: null,
    checking: false,
    config: {},
  },
  {
    runtime: "codex",
    label: "Codex",
    icon: Cpu,
    color: "#22c55e",
    description: "OpenAI's coding agent. Requires OPENAI_API_KEY or Codex CLI installed.",
    enabled: false,
    status: null,
    checking: false,
    config: {},
  },
  {
    runtime: "openclaw",
    label: "OpenClaw",
    icon: Server,
    color: "#06b6d4",
    description: "Remote agent accessible via SSH. Configure host, port, and SSH key.",
    enabled: false,
    status: null,
    checking: false,
    config: {},
  },
  {
    runtime: "hermes",
    label: "Hermes",
    icon: Globe,
    color: "#a855f7",
    description: "Hermes agent runtime. Local CLI or remote endpoint.",
    enabled: false,
    status: null,
    checking: false,
    config: {},
  },
];

// ---------------------------------------------------------------------------
// Steps
// ---------------------------------------------------------------------------

type Step = "welcome" | "runtimes" | "verify" | "done";

const STEPS: Step[] = ["welcome", "runtimes", "verify", "done"];

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

interface SetupWizardProps {
  onComplete: () => void;
}

export default function SetupWizard({ onComplete }: SetupWizardProps) {
  const { t } = useTranslation();
  const [step, setStep] = useState<Step>("welcome");
  const [runtimes, setRuntimes] = useState<RuntimeSetup[]>(INITIAL_RUNTIMES);

  // Auto-detect on mount
  useEffect(() => {
    detectAgentRuntimes().then((detected) => {
      setRuntimes((prev) =>
        prev.map((rt) => {
          const found = detected.find((d) => d.runtime === rt.runtime);
          return {
            ...rt,
            enabled: found?.available || rt.runtime === "claude",
            status: found
              ? {
                  runtime: found.runtime,
                  available: found.available,
                  healthy: found.available,
                  version: found.version || null,
                  path: found.path || null,
                  details: {},
                }
              : null,
          };
        })
      );
    });
  }, []);

  function toggleRuntime(runtime: AgentRuntime) {
    setRuntimes((prev) =>
      prev.map((rt) =>
        rt.runtime === runtime ? { ...rt, enabled: !rt.enabled } : rt
      )
    );
  }

  function updateConfig(runtime: AgentRuntime, key: string, value: string) {
    setRuntimes((prev) =>
      prev.map((rt) =>
        rt.runtime === runtime
          ? { ...rt, config: { ...rt.config, [key]: value } }
          : rt
      )
    );
  }

  async function handleSetPath(runtime: AgentRuntime, path: string) {
    await setRuntimePath(runtime, path);
    // Re-detect after setting custom path
    const status = await queryAgentStatus(runtime);
    setRuntimes((prev) =>
      prev.map((rt) =>
        rt.runtime === runtime
          ? { ...rt, status, config: { ...rt.config, customPath: path } }
          : rt
      )
    );
  }

  async function verifyRuntime(runtime: AgentRuntime) {
    setRuntimes((prev) =>
      prev.map((rt) =>
        rt.runtime === runtime ? { ...rt, checking: true } : rt
      )
    );

    const rt = runtimes.find((r) => r.runtime === runtime);
    const configJson = rt?.config && Object.keys(rt.config).length > 0
      ? rt.config
      : undefined;

    const status = await queryAgentStatus(
      runtime,
      configJson as unknown as OpenClawConfig | undefined
    );

    setRuntimes((prev) =>
      prev.map((rt) =>
        rt.runtime === runtime ? { ...rt, status, checking: false } : rt
      )
    );
  }

  async function verifyAll() {
    const enabled = runtimes.filter((rt) => rt.enabled);
    await Promise.all(enabled.map((rt) => verifyRuntime(rt.runtime)));
  }

  function handleComplete() {
    // Save setup state to localStorage
    const setupData = {
      completedAt: new Date().toISOString(),
      runtimes: runtimes
        .filter((rt) => rt.enabled)
        .map((rt) => ({
          runtime: rt.runtime,
          config: rt.config,
          verified: rt.status?.healthy || false,
        })),
    };
    localStorage.setItem("ato-setup", JSON.stringify(setupData));
    onComplete();
  }

  const stepIndex = STEPS.indexOf(step);
  const canNext = step !== "done";
  const canBack = stepIndex > 0 && step !== "done";

  return (
    <div className="fixed inset-0 bg-cs-bg z-50 flex items-center justify-center">
      <div className="w-full max-w-2xl mx-4">
        {/* Progress bar */}
        <div className="flex items-center gap-1 mb-8">
          {STEPS.map((s, i) => (
            <div
              key={s}
              className={cn(
                "h-1 flex-1 rounded-full transition-colors",
                i <= stepIndex ? "bg-cs-accent" : "bg-cs-border"
              )}
            />
          ))}
        </div>

        {/* Step content */}
        <div className="bg-cs-card border border-cs-border rounded-2xl p-8 shadow-2xl">
          {step === "welcome" && (
            <WelcomeStep onNext={() => setStep("runtimes")} />
          )}

          {step === "runtimes" && (
            <RuntimesStep
              runtimes={runtimes}
              onToggle={toggleRuntime}
              onUpdateConfig={updateConfig}
              onSetPath={handleSetPath}
            />
          )}

          {step === "verify" && (
            <VerifyStep
              runtimes={runtimes.filter((rt) => rt.enabled)}
              onVerify={verifyRuntime}
              onVerifyAll={verifyAll}
            />
          )}

          {step === "done" && <DoneStep />}
        </div>

        {/* Navigation */}
        <div className="flex items-center justify-between mt-6">
          {canBack ? (
            <button
              onClick={() => setStep(STEPS[stepIndex - 1])}
              className="flex items-center gap-2 px-4 py-2 text-sm rounded-lg border border-cs-border text-cs-muted hover:text-cs-text transition-colors"
            >
              <ArrowLeft size={14} />
              Back
            </button>
          ) : (
            <div />
          )}

          {step === "done" ? (
            <button
              onClick={handleComplete}
              className="flex items-center gap-2 px-6 py-2.5 text-sm rounded-lg bg-cs-accent text-cs-bg font-semibold hover:bg-cs-accent/90 transition-colors"
            >
              <Zap size={14} />
              Start Using ATO
            </button>
          ) : (
            <button
              onClick={() => {
                if (step === "runtimes") {
                  verifyAll().then(() => setStep("verify"));
                } else {
                  setStep(STEPS[stepIndex + 1]);
                }
              }}
              className="flex items-center gap-2 px-5 py-2.5 text-sm rounded-lg bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 transition-colors"
            >
              {step === "runtimes" ? "Verify Connections" : "Next"}
              <ArrowRight size={14} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Step components
// ---------------------------------------------------------------------------

function WelcomeStep({ onNext }: { onNext: () => void }) {
  return (
    <div className="text-center">
      <div className="w-16 h-16 rounded-2xl bg-cs-accent/10 border border-cs-accent/30 flex items-center justify-center mx-auto mb-6">
        <Zap size={28} className="text-cs-accent" />
      </div>
      <h2 className="text-2xl font-bold mb-2">Welcome to ATO</h2>
      <p className="text-cs-muted mb-6 max-w-md mx-auto">
        The multi-LLM control panel for AI coding tools. Let's connect your runtimes so you can manage skills, run automations, and monitor jobs across all your agents.
      </p>
      <div className="grid grid-cols-4 gap-3 max-w-sm mx-auto">
        {INITIAL_RUNTIMES.map((rt) => {
          const Icon = rt.icon;
          return (
            <div
              key={rt.runtime}
              className="flex flex-col items-center gap-1.5 p-3 rounded-xl border border-cs-border"
            >
              <Icon size={20} style={{ color: rt.color }} />
              <span className="text-[10px] font-medium text-cs-muted">{rt.label}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function RuntimesStep({
  runtimes,
  onToggle,
  onUpdateConfig,
  onSetPath,
}: {
  runtimes: RuntimeSetup[];
  onToggle: (runtime: AgentRuntime) => void;
  onUpdateConfig: (runtime: AgentRuntime, key: string, value: string) => void;
  onSetPath: (runtime: AgentRuntime, path: string) => void;
}) {
  return (
    <div>
      <h2 className="text-xl font-bold mb-1">Connect Your Runtimes</h2>
      <p className="text-cs-muted text-sm mb-6">
        Enable the AI coding tools you use. ATO will scan their skill directories and connect to their CLIs.
      </p>

      <div className="space-y-3">
        {runtimes.map((rt) => {
          const Icon = rt.icon;
          return (
            <div
              key={rt.runtime}
              className={cn(
                "rounded-xl border p-4 transition-colors",
                rt.enabled
                  ? "border-cs-accent/30 bg-cs-accent/5"
                  : "border-cs-border"
              )}
            >
              <div className="flex items-center justify-between mb-2">
                <div className="flex items-center gap-3">
                  <Icon size={20} style={{ color: rt.color }} />
                  <div>
                    <p className="text-sm font-semibold">{rt.label}</p>
                    <p className="text-[11px] text-cs-muted">{rt.description}</p>
                  </div>
                </div>
                <button
                  onClick={() => onToggle(rt.runtime)}
                  className={cn(
                    "relative w-10 h-5.5 rounded-full transition-colors duration-200",
                    rt.enabled ? "bg-cs-accent" : "bg-cs-border"
                  )}
                  style={{ width: 40, height: 22 }}
                >
                  <span
                    className={cn(
                      "absolute top-0.5 left-0.5 w-4.5 h-4.5 bg-white rounded-full transition-transform duration-200",
                      rt.enabled && "translate-x-[18px]"
                    )}
                    style={{ width: 18, height: 18 }}
                  />
                </button>
              </div>

              {/* Runtime-specific config fields */}
              {rt.enabled && rt.runtime === "openclaw" && (
                <div className="grid grid-cols-2 gap-2 mt-3 pl-8">
                  <input
                    type="text"
                    className="input text-xs"
                    placeholder="SSH Host"
                    value={rt.config.sshHost || ""}
                    onChange={(e) => onUpdateConfig("openclaw", "sshHost", e.target.value)}
                  />
                  <input
                    type="number"
                    className="input text-xs"
                    placeholder="Port (22)"
                    value={rt.config.sshPort || ""}
                    onChange={(e) => onUpdateConfig("openclaw", "sshPort", e.target.value)}
                  />
                  <input
                    type="text"
                    className="input text-xs"
                    placeholder="SSH User"
                    value={rt.config.sshUser || ""}
                    onChange={(e) => onUpdateConfig("openclaw", "sshUser", e.target.value)}
                  />
                  <input
                    type="text"
                    className="input text-xs"
                    placeholder="SSH Key Path"
                    value={rt.config.sshKeyPath || ""}
                    onChange={(e) => onUpdateConfig("openclaw", "sshKeyPath", e.target.value)}
                  />
                </div>
              )}

              {rt.enabled && rt.runtime === "codex" && (
                <div className="mt-3 pl-8">
                  <input
                    type="text"
                    className="input text-xs"
                    placeholder="API Key Path (optional — uses OPENAI_API_KEY env if not set)"
                    value={rt.config.apiKeyPath || ""}
                    onChange={(e) => onUpdateConfig("codex", "apiKeyPath", e.target.value)}
                  />
                </div>
              )}

              {rt.enabled && rt.runtime === "hermes" && (
                <div className="mt-3 pl-8">
                  <input
                    type="text"
                    className="input text-xs"
                    placeholder="Endpoint URL (optional — uses local CLI if not set)"
                    value={rt.config.endpoint || ""}
                    onChange={(e) => onUpdateConfig("hermes", "endpoint", e.target.value)}
                  />
                </div>
              )}

              {/* Auto-detected status */}
              {rt.status?.available && rt.enabled && (
                <div className="flex items-center gap-1.5 mt-2 pl-8 text-[10px] text-green-400">
                  <CheckCircle size={10} />
                  Auto-detected{rt.status.version ? ` (${rt.status.version})` : ""}
                  {rt.status.path && <span className="text-cs-muted font-mono ml-1">{rt.status.path}</span>}
                </div>
              )}

              {/* Fallback: manual path input when not auto-detected */}
              {rt.enabled && !rt.status?.available && rt.runtime !== "openclaw" && (
                <div className="mt-3 pl-8">
                  <div className="flex items-center gap-1.5 mb-1.5 text-[10px] text-yellow-400">
                    <XCircle size={10} />
                    Not found automatically — enter the path to the CLI binary
                  </div>
                  <div className="flex gap-2">
                    <input
                      type="text"
                      className="input text-xs flex-1"
                      placeholder={`/path/to/${rt.runtime}`}
                      value={rt.config.customPath || ""}
                      onChange={(e) => onUpdateConfig(rt.runtime, "customPath", e.target.value)}
                    />
                    <button
                      type="button"
                      onClick={() => {
                        const path = rt.config.customPath;
                        if (path?.trim()) onSetPath(rt.runtime, path.trim());
                      }}
                      disabled={!rt.config.customPath?.trim()}
                      className="px-3 py-1.5 text-[11px] rounded-lg bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50 shrink-0"
                    >
                      Verify
                    </button>
                  </div>
                  <p className="text-[9px] text-cs-muted mt-1">
                    Tip: run <code className="text-cs-accent">which {rt.runtime}</code> in your terminal to find the path
                  </p>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function VerifyStep({
  runtimes,
  onVerify,
  onVerifyAll,
}: {
  runtimes: RuntimeSetup[];
  onVerify: (runtime: AgentRuntime) => void;
  onVerifyAll: () => void;
}) {
  return (
    <div>
      <h2 className="text-xl font-bold mb-1">Connection Status</h2>
      <p className="text-cs-muted text-sm mb-6">
        Verifying each runtime is reachable, authenticated, and ready to use.
      </p>

      <div className="space-y-2">
        {runtimes.map((rt) => {
          const Icon = rt.icon;
          const isHealthy = rt.status?.healthy;
          const isAvailable = rt.status?.available;
          const isChecking = rt.checking;

          return (
            <div
              key={rt.runtime}
              className={cn(
                "flex items-center gap-3 px-4 py-3 rounded-xl border",
                isHealthy
                  ? "border-green-500/30 bg-green-500/5"
                  : isAvailable === false
                    ? "border-red-500/30 bg-red-500/5"
                    : "border-cs-border"
              )}
            >
              <Icon size={18} style={{ color: rt.color }} />
              <div className="flex-1 min-w-0">
                <p className="text-sm font-medium">{rt.label}</p>
                {rt.status && (
                  <p className="text-[10px] text-cs-muted font-mono truncate">
                    {rt.status.path || "Not found"}
                    {rt.status.version && ` — ${rt.status.version}`}
                  </p>
                )}
                {rt.status && !isAvailable && (
                  <p className="text-[10px] text-red-400">
                    CLI not found — install it or check your PATH
                  </p>
                )}
                {rt.status?.details && typeof rt.status.details === "object" && (
                  <div className="flex items-center gap-2 mt-0.5 text-[9px] text-cs-muted">
                    {rt.status.details.authenticated !== undefined && (
                      <span className={rt.status.details.authenticated ? "text-green-400" : "text-yellow-400"}>
                        {rt.status.details.authenticated ? "Authenticated" : "Not authenticated"}
                      </span>
                    )}
                    {rt.status.details.apiKeyEnv && (
                      <span>
                        API Key: {String(rt.status.details.apiKeyEnv)}
                      </span>
                    )}
                    {rt.status.details.sshReachable !== undefined && (
                      <span className={rt.status.details.sshReachable ? "text-green-400" : "text-red-400"}>
                        SSH: {rt.status.details.sshReachable ? "reachable" : "unreachable"}
                      </span>
                    )}
                  </div>
                )}
              </div>

              {/* Status indicator */}
              <div className="shrink-0">
                {isChecking ? (
                  <Loader2 size={16} className="text-yellow-400 animate-spin" />
                ) : isHealthy ? (
                  <CheckCircle size={16} className="text-green-400" />
                ) : isAvailable === false ? (
                  <XCircle size={16} className="text-red-400" />
                ) : (
                  <button
                    onClick={() => onVerify(rt.runtime)}
                    className="px-2.5 py-1 text-[10px] rounded-lg border border-cs-border text-cs-muted hover:text-cs-text transition-colors"
                  >
                    Re-check
                  </button>
                )}
              </div>
            </div>
          );
        })}
      </div>

      <button
        onClick={onVerifyAll}
        className="flex items-center gap-1.5 mt-4 px-3 py-1.5 text-xs rounded-lg border border-cs-border text-cs-muted hover:text-cs-text transition-colors"
      >
        <Loader2 size={12} />
        Re-check all
      </button>
    </div>
  );
}

function DoneStep() {
  return (
    <div className="text-center">
      <div className="w-16 h-16 rounded-2xl bg-green-500/10 border border-green-500/30 flex items-center justify-center mx-auto mb-6">
        <Shield size={28} className="text-green-400" />
      </div>
      <h2 className="text-2xl font-bold mb-2">You're All Set</h2>
      <p className="text-cs-muted mb-4 max-w-md mx-auto">
        ATO will now scan your connected runtimes for skills, monitor cron jobs, and let you run automations across all your AI coding agents.
      </p>
      <div className="text-left bg-cs-bg rounded-xl border border-cs-border p-4 max-w-sm mx-auto">
        <p className="text-xs font-semibold text-cs-muted uppercase tracking-wider mb-2">What's next</p>
        <ul className="space-y-1.5 text-sm text-cs-muted">
          <li className="flex items-center gap-2">
            <CheckCircle size={12} className="text-cs-accent shrink-0" />
            Skills from all runtimes will appear in Skills Manager
          </li>
          <li className="flex items-center gap-2">
            <CheckCircle size={12} className="text-cs-accent shrink-0" />
            Create subagents with any connected runtime
          </li>
          <li className="flex items-center gap-2">
            <CheckCircle size={12} className="text-cs-accent shrink-0" />
            Schedule cron jobs using your preferred agent
          </li>
          <li className="flex items-center gap-2">
            <CheckCircle size={12} className="text-cs-accent shrink-0" />
            Build automations mixing different runtimes
          </li>
        </ul>
      </div>
    </div>
  );
}
