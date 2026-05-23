/**
 * ConsentModal — the user-facing surface of the chunk-2 backend
 * consent gate (war-room 87E6CADF round 3, security-specialist
 * NON-NEGOTIABLE).
 *
 * Shown BEFORE the first save of any variable with a privileged
 * kind (file / db-query / computed). The user MUST see the exact
 * resource being granted access to + acknowledge before the
 * backend will run the resolver.
 *
 * Why "before save": if we showed it AFTER save, an attacker UI
 * could programmatically dismiss it. By gating save itself, the
 * user is forced to interact with the modal — the click is the
 * consent.
 */
import { useState } from "react";
import { ShieldAlert, FileText, Database, Code, X, AlertTriangle } from "lucide-react";
import type { ConsentScope, VariableKind } from "@/lib/agentVariables";

interface Props {
  open: boolean;
  variableName: string;
  variableKind: VariableKind;
  /** Human-readable description of what's being granted access to.
   *  This string is sent verbatim to the backend as
   *  `granted_resource` and stored for audit. Don't summarize. */
  resourceDescription: string;
  onConfirm: (scope: ConsentScope) => void;
  onCancel: () => void;
}

const KIND_ICON: Record<string, JSX.Element> = {
  file: <FileText size={16} className="text-amber-400" />,
  "db-query": <Database size={16} className="text-amber-400" />,
  computed: <Code size={16} className="text-amber-400" />,
};

const KIND_DESCRIPTION: Record<string, string> = {
  file: "Read a local file from your Mac and inline its contents into the agent prompt",
  "db-query": "Run a read-only SQL query against a local SQLite database",
  computed: "Evaluate a computed expression that may reference other variables or process state",
};

export default function ConsentModal({
  open,
  variableName,
  variableKind,
  resourceDescription,
  onConfirm,
  onCancel,
}: Props) {
  // v2.8.x chunk 3+4 war-room AMEND (claude + minimax + security all
  // converged): default to "once" not "always". Per security-specialist
  // — secure-by-default trumps convenient-by-default for a security
  // gate. Power users who hit grants often will tick "Always allow"
  // themselves; that's a deliberate decision instead of a default.
  const [scope, setScope] = useState<ConsentScope>("once");

  if (!open) return null;

  const icon = KIND_ICON[variableKind] ?? <ShieldAlert size={16} className="text-amber-400" />;
  const what = KIND_DESCRIPTION[variableKind] ?? "Access a privileged local resource";

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      role="dialog"
      aria-labelledby="consent-modal-title"
      aria-modal="true"
    >
      <div
        className="w-full max-w-lg rounded-lg border border-amber-500/30 bg-cs-bg-raised shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="flex items-start justify-between gap-3 border-b border-cs-border p-4">
          <div className="flex items-start gap-2 min-w-0">
            <ShieldAlert size={18} className="text-amber-400 shrink-0 mt-0.5" />
            <div className="min-w-0">
              <h3 id="consent-modal-title" className="text-sm font-medium text-cs-text">
                Grant access to local resource?
              </h3>
              <p className="mt-0.5 text-[11px] text-cs-muted">
                Variable <code className="text-cs-text">{variableName}</code> · kind <code className="text-cs-text">{variableKind}</code>
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={onCancel}
            className="rounded p-1 text-cs-muted hover:text-cs-text"
            aria-label="Cancel"
          >
            <X size={16} aria-hidden="true" />
          </button>
        </header>

        <div className="p-4 space-y-3 text-sm text-cs-text">
          <p className="text-cs-muted">{what}</p>

          <div className="flex items-start gap-2 rounded-md border border-amber-500/30 bg-amber-500/10 p-3 text-xs">
            {icon}
            <div className="min-w-0 flex-1">
              <div className="font-medium text-amber-200 mb-1">Will be granted:</div>
              <code className="block break-words text-cs-text bg-cs-bg/60 rounded px-2 py-1.5 text-[11px]">
                {resourceDescription}
              </code>
            </div>
          </div>

          <div className="flex items-start gap-2 rounded-md border border-cs-danger/30 bg-cs-danger/5 p-3 text-[11px] text-cs-muted">
            <AlertTriangle size={14} className="text-cs-danger shrink-0 mt-0.5" />
            <div>
              <strong className="text-cs-text">Security note</strong>: ATO inlines the
              resolved content into every LLM prompt that uses this variable.
              That means your LLM provider will see the file contents / query
              results / computed value. Only grant access to resources you're
              comfortable sending to your chosen provider.
            </div>
          </div>

          <fieldset className="rounded-md border border-cs-border p-3">
            <legend className="px-1 text-[11px] text-cs-muted">Scope</legend>
            <div className="space-y-1.5">
              <label className="flex items-start gap-2 text-xs cursor-pointer">
                <input
                  type="radio"
                  name="consent-scope"
                  value="always"
                  checked={scope === "always"}
                  onChange={() => setScope("always")}
                  className="mt-0.5"
                />
                <div>
                  <div className="text-cs-text">Always allow</div>
                  <div className="text-[10px] text-cs-muted">Until you revoke in Settings → Permissions</div>
                </div>
              </label>
              <label className="flex items-start gap-2 text-xs cursor-pointer">
                <input
                  type="radio"
                  name="consent-scope"
                  value="once"
                  checked={scope === "once"}
                  onChange={() => setScope("once")}
                  className="mt-0.5"
                />
                <div>
                  <div className="text-cs-text">Just once</div>
                  <div className="text-[10px] text-cs-muted">You'll be asked again next time this variable is needed</div>
                </div>
              </label>
            </div>
          </fieldset>
        </div>

        <footer className="flex items-center justify-end gap-2 border-t border-cs-border p-3">
          <button
            type="button"
            onClick={onCancel}
            className="rounded-md border border-cs-border bg-cs-bg px-3 py-1.5 text-xs text-cs-muted hover:text-cs-text"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={() => onConfirm(scope)}
            className="rounded-md bg-amber-500 px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-amber-400"
          >
            Grant access
          </button>
        </footer>
      </div>
    </div>
  );
}
