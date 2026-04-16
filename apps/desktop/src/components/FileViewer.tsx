import { useTranslation } from "react-i18next";
import { X, FileText, Copy, Check, Edit3, Save, Loader2, AlertCircle, RefreshCw, AlertTriangle } from "lucide-react";
import { cn } from "@/lib/utils";
import { useState, useEffect, lazy, Suspense } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import * as tauriApi from "@/lib/api";
import SaveConfirmDialog from "./editor/SaveConfirmDialog";
import BackupHistory from "./editor/BackupHistory";

const ATOEditor = lazy(() => import("./editor/ATOEditor"));

interface FileViewerProps {
  filePath: string;
  onClose: () => void;
  readOnly?: boolean;
}

export default function FileViewer({ filePath, onClose, readOnly = false }: FileViewerProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [copied, setCopied] = useState(false);
  const [isEditing, setIsEditing] = useState(false);
  const [editedContent, setEditedContent] = useState("");
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saveSuccess, setSaveSuccess] = useState<null | { added: number; removed: number; backup: string | null }>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);

  // Read file content via the safe agent-config path so we get a content hash.
  const { data: parsed, isLoading, error } = useQuery({
    queryKey: ["config-file", filePath],
    queryFn: () => tauriApi.readAgentConfigFile(filePath),
    retry: false,
  });

  const fileContent = parsed?.raw ?? "";
  const expectedHash = parsed?.contentHash;

  useEffect(() => {
    if (parsed) setEditedContent(parsed.raw);
  }, [parsed]);

  const content = isEditing ? editedContent : fileContent;
  const lineCount = content ? content.split("\n").length : 0;
  const hasChanges = isEditing && editedContent !== fileContent;

  const saveMutation = useMutation({
    mutationFn: async () => {
      return await tauriApi.writeAgentConfigFile(filePath, editedContent, {
        expectedHash,
      });
    },
    onSuccess: (res) => {
      setSaveSuccess({ added: res.addedLines, removed: res.removedLines, backup: res.backupPath });
      setSaveError(null);
      setIsEditing(false);
      setConfirmOpen(false);
      queryClient.invalidateQueries({ queryKey: ["config-file", filePath] });
      setTimeout(() => setSaveSuccess(null), 4000);
    },
    onError: (err) => {
      setSaveError(err instanceof Error ? err.message : String(err));
      setConfirmOpen(false);
    },
  });

  function handleCopy() {
    if (content) {
      navigator.clipboard.writeText(content);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  }

  function handleStartEdit() {
    setEditedContent(fileContent || "");
    setIsEditing(true);
    setSaveError(null);
  }

  function handleCancelEdit() {
    setIsEditing(false);
    setEditedContent(fileContent || "");
    setSaveError(null);
  }

  function requestSave() {
    setSaveError(null);
    setConfirmOpen(true);
  }

  function confirmSave() {
    saveMutation.mutate();
  }

  const isEditable = !readOnly && (
    filePath.endsWith(".json") ||
    filePath.endsWith(".md") ||
    filePath.endsWith(".yaml") ||
    filePath.endsWith(".yml") ||
    filePath.endsWith(".toml")
  );

  return (
    <>
      <div className="fixed inset-0 bg-black/30 z-40 lg:hidden" onClick={onClose} />
      <div className="fixed right-0 top-0 bottom-0 w-full lg:w-1/2 max-w-2xl bg-cs-card border-l border-cs-border z-50 flex flex-col shadow-2xl animate-slide-in-right">
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-cs-border">
          <div className="flex items-center gap-2 min-w-0">
            <FileText size={18} className={cn("shrink-0", isEditing ? "text-yellow-400" : "text-cs-accent")} />
            <div className="min-w-0">
              <h3 className="text-sm font-semibold truncate flex items-center gap-2">
                {filePath.split("/").pop()}
                {isEditing && (
                  <span className="text-[10px] px-1.5 py-0.5 rounded bg-yellow-500/10 text-yellow-400 font-normal">
                    Editing
                  </span>
                )}
                {hasChanges && (
                  <span className="text-[10px] px-1.5 py-0.5 rounded bg-cs-accent/10 text-cs-accent font-normal">
                    Modified
                  </span>
                )}
              </h3>
              <p className="text-[10px] text-cs-muted font-mono truncate">{filePath}</p>
            </div>
          </div>
          <div className="flex items-center gap-2 shrink-0 ml-3">
            {!isLoading && content && (
              <span className="text-[10px] text-cs-muted font-mono">{lineCount} lines</span>
            )}
            {!isEditing && content && (
              <button
                onClick={handleCopy}
                className="p-1.5 rounded hover:bg-cs-border transition-colors text-cs-muted hover:text-cs-text"
                title="Copy"
              >
                {copied ? <Check size={14} className="text-cs-accent" /> : <Copy size={14} />}
              </button>
            )}
            {isEditable && !isEditing && content && (
              <button
                onClick={handleStartEdit}
                className="p-1.5 rounded hover:bg-cs-border transition-colors text-cs-muted hover:text-cs-text"
                title="Edit"
              >
                <Edit3 size={14} />
              </button>
            )}
            {isEditing && (
              <>
                <button
                  onClick={handleCancelEdit}
                  className="px-2 py-1 rounded text-xs text-cs-muted hover:text-cs-text hover:bg-cs-border transition-colors"
                >
                  Cancel
                </button>
                <button
                  onClick={requestSave}
                  disabled={saveMutation.isPending || !hasChanges}
                  className={cn(
                    "flex items-center gap-1.5 px-2 py-1 rounded text-xs font-medium transition-colors",
                    hasChanges
                      ? "bg-cs-accent text-cs-bg hover:bg-cs-accent/90"
                      : "bg-cs-border/50 text-cs-muted cursor-not-allowed"
                  )}
                >
                  {saveMutation.isPending ? (
                    <Loader2 size={12} className="animate-spin" />
                  ) : (
                    <Save size={12} />
                  )}
                  Save
                </button>
              </>
            )}
            <button
              onClick={onClose}
              className="p-1.5 rounded hover:bg-cs-border transition-colors text-cs-muted hover:text-cs-text"
            >
              <X size={16} />
            </button>
          </div>
        </div>

        {/* Save error */}
        {saveError && !saveError.startsWith("CONFLICT:") && !saveError.startsWith("VALIDATION_FAILED:") && (
          <div className="mx-4 mt-4 flex items-start gap-2 px-3 py-2 rounded-lg bg-red-500/10 border border-red-500/20">
            <AlertCircle size={14} className="text-red-400 shrink-0 mt-0.5" />
            <p className="text-xs text-red-300 break-words">{saveError}</p>
          </div>
        )}

        {/* Conflict: file changed on disk */}
        {saveError?.startsWith("CONFLICT:") && (
          <div className="mx-4 mt-4 flex items-start gap-3 px-3 py-3 rounded-lg bg-yellow-500/10 border border-yellow-500/30">
            <AlertTriangle size={16} className="text-yellow-400 shrink-0 mt-0.5" />
            <div className="flex-1 text-xs text-yellow-100/90">
              <p className="font-semibold text-yellow-300 mb-0.5">File changed on disk</p>
              <p className="text-yellow-100/70 mb-2">
                Someone (or another process) wrote to this file after you opened it. Reload to see the
                latest version, or overwrite to force your changes.
              </p>
              <div className="flex gap-2">
                <button
                  onClick={() => {
                    setSaveError(null);
                    queryClient.invalidateQueries({ queryKey: ["config-file", filePath] });
                  }}
                  className="flex items-center gap-1.5 rounded-md bg-yellow-500/20 px-2.5 py-1 text-yellow-200 hover:bg-yellow-500/30"
                >
                  <RefreshCw size={11} /> Reload
                </button>
                <button
                  onClick={() => {
                    setSaveError(null);
                    tauriApi.writeAgentConfigFile(filePath, editedContent, { skipValidation: false })
                      .then((res) => {
                        setSaveSuccess({ added: res.addedLines, removed: res.removedLines, backup: res.backupPath });
                        setIsEditing(false);
                        queryClient.invalidateQueries({ queryKey: ["config-file", filePath] });
                      })
                      .catch((err) => setSaveError(err instanceof Error ? err.message : String(err)));
                  }}
                  className="flex items-center gap-1.5 rounded-md border border-yellow-500/30 px-2.5 py-1 text-yellow-200 hover:bg-yellow-500/10"
                >
                  Overwrite anyway
                </button>
              </div>
            </div>
          </div>
        )}

        {/* Validation failed */}
        {saveError?.startsWith("VALIDATION_FAILED:") && (
          <div className="mx-4 mt-4 flex items-start gap-2 px-3 py-2 rounded-lg bg-red-500/10 border border-red-500/30">
            <AlertTriangle size={14} className="text-red-400 shrink-0 mt-0.5" />
            <div className="text-xs text-red-300">
              <p className="font-semibold mb-1">Schema validation failed</p>
              <p className="text-red-200/70 break-words">{saveError.replace("VALIDATION_FAILED: ", "")}</p>
            </div>
          </div>
        )}

        {/* Save success */}
        {saveSuccess && (
          <div className="mx-4 mt-4 flex items-start gap-2 px-3 py-2 rounded-lg bg-cs-success/10 border border-cs-success/20">
            <Check size={14} className="text-cs-success shrink-0 mt-0.5" />
            <div className="text-xs text-cs-success">
              <p>Saved — +{saveSuccess.added} / −{saveSuccess.removed} lines.</p>
              {saveSuccess.backup && (
                <p className="text-[10px] text-cs-success/70 font-mono truncate mt-0.5">
                  Backup: {saveSuccess.backup.split("/").pop()}
                </p>
              )}
            </div>
          </div>
        )}

        {/* Backup history (collapsed by default) */}
        {isEditable && parsed && (
          <BackupHistory filePath={filePath} currentHash={expectedHash} />
        )}

        {/* Content */}
        <div className="flex-1 overflow-hidden p-4">
          {isLoading ? (
            <div className="flex items-center justify-center h-full">
              <Loader2 size={24} className="text-cs-muted animate-spin" />
            </div>
          ) : error ? (
            <div className="text-center py-12">
              <FileText size={32} className="text-cs-muted/30 mx-auto mb-3" />
              <p className="text-sm text-cs-muted">{t("context.fileNotFound")}</p>
              <p className="text-xs text-red-400/80 mt-2">{String(error)}</p>
              <p className="text-xs text-cs-muted/60 mt-1 font-mono">{filePath}</p>
            </div>
          ) : isEditing ? (
            <Suspense fallback={<div className="flex items-center justify-center h-full"><Loader2 size={24} className="text-cs-muted animate-spin" /></div>}>
              <ATOEditor
                value={editedContent}
                filePath={filePath}
                onChange={setEditedContent}
                onSave={hasChanges ? requestSave : undefined}
                className="h-full w-full overflow-hidden rounded-lg border border-cs-border bg-cs-bg"
              />
            </Suspense>
          ) : content ? (
            <Suspense fallback={<div className="flex items-center justify-center h-full"><Loader2 size={24} className="text-cs-muted animate-spin" /></div>}>
              <ATOEditor
                value={content}
                filePath={filePath}
                readOnly
                onChange={() => {}}
                className="h-full w-full overflow-hidden rounded-lg border border-cs-border bg-cs-bg"
              />
            </Suspense>
          ) : (
            <div className="text-center py-12">
              <FileText size={32} className="text-cs-muted/30 mx-auto mb-3" />
              <p className="text-sm text-cs-muted">File is empty</p>
              {isEditable && (
                <button
                  onClick={handleStartEdit}
                  className="mt-3 px-3 py-1.5 text-xs text-cs-accent hover:bg-cs-accent/10 rounded transition-colors"
                >
                  Start editing
                </button>
              )}
            </div>
          )}
        </div>
      </div>

      <SaveConfirmDialog
        open={confirmOpen}
        filePath={filePath}
        newContent={editedContent}
        onConfirm={confirmSave}
        onCancel={() => setConfirmOpen(false)}
        saving={saveMutation.isPending}
      />
    </>
  );
}
