import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { X, RotateCcw, Trash2, Loader2, History } from "lucide-react";
import {
  listSkillVersions,
  restoreSkillVersion,
  deleteSkillVersion,
  type SkillVersion,
} from "@/lib/tauri-api";
import { cn } from "@/lib/utils";

// v1.4.0 Polish-T2 — Skill version history drawer.
// Snapshots are taken automatically on edit (Rust side, in update_skill).
// This drawer surfaces the last 100 versions for the SKILL.md and lets the
// user restore one. Restoring also snapshots the current state so the action
// itself is reversible.

interface Props {
  filePath: string;
  open: boolean;
  onClose: () => void;
  /** Called after a successful restore so the parent can refetch the skill. */
  onRestored?: () => void;
}

export default function SkillVersionsDrawer({ filePath, open, onClose, onRestored }: Props) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [selected, setSelected] = useState<SkillVersion | null>(null);
  const [confirmRestore, setConfirmRestore] = useState(false);

  const { data: versions = [], isLoading } = useQuery({
    queryKey: ["skill-versions", filePath],
    queryFn: () => listSkillVersions(filePath),
    enabled: open,
    staleTime: 5_000,
  });

  const restoreMutation = useMutation({
    mutationFn: (versionId: string) => restoreSkillVersion(versionId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["skill-versions", filePath] });
      void queryClient.invalidateQueries({ queryKey: ["skill-detail"] });
      void queryClient.invalidateQueries({ queryKey: ["skills"] });
      onRestored?.();
      setConfirmRestore(false);
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (versionId: string) => deleteSkillVersion(versionId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["skill-versions", filePath] });
      setSelected(null);
    },
  });

  if (!open) return null;

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-4xl max-h-[85vh] flex flex-col rounded-2xl border border-cs-border bg-cs-card shadow-2xl">
        <header className="flex items-center justify-between p-4 border-b border-cs-border">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <History size={16} className="text-cs-accent" />
              <h2 className="text-sm font-semibold text-cs-text">
                {t("skills.versions.title", "Version history")}
              </h2>
            </div>
            <p className="mt-1 text-[11px] text-cs-muted font-mono truncate">{filePath}</p>
          </div>
          <button
            type="button"
            aria-label={t("common.close", "Close")}
            onClick={onClose}
            className="text-cs-muted hover:text-cs-text"
          >
            <X size={18} />
          </button>
        </header>

        <div className="flex-1 grid grid-cols-[280px_1fr] min-h-0">
          {/* Left: version list */}
          <aside className="border-r border-cs-border overflow-y-auto">
            {isLoading ? (
              <div className="flex items-center justify-center h-32">
                <Loader2 size={16} className="animate-spin text-cs-muted" />
              </div>
            ) : versions.length === 0 ? (
              <p className="p-4 text-xs text-cs-muted">
                {t(
                  "skills.versions.empty",
                  "No prior versions yet. They will appear here once you edit the skill."
                )}
              </p>
            ) : (
              <ul className="py-1">
                {versions.map((v) => {
                  const isSelected = selected?.id === v.id;
                  return (
                    <li key={v.id}>
                      <button
                        type="button"
                        onClick={() => {
                          setSelected(v);
                          setConfirmRestore(false);
                        }}
                        className={cn(
                          "w-full text-left px-3 py-2 border-l-2 transition",
                          isSelected
                            ? "border-cs-accent bg-cs-accent/10"
                            : "border-transparent hover:bg-cs-border/40"
                        )}
                      >
                        <div className="text-xs text-cs-text">
                          {new Date(v.createdAt).toLocaleString()}
                        </div>
                        <div className="mt-0.5 text-[10px] text-cs-muted font-mono">
                          {v.contentHash.slice(0, 10)}
                        </div>
                        {v.note && (
                          <div className="mt-0.5 text-[10px] text-cs-muted italic">
                            {v.note}
                          </div>
                        )}
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </aside>

          {/* Right: preview + actions */}
          <section className="flex flex-col min-h-0">
            {selected ? (
              <>
                <div className="flex items-center justify-between px-4 py-2 border-b border-cs-border bg-cs-bg-raised">
                  <div className="text-xs text-cs-muted">
                    {t("skills.versions.previewLabel", "Snapshot from")}{" "}
                    <span className="text-cs-text">
                      {new Date(selected.createdAt).toLocaleString()}
                    </span>
                  </div>
                  <div className="flex items-center gap-2">
                    {confirmRestore ? (
                      <>
                        <span className="text-xs text-cs-text">
                          {t("skills.versions.confirmRestore", "Replace current contents?")}
                        </span>
                        <button
                          type="button"
                          onClick={() => restoreMutation.mutate(selected.id)}
                          disabled={restoreMutation.isPending}
                          className="inline-flex items-center gap-1 rounded-md bg-cs-accent px-2.5 py-1 text-[11px] font-medium text-cs-bg hover:bg-cs-accent-hover disabled:opacity-50"
                        >
                          {restoreMutation.isPending ? (
                            <Loader2 size={10} className="animate-spin" />
                          ) : (
                            <RotateCcw size={10} />
                          )}
                          {t("skills.versions.confirmRestoreYes", "Restore")}
                        </button>
                        <button
                          type="button"
                          onClick={() => setConfirmRestore(false)}
                          className="rounded-md border border-cs-border px-2.5 py-1 text-[11px] text-cs-muted hover:text-cs-text"
                        >
                          {t("common.cancel", "Cancel")}
                        </button>
                      </>
                    ) : (
                      <>
                        <button
                          type="button"
                          onClick={() => setConfirmRestore(true)}
                          className="inline-flex items-center gap-1 rounded-md border border-cs-accent/40 bg-cs-accent/10 px-2.5 py-1 text-[11px] font-medium text-cs-accent hover:bg-cs-accent/20"
                        >
                          <RotateCcw size={10} />
                          {t("skills.versions.restore", "Restore this version")}
                        </button>
                        <button
                          type="button"
                          onClick={() => deleteMutation.mutate(selected.id)}
                          disabled={deleteMutation.isPending}
                          className="inline-flex items-center gap-1 rounded-md border border-cs-border px-2.5 py-1 text-[11px] text-cs-muted hover:text-cs-danger hover:border-cs-danger/40 disabled:opacity-50"
                        >
                          <Trash2 size={10} />
                          {t("common.delete", "Delete")}
                        </button>
                      </>
                    )}
                  </div>
                </div>
                <pre className="flex-1 overflow-auto bg-cs-bg p-3 text-xs text-cs-text font-mono whitespace-pre-wrap">
                  {selected.content}
                </pre>
              </>
            ) : (
              <div className="flex-1 flex items-center justify-center text-xs text-cs-muted">
                {t("skills.versions.pickHint", "Select a version to preview.")}
              </div>
            )}
          </section>
        </div>
      </div>
    </div>
  );
}
