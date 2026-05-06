import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  Briefcase,
  Sparkles,
  Server,
  ShoppingBag,
  Bot,
  ArrowRight,
  Layers,
  Check,
  Minus,
} from "lucide-react";
import { getSkills, getMcpServers } from "@/lib/api";
import { listAgents } from "@/lib/agents";
import { listRuntimeCapabilities } from "@/lib/runtimeCapabilities";
import { cn } from "@/lib/utils";

// v1.3.0 — Skills & MCPs landing page (the "Customize" overview).
// Mirrors Claude.ai's Customize layout but multi-runtime: skills + MCPs +
// marketplace are unified here, with live counts so the user knows the
// surface they already have. Sub-tab navigation is owned by SectionTabs above
// — this page just renders informational cards and gentle CTAs.

interface Props {
  onGoTo?: (subtabId: string) => void;
}

export default function CustomizeOverview({ onGoTo: _onGoTo }: Props) {
  const { t } = useTranslation();

  const { data: skills = [] } = useQuery({
    queryKey: ["all-skills"],
    queryFn: getSkills,
    staleTime: 30_000,
  });
  const { data: mcps = [] } = useQuery({
    queryKey: ["mcp-servers"],
    queryFn: getMcpServers,
    staleTime: 30_000,
  });
  const { data: agents = [] } = useQuery({
    queryKey: ["agents"],
    queryFn: () => listAgents(),
    staleTime: 30_000,
  });

  return (
    <div className="max-w-3xl mx-auto py-4 space-y-8">
      {/* Hero */}
      <header className="text-center">
        <div className="inline-flex items-center justify-center w-14 h-14 rounded-2xl bg-cs-accent/10 mb-4">
          <Briefcase size={26} className="text-cs-accent" />
        </div>
        <h1 className="text-2xl font-semibold text-cs-text">
          {t("customize.title", "Customize")}
        </h1>
        <p className="mt-2 text-sm text-cs-muted max-w-xl mx-auto">
          {t(
            "customize.subtitle",
            "Skills, MCP connectors and agents shape how every runtime works for you. One pane for Claude, Codex, Gemini, Ollama, OpenClaw and Hermes."
          )}
        </p>
        <p className="mt-3 text-[11px] uppercase tracking-wider text-cs-muted">
          {t(
            "customize.localFirst",
            "Local-first · Git-versionable · Multi-runtime"
          )}
        </p>
      </header>

      {/* Cards */}
      <div className="space-y-3">
        <Card
          icon={<Server size={18} />}
          title={t("customize.connectorsTitle", "Connect your apps")}
          body={t(
            "customize.connectorsBody",
            "Add MCP servers so your agents can read and write in the tools you already use — Gmail, GitHub, Slack, Postgres, your filesystem."
          )}
          stats={[
            { label: t("customize.installedStat", "installed"), value: mcps.length },
          ]}
        />
        <Card
          icon={<Sparkles size={18} />}
          title={t("customize.skillsTitle", "Build skills")}
          body={t(
            "customize.skillsBody",
            "Teach every runtime your processes, team norms, and tribal knowledge. Skills live as SKILL.md files you can share, fork, and version-control."
          )}
          stats={[
            { label: t("customize.skillsStat", "skills"), value: skills.length },
          ]}
        />
        <Card
          icon={<Bot size={18} />}
          title={t("customize.agentsTitle", "Build agents")}
          body={t(
            "customize.agentsBody",
            "Agents bundle a system prompt + scoped MCPs + permissions + skills into a single autonomous unit. Their Plugins, but multi-runtime, local, and you can schedule + observe runs."
          )}
          stats={[
            { label: t("customize.agentsStat", "agents"), value: agents.length },
          ]}
        />
        <Card
          icon={<ShoppingBag size={18} />}
          title={t("customize.marketplaceTitle", "Browse the marketplace")}
          body={t(
            "customize.marketplaceBody",
            "Pre-built skills and agent templates, contributed by the community. Install with one click. Free."
          )}
        />
      </div>

      {/* Why-us callout */}
      <section className="rounded-2xl border border-cs-border bg-cs-card p-5">
        <header className="flex items-center gap-2 mb-2">
          <Layers size={14} className="text-cs-accent" />
          <h2 className="text-sm font-medium text-cs-text">
            {t("customize.whyUsTitle", "Why ATO and not your runtime's built-in customize")}
          </h2>
        </header>
        <ul className="space-y-1.5 text-xs text-cs-muted">
          <li>
            <span className="text-cs-accent">·</span>{" "}
            {t(
              "customize.whyUs.runtime",
              "Works across every runtime you've got — not just one vendor's chat product."
            )}
          </li>
          <li>
            <span className="text-cs-accent">·</span>{" "}
            {t(
              "customize.whyUs.local",
              "Your config lives on your machine, in plain files, ready to commit to git or share with a teammate."
            )}
          </li>
          <li>
            <span className="text-cs-accent">·</span>{" "}
            {t(
              "customize.whyUs.agentic",
              "Agents are autonomous units (runtime + scoped MCPs + permissions + schedules + run history), not just chat presets."
            )}
          </li>
          <li>
            <span className="text-cs-accent">·</span>{" "}
            {t(
              "customize.whyUs.power",
              "Embedded terminal, ⌘K command palette, raw config edit when you want them."
            )}
          </li>
        </ul>
      </section>

      {/* Cross-runtime dispatch: install ATO as an MCP into your runtimes,
          then any agent works in any runtime — no caveats. */}
      <CrossRuntimeMcpHint />

      {/* Runtime parity matrix — honest about what works where today. */}
      <RuntimeParityMatrix />

      <p className="text-center text-[11px] text-cs-muted">
        {t(
          "customize.tabHint",
          "Use the tabs above to manage Skills, MCPs, and the Marketplace."
        )}
      </p>
    </div>
  );
}

