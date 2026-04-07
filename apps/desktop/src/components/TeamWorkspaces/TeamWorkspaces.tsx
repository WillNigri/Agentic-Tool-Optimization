import { useState, useEffect } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  Users,
  Plus,
  Settings,
  UserPlus,
  Sparkles,
  Trash2,
  Crown,
  Shield,
  User,
  Loader2,
  ChevronRight,
  ArrowLeft,
  Mail,
  Copy,
  Check,
  AlertCircle,
  LogIn,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { useCloudStore } from '@/stores/useCloudStore';
import {
  getTeams,
  getTeam,
  createTeam,
  deleteTeam,
  inviteTeamMember,
  removeTeamMember,
  getTeamSkills,
  deleteTeamSkill,
  type Team,
  type TeamWithMembers,
  type TeamSkill,
} from '@/lib/cloud-api';

type View = 'list' | 'detail' | 'create';

export default function TeamWorkspaces() {
  const { isAuthenticated, user } = useCloudStore();
  const queryClient = useQueryClient();
  const [view, setView] = useState<View>('list');
  const [selectedTeamId, setSelectedTeamId] = useState<string | null>(null);
  const [newTeamName, setNewTeamName] = useState('');
  const [newTeamDesc, setNewTeamDesc] = useState('');
  const [inviteEmail, setInviteEmail] = useState('');
  const [inviteRole, setInviteRole] = useState<'admin' | 'member'>('member');
  const [copiedId, setCopiedId] = useState<string | null>(null);

  // Fetch teams list
  const { data: teams = [], isLoading: teamsLoading } = useQuery({
    queryKey: ['teams'],
    queryFn: getTeams,
    enabled: isAuthenticated,
  });

  // Fetch team details when selected
  const { data: teamDetail, isLoading: detailLoading } = useQuery({
    queryKey: ['team', selectedTeamId],
    queryFn: () => getTeam(selectedTeamId!),
    enabled: !!selectedTeamId && isAuthenticated,
  });

  // Fetch team skills
  const { data: teamSkills = [] } = useQuery({
    queryKey: ['team-skills', selectedTeamId],
    queryFn: () => getTeamSkills(selectedTeamId!),
    enabled: !!selectedTeamId && isAuthenticated,
  });

  // Create team mutation
  const createTeamMutation = useMutation({
    mutationFn: () => createTeam(newTeamName, newTeamDesc || undefined),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['teams'] });
      setNewTeamName('');
      setNewTeamDesc('');
      setView('list');
    },
  });

  // Delete team mutation
  const deleteTeamMutation = useMutation({
    mutationFn: deleteTeam,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['teams'] });
      setSelectedTeamId(null);
      setView('list');
    },
  });

  // Invite member mutation
  const inviteMemberMutation = useMutation({
    mutationFn: () => inviteTeamMember(selectedTeamId!, inviteEmail, inviteRole),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['team', selectedTeamId] });
      setInviteEmail('');
    },
  });

  // Remove member mutation
  const removeMemberMutation = useMutation({
    mutationFn: (userId: string) => removeTeamMember(selectedTeamId!, userId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['team', selectedTeamId] });
    },
  });

  // Delete skill mutation
  const deleteSkillMutation = useMutation({
    mutationFn: (skillId: string) => deleteTeamSkill(selectedTeamId!, skillId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['team-skills', selectedTeamId] });
    },
  });

  const selectTeam = (teamId: string) => {
    setSelectedTeamId(teamId);
    setView('detail');
  };

  const copyInviteLink = (token: string) => {
    navigator.clipboard.writeText(`${window.location.origin}/invite/${token}`);
    setCopiedId(token);
    setTimeout(() => setCopiedId(null), 2000);
  };

  const getRoleIcon = (role: string) => {
    switch (role) {
      case 'owner':
        return <Crown size={14} className="text-yellow-400" />;
      case 'admin':
        return <Shield size={14} className="text-blue-400" />;
      default:
        return <User size={14} className="text-cs-muted" />;
    }
  };

  // Not authenticated view
  if (!isAuthenticated) {
    return (
      <div className="space-y-6">
        <div>
          <h2 className="text-xl font-semibold flex items-center gap-2">
            <Users className="text-cs-accent" size={24} />
            Team Workspaces
          </h2>
          <p className="text-sm text-cs-muted mt-1">
            Collaborate with your team on shared skills
          </p>
        </div>

        <div className="card text-center py-12">
          <Users size={48} className="mx-auto mb-4 text-cs-muted opacity-50" />
          <h3 className="text-lg font-medium mb-2">Sign in to access Teams</h3>
          <p className="text-sm text-cs-muted mb-4">
            Create or join teams to share skills and collaborate
          </p>
          <button
            onClick={() => {
              // Navigate to cloud section
              const event = new CustomEvent('navigate', { detail: 'cloud' });
              window.dispatchEvent(event);
            }}
            className="inline-flex items-center gap-2 px-4 py-2 bg-cs-accent text-cs-bg rounded-lg font-medium hover:bg-cs-accent/90 transition-colors"
          >
            <LogIn size={18} />
            Sign in to ATO Cloud
          </button>
        </div>
      </div>
    );
  }

  // Create team view
  if (view === 'create') {
    return (
      <div className="space-y-6">
        <div className="flex items-center gap-4">
          <button
            onClick={() => setView('list')}
            className="p-2 hover:bg-cs-border/50 rounded-lg transition-colors"
          >
            <ArrowLeft size={20} />
          </button>
          <div>
            <h2 className="text-xl font-semibold">Create Team</h2>
            <p className="text-sm text-cs-muted mt-1">
              Set up a new team workspace
            </p>
          </div>
        </div>

        <form
          onSubmit={(e) => {
            e.preventDefault();
            createTeamMutation.mutate();
          }}
          className="card space-y-4"
        >
          <div>
            <label className="block text-sm font-medium mb-1">Team Name</label>
            <input
              type="text"
              value={newTeamName}
              onChange={(e) => setNewTeamName(e.target.value)}
              placeholder="My Awesome Team"
              required
              className="w-full px-4 py-2 bg-cs-bg border border-cs-border rounded-lg text-sm focus:outline-none focus:border-cs-accent"
            />
          </div>

          <div>
            <label className="block text-sm font-medium mb-1">Description (optional)</label>
            <textarea
              value={newTeamDesc}
              onChange={(e) => setNewTeamDesc(e.target.value)}
              placeholder="What does this team work on?"
              rows={3}
              className="w-full px-4 py-2 bg-cs-bg border border-cs-border rounded-lg text-sm focus:outline-none focus:border-cs-accent resize-none"
            />
          </div>

          <div className="flex items-center gap-3 pt-2">
            <button
              type="button"
              onClick={() => setView('list')}
              className="px-4 py-2 border border-cs-border rounded-lg text-sm hover:bg-cs-border/50 transition-colors"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={!newTeamName || createTeamMutation.isPending}
              className="flex items-center gap-2 px-4 py-2 bg-cs-accent text-cs-bg rounded-lg text-sm font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50"
            >
              {createTeamMutation.isPending ? (
                <Loader2 size={16} className="animate-spin" />
              ) : (
                <Plus size={16} />
              )}
              Create Team
            </button>
          </div>
        </form>
      </div>
    );
  }

  // Team detail view
  if (view === 'detail' && selectedTeamId) {
    const isOwner = teamDetail?.members.some(
      (m) => m.user_id === user?.id && m.role === 'owner'
    );
    const isAdmin = teamDetail?.members.some(
      (m) => m.user_id === user?.id && (m.role === 'owner' || m.role === 'admin')
    );

    return (
      <div className="space-y-6">
        {/* Header */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            <button
              onClick={() => {
                setSelectedTeamId(null);
                setView('list');
              }}
              className="p-2 hover:bg-cs-border/50 rounded-lg transition-colors"
            >
              <ArrowLeft size={20} />
            </button>
            <div>
              <h2 className="text-xl font-semibold flex items-center gap-2">
                {teamDetail?.name || 'Loading...'}
              </h2>
              <p className="text-sm text-cs-muted mt-1">
                {teamDetail?.description || 'No description'}
              </p>
            </div>
          </div>
          {isOwner && (
            <button
              onClick={() => {
                if (confirm('Are you sure you want to delete this team?')) {
                  deleteTeamMutation.mutate(selectedTeamId);
                }
              }}
              className="flex items-center gap-2 px-3 py-2 text-red-400 hover:bg-red-400/10 rounded-lg transition-colors"
            >
              <Trash2 size={16} />
              Delete Team
            </button>
          )}
        </div>

        {detailLoading ? (
          <div className="flex items-center justify-center py-12">
            <Loader2 size={32} className="animate-spin text-cs-accent" />
          </div>
        ) : (
          <div className="grid grid-cols-2 gap-6">
            {/* Members */}
            <div className="card">
              <div className="flex items-center justify-between mb-4">
                <h3 className="text-sm font-medium text-cs-muted">Members</h3>
                <span className="text-xs text-cs-muted">
                  {teamDetail?.member_count} member{teamDetail?.member_count !== 1 ? 's' : ''}
                </span>
              </div>

              <div className="space-y-2 mb-4">
                {teamDetail?.members.map((member) => (
                  <div
                    key={member.id}
                    className="flex items-center justify-between p-2 bg-cs-bg rounded-lg"
                  >
                    <div className="flex items-center gap-3">
                      {member.user?.avatar_url ? (
                        <img
                          src={member.user.avatar_url}
                          alt={member.user.name}
                          className="w-8 h-8 rounded-full"
                        />
                      ) : (
                        <div className="w-8 h-8 rounded-full bg-cs-border flex items-center justify-center">
                          <User size={14} className="text-cs-muted" />
                        </div>
                      )}
                      <div>
                        <p className="text-sm font-medium">{member.user?.name}</p>
                        <p className="text-xs text-cs-muted flex items-center gap-1">
                          {getRoleIcon(member.role)}
                          {member.role}
                        </p>
                      </div>
                    </div>
                    {isAdmin && member.role !== 'owner' && member.user_id !== user?.id && (
                      <button
                        onClick={() => removeMemberMutation.mutate(member.user_id)}
                        className="p-1 text-cs-muted hover:text-red-400 transition-colors"
                      >
                        <Trash2 size={14} />
                      </button>
                    )}
                  </div>
                ))}
              </div>

              {/* Invite form */}
              {isAdmin && (
                <form
                  onSubmit={(e) => {
                    e.preventDefault();
                    inviteMemberMutation.mutate();
                  }}
                  className="space-y-3 pt-4 border-t border-cs-border"
                >
                  <h4 className="text-sm font-medium">Invite Member</h4>
                  <div className="flex items-center gap-2">
                    <input
                      type="email"
                      value={inviteEmail}
                      onChange={(e) => setInviteEmail(e.target.value)}
                      placeholder="email@example.com"
                      required
                      className="flex-1 px-3 py-2 bg-cs-bg border border-cs-border rounded-lg text-sm focus:outline-none focus:border-cs-accent"
                    />
                    <select
                      value={inviteRole}
                      onChange={(e) => setInviteRole(e.target.value as 'admin' | 'member')}
                      className="px-3 py-2 bg-cs-bg border border-cs-border rounded-lg text-sm focus:outline-none focus:border-cs-accent"
                    >
                      <option value="member">Member</option>
                      <option value="admin">Admin</option>
                    </select>
                  </div>
                  <button
                    type="submit"
                    disabled={!inviteEmail || inviteMemberMutation.isPending}
                    className="w-full flex items-center justify-center gap-2 px-3 py-2 bg-cs-accent text-cs-bg rounded-lg text-sm font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50"
                  >
                    {inviteMemberMutation.isPending ? (
                      <Loader2 size={14} className="animate-spin" />
                    ) : (
                      <UserPlus size={14} />
                    )}
                    Send Invite
                  </button>
                </form>
              )}
            </div>

            {/* Team Skills */}
            <div className="card">
              <div className="flex items-center justify-between mb-4">
                <h3 className="text-sm font-medium text-cs-muted">Shared Skills</h3>
                <span className="text-xs text-cs-muted">
                  {teamSkills.length} skill{teamSkills.length !== 1 ? 's' : ''}
                </span>
              </div>

              {teamSkills.length === 0 ? (
                <div className="text-center py-8">
                  <Sparkles size={32} className="mx-auto mb-2 text-cs-muted opacity-50" />
                  <p className="text-sm text-cs-muted">No skills shared yet</p>
                  <p className="text-xs text-cs-muted mt-1">
                    Share skills from your library to collaborate
                  </p>
                </div>
              ) : (
                <div className="space-y-2">
                  {teamSkills.map((skill) => (
                    <div
                      key={skill.id}
                      className="flex items-center justify-between p-3 bg-cs-bg rounded-lg"
                    >
                      <div className="flex items-center gap-3">
                        <div className="w-8 h-8 rounded-lg bg-cs-accent/20 flex items-center justify-center">
                          <Sparkles size={14} className="text-cs-accent" />
                        </div>
                        <div>
                          <p className="text-sm font-medium">{skill.name}</p>
                          <p className="text-xs text-cs-muted">
                            {skill.token_count} tokens · v{skill.version}
                          </p>
                        </div>
                      </div>
                      {(isAdmin || skill.shared_by === user?.id) && (
                        <button
                          onClick={() => deleteSkillMutation.mutate(skill.id)}
                          className="p-1 text-cs-muted hover:text-red-400 transition-colors"
                        >
                          <Trash2 size={14} />
                        </button>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    );
  }

  // Teams list view
  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold flex items-center gap-2">
            <Users className="text-cs-accent" size={24} />
            Team Workspaces
          </h2>
          <p className="text-sm text-cs-muted mt-1">
            Collaborate with your team on shared skills
          </p>
        </div>
        <button
          onClick={() => setView('create')}
          className="flex items-center gap-2 px-4 py-2 bg-cs-accent text-cs-bg rounded-lg text-sm font-medium hover:bg-cs-accent/90 transition-colors"
        >
          <Plus size={16} />
          New Team
        </button>
      </div>

      {/* Teams Grid */}
      {teamsLoading ? (
        <div className="flex items-center justify-center py-12">
          <Loader2 size={32} className="animate-spin text-cs-accent" />
        </div>
      ) : teams.length === 0 ? (
        <div className="card text-center py-12">
          <Users size={48} className="mx-auto mb-4 text-cs-muted opacity-50" />
          <h3 className="text-lg font-medium mb-2">No teams yet</h3>
          <p className="text-sm text-cs-muted mb-4">
            Create a team to start collaborating with others
          </p>
          <button
            onClick={() => setView('create')}
            className="inline-flex items-center gap-2 px-4 py-2 bg-cs-accent text-cs-bg rounded-lg font-medium hover:bg-cs-accent/90 transition-colors"
          >
            <Plus size={18} />
            Create Your First Team
          </button>
        </div>
      ) : (
        <div className="grid grid-cols-2 gap-4">
          {teams.map((team) => (
            <button
              key={team.id}
              onClick={() => selectTeam(team.id)}
              className="card text-left hover:border-cs-accent/50 transition-colors"
            >
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <div className="w-12 h-12 rounded-lg bg-cs-accent/20 flex items-center justify-center">
                    <Users size={24} className="text-cs-accent" />
                  </div>
                  <div>
                    <h3 className="font-medium">{team.name}</h3>
                    <p className="text-xs text-cs-muted flex items-center gap-2">
                      {getRoleIcon(team.role || 'member')}
                      <span>{team.role}</span>
                      <span>·</span>
                      <span>{team.member_count} member{team.member_count !== 1 ? 's' : ''}</span>
                    </p>
                  </div>
                </div>
                <ChevronRight size={20} className="text-cs-muted" />
              </div>
              {team.description && (
                <p className="text-sm text-cs-muted mt-3 line-clamp-2">
                  {team.description}
                </p>
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
