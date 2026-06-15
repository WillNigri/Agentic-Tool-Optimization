// Wave 2 — chat message stream renderer.
// Mirrors desktop ChatThreadDetailView's message list but read-only.
//
// Snapshot shape: snapshot.messages[] from teamShareSnapshot.buildChatSnapshot().
// Each message is a ChatMessage row (camelCase on wire):
//   id, role, content, runtime, agent_slug, metadata, created_at, initiator_*

import InitiatorBadge from './InitiatorBadge';
import {
  formatTime,
  runtimeChipClass,
  personaChipClass,
  personaDisplay,
  roleStyleClass,
} from './_helpers';

export interface SnapshotMessage {
  id?: string | null;
  role: string;
  content?: string | null;
  runtime?: string | null;
  agentSlug?: string | null;
  agent_slug?: string | null;
  metadata?: string | null;
  createdAt?: string | null;
  created_at?: string | null;
  initiatorKind?: string | null;
  initiator_kind?: string | null;
  clientSurface?: string | null;
  client_surface?: string | null;
  initiatorId?: string | null;
  initiator_id?: string | null;
}

interface Props {
  messages: SnapshotMessage[];
}

/**
 * Vertical message stream with role-based styling.
 * Mirrors ChatThreadDetailView's card-per-message layout.
 */
export default function ChatMessagesRenderer({ messages }: Props) {
  if (messages.length === 0) {
    return (
      <div className="rounded-lg border border-[#2a2a3a] bg-[#16161e] p-6 text-center text-[#8888a0] text-sm">
        No messages in this chat snapshot.
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {messages.map((m, i) => {
        const agentSlug = m.agentSlug ?? m.agent_slug ?? null;
        const createdAt = m.createdAt ?? m.created_at ?? null;
        const initiatorKind = m.initiatorKind ?? m.initiator_kind ?? null;
        const clientSurface = m.clientSurface ?? m.client_surface ?? null;
        const initiatorId = m.initiatorId ?? m.initiator_id ?? null;
        const content = m.content ?? '';

        return (
          <div
            key={m.id ?? i}
            className={`rounded-lg p-4 ${roleStyleClass(m.role)}`}
          >
            {/* Meta row */}
            <div className="flex items-center gap-2 flex-wrap mb-2">
              <span className="text-[10px] uppercase tracking-wider text-[#8888a0] font-medium">
                {m.role}
              </span>
              {m.runtime && (
                <span className={runtimeChipClass(m.runtime)}>{m.runtime}</span>
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
                <span className="ml-auto text-[11px] text-[#8888a0]">
                  {formatTime(createdAt)}
                </span>
              )}
            </div>

            {/* Body */}
            <pre className="text-xs text-[#e8e8f0] whitespace-pre-wrap break-words font-mono">
              {content || <span className="text-[#8888a0] italic">(empty)</span>}
            </pre>
          </div>
        );
      })}
    </div>
  );
}
