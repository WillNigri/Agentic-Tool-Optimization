import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { X, Save, AlertCircle } from "lucide-react";
import { cn } from "@/lib/utils";
import { saveProfileSnapshot, type AgentConfigRuntime } from "@/lib/tauri-api";

interface Props {
  currentRuntime: AgentConfigRuntime;
  onClose: () => void;
  onSaved: () => void;
}

export default function SaveProfileModal({ currentRuntime, onClose, onSaved }: Props) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");

  const saveMutation = useMutation({
    mutationFn: () => saveProfileSnapshot(name, description || null, currentRuntime),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["profile-snapshots"] });
      onSaved();
    },
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (name.trim()) {
      saveMutation.mutate();
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-cs-card border border-cs-border rounded-lg w-full max-w-md mx-4 overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-cs-border">
          <div className="flex items-center gap-2">
            <Save size={18} className="text-cs-accent" />
            <h2 className="font-semibold">
              {t("agentManager.profiles.saveTitle", "Save Profile")}
            </h2>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-cs-border transition-colors"
          >
            <X size={18} />
          </button>
        </div>

        {/* Form */}
        <form onSubmit={handleSubmit} className="p-4 space-y-4">
          <div>
            <label className="block text-sm font-medium mb-1.5">
              {t("agentManager.profiles.name", "Profile Name")}
            </label>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("agentManager.profiles.namePlaceholder", "e.g., Python Backend")}
              className="w-full px-3 py-2 bg-cs-card border border-cs-border rounded-md text-sm focus:outline-none focus:border-cs-accent"
              autoFocus
            />
          </div>

          <div>
            <label className="block text-sm font-medium mb-1.5">
              {t("agentManager.profiles.description", "Description")}
              <span className="text-cs-muted font-normal ml-1">(optional)</span>
            </label>
            <textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder={t("agentManager.profiles.descriptionPlaceholder", "What is this profile for?")}
              rows={2}
              className="w-full px-3 py-2 bg-cs-card border border-cs-border rounded-md text-sm focus:outline-none focus:border-cs-accent resize-none"
            />
          </div>

          <div className="bg-cs-bg rounded-md p-3">
            <p className="text-xs text-cs-muted">
              {t("agentManager.profiles.saveInfo", "This will save all config files for")}
              {" "}
              <span className="font-medium text-cs-text">{currentRuntime}</span>
              {" "}
              {t("agentManager.profiles.saveInfo2", "including settings, skills, and project config.")}
            </p>
          </div>

          {/* Error */}
          {saveMutation.isError && (
            <div className="flex items-center gap-2 text-sm text-red-400">
              <AlertCircle size={14} />
              <span>
                {saveMutation.error instanceof Error
                  ? saveMutation.error.message
                  : t("common.error", "An error occurred")}
              </span>
            </div>
          )}

          {/* Actions */}
          <div className="flex items-center justify-end gap-2 pt-2">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 rounded-md text-sm text-cs-muted hover:text-cs-text transition-colors"
            >
              {t("common.cancel", "Cancel")}
            </button>
            <button
              type="submit"
              disabled={!name.trim() || saveMutation.isPending}
              className={cn(
                "px-4 py-2 rounded-md text-sm font-medium transition-colors",
                name.trim()
                  ? "bg-cs-accent text-black hover:bg-cs-accent/90"
                  : "bg-cs-border text-cs-muted cursor-not-allowed"
              )}
            >
              {saveMutation.isPending
                ? t("common.saving", "Saving...")
                : t("agentManager.profiles.saveButton", "Save Profile")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
