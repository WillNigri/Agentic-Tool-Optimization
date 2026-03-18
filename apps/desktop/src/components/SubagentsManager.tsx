import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Search,
  Plus,
  X,
  Save,
  Bot,
  Compass,
  Map,
  Wrench,
  Link2,
  ChevronRight,
  Cpu,
  Terminal,
  Server,
  Globe,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { AgentRuntime, OpenClawConfig, CodexConfig, HermesConfig } from "@/components/cron/types";

// ---------------------------------------------------------------------------
// Types & mock data
// ---------------------------------------------------------------------------

interface Subagent {
  id: string;
  name: string;
  description: string;
  type: "general-purpose" | "Explore" | "Plan" | "custom";
  runtime: AgentRuntime;
  runtimeConfig?: OpenClawConfig | CodexConfig | HermesConfig;
  skills: string[];
  allowedTools: string[];
  model?: string;
  instructions?: string;
  enabled: boolean;
}

const AVAILABLE_TOOLS = ["Read", "Write", "Edit", "Bash", "Grep", "Glob", "Agent"];
const AVAILABLE_MODELS = ["claude-sonnet-4-5", "claude-opus-4-5", "claude-haiku-4-5"];
const AVAILABLE_SKILLS = [
  "code-review",
  "project-conventions",
  "typescript-expert",
  "api-guidelines",
  "testing-patterns",
  "documentation",
];

const AGENT_TYPES: Subagent["type"][] = ["general-purpose", "Explore", "Plan", "custom"];
const RUNTIMES: AgentRuntime[] = ["claude", "codex", "openclaw", "hermes"];

const MOCK_SUBAGENTS: Subagent[] = [
  {
    id: "sa-1",
    name: "code-reviewer",
    description: "Reviews code changes for quality, consistency, and adherence to project conventions.",
    type: "general-purpose",
    runtime: "claude",
    skills: ["code-review", "project-conventions"],
    allowedTools: ["Read", "Grep", "Glob"],
    enabled: true,
  },
  {
    id: "sa-2",
    name: "ts-architect",
    description: "Plans TypeScript architecture decisions, module boundaries, and type structures.",
    type: "Plan",
    runtime: "claude",
    skills: ["typescript-expert"],
    allowedTools: ["Read", "Write", "Bash", "Glob"],
    model: "claude-sonnet-4-5",
    enabled: true,
  },
  {
    id: "sa-3",
    name: "codebase-explorer",
    description: "Navigates and summarises large codebases to answer structural questions.",
    type: "Explore",
    runtime: "codex",
    runtimeConfig: { apiKeyPath: "~/.config/codex/api-key" } as CodexConfig,
    skills: [],
    allowedTools: ["Read", "Grep", "Glob"],
    enabled: true,
  },
  {
    id: "sa-4",
    name: "deploy-helper",
    description: "Assists with CI/CD pipelines, deployment scripts, and release workflows.",
    type: "custom",
    runtime: "openclaw",
    runtimeConfig: { sshHost: "dev.internal", sshPort: 22, sshUser: "deploy" } as OpenClawConfig,
    skills: ["api-guidelines"],
    allowedTools: ["Read", "Bash"],
    instructions: "Focus on CI/CD pipelines and deployment automation. Always validate environment variables before running scripts.",
    enabled: false,
  },
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const TYPE_ICON: Record<Subagent["type"], typeof Bot> = {
  "general-purpose": Bot,
  Explore: Compass,
  Plan: Map,
  custom: Wrench,
};

const TYPE_COLOR: Record<Subagent["type"], string> = {
  "general-purpose": "border-blue-500/40 bg-blue-500/10 text-blue-400",
  Explore: "border-amber-500/40 bg-amber-500/10 text-amber-400",
  Plan: "border-violet-500/40 bg-violet-500/10 text-violet-400",
  custom: "border-rose-500/40 bg-rose-500/10 text-rose-400",
};

const RUNTIME_COLOR: Record<AgentRuntime, string> = {
  claude: "border-orange-500/40 bg-orange-500/10 text-orange-400",
  codex: "border-green-500/40 bg-green-500/10 text-green-400",
  openclaw: "border-cyan-500/40 bg-cyan-500/10 text-cyan-400",
  hermes: "border-purple-500/40 bg-purple-500/10 text-purple-400",
};

const RUNTIME_ICON: Record<AgentRuntime, typeof Bot> = {
  claude: Terminal,
  codex: Cpu,
  openclaw: Server,
  hermes: Globe,
};

function TypeBadge({ type }: { type: Subagent["type"] }) {
  const Icon = TYPE_ICON[type];
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider rounded-full border",
        TYPE_COLOR[type]
      )}
    >
      <Icon size={12} />
      {type}
    </span>
  );
}

