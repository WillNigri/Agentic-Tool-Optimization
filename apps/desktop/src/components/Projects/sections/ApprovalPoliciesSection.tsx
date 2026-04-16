import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ShieldAlert, Plus, Trash2, Edit3, Save, X, Loader2 } from "lucide-react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { writeApprovalPolicies, type ApprovalPolicy } from "@/lib/api";
import SectionShell, { ScopeBadge, EmptyRow } from "./SectionShell";
import { cn } from "@/lib/utils";

interface ApprovalPoliciesSectionProps {
  policies: ApprovalPolicy[];
  projectPath: string;
  onCreate?: () => void;
}

const POLICY_COLORS: Record<string, string> = {
  untrusted: "text-red-300 bg-red-500/10",
  never: "text-red-300 bg-red-500/10",
  "on-request": "text-yellow-300 bg-yellow-500/10",
  always: "text-green-300 bg-green-500/10",
  granular: "text-blue-300 bg-blue-500/10",
};

const POLICY_OPTIONS = ["always", "on-request", "untrusted", "never", "granular"];

export default function ApprovalPoliciesSection({ policies, projectPath, onCreate }: ApprovalPoliciesSectionProps) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<ApprovalPolicy[]>(policies);
  const queryClient = useQueryClient();

  const saveMutation = useMutation({
    mutationFn: () => writeApprovalPolicies(projectPath, draft),
    onSuccess: () => {
      setEditing(false);
      queryClient.invalidateQueries({ queryKey: ["project-bundle"] });
    },
  });

  function startEdit() {
    setDraft([...policies]);
    setEditing(true);
  }

  function addRow() {
    setDraft([...draft, { toolName: "", policy: "on-request", scope: "project" }]);
  }

  function removeRow(i: number) {
    setDraft(draft.filter((_, idx) => idx !== i));
  }

  function updateRow(i: number, field: keyof ApprovalPolicy, value: string) {
    setDraft(draft.map((p, idx) => idx === i ? { ...p, [field]: value } : p));
  }

  return (
    <SectionShell
      icon={ShieldAlert}
      title={t("projects.policies", "Approval Policies")}
      subtitle={t("projects.policiesSubtitle", "Per-tool approval rules for OpenAI Agents SDK")}
      count={policies.length}
      actions={policies.length > 0 && !editing && (
        <button onClick={startEdit} className="flex items-center gap-1 text-[10px] text-cs-muted hover:text-cs-accent"><Edit3 size={10} /> Edit</button>
      )}
    >
      {policies.length === 0 && !editing ? (
        <EmptyRow
          message={t("projects.policiesEmpty", "No approval policies configured. Policies control which tools agents can use without asking.")}
          actionLabel={onCreate ? t("projects.policiesCreate", "Create policies.json") : undefined}
          onAction={onCreate}
        />
      ) : (
        <div>
          <div className="overflow-x-auto">
            <table className="w-full text-xs">
              <thead>
                <tr className="border-b border-cs-border/60">
                  <th className="pb-1.5 text-left font-medium text-cs-muted">Tool</th>
                  <th className="pb-1.5 text-left font-medium text-cs-muted">Policy</th>
                  <th className="pb-1.5 text-left font-medium text-cs-muted">Scope</th>
                  {editing && <th className="pb-1.5 w-8"></th>}
                </tr>
              </thead>
              <tbody>
                {(editing ? draft : policies).map((p, i) => (
                  <tr key={`${p.scope}-${p.toolName}-${i}`} className="border-b border-cs-border/30">
                    <td className="py-1.5 pr-3">
                      {editing ? (
                        <input
                          value={p.toolName}
                          onChange={(e) => updateRow(i, "toolName", e.target.value)}
                          placeholder="tool_name"
                          className="w-full rounded border border-cs-border bg-cs-bg px-2 py-0.5 font-mono text-xs focus:outline-none focus:border-cs-accent"
                        />
                      ) : (
                        <span className="font-mono">{p.toolName}</span>
                      )}
                    </td>
                    <td className="py-1.5 pr-3">
                      {editing ? (
                        <select
                          value={p.policy}
                          onChange={(e) => updateRow(i, "policy", e.target.value)}
                          className="rounded border border-cs-border bg-cs-bg px-2 py-0.5 text-xs focus:outline-none focus:border-cs-accent"
                        >
                          {POLICY_OPTIONS.map((opt) => <option key={opt} value={opt}>{opt}</option>)}
                        </select>
                      ) : (
                        <span className={cn("rounded px-1.5 py-0.5 text-[10px] font-medium", POLICY_COLORS[p.policy] ?? "text-cs-muted bg-cs-border/60")}>{p.policy}</span>
                      )}
                    </td>
                    <td className="py-1.5">
                      <ScopeBadge scope={p.scope as "user" | "project"} />
                    </td>
                    {editing && (
                      <td className="py-1.5">
                        <button onClick={() => removeRow(i)} className="p-1 text-red-400 hover:bg-red-500/10 rounded"><Trash2 size={11} /></button>
                      </td>
                    )}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {editing && (
            <div className="mt-3 flex items-center justify-between border-t border-cs-border/60 pt-3">
              <button onClick={addRow} className="flex items-center gap-1 text-xs text-cs-muted hover:text-cs-accent"><Plus size={11} /> Add rule</button>
              <div className="flex items-center gap-2">
                <button onClick={() => setEditing(false)} className="px-3 py-1 rounded text-xs text-cs-muted hover:bg-cs-border"><X size={11} className="inline mr-1" />Cancel</button>
                <button
                  onClick={() => saveMutation.mutate()}
                  disabled={saveMutation.isPending}
                  className="flex items-center gap-1 px-3 py-1 rounded text-xs font-medium bg-cs-accent text-cs-bg hover:bg-cs-accent/90 disabled:opacity-50"
                >
                  {saveMutation.isPending ? <Loader2 size={11} className="animate-spin" /> : <Save size={11} />} Save
                </button>
              </div>
            </div>
          )}
          {saveMutation.isError && (
            <p className="mt-2 text-[11px] text-red-300">{saveMutation.error instanceof Error ? saveMutation.error.message : "Save failed"}</p>
          )}
        </div>
      )}
    </SectionShell>
  );
}
