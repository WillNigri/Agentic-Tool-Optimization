import { useTranslation } from "react-i18next";
import { ShieldCheck } from "lucide-react";
import type { ProjectPermissions } from "@/lib/api";
import SectionShell, { ScopeBadge, EmptyRow } from "./SectionShell";
import { cn } from "@/lib/utils";

interface PermissionsSectionProps {
  user: ProjectPermissions;
  project: ProjectPermissions;
}

export default function PermissionsSection({ user, project }: PermissionsSectionProps) {
  const { t } = useTranslation();
  const totalRules =
    user.allow.length + user.deny.length + user.ask.length +
    project.allow.length + project.deny.length + project.ask.length;

  return (
    <SectionShell
      icon={ShieldCheck}
      title={t("projects.permissions", "Permissions")}
      subtitle={t("projects.permissionsSubtitle", "allow / deny / ask rules from settings.json")}
      count={totalRules}
    >
      <div className="space-y-4">
        <ScopeGroup title="Global" scope="user" perms={user} />
        <ScopeGroup title="Project" scope="project" perms={project} />
      </div>
    </SectionShell>
  );
}

function ScopeGroup({
  title,
  scope,
  perms,
}: {
  title: string;
  scope: "user" | "project";
  perms: ProjectPermissions;
}) {
  const empty = perms.allow.length === 0 && perms.deny.length === 0 && perms.ask.length === 0;
  return (
    <div>
      <div className="mb-2 flex items-center gap-2">
        <h3 className="text-xs font-medium text-cs-muted">{title}</h3>
        <ScopeBadge scope={scope} />
      </div>
      {empty ? (
        <EmptyRow message={scope === "project" ? "No project rules." : "No global rules."} />
      ) : (
        <div className="grid grid-cols-1 gap-2 md:grid-cols-3">
          <RuleList label="Allow" color="text-green-300" tone="bg-green-500/5 border-green-500/20" rules={perms.allow} />
          <RuleList label="Deny" color="text-red-300" tone="bg-red-500/5 border-red-500/20" rules={perms.deny} />
          <RuleList label="Ask" color="text-yellow-300" tone="bg-yellow-500/5 border-yellow-500/20" rules={perms.ask} />
        </div>
      )}
    </div>
  );
}

function RuleList({
  label,
  color,
  tone,
  rules,
}: {
  label: string;
  color: string;
  tone: string;
  rules: string[];
}) {
  return (
    <div className={cn("rounded-md border px-3 py-2", tone)}>
      <div className={cn("mb-1.5 text-[10px] font-semibold uppercase tracking-wide", color)}>
        {label} ({rules.length})
      </div>
      {rules.length === 0 ? (
        <p className="text-[11px] text-cs-muted">none</p>
      ) : (
        <ul className="space-y-0.5">
          {rules.map((rule, i) => (
            <li key={i} className="truncate font-mono text-[11px] text-cs-text">
              {rule}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
