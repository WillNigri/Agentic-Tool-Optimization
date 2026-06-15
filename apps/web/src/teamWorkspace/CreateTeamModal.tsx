import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { X, Loader2 } from "lucide-react";
import { createTeam } from "../lib/api";

interface CreateTeamModalProps {
  open: boolean;
  onClose: () => void;
  onCreated?: (teamId: string) => void;
}

export default function CreateTeamModal({
  open,
  onClose,
  onCreated,
}: CreateTeamModalProps) {
  const [name, setName] = useState("");
  const [error, setError] = useState("");
  const queryClient = useQueryClient();

  const mutation = useMutation({
    mutationFn: () => createTeam(name.trim()),
    onSuccess: (team) => {
      // Refresh both the teams list and any per-team caches so the
      // newly-created team shows up immediately in the workspace list.
      queryClient.invalidateQueries({ queryKey: ["teams"] });
      onCreated?.(team.id);
      setName("");
      onClose();
    },
    onError: (e: Error) => setError(e.message),
  });

  if (!open) return null;

  const trimmed = name.trim();
  const disabled = trimmed.length < 2 || mutation.isPending;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 px-4"
      role="dialog"
      aria-modal="true"
      aria-labelledby="create-team-title"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-md bg-[#0f0f17] border border-[#2a2a3a] rounded-xl p-6 space-y-5">
        <div className="flex items-start justify-between">
          <div>
            <h2 id="create-team-title" className="text-lg font-semibold text-white">
              Create a team
            </h2>
            <p className="text-xs text-[#8888a0] mt-1">
              You'll be the owner. You can invite teammates after.
            </p>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded-md text-[#8888a0] hover:text-white hover:bg-[#16161e] transition-colors"
            aria-label="Close"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="space-y-2">
          <label htmlFor="team-name" className="block text-xs uppercase tracking-wide text-[#8888a0]">
            Team name
          </label>
          <input
            id="team-name"
            type="text"
            value={name}
            onChange={(e) => {
              setName(e.target.value);
              setError("");
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !disabled) mutation.mutate();
              if (e.key === "Escape") onClose();
            }}
            placeholder="e.g. Acme Eng"
            autoFocus
            className="w-full px-4 py-3 bg-[#16161e] border border-[#2a2a3a] rounded-lg text-white text-sm placeholder:text-[#5a5a6e] focus:outline-none focus:border-[#00FFB2]/50"
          />
          <p className="text-[11px] text-[#5a5a6e]">
            2–60 characters. The URL slug is auto-generated.
          </p>
        </div>

        {error && (
          <div className="px-3 py-2 rounded-md bg-red-500/10 border border-red-500/30 text-xs text-red-400">
            {error}
          </div>
        )}

        <div className="flex items-center justify-end gap-2 pt-2">
          <button
            onClick={onClose}
            className="px-4 py-2 text-sm text-[#aaaab8] hover:text-white transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={() => mutation.mutate()}
            disabled={disabled}
            className="px-4 py-2 rounded-md bg-[#00FFB2] text-black text-sm font-semibold hover:bg-[#00FFB2]/90 disabled:opacity-40 disabled:cursor-not-allowed transition-colors inline-flex items-center gap-2"
          >
            {mutation.isPending && <Loader2 className="w-3.5 h-3.5 animate-spin" />}
            Create team
          </button>
        </div>
      </div>
    </div>
  );
}
