import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Users,
  UserPlus,
  Trash2,
  Crown,
  Shield,
  User as UserIcon,
  AlertTriangle,
  Loader2,
  ArrowLeft,
  Check,
} from "lucide-react";
import {
  getTeam,
  listTeamMembers,
  renameTeam,
  deleteTeam,
  inviteTeamMember,
  updateTeamMemberRole,
  removeTeamMember,
  type TeamMember,
} from "../lib/api";

interface TeamSettingsPageProps {
  teamId: string;
  onBack: () => void;
  onDeleted: () => void;
}

export default function TeamSettingsPage({
  teamId,
  onBack,
  onDeleted,
}: TeamSettingsPageProps) {
  const queryClient = useQueryClient();
  const teamQuery = useQuery({
    queryKey: ["team", teamId],
    queryFn: () => getTeam(teamId),
  });
  const membersQuery = useQuery({
    queryKey: ["team-members", teamId],
    queryFn: () => listTeamMembers(teamId),
  });

  const team = teamQuery.data;
  const members = membersQuery.data ?? [];
  const myRole = team?.role ?? "member";
  const canManage = myRole === "owner" || myRole === "admin";
  const canDestroy = myRole === "owner";

  // ── Rename ──────────────────────────────────────────────────────
  const [newName, setNewName] = useState("");
  const [renameSaved, setRenameSaved] = useState(false);
  const renameMutation = useMutation({
    mutationFn: () => renameTeam(teamId, newName.trim()),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["team", teamId] });
      queryClient.invalidateQueries({ queryKey: ["teams"] });
      setRenameSaved(true);
      setTimeout(() => setRenameSaved(false), 1500);
    },
  });

  // ── Invite ──────────────────────────────────────────────────────
  const [inviteEmail, setInviteEmail] = useState("");
  const [inviteRole, setInviteRole] = useState<"admin" | "member">("member");
  const [inviteSent, setInviteSent] = useState(false);
  const inviteMutation = useMutation({
    mutationFn: () =>
      inviteTeamMember(teamId, inviteEmail.trim(), inviteRole),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["team-members", teamId] });
      setInviteEmail("");
      setInviteSent(true);
      setTimeout(() => setInviteSent(false), 2000);
    },
  });

  // ── Delete (with explicit-name typing confirmation) ─────────────
  const [showDeletePanel, setShowDeletePanel] = useState(false);
  const [deleteConfirm, setDeleteConfirm] = useState("");
  const deleteMutation = useMutation({
    mutationFn: () => deleteTeam(teamId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["teams"] });
      onDeleted();
    },
  });

  if (teamQuery.isLoading) {
    return (
      <div className="p-8 text-[#8888a0] text-sm">Loading team…</div>
    );
  }
  if (teamQuery.isError || !team) {
    return (
      <div className="p-8 text-red-400 text-sm">
        Couldn't load this team. {teamQuery.error?.message}
      </div>
    );
  }

  return (
    <div className="p-6 max-w-3xl mx-auto space-y-8">
      <div>
        <button
          onClick={onBack}
          className="inline-flex items-center gap-1.5 text-xs text-[#8888a0] hover:text-white transition-colors mb-3"
        >
          <ArrowLeft className="w-3 h-3" /> Back to workspaces
        </button>
        <h1 className="text-xl font-semibold text-white">{team.name}</h1>
        <p className="text-xs text-[#8888a0] mt-1">
          {team.role} · {team.member_count ?? members.length} member
          {(team.member_count ?? members.length) === 1 ? "" : "s"}
          {team.plan ? ` · ${team.plan}` : ""}
        </p>
      </div>

      {/* Rename */}
      <section className="rounded-xl border border-[#2a2a3a] bg-[#0f0f17] p-5 space-y-3">
        <div className="flex items-center gap-2">
          <Shield className="w-4 h-4 text-[#00FFB2]" />
          <h2 className="text-sm font-semibold text-white">Team name</h2>
        </div>
        <p className="text-xs text-[#8888a0]">
          Shown in workspace pickers and shared receipts. The URL slug stays the same.
        </p>
        <div className="flex items-center gap-2">
          <input
            type="text"
            value={newName || team.name}
            onChange={(e) => {
              setNewName(e.target.value);
              setRenameSaved(false);
            }}
            disabled={!canManage}
            className="flex-1 px-3 py-2 bg-[#16161e] border border-[#2a2a3a] rounded-md text-white text-sm placeholder:text-[#5a5a6e] focus:outline-none focus:border-[#00FFB2]/50 disabled:opacity-50"
          />
          <button
            onClick={() => renameMutation.mutate()}
            disabled={
              !canManage ||
              renameMutation.isPending ||
              !newName.trim() ||
              newName.trim() === team.name
            }
            className="px-3 py-2 rounded-md bg-[#00FFB2] text-black text-sm font-semibold hover:bg-[#00FFB2]/90 disabled:opacity-40 disabled:cursor-not-allowed transition-colors inline-flex items-center gap-1.5"
          >
            {renameMutation.isPending && (
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
            )}
            {renameSaved ? (
              <>
                <Check className="w-3.5 h-3.5" /> Saved
              </>
            ) : (
              "Save"
            )}
          </button>
        </div>
        {!canManage && (
          <p className="text-[11px] text-[#5a5a6e]">
            Only owners and admins can rename the team.
          </p>
        )}
        {renameMutation.isError && (
          <p className="text-xs text-red-400">{renameMutation.error?.message}</p>
        )}
      </section>

      {/* Members */}
      <section className="rounded-xl border border-[#2a2a3a] bg-[#0f0f17] p-5 space-y-4">
        <div className="flex items-center gap-2">
          <Users className="w-4 h-4 text-[#00FFB2]" />
          <h2 className="text-sm font-semibold text-white">Members</h2>
        </div>

        {/* Invite row */}
        {canManage && (
          <div className="rounded-lg bg-[#16161e]/60 border border-[#2a2a3a] p-3 space-y-2">
            <p className="text-xs text-[#8888a0] flex items-center gap-1.5">
              <UserPlus className="w-3.5 h-3.5" />
              Invite by email
            </p>
            <div className="flex items-center gap-2">
              <input
                type="email"
                value={inviteEmail}
                onChange={(e) => setInviteEmail(e.target.value)}
                placeholder="teammate@company.com"
                className="flex-1 px-3 py-2 bg-[#16161e] border border-[#2a2a3a] rounded-md text-white text-sm placeholder:text-[#5a5a6e] focus:outline-none focus:border-[#00FFB2]/50"
              />
              <select
                value={inviteRole}
                onChange={(e) =>
                  setInviteRole(e.target.value as "admin" | "member")
                }
                className="appearance-none cursor-pointer pl-3 pr-8 py-2 bg-[#16161e] border border-[#2a2a3a] rounded-md text-white text-sm focus:outline-none focus:border-[#00FFB2]/50 bg-no-repeat bg-[right_8px_center]"
                style={{
                  backgroundImage:
                    "url(\"data:image/svg+xml;charset=UTF-8,%3csvg xmlns='http://www.w3.org/2000/svg' width='14' height='14' viewBox='0 0 24 24' fill='none' stroke='%238888a0' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3e%3cpolyline points='6 9 12 15 18 9'%3e%3c/polyline%3e%3c/svg%3e\")",
                  backgroundSize: "14px",
                }}
              >
                <option value="member" className="bg-[#16161e]">
                  Member
                </option>
                <option value="admin" className="bg-[#16161e]">
                  Admin
                </option>
              </select>
              <button
                onClick={() => inviteMutation.mutate()}
                disabled={
                  inviteMutation.isPending || !inviteEmail.trim().includes("@")
                }
                className="px-3 py-2 rounded-md bg-[#00FFB2] text-black text-sm font-semibold hover:bg-[#00FFB2]/90 disabled:opacity-40 disabled:cursor-not-allowed transition-colors inline-flex items-center gap-1.5"
              >
                {inviteMutation.isPending && (
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                )}
                {inviteSent ? (
                  <>
                    <Check className="w-3.5 h-3.5" /> Sent
                  </>
                ) : (
                  "Invite"
                )}
              </button>
            </div>
            {inviteMutation.isError && (
              <p className="text-xs text-red-400">
                {inviteMutation.error?.message}
              </p>
            )}
          </div>
        )}

        {/* Member list */}
        {membersQuery.isLoading ? (
          <p className="text-xs text-[#8888a0]">Loading members…</p>
        ) : members.length === 0 ? (
          <p className="text-xs text-[#8888a0]">No members yet.</p>
        ) : (
          <ul className="divide-y divide-[#2a2a3a]/60 -my-2">
            {members.map((m) => (
              <MemberRow
                key={m.user_id}
                member={m}
                teamId={teamId}
                canManage={canManage}
                isMyRow={false}
              />
            ))}
          </ul>
        )}
      </section>

      {/* Danger zone */}
      {canDestroy && (
        <section className="rounded-xl border border-red-500/30 bg-red-500/5 p-5 space-y-3">
          <div className="flex items-center gap-2">
            <AlertTriangle className="w-4 h-4 text-red-400" />
            <h2 className="text-sm font-semibold text-red-400">Danger zone</h2>
          </div>
          <p className="text-xs text-[#aaaab8]">
            Deleting a team is irreversible. All shared sessions, war rooms, chats,
            loops, and missions stay on members' desktops; only the team's cloud
            mirror is removed.
          </p>
          {!showDeletePanel ? (
            <button
              onClick={() => setShowDeletePanel(true)}
              className="px-3 py-2 rounded-md border border-red-500/30 bg-red-500/10 text-red-400 text-sm hover:bg-red-500/20 transition-colors inline-flex items-center gap-1.5"
            >
              <Trash2 className="w-3.5 h-3.5" /> Delete team
            </button>
          ) : (
            <div className="space-y-2">
              <p className="text-xs text-[#aaaab8]">
                Type <span className="font-mono text-red-400">{team.name}</span>{" "}
                to confirm.
              </p>
              <div className="flex items-center gap-2">
                <input
                  type="text"
                  value={deleteConfirm}
                  onChange={(e) => setDeleteConfirm(e.target.value)}
                  className="flex-1 px-3 py-2 bg-[#16161e] border border-[#2a2a3a] rounded-md text-white text-sm focus:outline-none focus:border-red-500/50"
                />
                <button
                  onClick={() => deleteMutation.mutate()}
                  disabled={
                    deleteMutation.isPending || deleteConfirm !== team.name
                  }
                  className="px-3 py-2 rounded-md bg-red-500/80 text-white text-sm font-semibold hover:bg-red-500 disabled:opacity-40 disabled:cursor-not-allowed transition-colors inline-flex items-center gap-1.5"
                >
                  {deleteMutation.isPending && (
                    <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  )}
                  Delete forever
                </button>
                <button
                  onClick={() => {
                    setShowDeletePanel(false);
                    setDeleteConfirm("");
                  }}
                  className="px-3 py-2 text-sm text-[#aaaab8] hover:text-white transition-colors"
                >
                  Cancel
                </button>
              </div>
              {deleteMutation.isError && (
                <p className="text-xs text-red-400">
                  {deleteMutation.error?.message}
                </p>
              )}
            </div>
          )}
        </section>
      )}
    </div>
  );
}

