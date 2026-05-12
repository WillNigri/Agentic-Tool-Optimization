import { invoke } from "@tauri-apps/api/core";

// v2.3.20 Phase 5.5 — Activity feed (posts) frontend wrappers.
//
// Rust source of truth: apps/desktop/src-tauri/src/posts.rs +
// the posts_* Tauri commands in commands.rs.

export type PostAuthorKind = "human" | "agent" | "system";

export type PostKind =
  | "message"
  | "event_notice"
  | "approval_request"
  | "approval_decision";

export interface Post {
  id: string;
  /** RFC3339 timestamp. */
  created_at: string;
  author_kind: PostAuthorKind;
  /** Optional. For human posts often null; for agent / system, the slug. */
  author_slug: string | null;
  kind: PostKind;
  text: string;
  related_event_seq: number | null;
  payload: unknown | null;
}

export async function listPosts(
  limit: number = 50,
  kind?: PostKind
): Promise<Post[]> {
  return await invoke<Post[]>("posts_list", { limit, kind: kind ?? null });
}

export async function createPost(
  text: string,
  authorKind: PostAuthorKind = "human",
  authorSlug?: string,
  kind: PostKind = "message"
): Promise<Post> {
  return await invoke<Post>("posts_create", {
    text,
    authorKind,
    authorSlug: authorSlug ?? null,
    kind,
  });
}

export async function listPending(limit: number = 20): Promise<Post[]> {
  return await invoke<Post[]>("posts_pending", { limit });
}

export async function decidePost(
  requestId: string,
  approved: boolean,
  notes?: string
): Promise<Post> {
  return await invoke<Post>("posts_decide", {
    requestId,
    approved,
    notes: notes ?? null,
  });
}

// v2.3.24 Phase 5.6 — count-only fetch for the sidebar badge.
// Faster than listPending() because the SQL returns one row.
export async function pendingCount(): Promise<number> {
  return await invoke<number>("posts_pending_count");
}
