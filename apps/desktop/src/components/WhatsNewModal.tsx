// v2.18.0 — "What's new" first-launch-after-upgrade modal.
//
// Fires ONCE per version bump. Reads `localStorage.ato-last-seen-version`;
// if it differs from the current `APP_VERSION` constant below, the modal
// shows. Dismissing or clicking "Got it" stamps the current version
// into localStorage so the modal stays quiet until the next bump.
//
// Why not Tauri's window-open events? The modal needs the React state
// store for theme + Suspense for icons; mounting from <App /> at the
// banner-stack level is the same place the v2.17 TetherApprovalModal
// lives, so we follow that precedent.

import { useEffect, useState } from "react";
import { X, Sparkles, ExternalLink } from "lucide-react";

const APP_VERSION = "2.18.0";
const STORAGE_KEY = "ato-last-seen-version";

interface FeatureBullet {
  icon: string;
  title: string;
  body: string;
}

const FEATURES: FeatureBullet[] = [
  {
    icon: "🌐",
    title: "Team Workspaces on the web",
    body: "Sign in at agentictool.ai and browse every shared session, war-room, chat, loop, and mission with the same fidelity as the desktop. Mobile-responsive.",
  },
  {
    icon: "🔗",
    title: "Browser ⇄ Desktop tether",
    body: "Pair your browser to a running desktop with X25519 + AEAD. Fingerprint-verified handshake. The cloud never sees plaintext.",
  },
  {
    icon: "👥",
    title: "Team management from the web",
    body: 'Create teams, invite by email, change roles, delete (with name-typing confirm). New "Account" panel for profile + plan.',
  },
  {
    icon: "🧹",
    title: "ato war-rooms sweep",
    body: "Auto-closes idle war-rooms with a coordinator summary. Wire to cron and one-shot R1 fan-outs self-close — no more invisible war-rooms in the Sessions feed.",
  },
  {
    icon: "📝",
    title: "ato subagent log",
    body: "Claude Code's Agent (Task) tool dispatches now show up in execution_logs with canonical auth_mode + billing_surface vocab + git commit SHA per receipt.",
  },
  {
    icon: "🎨",
    title: "Sign-in + Onboarding redesign",
    body: "Minimal centered sign-in card. New 3-step setup guide that explains the .env / SDK / wrap-client flow for non-technical users.",
  },
];

export default function WhatsNewModal() {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const seen = localStorage.getItem(STORAGE_KEY);
    // First-time-ever users: don't pop the modal (they see fresh
    // Onboarding instead). Only fire on actual UPGRADE — seen value
    // exists and differs from the current.
    if (seen && seen !== APP_VERSION) {
      setOpen(true);
    } else if (!seen) {
      // Brand-new install — stamp the version so the next upgrade triggers.
      localStorage.setItem(STORAGE_KEY, APP_VERSION);
    }
  }, []);

  const dismiss = () => {
    localStorage.setItem(STORAGE_KEY, APP_VERSION);
    setOpen(false);
  };

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/70 px-4"
      role="dialog"
      aria-modal="true"
      aria-labelledby="whats-new-title"
      onKeyDown={(e) => {
        if (e.key === "Escape") dismiss();
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget) dismiss();
      }}
    >
      <div className="w-full max-w-lg bg-[#0f0f17] border border-[#2a2a3a] rounded-xl overflow-hidden flex flex-col max-h-[90vh]">
        {/* Header */}
        <div className="flex items-start justify-between gap-3 px-6 py-5 border-b border-[#2a2a3a]">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-[#00FFB2]/15 flex items-center justify-center shrink-0">
              <Sparkles className="w-5 h-5 text-[#00FFB2]" />
            </div>
            <div>
              <h2 id="whats-new-title" className="text-lg font-semibold text-white">
                What's new in v{APP_VERSION}
              </h2>
              <p className="text-xs text-[#8888a0] mt-0.5">
                The "every AI in one war room" release.
              </p>
            </div>
          </div>
          <button
            onClick={dismiss}
            className="p-1 rounded-md text-[#8888a0] hover:text-white hover:bg-[#16161e] transition-colors shrink-0"
            aria-label="Close"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Feature list */}
        <div className="flex-1 overflow-y-auto px-6 py-5 space-y-4">
          {FEATURES.map((f) => (
            <div key={f.title} className="flex gap-3">
              <div className="text-xl shrink-0 leading-tight pt-0.5">{f.icon}</div>
              <div>
                <p className="text-sm font-semibold text-white">{f.title}</p>
                <p className="text-xs text-[#aaaab8] mt-1 leading-relaxed">
                  {f.body}
                </p>
              </div>
            </div>
          ))}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between gap-3 px-6 py-4 border-t border-[#2a2a3a] bg-[#0a0a0f]">
          <a
            href="https://github.com/WillNigri/Agentic-Tool-Optimization/blob/main/CHANGELOG.md"
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-1 text-xs text-[#8888a0] hover:text-[#00FFB2] transition-colors"
          >
            Full changelog <ExternalLink className="w-3 h-3" />
          </a>
          <button
            onClick={dismiss}
            className="px-4 py-2 rounded-md bg-[#00FFB2] text-black text-sm font-semibold hover:bg-[#00FFB2]/90 transition-colors"
          >
            Got it
          </button>
        </div>
      </div>
    </div>
  );
}
