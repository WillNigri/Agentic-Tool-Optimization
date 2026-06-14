import { invoke } from "@tauri-apps/api/core";

const MAX_SNAPSHOT_BYTES = 950_000;

type SnapshotResult<T extends Record<string, unknown>> = {
  snapshot: T & { truncated_at?: string };
  truncated_at?: string;
};

interface SessionTurnSnapshot {
  turnIndex: number;
  role: string;
  text: string;
  runtime: string;
  createdAt: string;
  agentSlug: string | null;
  initiatorKind?: string | null;
  clientSurface?: string | null;
  initiatorId?: string | null;
}

interface SessionTranscriptSnapshot {
  id: string;
  runtime: string;
  agentSlug: string | null;
  title: string | null;
  turns: SessionTurnSnapshot[];
  status: "open" | "closed";
  closedAt: string | null;
  autoTitle: string | null;
  summary: string | null;
  tags: string[];
  projectId: string | null;
  humanComment: string | null;
  initiatorKind?: string | null;
  clientSurface?: string | null;
  initiatorId?: string | null;
}

interface WarRoomSeatSnapshot {
  id: string;
  runtime: string;
  agentSlug: string | null;
  model: string | null;
  status: string;
  prompt: string | null;
  response: string | null;
  errorMessage: string | null;
  createdAt: string;
  durationMs: number | null;
  tokensIn: number | null;
  tokensOut: number | null;
  costUsdEstimated: number | null;
  authMode: string | null;
  warRoomRound: number | null;
  toolCallsSummary?: string | null;
  gitCommitSha?: string | null;
  initiatorKind?: string | null;
  clientSurface?: string | null;
  initiatorId?: string | null;
}

interface WarRoomSnapshot {
  id: string;
  status: "open" | "closed";
  closedAt: string | null;
  autoTitle: string | null;
  summary: string | null;
  coordinatorRuntime: string | null;
  humanComment: string | null;
  tags: string[];
  seatCount: number;
}

interface ChatMessageSnapshot {
  id: string;
  threadId: string;
  role: string;
  content: string;
  runtime: string | null;
  agentSlug: string | null;
  metadata: string | null;
  createdAt: string;
}

interface ChatThreadSnapshot {
  id: string;
  title: string;
  status: "open" | "closed";
  closedAt: string | null;
  autoTitle: string | null;
  summary: string | null;
  coordinatorRuntime: string | null;
  humanComment: string | null;
  tags: string[];
  messageCount: number;
  agentId?: string | null;
  agentSlug?: string | null;
}

export function byteLengthUtf8(value: string): number {
  // Browser-first: TextEncoder is available everywhere we run. Avoid
  // the Buffer.byteLength shortcut because @types/node isn't installed
  // in apps/desktop and TextEncoder is already exact for utf-8 (the
  // only encoding we care about for snapshot serialization).
  return new TextEncoder().encode(value).length;
}

function serializeSize(value: unknown): number {
  return byteLengthUtf8(JSON.stringify(value) ?? "null");
}

function withTruncation<T extends Record<string, unknown>>(
  snapshot: T,
  childKey: "turns" | "seats" | "messages",
  timestampKey: "createdAt",
): SnapshotResult<T> {
  const children = Array.isArray(snapshot[childKey]) ? [...(snapshot[childKey] as Array<Record<string, unknown>>)] : [];
  if (serializeSize(snapshot) <= MAX_SNAPSHOT_BYTES) {
    return { snapshot: snapshot as T & { truncated_at?: string } };
  }

  let truncatedAt: string | undefined;
  while (children.length > 0) {
    const removed = children.shift();
    if (!truncatedAt) {
      const value = removed?.[timestampKey];
      truncatedAt = typeof value === "string" ? value : undefined;
    }
    const candidate = { ...snapshot, [childKey]: children } as T & { truncated_at?: string };
    if (truncatedAt) {
      candidate.truncated_at = truncatedAt;
    }
    if (serializeSize(candidate) <= MAX_SNAPSHOT_BYTES) {
      return {
        snapshot: candidate,
        truncated_at: truncatedAt,
      };
    }
  }

  const candidate = { ...snapshot, [childKey]: [] } as T & { truncated_at?: string };
  if (truncatedAt) {
    candidate.truncated_at = truncatedAt;
  }
  return {
    snapshot: candidate,
    truncated_at: truncatedAt,
  };
}

