import { invoke } from "@tauri-apps/api/core";

// v1.5.0 — Persistent chat threads.
// Threads are the workspace primitive: many can exist, each survives
// restart, and any single thread can hop runtimes/agents mid-flight.

export interface ChatThread {
  id: string;
  title: string;
  projectId: string | null;
  agentId: string | null;
  createdAt: string;
  lastMessageAt: string | null;
  messageCount: number;
  archived: boolean;
}

export type ChatRole = "user" | "assistant" | "system" | "attachment" | "error";

export interface ChatMessage {
  id: string;
  threadId: string;
  role: ChatRole;
  content: string;
  runtime: string | null;
  agentSlug: string | null;
  metadata: string | null;
  createdAt: string;
}

export async function listChatThreads(input?: {
  projectId?: string | null;
  limit?: number;
}): Promise<ChatThread[]> {
  return invoke<ChatThread[]>("list_chat_threads", {
    projectId: input?.projectId ?? null,
    limit: input?.limit ?? null,
  });
}

export async function createChatThread(input: {
  title: string;
  projectId?: string | null;
  agentId?: string | null;
}): Promise<ChatThread> {
  return invoke<ChatThread>("create_chat_thread", {
    title: input.title,
    projectId: input.projectId ?? null,
    agentId: input.agentId ?? null,
  });
}

export async function renameChatThread(id: string, title: string): Promise<void> {
  return invoke("rename_chat_thread", { id, title });
}

export async function deleteChatThread(id: string): Promise<void> {
  return invoke("delete_chat_thread", { id });
}

export async function setChatThreadAgent(id: string, agentId: string | null): Promise<void> {
  return invoke("set_chat_thread_agent", { id, agentId });
}

export async function getChatMessages(threadId: string): Promise<ChatMessage[]> {
  return invoke<ChatMessage[]>("get_chat_messages", { threadId });
}

export async function appendChatMessage(input: {
  threadId: string;
  role: ChatRole;
  content: string;
  runtime?: string | null;
  agentSlug?: string | null;
  metadata?: string | null;
}): Promise<ChatMessage> {
  return invoke<ChatMessage>("append_chat_message", {
    threadId: input.threadId,
    role: input.role,
    content: input.content,
    runtime: input.runtime ?? null,
    agentSlug: input.agentSlug ?? null,
    metadata: input.metadata ?? null,
  });
}

export async function deleteChatMessage(id: string): Promise<void> {
  return invoke("delete_chat_message", { id });
}

/** Auto-title from the user's first message: trim, take first ~60 chars,
 *  fall back to a date-stamped default when empty. */
export function defaultThreadTitle(firstUserContent: string): string {
  const cleaned = firstUserContent.trim().replace(/\s+/g, " ");
  if (!cleaned) {
    return `New conversation · ${new Date().toLocaleDateString()}`;
  }
  return cleaned.slice(0, 60);
}
