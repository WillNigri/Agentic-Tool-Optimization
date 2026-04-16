import { useState } from "react";
import { Box, ExternalLink, Edit3, Save, X, Loader2 } from "lucide-react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { writeSandboxConfig, type SandboxConfig } from "@/lib/api";
import SectionShell, { EmptyRow } from "./SectionShell";
import { cn } from "@/lib/utils";

interface SandboxSectionProps {
  config: SandboxConfig | null;
  projectPath: string;
  onOpenSource: (path: string) => void;
  onCreate?: () => void;
}

const DEFAULT_CONFIG: SandboxConfig = {
  enabled: true,
  networkIsolation: true,
  allowedPorts: [],
  filesystemPolicy: "read-only",
  timeoutSecs: 300,
  snapshotEnabled: false,
  sourcePath: "",
};

export default function SandboxSection({ config, projectPath, onOpenSource, onCreate }: SandboxSectionProps) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<SandboxConfig>(config ?? DEFAULT_CONFIG);
  const queryClient = useQueryClient();

  const saveMutation = useMutation({
    mutationFn: () => writeSandboxConfig(projectPath, draft),
    onSuccess: () => {
      setEditing(false);
      queryClient.invalidateQueries({ queryKey: ["project-bundle"] });
    },
  });

  function startEdit() {
    setDraft(config ?? DEFAULT_CONFIG);
    setEditing(true);
  }

  return (
    <SectionShell
      icon={Box}
      title="Sandbox"
      subtitle="OpenAI Agents SDK execution sandbox configuration"
      actions={config && !editing && (
        <div className="flex items-center gap-2">
          <button onClick={startEdit} className="flex items-center gap-1 text-[10px] text-cs-muted hover:text-cs-accent"><Edit3 size={10} /> Edit</button>
          <button onClick={() => onOpenSource(config.sourcePath)} className="flex items-center gap-1 text-[10px] text-cs-muted hover:text-cs-accent"><ExternalLink size={10} /> Raw</button>
        </div>
      )}
    >
      {!config && !editing ? (
        <EmptyRow
          message="No sandbox configuration found. Sandbox isolates agent execution in Docker containers with network + filesystem policies."
          actionLabel={onCreate ? "Create sandbox config" : undefined}
          onAction={onCreate}
        />
      ) : (
        <div className="space-y-3">
          <div className="grid grid-cols-2 gap-3 md:grid-cols-3">
            <ToggleCard label="Sandbox" value={draft.enabled} editing={editing} onChange={(v) => setDraft({ ...draft, enabled: v })} />
            <ToggleCard label="Network Isolation" value={draft.networkIsolation} editing={editing} onChange={(v) => setDraft({ ...draft, networkIsolation: v })} accent="yellow" />
            <ToggleCard label="Snapshots" value={draft.snapshotEnabled} editing={editing} onChange={(v) => setDraft({ ...draft, snapshotEnabled: v })} />
            {editing ? (
              <>
                <div className="rounded-md border border-cs-border/60 bg-cs-bg/40 px-3 py-2">
                  <label className="mb-1 block text-[10px] text-cs-muted uppercase tracking-wide">Filesystem</label>
                  <select
                    value={draft.filesystemPolicy}
                    onChange={(e) => setDraft({ ...draft, filesystemPolicy: e.target.value })}
                    className="w-full rounded border border-cs-border bg-cs-bg px-2 py-1 text-xs focus:outline-none focus:border-cs-accent"
                  >
                    <option value="read-only">read-only</option>
                    <option value="read-write">read-write</option>
                    <option value="scoped">scoped</option>
                  </select>
                </div>
                <div className="rounded-md border border-cs-border/60 bg-cs-bg/40 px-3 py-2">
                  <label className="mb-1 block text-[10px] text-cs-muted uppercase tracking-wide">Timeout (s)</label>
                  <input
                    type="number"
                    value={draft.timeoutSecs ?? ""}
                    onChange={(e) => setDraft({ ...draft, timeoutSecs: e.target.value ? Number(e.target.value) : null })}
                    placeholder="none"
                    className="w-full rounded border border-cs-border bg-cs-bg px-2 py-1 text-xs focus:outline-none focus:border-cs-accent"
                  />
                </div>
              </>
            ) : (
              <>
                <StatusCard label="Filesystem" value={draft.filesystemPolicy} active={draft.filesystemPolicy !== "read-write"} accent="blue" />
                <StatusCard label="Timeout" value={draft.timeoutSecs ? `${draft.timeoutSecs}s` : "None"} active={draft.timeoutSecs !== null} accent="purple" />
              </>
            )}
          </div>
          {editing && (
            <div className="flex items-center justify-end gap-2 pt-2 border-t border-cs-border/60">
              <button onClick={() => setEditing(false)} className="px-3 py-1 rounded text-xs text-cs-muted hover:bg-cs-border"><X size={11} className="inline mr-1" />Cancel</button>
              <button
                onClick={() => saveMutation.mutate()}
                disabled={saveMutation.isPending}
                className="flex items-center gap-1 px-3 py-1 rounded text-xs font-medium bg-cs-accent text-cs-bg hover:bg-cs-accent/90 disabled:opacity-50"
              >
                {saveMutation.isPending ? <Loader2 size={11} className="animate-spin" /> : <Save size={11} />}
                Save
              </button>
            </div>
          )}
          {saveMutation.isError && (
            <p className="text-[11px] text-red-300">{saveMutation.error instanceof Error ? saveMutation.error.message : "Save failed"}</p>
          )}
        </div>
      )}
    </SectionShell>
  );
}

function ToggleCard({ label, value, editing, onChange, accent = "green" }: {
  label: string; value: boolean; editing: boolean; onChange: (v: boolean) => void; accent?: string;
}) {
  const tones: Record<string, string> = {
    green: "border-green-500/20 bg-green-500/5",
    yellow: "border-yellow-500/20 bg-yellow-500/5",
  };
  return (
    <div className={cn("rounded-md border px-3 py-2", value ? (tones[accent] ?? tones.green) : "border-cs-border/60 bg-cs-bg/40")}>
      <div className="mb-0.5 text-[10px] text-cs-muted uppercase tracking-wide">{label}</div>
      {editing ? (
        <button onClick={() => onChange(!value)} className={cn("text-sm font-medium", value ? "text-green-300" : "text-cs-muted")}>
          {value ? "Enabled" : "Disabled"} <span className="text-[9px]">(click to toggle)</span>
        </button>
      ) : (
        <div className="text-sm font-medium">{value ? "Enabled" : "Disabled"}</div>
      )}
    </div>
  );
}

function StatusCard({ label, value, active, accent = "green" }: { label: string; value: string; active: boolean; accent?: string }) {
  const tones: Record<string, string> = { green: "border-green-500/20 bg-green-500/5", yellow: "border-yellow-500/20 bg-yellow-500/5", blue: "border-blue-500/20 bg-blue-500/5", purple: "border-purple-500/20 bg-purple-500/5" };
  return (
    <div className={cn("rounded-md border px-3 py-2", active ? (tones[accent] ?? tones.green) : "border-cs-border/60 bg-cs-bg/40")}>
      <div className="mb-0.5 text-[10px] text-cs-muted uppercase tracking-wide">{label}</div>
      <div className="text-sm font-medium">{value}</div>
    </div>
  );
}