function RuntimeBadge({ runtime }: { runtime: AgentRuntime }) {
  const Icon = RUNTIME_ICON[runtime];
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider rounded-full border",
        RUNTIME_COLOR[runtime]
      )}
    >
      <Icon size={12} />
      {runtime}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Runtime Config Fields
// ---------------------------------------------------------------------------

function RuntimeConfigFields({
  runtime,
  config,
  onChange,
}: {
  runtime: AgentRuntime;
  config?: OpenClawConfig | CodexConfig | HermesConfig;
  onChange: (config: OpenClawConfig | CodexConfig | HermesConfig) => void;
}) {
  const { t } = useTranslation();

  if (runtime === "claude") return null;

  if (runtime === "openclaw") {
    const oc = (config as OpenClawConfig) || { sshHost: "", sshPort: 22 };
    return (
      <div className="space-y-2 p-3 rounded-lg border border-cyan-500/20 bg-cyan-500/5">
        <p className="text-[10px] font-semibold text-cyan-400 uppercase tracking-wider">
          {t("subagents.runtimeConfig.openclawTitle")}
        </p>
        <div className="grid grid-cols-2 gap-2">
          <div>
            <label className="block text-[10px] text-cs-muted mb-0.5">{t("subagents.runtimeConfig.sshHost")}</label>
            <input
              type="text"
              className="input text-xs"
              placeholder="host.example.com"
              value={oc.sshHost || ""}
              onChange={(e) => onChange({ ...oc, sshHost: e.target.value })}
            />
          </div>
          <div>
            <label className="block text-[10px] text-cs-muted mb-0.5">{t("subagents.runtimeConfig.sshPort")}</label>
            <input
              type="number"
              className="input text-xs"
              placeholder="22"
              value={oc.sshPort || 22}
              onChange={(e) => onChange({ ...oc, sshPort: parseInt(e.target.value) || 22 })}
            />
          </div>
        </div>
        <div className="grid grid-cols-2 gap-2">
          <div>
            <label className="block text-[10px] text-cs-muted mb-0.5">{t("subagents.runtimeConfig.sshUser")}</label>
            <input
              type="text"
              className="input text-xs"
              placeholder="root"
              value={oc.sshUser || ""}
              onChange={(e) => onChange({ ...oc, sshUser: e.target.value })}
            />
          </div>
          <div>
            <label className="block text-[10px] text-cs-muted mb-0.5">{t("subagents.runtimeConfig.sshKeyPath")}</label>
            <input
              type="text"
              className="input text-xs"
              placeholder="~/.ssh/id_rsa"
              value={oc.sshKeyPath || ""}
              onChange={(e) => onChange({ ...oc, sshKeyPath: e.target.value })}
            />
          </div>
        </div>
      </div>
    );
  }

  if (runtime === "codex") {
    const cx = (config as CodexConfig) || {};
    return (
      <div className="p-3 rounded-lg border border-green-500/20 bg-green-500/5">
        <p className="text-[10px] font-semibold text-green-400 uppercase tracking-wider mb-2">
          {t("subagents.runtimeConfig.codexTitle")}
        </p>
        <div>
          <label className="block text-[10px] text-cs-muted mb-0.5">{t("subagents.runtimeConfig.apiKeyPath")}</label>
          <input
            type="text"
            className="input text-xs"
            placeholder="~/.config/codex/api-key"
            value={cx.apiKeyPath || ""}
            onChange={(e) => onChange({ apiKeyPath: e.target.value })}
          />
        </div>
      </div>
    );
  }

  if (runtime === "hermes") {
    const hm = (config as HermesConfig) || {};
    return (
      <div className="p-3 rounded-lg border border-purple-500/20 bg-purple-500/5">
        <p className="text-[10px] font-semibold text-purple-400 uppercase tracking-wider mb-2">
          {t("subagents.runtimeConfig.hermesTitle")}
        </p>
        <div>
          <label className="block text-[10px] text-cs-muted mb-0.5">{t("subagents.runtimeConfig.endpoint")}</label>
          <input
            type="text"
            className="input text-xs"
            placeholder="http://localhost:8080"
            value={hm.endpoint || ""}
            onChange={(e) => onChange({ endpoint: e.target.value })}
          />
        </div>
      </div>
    );
  }

  return null;
}

