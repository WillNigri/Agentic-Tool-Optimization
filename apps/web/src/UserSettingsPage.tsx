import { useQuery } from "@tanstack/react-query";
import { User, Mail, Calendar, LogOut, Shield, ExternalLink } from "lucide-react";
import { getMe, signOut } from "./lib/api";

interface UserSettingsPageProps {
  onSignedOut: () => void;
}

export default function UserSettingsPage({ onSignedOut }: UserSettingsPageProps) {
  const me = useQuery({ queryKey: ["me"], queryFn: getMe });

  const handleSignOut = async () => {
    try {
      await signOut();
    } catch {
      // Even if the call fails (network / token), clear local state.
    }
    localStorage.removeItem("ato-auth");
    onSignedOut();
  };

  if (me.isLoading) {
    return <div className="p-8 text-[#8888a0] text-sm">Loading profile…</div>;
  }
  if (me.isError || !me.data) {
    return (
      <div className="p-8 text-red-400 text-sm">
        Couldn't load your profile. {me.error?.message}
      </div>
    );
  }

  const profile = me.data;
  const createdAt = new Date(profile.created_at).toLocaleDateString(undefined, {
    year: "numeric",
    month: "long",
    day: "numeric",
  });

  return (
    <div className="p-6 max-w-2xl mx-auto space-y-6">
      <div>
        <h1 className="text-xl font-semibold text-white">Account</h1>
        <p className="text-xs text-[#8888a0] mt-1">
          Your ATO cloud identity. LLM provider keys, runtimes, and skills live in
          the desktop app under Settings.
        </p>
      </div>

      {/* Profile card */}
      <section className="rounded-xl border border-[#2a2a3a] bg-[#0f0f17] p-5 space-y-4">
        <div className="flex items-center gap-2">
          <User className="w-4 h-4 text-[#00FFB2]" />
          <h2 className="text-sm font-semibold text-white">Profile</h2>
        </div>

        <div className="space-y-3">
          <ProfileRow
            icon={<User className="w-3.5 h-3.5" />}
            label="Name"
            value={profile.name || "—"}
          />
          <ProfileRow
            icon={<Mail className="w-3.5 h-3.5" />}
            label="Email"
            value={profile.email}
          />
          <ProfileRow
            icon={<Shield className="w-3.5 h-3.5" />}
            label="Plan"
            value={profile.plan}
            valueClassName="uppercase tracking-wide"
          />
          <ProfileRow
            icon={<Calendar className="w-3.5 h-3.5" />}
            label="Joined"
            value={createdAt}
          />
        </div>

        <p className="text-[11px] text-[#5a5a6e] pt-2 border-t border-[#2a2a3a]/60">
          Need to change your email, password, or delete your account? Email{" "}
          <a
            href="mailto:support@agentictool.ai"
            className="text-[#00FFB2] hover:underline"
          >
            support@agentictool.ai
          </a>{" "}
          — self-serve flows are on the v2.19 roadmap.
        </p>
      </section>

      {/* Sign out */}
      <section className="rounded-xl border border-[#2a2a3a] bg-[#0f0f17] p-5 space-y-3">
        <div className="flex items-center gap-2">
          <LogOut className="w-4 h-4 text-[#00FFB2]" />
          <h2 className="text-sm font-semibold text-white">Session</h2>
        </div>
        <p className="text-xs text-[#aaaab8]">
          Signs you out of this browser session. Your desktop app and other devices
          stay signed in.
        </p>
        <button
          onClick={handleSignOut}
          className="px-4 py-2 rounded-md bg-[#16161e] border border-[#2a2a3a] text-white text-sm hover:bg-[#1d1d28] transition-colors inline-flex items-center gap-2"
        >
          <LogOut className="w-3.5 h-3.5" /> Sign out
        </button>
      </section>

      {/* What's not here yet — set expectations honestly. */}
      <section className="rounded-xl border border-[#2a2a3a] bg-[#0f0f17]/50 p-5 space-y-2">
        <p className="text-xs uppercase tracking-wide text-[#8888a0]">
          On the desktop app, not here
        </p>
        <ul className="text-xs text-[#aaaab8] space-y-1 list-disc list-inside marker:text-[#5a5a6e]">
          <li>LLM API keys (Anthropic, OpenAI, Google) — stored in OS Keychain.</li>
          <li>Runtime detection (Claude CLI, Codex CLI, Gemini CLI).</li>
          <li>Skills + MCP installs (filesystem operations).</li>
          <li>End-to-end team key material (stays local to your machine).</li>
        </ul>
        <a
          href="https://github.com/WillNigri/Agentic-Tool-Optimization/releases/latest"
          target="_blank"
          rel="noreferrer"
          className="inline-flex items-center gap-1 text-xs text-[#00FFB2] hover:underline pt-1"
        >
          <ExternalLink className="w-3 h-3" /> Download the desktop app
        </a>
      </section>
    </div>
  );
}

function ProfileRow({
  icon,
  label,
  value,
  valueClassName = "",
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  valueClassName?: string;
}) {
  return (
    <div className="flex items-center justify-between gap-3 py-2 border-b border-[#2a2a3a]/40 last:border-b-0">
      <div className="flex items-center gap-2 text-xs text-[#8888a0]">
        {icon}
        {label}
      </div>
      <div className={`text-sm text-white ${valueClassName}`}>{value}</div>
    </div>
  );
}