function CrossRuntimeMcpHint() {
  const { t } = useTranslation();
  const snippet = JSON.stringify(
    {
      ato: {
        command: "npx",
        args: ["@ato/mcp-server"],
      },
    },
    null,
    2
  );
  return (
    <section className="rounded-2xl border border-cs-accent/40 bg-cs-accent/5 p-5">
      <header className="mb-2 flex items-center gap-2">
        <Sparkles size={14} className="text-cs-accent" />
        <h2 className="text-sm font-medium text-cs-text">
          {t("customize.crossRuntime.title", "Cross-runtime dispatch (no caveats)")}
        </h2>
      </header>
      <p className="text-xs text-cs-muted">
        {t(
          "customize.crossRuntime.body",
          "Install ATO's MCP into Claude / Codex / Gemini once and every runtime can call every agent natively via run_agent — including agents that live on a different runtime. The honest caveats in the parity table below disappear once this is set up."
        )}
      </p>
      <pre className="mt-3 rounded-lg border border-cs-border bg-cs-bg p-3 text-xs text-cs-text font-mono overflow-x-auto">
        {snippet}
      </pre>
      <p className="mt-2 text-[11px] text-cs-muted">
        {t(
          "customize.crossRuntime.hint",
          "Drop this into your runtime's mcpServers config (Skills & MCPs → MCPs → + Add MCP → Custom). Then ask the runtime to use the run_agent tool."
        )}
      </p>
    </section>
  );
}

function RuntimeParityMatrix() {
  const { t } = useTranslation();
  const rows = listRuntimeCapabilities();
  return (
    <section className="rounded-2xl border border-cs-border bg-cs-card p-5">
      <header className="mb-3">
        <h2 className="text-sm font-medium text-cs-text">
          {t("customize.parityTitle", "Runtime parity")}
        </h2>
        <p className="mt-1 text-xs text-cs-muted">
          {t(
            "customize.paritySubtitle",
            "Honest status of what ATO can drive end-to-end per runtime today. Empty cells mean we'll route you to the runtime's own tools."
          )}
        </p>
      </header>
      <div className="overflow-x-auto -mx-2 px-2">
        <table className="w-full text-xs">
          <thead>
            <tr className="text-left text-[10px] uppercase tracking-wide text-cs-muted">
              <th className="font-medium pb-2 pr-3">{t("customize.parity.runtime", "Runtime")}</th>
              <th className="font-medium pb-2 px-3 text-center">
                {t("customize.parity.create", "Create agent")}
              </th>
              <th className="font-medium pb-2 px-3 text-center">
                {t("customize.parity.installMcp", "Install MCP")}
              </th>
              <th className="font-medium pb-2 px-3 text-center">
                {t("customize.parity.runShell", "Run in shell")}
              </th>
              <th className="font-medium pb-2 pl-3">{t("customize.parity.notes", "Notes")}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(({ runtime, cap }) => {
              const canShell = cap.invocation.kind !== "manual";
              const isMention = cap.invocation.kind === "mention";
              return (
                <tr key={runtime} className="border-t border-cs-border/40">
                  <td className="py-2 pr-3">
                    <span className="inline-flex items-center gap-2">
                      <span className={cn("inline-block w-1.5 h-1.5 rounded-full", cap.dotClass)} />
                      <span className="text-cs-text">{cap.label}</span>
                    </span>
                  </td>
                  <td className="py-2 px-3 text-center">
                    <ParityCell ok={cap.canCreateAgent} />
                  </td>
                  <td className="py-2 px-3 text-center">
                    <ParityCell ok={cap.canInstallMcp} />
                  </td>
                  <td className="py-2 px-3 text-center">
                    <ParityCell
                      ok={canShell}
                      label={canShell ? (isMention ? "native" : "prefix") : ""}
                    />
                  </td>
                  <td className="py-2 pl-3 text-cs-muted text-[11px]">{cap.note ?? "—"}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function ParityCell({ ok, label }: { ok: boolean; label?: string }) {
  if (!ok) {
    return (
      <span className="inline-flex items-center justify-center text-cs-muted/40">
        <Minus size={12} />
      </span>
    );
  }
  return (
    <span className="inline-flex items-center justify-center gap-1 text-cs-accent">
      <Check size={12} />
      {label && <span className="text-[10px] uppercase tracking-wide">{label}</span>}
    </span>
  );
}

function Card({
  icon,
  title,
  body,
  stats,
}: {
  icon: React.ReactNode;
  title: string;
  body: string;
  stats?: { label: string; value: number }[];
}) {
  return (
    <div className="rounded-2xl border border-cs-border bg-cs-card p-5 flex items-start gap-4 hover:border-cs-hover transition">
      <div className="w-10 h-10 rounded-xl bg-cs-bg-raised flex items-center justify-center text-cs-muted shrink-0">
        {icon}
      </div>
      <div className="flex-1 min-w-0">
        <h3 className="text-sm font-medium text-cs-text">{title}</h3>
        <p className="mt-1 text-xs text-cs-muted">{body}</p>
        {stats && stats.length > 0 && (
          <p className="mt-2 text-[11px] text-cs-muted">
            {stats.map((s, idx) => (
              <span key={s.label}>
                <span className="text-cs-text font-medium">{s.value}</span> {s.label}
                {idx < stats.length - 1 && " · "}
              </span>
            ))}
          </p>
        )}
      </div>
      <ArrowRight size={14} className="text-cs-muted shrink-0 mt-1" />
    </div>
  );
}