// ---------------------------------------------------------------------------
// Runtime Selector (4 colored buttons)
// ---------------------------------------------------------------------------

function RuntimeSelector({
  value,
  onChange,
}: {
  value: AgentRuntime;
  onChange: (runtime: AgentRuntime) => void;
}) {
  const { t } = useTranslation();
  return (
    <div>
      <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
        {t("subagents.runtime")}
      </label>
      <div className="grid grid-cols-4 gap-2">
        {RUNTIMES.map((rt) => {
          const Icon = RUNTIME_ICON[rt];
          return (
            <button
              key={rt}
              type="button"
              onClick={() => onChange(rt)}
              className={cn(
                "flex items-center justify-center gap-1.5 px-2 py-2 text-xs font-medium rounded-lg border transition-colors",
                value === rt
                  ? RUNTIME_COLOR[rt]
                  : "border-cs-border text-cs-muted hover:text-cs-text"
              )}
            >
              <Icon size={14} />
              {t(`subagents.runtimes.${rt}`)}
            </button>
          );
        })}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Main component
// ---------------------------------------------------------------------------

export default function SubagentsManager() {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [subagents, setSubagents] = useState<Subagent[]>(MOCK_SUBAGENTS);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [showCreateModal, setShowCreateModal] = useState(false);

  const filtered = subagents.filter(
    (sa) =>
      sa.name.toLowerCase().includes(search.toLowerCase()) ||
      sa.description.toLowerCase().includes(search.toLowerCase())
  );

  function handleSave(updated: Subagent) {
    setSubagents((prev) => prev.map((sa) => (sa.id === updated.id ? updated : sa)));
    setSelectedId(null);
  }

  function handleCreate(newAgent: Subagent) {
    setSubagents((prev) => [...prev, newAgent]);
    setShowCreateModal(false);
  }

  function handleToggle(id: string) {
    setSubagents((prev) =>
      prev.map((sa) => (sa.id === id ? { ...sa, enabled: !sa.enabled } : sa))
    );
  }

  return (
    <>
      <div className="space-y-6">
        {/* Header */}
        <div>
          <h2 className="text-xl font-semibold mb-1">{t("subagents.title")}</h2>
          <p className="text-cs-muted text-sm">{t("subagents.subtitle")}</p>
        </div>

        {/* Search */}
        <div className="relative">
          <Search
            size={16}
            className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted"
          />
          <input
            type="text"
            className="input pl-9"
            placeholder={t("subagents.search")}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>

        {/* Subagent list */}
        <div className="space-y-2">
          {filtered.map((sa) => (
            <div
              key={sa.id}
              onClick={() => setSelectedId(sa.id)}
              className={cn(
                "card cursor-pointer transition-colors",
                selectedId === sa.id
                  ? "border-cs-accent/50 bg-cs-accent/5"
                  : "hover:border-cs-border/80"
              )}
            >
              <div className="flex items-start justify-between gap-4">
                {/* Left content */}
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2 mb-1 flex-wrap">
                    <span
                      className={cn(
                        "w-2 h-2 rounded-full shrink-0",
                        sa.enabled ? "bg-green-400" : "bg-cs-muted/40"
                      )}
                    />
                    <p className="text-sm font-medium truncate">{sa.name}</p>
                    <TypeBadge type={sa.type} />
                    <RuntimeBadge runtime={sa.runtime} />
                    {sa.model && (
                      <span className="inline-flex items-center gap-1 px-1.5 py-0.5 text-[10px] font-mono rounded border border-cs-border text-cs-muted">
                        <Cpu size={10} />
                        {sa.model}
                      </span>
                    )}
                  </div>
                  <p className="text-xs text-cs-muted line-clamp-1 mb-2">
                    {sa.description}
                  </p>

                  {/* Skill tags */}
                  {sa.skills.length > 0 && (
                    <div className="flex items-center gap-1.5 flex-wrap">
                      <Link2 size={12} className="text-cs-accent shrink-0" />
                      {sa.skills.map((skill) => (
                        <span
                          key={skill}
                          className="px-2 py-0.5 text-[11px] font-mono rounded-full border border-cs-accent/30 bg-cs-accent/10 text-cs-accent"
                        >
                          {skill}
                        </span>
                      ))}
                    </div>
                  )}

                  {/* Allowed tools */}
                  <div className="flex items-center gap-1.5 mt-1.5 flex-wrap">
                    {sa.allowedTools.map((tool) => (
                      <span
                        key={tool}
                        className="px-1.5 py-0.5 text-[10px] font-mono rounded border border-cs-border text-cs-muted"
                      >
                        {tool}
                      </span>
                    ))}
                  </div>
                </div>

                {/* Right side */}
                <div className="flex items-center gap-3 shrink-0 pt-1">
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      handleToggle(sa.id);
                    }}
                    className={cn(
                      "relative w-9 h-5 rounded-full transition-colors duration-200",
                      sa.enabled ? "bg-cs-accent" : "bg-cs-border"
                    )}
                  >
                    <span
                      className={cn(
                        "absolute top-0.5 left-0.5 w-4 h-4 bg-white rounded-full transition-transform duration-200",
                        sa.enabled && "translate-x-4"
                      )}
                    />
                  </button>
                  <ChevronRight size={16} className="text-cs-muted" />
                </div>
              </div>
            </div>
          ))}
        </div>

        {filtered.length === 0 && (
          <p className="text-cs-muted text-sm text-center py-8">
            {search ? t("common.noResults") : t("subagents.noSubagents")}
          </p>
        )}

        {/* New Subagent button */}
        <button
          onClick={() => setShowCreateModal(true)}
          className="w-full flex items-center justify-center gap-2 py-3 rounded-lg border border-dashed border-cs-border text-cs-muted hover:text-cs-accent hover:border-cs-accent/40 transition-colors text-sm"
        >
          <Plus size={16} />
          {t("subagents.createNew")}
        </button>
      </div>

      {/* Detail panel */}
      {selectedId && (
        <SubagentDetailPanel
          subagent={subagents.find((sa) => sa.id === selectedId)!}
          onClose={() => setSelectedId(null)}
          onSave={handleSave}
        />
      )}

      {/* Create modal */}
      {showCreateModal && (
        <CreateSubagentModal
          onClose={() => setShowCreateModal(false)}
          onCreate={handleCreate}
        />
      )}
    </>
  );
}

