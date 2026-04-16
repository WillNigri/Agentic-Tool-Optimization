import { useTranslation } from "react-i18next";
import { Webhook } from "lucide-react";
import type { ProjectHookSummary } from "@/lib/api";
import SectionShell, { ScopeBadge, EmptyRow } from "./SectionShell";

interface HooksSectionProps {
  hooks: ProjectHookSummary[];
  onOpenFile?: (path: string) => void;
  settingsPath?: string;
}

export default function HooksSection({ hooks, onOpenFile, settingsPath }: HooksSectionProps) {
  const { t } = useTranslation();
  const byEvent = hooks.reduce<Record<string, ProjectHookSummary[]>>((acc, h) => {
    (acc[h.event] ||= []).push(h);
    return acc;
  }, {});
  const events = Object.keys(byEvent).sort();

  return (
    <SectionShell
      icon={Webhook}
      title={t("projects.hooks", "Hooks")}
      subtitle={t("projects.hooksSubtitle", "Event triggers from settings.json (user + project)")}
      count={hooks.length}
    >
      {hooks.length === 0 ? (
        <EmptyRow
          message={t("projects.hooksEmpty", "No hooks configured. Hooks run shell commands on events like pre-tool-use or post-commit.")}
          actionLabel={settingsPath ? t("projects.hooksEditSettings", "Edit settings.json") : undefined}
          onAction={settingsPath && onOpenFile ? () => onOpenFile(settingsPath) : undefined}
        />
      ) : (
        <div className="space-y-3">
          {events.map((event) => (
            <div key={event}>
              <div className="mb-1.5 flex items-center gap-2">
                <h3 className="text-xs font-medium">{event}</h3>
                <span className="text-[10px] text-cs-muted">({byEvent[event].length})</span>
              </div>
              <ul className="space-y-1">
                {byEvent[event].map((hook, idx) => (
                  <li
                    key={`${event}-${idx}`}
                    className="rounded-md border border-cs-border/60 bg-cs-bg/40 px-3 py-2"
                  >
                    <div className="mb-1 flex items-center gap-2">
                      {hook.matcher && (
                        <code className="rounded bg-cs-border/60 px-1.5 py-0.5 text-[10px] text-cs-text">
                          {hook.matcher}
                        </code>
                      )}
                      <ScopeBadge scope={hook.scope} />
                    </div>
                    <pre className="whitespace-pre-wrap break-all font-mono text-[11px] text-cs-text">
                      {hook.command}
                    </pre>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      )}
    </SectionShell>
  );
}
