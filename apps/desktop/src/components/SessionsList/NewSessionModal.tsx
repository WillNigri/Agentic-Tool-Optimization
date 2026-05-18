// NewSessionModal — "New session" form opened from the Sessions tab's
// "+ New session" button (and, after Path B, from the bottom-pane
// multi-launcher via the pendingOpenNewSession flag).
//
// Extracted from SessionsList.tsx (2026-05-18 elegance push #2) so the
// parent file shrinks. Self-contained: takes onClose + onCreated
// callbacks and walks the user through runtime + title + persona +
// optional project pickers, then invokes `create_session` and routes
// the new id back via onCreated.
//
// PR 11 — snapshots the active project from the sidebar at modal-open
// time so switching projects mid-edit doesn't change the submitted
// project_id under the user's feet.

import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Loader2, Plus, X } from "lucide-react";

import { useProjectStore } from "@/stores/useProjectStore";
import { NEW_SESSION_RUNTIMES } from "./_helpers";

export default function NewSessionModal({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: (id: string) => void;
}) {
  const [runtime, setRuntime] = useState("claude");
  const [title, setTitle] = useState("");
  const [agentSlug, setAgentSlug] = useState("");
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // PR 11 — snapshot the active project from the sidebar into the
  // new session's `project_id` at create time. The Project store is
  // the source of truth for which project the user is "in" right
  // now; reading it here means the session inherits that scope
  // without the user needing to pick from a dropdown. When no
  // project is active ("NO PROJECT" in the sidebar), project_id
  // stays null and the close-time coordinator may still suggest
  // one. Codex Round-1 #1: snapshot BOTH the id AND the display
  // name. Previously we froze only the id but read `activeProject.
  // name` live at render time — if the user switched projects in
  // the sidebar while the modal was open, the displayed name would
  // drift from the snapshotted id. Freezing the full {id, name}
  // pair keeps the label honest about what gets submitted.
  const activeProject = useProjectStore((s) => s.activeProject);
  const [projectSnapshot] = useState<{ id: string; name: string } | null>(
    activeProject ? { id: activeProject.id, name: activeProject.name } : null,
  );

  const handleCreate = async () => {
    setCreating(true);
    setError(null);
    try {
      const id = await invoke<string>("create_session", {
        runtime,
        title: title.trim() || null,
        agentSlug: agentSlug.trim() || null,
        projectId: projectSnapshot?.id ?? null,
      });
      onCreated(id);
    } catch (e) {
      setError(String(e));
    } finally {
      setCreating(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={onClose}
    >
      <div
        className="relative bg-cs-card border border-cs-border rounded-lg p-6 w-full max-w-md space-y-4"
        onClick={(e) => e.stopPropagation()}
      >
        <button
          onClick={onClose}
          className="absolute top-3 right-3 text-cs-muted hover:text-cs-text"
          aria-label="close"
        >
          <X size={16} />
        </button>
        <h3 className="text-lg font-semibold text-cs-text">New session</h3>
        {/* PR 11 — show the project snapshot inline so the user knows
            which project the new session will be tagged to. Reads from
            useProjectStore.activeProject; null when sidebar shows "NO
            PROJECT". The session inherits whatever's active at the
            moment of create; switching projects after this modal opens
            does NOT change the snapshot (intentionally — the modal
            shouldn't surprise the user mid-edit). */}
        <div className="text-[11px] text-cs-muted flex items-center gap-2">
          <span className="uppercase tracking-wider">Project:</span>
          {projectSnapshot ? (
            <span
              className="text-cs-accent font-mono"
              title={`project_id at snapshot: ${projectSnapshot.id}`}
            >
              {projectSnapshot.name}
            </span>
          ) : (
            <span className="italic">no project (session created project-less)</span>
          )}
        </div>
        <div className="space-y-3">
          <div>
            <label className="text-xs text-cs-muted uppercase font-medium">Runtime</label>
            <select
              value={runtime}
              onChange={(e) => setRuntime(e.target.value)}
              className="mt-1 w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent"
            >
              {NEW_SESSION_RUNTIMES.map((r) => (
                <option key={r} value={r}>
                  {r}
                </option>
              ))}
            </select>
            <div className="mt-1 text-[10px] text-cs-muted">
              Anchor runtime. Cross-runtime turns via @-mentions in --tag-bridge or by
              dispatching into the session from a different runtime later.
            </div>
          </div>
          <div>
            <label className="text-xs text-cs-muted uppercase font-medium">Title (optional)</label>
            <input
              type="text"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="e.g. SSH adapter design review"
              className="mt-1 w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent"
            />
          </div>
          <div>
            <label className="text-xs text-cs-muted uppercase font-medium">Agent slug (optional)</label>
            <input
              type="text"
              value={agentSlug}
              onChange={(e) => setAgentSlug(e.target.value)}
              placeholder="e.g. codex-reviewer"
              className="mt-1 w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent"
            />
          </div>
        </div>
        {error && <div className="text-xs text-cs-danger">{error}</div>}
        <div className="flex items-center justify-end gap-2">
          <button
            onClick={onClose}
            disabled={creating}
            className="px-3 py-2 rounded-md border border-cs-border text-sm hover:bg-cs-border/30"
          >
            Cancel
          </button>
          <button
            onClick={handleCreate}
            disabled={creating}
            className="flex items-center gap-2 px-3 py-2 rounded-md bg-cs-accent text-cs-bg text-sm font-medium hover:opacity-90 disabled:opacity-40"
          >
            {creating ? <Loader2 size={14} className="animate-spin" /> : <Plus size={14} />}
            Create
          </button>
        </div>
      </div>
    </div>
  );
}
