import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  X,
  FolderOpen,
  Trash2,
  Download,
  Upload,
  Loader2,
  AlertCircle,
} from "lucide-react";
import { cn } from "@/lib/utils";
import {
  listProfileSnapshots,
  deleteProfileSnapshot,
  loadProfileSnapshot,
  exportProfileSnapshot,
  type ProfileSnapshot,
} from "@/lib/tauri-api";

interface Props {
  onClose: () => void;
}

export default function ProfileManagerModal({ onClose }: Props) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [loadingId, setLoadingId] = useState<string | null>(null);

  const { data: profiles = [], isLoading } = useQuery({
    queryKey: ["profile-snapshots"],
    queryFn: listProfileSnapshots,
  });

  const deleteMutation = useMutation({
    mutationFn: deleteProfileSnapshot,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["profile-snapshots"] });
    },
  });

  const loadMutation = useMutation({
    mutationFn: loadProfileSnapshot,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["agent-config-files"] });
      alert(t("agentManager.profiles.loadSuccess", "Profile loaded successfully!"));
    },
  });

  const handleLoad = async (profileId: string) => {
    if (confirm(t("agentManager.profiles.loadConfirm", "This will overwrite existing config files. Continue?"))) {
      setLoadingId(profileId);
      try {
        await loadMutation.mutateAsync(profileId);
      } finally {
        setLoadingId(null);
      }
    }
  };

  const handleExport = async (profileId: string, profileName: string) => {
    try {
      const json = await exportProfileSnapshot(profileId);
      const blob = new Blob([json], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${profileName.toLowerCase().replace(/\s+/g, "-")}.json`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      console.error("Export failed:", err);
    }
  };

  const handleDelete = (profileId: string) => {
    if (confirm(t("agentManager.profiles.deleteConfirm", "Delete this profile?"))) {
      deleteMutation.mutate(profileId);
    }
  };

  const formatDate = (dateStr: string) => {
    try {
      return new Date(dateStr).toLocaleDateString(undefined, {
        year: "numeric",
        month: "short",
        day: "numeric",
      });
    } catch {
      return dateStr;
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-cs-card border border-cs-border rounded-lg w-full max-w-lg mx-4 overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-cs-border">
          <div className="flex items-center gap-2">
            <FolderOpen size={18} className="text-cs-accent" />
            <h2 className="font-semibold">
              {t("agentManager.profiles.manageTitle", "Manage Profiles")}
            </h2>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-cs-border transition-colors"
          >
            <X size={18} />
          </button>
        </div>

        {/* Content */}
        <div className="max-h-[400px] overflow-y-auto">
          {isLoading ? (
            <div className="flex items-center justify-center py-12">
              <Loader2 size={24} className="animate-spin text-cs-muted" />
            </div>
          ) : profiles.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-12 text-cs-muted">
              <FolderOpen size={32} className="mb-2 opacity-50" />
              <p className="text-sm">{t("agentManager.profiles.empty", "No saved profiles")}</p>
              <p className="text-xs mt-1">Save a profile from the Profiles dropdown</p>
            </div>
          ) : (
            <div className="divide-y divide-cs-border">
              {profiles.map((profile) => (
                <div
                  key={profile.id}
                  className="flex items-center justify-between p-4 hover:bg-cs-border/20"
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <h3 className="font-medium truncate">{profile.name}</h3>
                      <span className="text-xs px-2 py-0.5 rounded bg-cs-border text-cs-muted">
                        {profile.runtime}
                      </span>
                    </div>
                    {profile.description && (
                      <p className="text-sm text-cs-muted mt-0.5 truncate">
                        {profile.description}
                      </p>
                    )}
                    <p className="text-xs text-cs-muted mt-1">
                      {profile.files.length} files • {formatDate(profile.createdAt)}
                    </p>
                  </div>

                  <div className="flex items-center gap-1 ml-4">
                    <button
                      onClick={() => handleLoad(profile.id)}
                      disabled={loadingId === profile.id}
                      className="p-2 rounded-md hover:bg-cs-border transition-colors"
                      title={t("agentManager.profiles.load", "Load Profile")}
                    >
                      {loadingId === profile.id ? (
                        <Loader2 size={14} className="animate-spin" />
                      ) : (
                        <Upload size={14} />
                      )}
                    </button>
                    <button
                      onClick={() => handleExport(profile.id, profile.name)}
                      className="p-2 rounded-md hover:bg-cs-border transition-colors"
                      title={t("agentManager.profiles.export", "Export")}
                    >
                      <Download size={14} />
                    </button>
                    <button
                      onClick={() => handleDelete(profile.id)}
                      disabled={deleteMutation.isPending}
                      className="p-2 rounded-md hover:bg-red-500/20 text-red-400 transition-colors"
                      title={t("common.delete", "Delete")}
                    >
                      <Trash2 size={14} />
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end px-4 py-3 border-t border-cs-border">
          <button
            onClick={onClose}
            className="px-4 py-2 rounded-md text-sm bg-cs-accent text-black font-medium hover:bg-cs-accent/90 transition-colors"
          >
            {t("common.done", "Done")}
          </button>
        </div>
      </div>
    </div>
  );
}
