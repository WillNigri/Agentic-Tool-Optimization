import { useTranslation } from "react-i18next";
import { X, FileText, Copy, Check, Edit3, Save, Loader2, AlertCircle } from "lucide-react";
import { cn } from "@/lib/utils";
import { useState, useEffect } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import * as tauriApi from "@/lib/tauri-api";

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
  const [saveSuccess, setSaveSuccess] = useState(false);

  // Read file content from Tauri
  const { data: fileContent, isLoading, error } = useQuery({
    queryKey: ["context-file", filePath],
    queryFn: () => tauriApi.readContextFile(filePath),
    retry: false,
  });

  // Update edited content when file content loads
  useEffect(() => {
    if (fileContent) {
      setEditedContent(fileContent);
    }
  }, [fileContent]);

  const content = isEditing ? editedContent : fileContent;
  const lineCount = content ? content.split("\n").length : 0;
  const hasChanges = isEditing && editedContent !== fileContent;

  // Save mutation
  const saveMutation = useMutation({
    mutationFn: async () => {
      await tauriApi.writeContextFile(filePath, editedContent);
    },
    onSuccess: () => {
      setSaveSuccess(true);
      setSaveError(null);
      setIsEditing(false);
      queryClient.invalidateQueries({ queryKey: ["context-file", filePath] });
      setTimeout(() => setSaveSuccess(false), 2500);
    },
    onError: (err) => {
      setSaveError(err instanceof Error ? err.message : "Failed to save file");
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

  function handleSave() {
    saveMutation.mutate();
  }

  // Determine if file is editable (config/skill files)
  const isEditable = !readOnly && (
    filePath.endsWith(".json") ||
    filePath.endsWith(".md") ||
    filePath.endsWith(".yaml") ||
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
                  onClick={handleSave}
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
        {saveError && (
          <div className="mx-4 mt-4 flex items-center gap-2 px-3 py-2 rounded-lg bg-red-500/10 border border-red-500/20">
            <AlertCircle size={14} className="text-red-400 shrink-0" />
            <p className="text-xs text-red-300">{saveError}</p>
          </div>
        )}

        {/* Save success */}
        {saveSuccess && (
          <div className="mx-4 mt-4 flex items-center gap-2 px-3 py-2 rounded-lg bg-cs-success/10 border border-cs-success/20">
            <Check size={14} className="text-cs-success shrink-0" />
            <p className="text-xs text-cs-success">File saved successfully</p>
          </div>
        )}

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-4">
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
            <textarea
              value={editedContent}
              onChange={(e) => setEditedContent(e.target.value)}
              className="w-full h-full min-h-[400px] text-sm font-mono text-cs-text bg-cs-bg border border-cs-border rounded-lg p-4 resize-none focus:outline-none focus:border-cs-accent/50 focus:ring-1 focus:ring-cs-accent/20"
              spellCheck={false}
            />
          ) : content ? (
            <pre className="text-sm font-mono text-cs-text whitespace-pre-wrap leading-relaxed">
              {content.split("\n").map((line, i) => (
                <div key={i} className="flex hover:bg-cs-bg/50 -mx-2 px-2 rounded">
                  <span className="text-cs-muted/40 select-none w-8 shrink-0 text-right mr-3 text-xs leading-relaxed">
                    {i + 1}
                  </span>
                  <span className="flex-1">{line || "\u00A0"}</span>
                </div>
              ))}
            </pre>
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
    </>
  );
}