function MemberRow({
  member,
  teamId,
  canManage,
}: {
  member: TeamMember;
  teamId: string;
  canManage: boolean;
  isMyRow: boolean;
}) {
  const queryClient = useQueryClient();
  const roleMutation = useMutation({
    mutationFn: (role: "admin" | "member") =>
      updateTeamMemberRole(teamId, member.user_id, role),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: ["team-members", teamId] }),
  });
  const removeMutation = useMutation({
    mutationFn: () => removeTeamMember(teamId, member.user_id),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: ["team-members", teamId] }),
  });

  const isOwner = member.role === "owner";

  return (
    <li className="py-3 flex items-center justify-between gap-3">
      <div className="flex items-center gap-3 min-w-0">
        <div className="w-8 h-8 rounded-full bg-[#16161e] border border-[#2a2a3a] flex items-center justify-center shrink-0">
          {isOwner ? (
            <Crown className="w-3.5 h-3.5 text-[#00FFB2]" />
          ) : (
            <UserIcon className="w-3.5 h-3.5 text-[#8888a0]" />
          )}
        </div>
        <div className="min-w-0">
          <p className="text-sm text-white truncate">
            {member.name || member.email}
          </p>
          <p className="text-[11px] text-[#8888a0] truncate">
            {member.email}
            {member.invite_pending && " · invite pending"}
          </p>
        </div>
      </div>
      <div className="flex items-center gap-2 shrink-0">
        {canManage && !isOwner ? (
          <>
            <select
              value={member.role}
              onChange={(e) =>
                roleMutation.mutate(e.target.value as "admin" | "member")
              }
              disabled={roleMutation.isPending}
              className="appearance-none cursor-pointer pl-2 pr-7 py-1 bg-[#16161e] border border-[#2a2a3a] rounded text-white text-xs focus:outline-none focus:border-[#00FFB2]/50 bg-no-repeat bg-[right_6px_center]"
              style={{
                backgroundImage:
                  "url(\"data:image/svg+xml;charset=UTF-8,%3csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%238888a0' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3e%3cpolyline points='6 9 12 15 18 9'%3e%3c/polyline%3e%3c/svg%3e\")",
                backgroundSize: "12px",
              }}
            >
              <option value="member" className="bg-[#16161e]">
                Member
              </option>
              <option value="admin" className="bg-[#16161e]">
                Admin
              </option>
            </select>
            <button
              onClick={() => {
                if (
                  confirm(
                    `Remove ${member.name || member.email} from this team?`,
                  )
                ) {
                  removeMutation.mutate();
                }
              }}
              disabled={removeMutation.isPending}
              className="p-1.5 rounded-md text-[#8888a0] hover:text-red-400 hover:bg-red-500/10 transition-colors"
              aria-label={`Remove ${member.email}`}
            >
              <Trash2 className="w-3.5 h-3.5" />
            </button>
          </>
        ) : (
          <span className="text-[11px] uppercase tracking-wide text-[#8888a0]">
            {member.role}
          </span>
        )}
      </div>
    </li>
  );
}
