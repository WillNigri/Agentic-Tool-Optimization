// Wave 2 — session turn timeline renderer.
// Mirrors desktop SessionTranscriptView's turn-card stack but read-only:
// no send input, no close/reopen lifecycle, no streaming.
//
// Snapshot shape: snapshot.turns[] from teamShareSnapshot.buildSessionSnapshot().
// Each turn maps to the desktop's SessionTurn row (camelCase on wire):
//   role, text, runtime, agentSlug, createdAt, initiatorKind, clientSurface, initiatorId

import InitiatorBadge from './InitiatorBadge';
import {
  formatTime,
  runtimeChipClass,
  personaChipClass,
  personaDisplay,
} from './_helpers';

export interface SnapshotTurn {
  role: string;
  text?: string | null;
  // Both camelCase (Tauri wire) and snake_case (older snapshot writes) accepted.
  runtime?: string | null;
  agentSlug?: string | null;
  agent_slug?: string | null;
  createdAt?: string | null;
  created_at?: string | null;
  initiatorKind?: string | null;
  initiator_kind?: string | null;
  clientSurface?: string | null;
  client_surface?: string | null;
  initiatorId?: string | null;
  initiator_id?: string | null;
  // turn index when present (snapshot may include it)
  turnIndex?: number | null;
  turn_index?: number | null;
}

interface Props {
  turns: SnapshotTurn[];
}

/**
 * Vertical turn timeline — one card per turn, alternating user/assistant.
 * Matches the WhatsApp-style layout of the desktop SessionTranscriptView:
 * user turns right-aligned, assistant + coordinator left-aligned.
 */
export default function SessionTurnsRenderer({ turns }: Props) {
  if (turns.length === 0) {
    return (
      <div className="rounded-lg border border-[#2a2a3a] bg-[#16161e] p-6 text-center text-[#8888a0] text-sm">
        No turns in this session snapshot.
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {turns.map((turn, i) => {
        // Normalize camelCase / snake_case field variants.
        const runtime = turn.runtime ?? null;
        const agentSlug = turn.agentSlug ?? turn.agent_slug ?? null;
        const createdAt = turn.createdAt ?? turn.created_at ?? null;
        const initiatorKind = turn.initiatorKind ?? turn.initiator_kind ?? null;
        const clientSurface = turn.clientSurface ?? turn.client_surface ?? null;
        const initiatorId = turn.initiatorId ?? turn.initiator_id ?? null;
        const text = turn.text ?? '';
        const role = turn.role ?? 'unknown';
        const isAssistant = role === 'assistant';

        // Persona-aware speaker label.
        const personaLabel = agentSlug ? personaDisplay(agentSlug) : null;
        const speakerLabel = isAssistant
          ? personaLabel ?? (runtime ?? 'assistant')
          : 'You';

        // WhatsApp alignment: human turns right, everything else left.
        const isYou = role === 'user';

        // Avatar initials — at most 2 chars.
        const initials = speakerLabel
          .split(/\s+/)
          .slice(0, 2)
          .map((w) => w[0]?.toUpperCase() ?? '')
          .join('');

        // Avatar bg: orange for claude, green for codex, muted for user.
        const avatarBg = isAssistant
          ? runtime === 'claude'
            ? 'bg-orange-400/20 text-orange-400'
            : runtime === 'codex'
              ? 'bg-green-400/20 text-green-400'
              : runtime === 'gemini'
                ? 'bg-blue-400/20 text-blue-400'
                : 'bg-[#2a2a3a] text-[#8888a0]'
          : 'bg-[#2a2a3a] text-[#8888a0]';

        // Bubble border tint for assistant turns.
        const bubbleCls = isAssistant
          ? 'border border-[#2a2a3a]/60 bg-[#16161e]/60'
          : 'border border-[#2a2a3a] bg-[#16161e]';

        return (
          <div
            key={turn.turnIndex ?? turn.turn_index ?? i}
            className={`flex gap-3 ${isYou ? 'flex-row-reverse' : ''}`}
          >
            {/* Avatar */}
            <div
              className={`shrink-0 w-8 h-8 rounded-full flex items-center justify-center text-[10px] font-semibold ${avatarBg}`}
              title={speakerLabel}
            >
              {initials || '?'}
            </div>

            {/* Content */}
            <div className={`flex-1 min-w-0 ${isYou ? 'text-right' : ''}`}>
              {/* Meta row */}
              <div
                className={`flex items-center gap-2 mb-1 flex-wrap ${isYou ? 'justify-end' : ''}`}
              >
                <span
                  className={`text-xs font-medium ${
                    isAssistant ? 'text-[#e8e8f0]' : 'text-[#8888a0]'
                  }`}
                >
                  {speakerLabel}
                </span>
                {isAssistant && runtime && (
                  <span className={runtimeChipClass(runtime)}>{runtime}</span>
                )}
                {agentSlug && (
                  <span className={personaChipClass()}>
                    {personaDisplay(agentSlug)}
                  </span>
                )}
                {(initiatorKind || clientSurface) && (
                  <InitiatorBadge
                    initiatorKind={initiatorKind}
                    clientSurface={clientSurface}
                    initiatorId={initiatorId}
                  />
                )}
                {createdAt && (
                  <span className="text-[10px] text-[#8888a0]">
                    {formatTime(createdAt)}
                  </span>
                )}
              </div>

              {/* Bubble */}
              <pre
                className={`p-3 rounded-md text-sm whitespace-pre-wrap font-sans text-left ${bubbleCls}`}
              >
                {text || <span className="text-[#8888a0] italic">(empty)</span>}
              </pre>
            </div>
          </div>
        );
      })}
    </div>
  );
}
