import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  ChevronDown,
  Save,
  FolderOpen,
  Settings,
  Loader2,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { listProfileSnapshots, type ProfileSnapshot } from "@/lib/api";

interface Props {
  onSaveProfile: () => void;
  onLoadProfile: (profile: ProfileSnapshot) => void;
  onManageProfiles: () => void;
}

export default function ProfileDropdown({
  onSaveProfile,
  onLoadProfile,
  onManageProfiles,
}: Props) {
  const { t } = useTranslation();
  const [isOpen, setIsOpen] = useState(false);

  const { data: profiles = [], isLoading } = useQuery({
    queryKey: ["profile-snapshots"],
    queryFn: listProfileSnapshots,
  });

  return (
    <div className="relative">
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="flex items-center gap-2 px-3 py-1.5 rounded-md border border-cs-border text-sm hover:bg-cs-border/50 transition-colors"
      >
        <FolderOpen size={14} />
        <span>{t("agentManager.profiles.button", "Profiles")}</span>
        <ChevronDown size={14} className={cn("transition-transform", isOpen && "rotate-180")} />
      </button>

      {isOpen && (
        <>
          {/* Backdrop */}
          <div
            className="fixed inset-0 z-40"
            onClick={() => setIsOpen(false)}
          />

          {/* Dropdown */}
          <div className="absolute right-0 top-full mt-1 w-64 bg-cs-card border border-cs-border rounded-lg shadow-xl z-50 overflow-hidden">
            {/* Actions */}
            <div className="p-2 border-b border-cs-border">
              <button
                onClick={() => {
                  setIsOpen(false);
                  onSaveProfile();
                }}
                className="w-full flex items-center gap-2 px-3 py-2 rounded-md text-sm text-left hover:bg-cs-border/50 transition-colors"
              >
                <Save size={14} className="text-cs-accent" />
                <span>{t("agentManager.profiles.save", "Save Current as Profile...")}</span>
              </button>
              <button
                onClick={() => {
                  setIsOpen(false);
                  onManageProfiles();
                }}
                className="w-full flex items-center gap-2 px-3 py-2 rounded-md text-sm text-left hover:bg-cs-border/50 transition-colors"
              >
                <Settings size={14} className="text-cs-muted" />
                <span>{t("agentManager.profiles.manage", "Manage Profiles...")}</span>
              </button>
            </div>

            {/* Saved profiles */}
            <div className="max-h-64 overflow-y-auto">
              {isLoading ? (
                <div className="flex items-center justify-center py-4">
                  <Loader2 size={16} className="animate-spin text-cs-muted" />
                </div>
              ) : profiles.length === 0 ? (
                <div className="px-3 py-4 text-center text-sm text-cs-muted">
                  {t("agentManager.profiles.empty", "No saved profiles")}
                </div>
              ) : (
                <div className="p-2">
                  <p className="px-2 py-1 text-xs text-cs-muted uppercase tracking-wide">
                    {t("agentManager.profiles.load", "Load Profile")}
                  </p>
                  {profiles.map((profile) => (
                    <button
                      key={profile.id}
                      onClick={() => {
                        setIsOpen(false);
                        onLoadProfile(profile);
                      }}
                      className="w-full flex items-start gap-2 px-3 py-2 rounded-md text-sm text-left hover:bg-cs-border/50 transition-colors"
                    >
                      <FolderOpen size={14} className="mt-0.5 text-cs-muted shrink-0" />
                      <div className="min-w-0">
                        <p className="font-medium truncate">{profile.name}</p>
                        <p className="text-xs text-cs-muted">
                          {profile.runtime} • {profile.files.length} files
                        </p>
                      </div>
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  );
}