export async function buildSessionSnapshot(
  sessionId: string,
): Promise<SnapshotResult<{
  kind: "session";
  id: string;
  runtime: string;
  agent_slug: string | null;
  title: string | null;
  status: "open" | "closed";
  closed_at: string | null;
  auto_title: string | null;
  summary: string | null;
  tags: string[];
  project_id: string | null;
  human_comment: string | null;
  initiator_kind?: string | null;
  client_surface?: string | null;
  initiator_id?: string | null;
  turns: Array<{
    turn_index: number;
    role: string;
    text: string;
    runtime: string;
    createdAt: string;
    agent_slug: string | null;
    initiator_kind?: string | null;
    client_surface?: string | null;
    initiator_id?: string | null;
  }>;
}>> {
  const transcript = await invoke<SessionTranscriptSnapshot>("get_session_transcript", {
    sessionId,
  });
  const snapshot = {
    kind: "session" as const,
    id: transcript.id,
    runtime: transcript.runtime,
    agent_slug: transcript.agentSlug,
    title: transcript.title,
    status: transcript.status,
    closed_at: transcript.closedAt,
    auto_title: transcript.autoTitle,
    summary: transcript.summary,
    tags: transcript.tags,
    project_id: transcript.projectId,
    human_comment: transcript.humanComment,
    initiator_kind: transcript.initiatorKind,
    client_surface: transcript.clientSurface,
    initiator_id: transcript.initiatorId,
    turns: transcript.turns.map((turn) => ({
      turn_index: turn.turnIndex,
      role: turn.role,
      text: turn.text,
      runtime: turn.runtime,
      createdAt: turn.createdAt,
      agent_slug: turn.agentSlug,
      initiator_kind: turn.initiatorKind,
      client_surface: turn.clientSurface,
      initiator_id: turn.initiatorId,
    })),
  };
  return withTruncation(snapshot, "turns", "createdAt");
}

export async function buildWarRoomSnapshot(
  warRoomId: string,
): Promise<SnapshotResult<{
  kind: "war_room";
  id: string;
  status: "open" | "closed";
  closed_at: string | null;
  auto_title: string | null;
  summary: string | null;
  coordinator_runtime: string | null;
  human_comment: string | null;
  tags: string[];
  seat_count: number;
  seats: Array<{
    id: string;
    runtime: string;
    agent_slug: string | null;
    model: string | null;
    status: string;
    prompt: string | null;
    response: string | null;
    error_message: string | null;
    createdAt: string;
    duration_ms: number | null;
    tokens_in: number | null;
    tokens_out: number | null;
    cost_usd_estimated: number | null;
    auth_mode: string | null;
    war_room_round: number | null;
    tool_calls_summary?: string | null;
    git_commit_sha?: string | null;
    initiator_kind?: string | null;
    client_surface?: string | null;
    initiator_id?: string | null;
  }>;
}>> {
  const [warRoom, seats] = await Promise.all([
    invoke<WarRoomSnapshot>("get_war_room", { warRoomId }),
    invoke<WarRoomSeatSnapshot[]>("get_war_room_constituents", { warRoomId }),
  ]);
  const snapshot = {
    kind: "war_room" as const,
    id: warRoom.id,
    status: warRoom.status,
    closed_at: warRoom.closedAt,
    auto_title: warRoom.autoTitle,
    summary: warRoom.summary,
    coordinator_runtime: warRoom.coordinatorRuntime,
    human_comment: warRoom.humanComment,
    tags: warRoom.tags,
    seat_count: warRoom.seatCount,
    seats: seats.map((seat) => ({
      id: seat.id,
      runtime: seat.runtime,
      agent_slug: seat.agentSlug,
      model: seat.model,
      status: seat.status,
      prompt: seat.prompt,
      response: seat.response,
      error_message: seat.errorMessage,
      createdAt: seat.createdAt,
      duration_ms: seat.durationMs,
      tokens_in: seat.tokensIn,
      tokens_out: seat.tokensOut,
      cost_usd_estimated: seat.costUsdEstimated,
      auth_mode: seat.authMode,
      war_room_round: seat.warRoomRound,
      tool_calls_summary: seat.toolCallsSummary,
      git_commit_sha: seat.gitCommitSha,
      initiator_kind: seat.initiatorKind,
      client_surface: seat.clientSurface,
      initiator_id: seat.initiatorId,
    })),
  };
  return withTruncation(snapshot, "seats", "createdAt");
}

export async function buildChatSnapshot(
  chatId: string,
): Promise<SnapshotResult<{
  kind: "chat";
  id: string;
  title: string;
  status: "open" | "closed";
  closed_at: string | null;
  auto_title: string | null;
  summary: string | null;
  coordinator_runtime: string | null;
  human_comment: string | null;
  tags: string[];
  message_count: number;
  agent_id?: string | null;
  agent_slug?: string | null;
  messages: Array<{
    id: string;
    thread_id: string;
    role: string;
    content: string;
    runtime: string | null;
    agent_slug: string | null;
    metadata: string | null;
    createdAt: string;
  }>;
}>> {
  const [thread, messages] = await Promise.all([
    invoke<ChatThreadSnapshot>("get_chat", { chatId }),
    invoke<ChatMessageSnapshot[]>("get_chat_messages", { threadId: chatId }),
  ]);
  const snapshot = {
    kind: "chat" as const,
    id: thread.id,
    title: thread.title,
    status: thread.status,
    closed_at: thread.closedAt,
    auto_title: thread.autoTitle,
    summary: thread.summary,
    coordinator_runtime: thread.coordinatorRuntime,
    human_comment: thread.humanComment,
    tags: thread.tags,
    message_count: thread.messageCount,
    agent_id: thread.agentId,
    agent_slug: thread.agentSlug,
    messages: messages.map((message) => ({
      id: message.id,
      thread_id: message.threadId,
      role: message.role,
      content: message.content,
      runtime: message.runtime,
      agent_slug: message.agentSlug,
      metadata: message.metadata,
      createdAt: message.createdAt,
    })),
  };
  return withTruncation(snapshot, "messages", "createdAt");
}