// ---------------------------------------------------------------------------
// Detail Panel (slide-over from right)
// ---------------------------------------------------------------------------

function SubagentDetailPanel({
  subagent,
  onClose,
  onSave,
}: {
  subagent: Subagent;
  onClose: () => void;
  onSave: (updated: Subagent) => void;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<Subagent>({ ...subagent });

  function toggleSkill(skill: string) {
    setDraft((d) => ({
      ...d,
      skills: d.skills.includes(skill)
        ? d.skills.filter((s) => s !== skill)
        : [...d.skills, skill],
    }));
  }

  function toggleTool(tool: string) {
    setDraft((d) => ({
      ...d,
      allowedTools: d.allowedTools.includes(tool)
        ? d.allowedTools.filter((t) => t !== tool)
        : [...d.allowedTools, tool],
    }));
  }

  const hasChanges =
    JSON.stringify(draft) !== JSON.stringify(subagent);

  return (
    <>
      {/* Backdrop */}
      <div
        className="fixed inset-0 bg-black/30 z-40 lg:hidden"
        onClick={onClose}
      />
      {/* Panel */}
      <div className="fixed right-0 top-0 bottom-0 w-full lg:w-1/2 max-w-2xl bg-cs-card border-l border-cs-border z-50 flex flex-col shadow-2xl animate-slide-in-right">
        {/* Header */}
        <div className="flex items-start justify-between p-4 border-b border-cs-border">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2 flex-wrap">
              <Bot size={18} className="text-cs-accent shrink-0" />
              <h3 className="text-lg font-semibold truncate">{subagent.name}</h3>
              <TypeBadge type={subagent.type} />
              <RuntimeBadge runtime={draft.runtime} />
            </div>
            {subagent.model && (
              <p className="text-xs text-cs-muted font-mono mt-1 flex items-center gap-1">
                <Cpu size={12} /> {subagent.model}
              </p>
            )}
          </div>
          <button
            onClick={onClose}
            className="p-1.5 rounded hover:bg-cs-border transition-colors text-cs-muted hover:text-cs-text"
          >
            <X size={16} />
          </button>
        </div>

        <div className="p-4 space-y-5 overflow-y-auto flex-1">
          {/* Description */}
          <div>
            <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
              {t("subagents.description")}
            </label>
            <input
              type="text"
              className="input"
              value={draft.description}
              onChange={(e) =>
                setDraft((d) => ({ ...d, description: e.target.value }))
              }
            />
          </div>

          {/* Runtime selector */}
          <RuntimeSelector
            value={draft.runtime}
            onChange={(runtime) =>
              setDraft((d) => ({ ...d, runtime, runtimeConfig: undefined }))
            }
          />

          {/* Runtime config fields */}
          <RuntimeConfigFields
            runtime={draft.runtime}
            config={draft.runtimeConfig}
            onChange={(config) => setDraft((d) => ({ ...d, runtimeConfig: config }))}
          />

          {/* Assigned Skills — prominent section */}
          <div className="rounded-lg border border-cs-accent/30 bg-cs-accent/5 p-4">
            <div className="flex items-center gap-2 mb-3">
              <Link2 size={16} className="text-cs-accent" />
              <h4 className="text-sm font-semibold text-cs-accent uppercase tracking-wider">
                {t("subagents.skills")}
              </h4>
            </div>
            <p className="text-xs text-cs-muted mb-3">
              {t("subagents.skillsHint")}
            </p>

            <div className="space-y-2">
              {AVAILABLE_SKILLS.map((skill) => {
                const assigned = draft.skills.includes(skill);
                return (
                  <label
                    key={skill}
                    className={cn(
                      "flex items-center gap-3 px-3 py-2 rounded-lg border cursor-pointer transition-colors",
                      assigned
                        ? "border-cs-accent/40 bg-cs-accent/10"
                        : "border-cs-border hover:border-cs-border/80"
                    )}
                  >
                    <input
                      type="checkbox"
                      checked={assigned}
                      onChange={() => toggleSkill(skill)}
                      className="accent-[#00FFB2]"
                    />
                    <div className="flex items-center gap-2 flex-1 min-w-0">
                      {assigned && (
                        <span className="w-4 border-t border-dashed border-cs-accent" />
                      )}
                      <span
                        className={cn(
                          "text-sm font-mono",
                          assigned ? "text-cs-accent" : "text-cs-muted"
                        )}
                      >
                        {skill}
                      </span>
                    </div>
                  </label>
                );
              })}
            </div>
          </div>

          {/* Allowed Tools */}
          <div>
            <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
              {t("subagents.allowedTools")}
            </label>
            <div className="flex flex-wrap gap-2">
              {AVAILABLE_TOOLS.map((tool) => (
                <button
                  key={tool}
                  type="button"
                  onClick={() => toggleTool(tool)}
                  className={cn(
                    "px-2.5 py-1 text-xs font-mono rounded-full border transition-colors",
                    draft.allowedTools.includes(tool)
                      ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                      : "border-cs-border text-cs-muted hover:text-cs-text"
                  )}
                >
                  {tool}
                </button>
              ))}
            </div>
          </div>

          {/* Model override */}
          <div>
            <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
              {t("subagents.model")}
            </label>
            <select
              className="input"
              value={draft.model || ""}
              onChange={(e) =>
                setDraft((d) => ({
                  ...d,
                  model: e.target.value || undefined,
                }))
              }
            >
              <option value="">{t("subagents.anyModel")}</option>
              {AVAILABLE_MODELS.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
          </div>

          {/* Custom instructions */}
          <div>
            <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
              {t("subagents.instructions")}
            </label>
            <textarea
              className="w-full h-32 p-3 bg-cs-bg border border-cs-border rounded-lg text-sm font-mono text-cs-text resize-y focus:outline-none focus:border-cs-accent"
              value={draft.instructions || ""}
              onChange={(e) =>
                setDraft((d) => ({ ...d, instructions: e.target.value }))
              }
              placeholder={t("subagents.instructionsPlaceholder")}
            />
          </div>
        </div>

        {/* Footer actions */}
        <div className="flex gap-2 p-4 border-t border-cs-border">
          <button
            onClick={() => onSave(draft)}
            disabled={!hasChanges}
            className="inline-flex items-center gap-1.5 px-4 py-2 text-sm rounded-lg bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50"
          >
            <Save size={14} />
            {t("common.save")}
          </button>
          <button
            onClick={onClose}
            className="px-4 py-2 text-sm rounded-lg border border-cs-border text-cs-muted hover:text-cs-text transition-colors"
          >
            {t("common.cancel")}
          </button>
        </div>
      </div>
    </>
  );
}

// ---------------------------------------------------------------------------
// Create Subagent Modal
// ---------------------------------------------------------------------------

function CreateSubagentModal({
  onClose,
  onCreate,
}: {
  onClose: () => void;
  onCreate: (agent: Subagent) => void;
}) {
  const { t } = useTranslation();

  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [type, setType] = useState<Subagent["type"]>("general-purpose");
  const [runtime, setRuntime] = useState<AgentRuntime>("claude");
  const [runtimeConfig, setRuntimeConfig] = useState<OpenClawConfig | CodexConfig | HermesConfig | undefined>();
  const [selectedSkills, setSelectedSkills] = useState<string[]>([]);
  const [selectedTools, setSelectedTools] = useState<string[]>([]);
  const [model, setModel] = useState("");
  const [instructions, setInstructions] = useState("");

  function toggleSkill(skill: string) {
    setSelectedSkills((prev) =>
      prev.includes(skill) ? prev.filter((s) => s !== skill) : [...prev, skill]
    );
  }

  function toggleTool(tool: string) {
    setSelectedTools((prev) =>
      prev.includes(tool) ? prev.filter((t) => t !== tool) : [...prev, tool]
    );
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim()) return;

    const newAgent: Subagent = {
      id: `sa-${Date.now()}`,
      name: name.trim(),
      description: description.trim(),
      type,
      runtime,
      runtimeConfig,
      skills: selectedSkills,
      allowedTools: selectedTools,
      model: model || undefined,
      instructions: instructions.trim() || undefined,
      enabled: true,
    };

    onCreate(newAgent);
  }

  return (
    <>
      {/* Backdrop */}
      <div className="fixed inset-0 bg-black/50 z-50" onClick={onClose} />

      {/* Modal */}
      <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
        <div
          className="bg-cs-card border border-cs-border rounded-xl w-full max-w-lg max-h-[90vh] overflow-y-auto shadow-2xl"
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div className="flex items-center justify-between p-4 border-b border-cs-border">
            <h3 className="text-lg font-semibold">{t("subagents.createNew")}</h3>
            <button
              onClick={onClose}
              className="p-1.5 rounded hover:bg-cs-border transition-colors text-cs-muted hover:text-cs-text"
            >
              <X size={16} />
            </button>
          </div>

          <form onSubmit={handleSubmit} className="p-4 space-y-4">
            {/* Name */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("subagents.name")}
              </label>
              <input
                type="text"
                className="input"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={t("subagents.namePlaceholder")}
                required
              />
            </div>

            {/* Type selector */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("subagents.type")}
              </label>
              <div className="grid grid-cols-2 gap-2">
                {AGENT_TYPES.map((agentType) => {
                  const Icon = TYPE_ICON[agentType];
                  return (
                    <button
                      key={agentType}
                      type="button"
                      onClick={() => setType(agentType)}
                      className={cn(
                        "flex items-center gap-2 px-3 py-2 text-sm rounded-lg border transition-colors",
                        type === agentType
                          ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                          : "border-cs-border text-cs-muted hover:text-cs-text"
                      )}
                    >
                      <Icon size={14} />
                      {agentType}
                    </button>
                  );
                })}
              </div>
            </div>

            {/* Runtime selector */}
            <RuntimeSelector
              value={runtime}
              onChange={(rt) => {
                setRuntime(rt);
                setRuntimeConfig(undefined);
              }}
            />

            {/* Runtime config */}
            <RuntimeConfigFields
              runtime={runtime}
              config={runtimeConfig}
              onChange={setRuntimeConfig}
            />

            {/* Description */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("subagents.description")}
              </label>
              <input
                type="text"
                className="input"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
              />
            </div>

            {/* Skills multi-select */}
            <div className="rounded-lg border border-cs-accent/30 bg-cs-accent/5 p-3">
              <div className="flex items-center gap-2 mb-2">
                <Link2 size={14} className="text-cs-accent" />
                <label className="text-xs font-semibold text-cs-accent uppercase tracking-wider">
                  {t("subagents.skills")}
                </label>
              </div>
              <div className="space-y-1.5">
                {AVAILABLE_SKILLS.map((skill) => (
                  <label
                    key={skill}
                    className={cn(
                      "flex items-center gap-2.5 px-2.5 py-1.5 rounded-lg border cursor-pointer transition-colors text-sm",
                      selectedSkills.includes(skill)
                        ? "border-cs-accent/40 bg-cs-accent/10"
                        : "border-cs-border hover:border-cs-border/80"
                    )}
                  >
                    <input
                      type="checkbox"
                      checked={selectedSkills.includes(skill)}
                      onChange={() => toggleSkill(skill)}
                      className="accent-[#00FFB2]"
                    />
                    <span
                      className={cn(
                        "font-mono",
                        selectedSkills.includes(skill)
                          ? "text-cs-accent"
                          : "text-cs-muted"
                      )}
                    >
                      {skill}
                    </span>
                  </label>
                ))}
              </div>
            </div>

            {/* Allowed Tools */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("subagents.allowedTools")}
              </label>
              <div className="flex flex-wrap gap-2">
                {AVAILABLE_TOOLS.map((tool) => (
                  <button
                    key={tool}
                    type="button"
                    onClick={() => toggleTool(tool)}
                    className={cn(
                      "px-2.5 py-1 text-xs font-mono rounded-full border transition-colors",
                      selectedTools.includes(tool)
                        ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                        : "border-cs-border text-cs-muted hover:text-cs-text"
                    )}
                  >
                    {tool}
                  </button>
                ))}
              </div>
            </div>

            {/* Model */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("subagents.model")}
              </label>
              <select
                className="input"
                value={model}
                onChange={(e) => setModel(e.target.value)}
              >
                <option value="">{t("subagents.anyModel")}</option>
                {AVAILABLE_MODELS.map((m) => (
                  <option key={m} value={m}>
                    {m}
                  </option>
                ))}
              </select>
            </div>

            {/* Custom instructions */}
            <div>
              <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                {t("subagents.instructions")}
              </label>
              <textarea
                className="w-full h-32 p-3 bg-cs-bg border border-cs-border rounded-lg text-sm font-mono text-cs-text resize-y focus:outline-none focus:border-cs-accent"
                value={instructions}
                onChange={(e) => setInstructions(e.target.value)}
                placeholder={t("subagents.instructionsPlaceholder")}
              />
            </div>

            {/* Actions */}
            <div className="flex gap-2 pt-2">
              <button
                type="submit"
                disabled={!name.trim()}
                className="flex-1 px-4 py-2 text-sm rounded-lg bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50"
              >
                {t("common.create")}
              </button>
              <button
                type="button"
                onClick={onClose}
                className="px-4 py-2 text-sm rounded-lg border border-cs-border text-cs-muted hover:text-cs-text transition-colors"
              >
                {t("common.cancel")}
              </button>
            </div>
          </form>
        </div>
      </div>
    </>
  );
}
