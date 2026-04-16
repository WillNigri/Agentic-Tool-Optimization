import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Save, RotateCcw, FileText, AlertCircle, Check, Download } from "lucide-react";
import { cn } from "@/lib/utils";
import { useAgentConfigStore } from "@/stores/useAgentConfigStore";
import {
  readAgentConfigFile,
  writeAgentConfigFile,
  type AgentConfigRuntime,
} from "@/lib/api";
import ExportModal from "./ExportModal";

export default function ConfigFileEditor() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [saveStatus, setSaveStatus] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [showExport, setShowExport] = useState(false);

  const {
    selectedFilePath,
    editingContent,
    originalContent,
    dirty,
    setEditingContent,
    setOriginalContent,
    setDirty,
    updateRawContent,
    getSelectedFile,
  } = useAgentConfigStore();

  const selectedFile = getSelectedFile();

  // Load file content when selected
  useEffect(() => {
    if (!selectedFilePath) return;

    readAgentConfigFile(selectedFilePath)
      .then((parsed) => {
        setEditingContent(parsed);
        setOriginalContent(parsed.raw);
        setDirty(false);
      })
      .catch((err) => {
        console.error("Failed to read config file:", err);
        setEditingContent(null);
        setOriginalContent(null);
      });
  }, [selectedFilePath, setEditingContent, setOriginalContent, setDirty]);

  // Save mutation
  const saveMutation = useMutation({
    mutationFn: async () => {
      if (!selectedFilePath || !editingContent) {
        throw new Error("No file selected");
      }
      await writeAgentConfigFile(selectedFilePath, editingContent.raw);
    },
    onMutate: () => {
      setSaveStatus("saving");
    },
    onSuccess: () => {
      setSaveStatus("saved");
      setOriginalContent(editingContent?.raw || null);
      setDirty(false);
      queryClient.invalidateQueries({ queryKey: ["agent-config-files"] });
      setTimeout(() => setSaveStatus("idle"), 2000);
    },
    onError: () => {
      setSaveStatus("error");
      setTimeout(() => setSaveStatus("idle"), 3000);
    },
  });

  const handleRevert = () => {
    if (originalContent && editingContent) {
      updateRawContent(originalContent);
    }
  };

  if (!selectedFile) {
    return null;
  }

  if (!selectedFile.exists) {
    return (
      <div className="h-full flex items-center justify-center bg-cs-card">
        <div className="text-center">
          <AlertCircle size={48} className="mx-auto mb-3 text-cs-muted opacity-50" />
          <p className="text-cs-muted">
            {t("agentManager.editor.fileNotFound", "File does not exist")}
          </p>
          <p className="text-sm text-cs-muted mt-1">{selectedFilePath}</p>
          <button className="mt-4 px-4 py-2 rounded-md bg-cs-accent text-black text-sm font-medium hover:bg-cs-accent/90 transition-colors">
            {t("agentManager.editor.createFile", "Create File")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col bg-cs-card">
      {/* Toolbar */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-cs-border">
        <div className="flex items-center gap-2">
          <FileText size={16} className="text-cs-muted" />
          <span className="text-sm font-medium truncate max-w-md">
            {selectedFilePath?.split("/").pop()}
          </span>
          {editingContent && (
            <span className="text-xs px-2 py-0.5 rounded bg-cs-border text-cs-muted">
              {editingContent.format}
            </span>
          )}
          {dirty && (
            <span className="text-xs px-2 py-0.5 rounded bg-yellow-500/20 text-yellow-400">
              {t("agentManager.editor.unsaved", "Unsaved")}
            </span>
          )}
        </div>

        <div className="flex items-center gap-2">
          {saveStatus === "saved" && (
            <span className="flex items-center gap-1 text-xs text-green-400">
              <Check size={14} />
              {t("agentManager.editor.saved", "Saved")}
            </span>
          )}
          {saveStatus === "error" && (
            <span className="flex items-center gap-1 text-xs text-red-400">
              <AlertCircle size={14} />
              {t("agentManager.editor.saveFailed", "Save failed")}
            </span>
          )}

          <button
            onClick={handleRevert}
            disabled={!dirty}
            className={cn(
              "p-1.5 rounded-md border border-cs-border transition-colors",
              dirty
                ? "hover:bg-cs-border/50 text-cs-text"
                : "opacity-50 cursor-not-allowed text-cs-muted"
            )}
            title={t("agentManager.editor.revert", "Revert changes")}
          >
            <RotateCcw size={14} />
          </button>

          <button
            onClick={() => setShowExport(true)}
            className="p-1.5 rounded-md border border-cs-border hover:bg-cs-border/50 text-cs-text transition-colors"
            title={t("agentManager.editor.export", "Export to other format")}
          >
            <Download size={14} />
          </button>

          <button
            onClick={() => saveMutation.mutate()}
            disabled={!dirty || saveMutation.isPending}
            className={cn(
              "flex items-center gap-1.5 px-3 py-1.5 rounded-md text-sm font-medium transition-colors",
              dirty
                ? "bg-cs-accent text-black hover:bg-cs-accent/90"
                : "bg-cs-border text-cs-muted cursor-not-allowed"
            )}
          >
            <Save size={14} />
            {saveMutation.isPending
              ? t("common.saving", "Saving...")
              : t("common.save", "Save")}
          </button>
        </div>
      </div>

      {/* Editor */}
      <div className="flex-1 overflow-hidden">
        {editingContent ? (
          <textarea
            value={editingContent.raw}
            onChange={(e) => updateRawContent(e.target.value)}
            className="w-full h-full p-4 bg-transparent text-cs-text font-mono text-sm resize-none focus:outline-none"
            spellCheck={false}
          />
        ) : (
          <div className="h-full flex items-center justify-center">
            <div className="animate-pulse text-cs-muted">
              {t("common.loading", "Loading...")}
            </div>
          </div>
        )}
      </div>

      {/* Status bar */}
      <div className="flex items-center justify-between px-4 py-1.5 border-t border-cs-border text-xs text-cs-muted">
        <div className="flex items-center gap-4">
          <span>
            {selectedFile.runtime.charAt(0).toUpperCase() + selectedFile.runtime.slice(1)}
          </span>
          <span>{selectedFile.scope}</span>
          <span>{selectedFile.fileType}</span>
        </div>
        {selectedFile.tokenCount && (
          <span>~{selectedFile.tokenCount.toLocaleString()} tokens</span>
        )}
      </div>

      {/* Export modal */}
      {showExport && selectedFilePath && (
        <ExportModal
          sourcePath={selectedFilePath}
          sourceRuntime={selectedFile.runtime as AgentConfigRuntime}
          onClose={() => setShowExport(false)}
        />
      )}
    </div>
  );
}
