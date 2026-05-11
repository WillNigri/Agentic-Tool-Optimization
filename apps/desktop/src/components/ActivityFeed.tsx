import { useEffect, useState, useCallback } from "react";
import { Loader2, Send, Check, X, Bot, User, Bell, MessageSquare } from "lucide-react";
import { cn } from "@/lib/utils";
import {
  listPosts,
  createPost,
  decidePost,
  type Post,
  type PostKind,
} from "@/lib/activityPosts";

// v2.3.20 Phase 5.5 — Activity feed GUI.
//
// One pane, three concerns:
//   1. Feed list (newest first), polling every 1s
//   2. Compose form (post a message as "human")
//   3. Inline approve/deny buttons on ApprovalRequest cards
//
// Live tail via Tauri event subscription is deferred to 5.6;
// polling is simple and the load is trivial (a SELECT every 1s).

const POLL_MS = 1000;

const KIND_FILTERS: { id: PostKind | "all"; label: string }[] = [
  { id: "all", label: "All" },
  { id: "message", label: "Messages" },
  { id: "event_notice", label: "Events" },
  { id: "approval_request", label: "Approvals" },
  { id: "approval_decision", label: "Decisions" },
];

export default function ActivityFeed() {
  const [posts, setPosts] = useState<Post[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<PostKind | "all">("all");
  const [composeText, setComposeText] = useState("");
  const [composing, setComposing] = useState(false);
  const [composeError, setComposeError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const kind = filter === "all" ? undefined : filter;
      const rows = await listPosts(100, kind);
      setPosts(rows);
      setError(null);
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setLoading(false);
    }
  }, [filter]);

  useEffect(() => {
    void refresh();
    const id = window.setInterval(refresh, POLL_MS);
    return () => window.clearInterval(id);
  }, [refresh]);

  async function handleSubmit() {
    const text = composeText.trim();
    if (!text) return;
    setComposing(true);
    setComposeError(null);
    try {
      await createPost(text, "human");
      setComposeText("");
      void refresh();
    } catch (e) {
      setComposeError(typeof e === "string" ? e : String(e));
    } finally {
      setComposing(false);
    }
  }

  async function handleDecide(requestId: string, approved: boolean) {
    try {
      await decidePost(requestId, approved);
      void refresh();
    } catch (e) {
      // Surface via the global error banner — same as compose errors.
      setError(typeof e === "string" ? e : String(e));
    }
  }

  return (
    <div className="flex flex-col h-full max-w-3xl">
      <div className="flex items-baseline justify-between mb-4">
        <div>
          <h2 className="text-xl font-semibold">Activity feed</h2>
          <p className="text-sm text-cs-muted">
            Shared stream of human + agent + system posts.
            Approvals get inline buttons.
          </p>
        </div>
      </div>

      {/* Filter strip */}
      <div className="flex gap-2 mb-3 text-xs">
        {KIND_FILTERS.map((f) => (
          <button
            key={f.id}
            onClick={() => setFilter(f.id)}
            className={cn(
              "px-3 py-1 rounded-md border transition-colors",
              filter === f.id
                ? "bg-cs-accent/20 border-cs-accent text-cs-text"
                : "border-cs-border text-cs-muted hover:border-cs-accent/50"
            )}
          >
            {f.label}
          </button>
        ))}
      </div>

      {/* Compose */}
      <div className="border border-cs-border rounded-md p-3 mb-4 bg-cs-card">
        <textarea
          value={composeText}
          onChange={(e) => setComposeText(e.target.value)}
          placeholder="Write a message to the feed…"
          rows={2}
          maxLength={4096}
          className="w-full bg-transparent text-sm resize-none focus:outline-none"
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              void handleSubmit();
            }
          }}
        />
        <div className="flex items-center justify-between mt-2 text-xs text-cs-muted">
          <span>{composeText.length}/4096 — ⌘+Enter to post</span>
          <button
            onClick={handleSubmit}
            disabled={composing || composeText.trim().length === 0}
            className={cn(
              "flex items-center gap-1 px-3 py-1 rounded-md border transition-colors",
              "border-cs-accent text-cs-accent hover:bg-cs-accent/10",
              "disabled:opacity-50 disabled:cursor-not-allowed"
            )}
          >
            {composing ? (
              <Loader2 size={14} className="animate-spin" />
            ) : (
              <Send size={14} />
            )}
            Post
          </button>
        </div>
        {composeError ? (
          <p className="text-xs text-red-400 mt-2">{composeError}</p>
        ) : null}
      </div>

      {/* Error banner */}
      {error ? (
        <div className="mb-3 p-2 text-xs rounded-md border border-red-500/40 bg-red-500/10 text-red-300">
          {error}
        </div>
      ) : null}

      {/* Feed */}
      <div className="flex-1 overflow-y-auto space-y-2">
        {loading ? (
          <div className="flex items-center justify-center py-8 text-cs-muted">
            <Loader2 size={20} className="animate-spin" />
          </div>
        ) : posts.length === 0 ? (
          <p className="text-sm text-cs-muted text-center py-8">
            No activity yet. Post something or fire a recipe with the
            <code className="px-1">notify_human</code> action.
          </p>
        ) : (
          posts.map((p) => (
            <PostCard
              key={p.id}
              post={p}
              onDecide={handleDecide}
            />
          ))
        )}
      </div>
    </div>
  );
}

function PostCard({
  post,
  onDecide,
}: {
  post: Post;
  onDecide: (requestId: string, approved: boolean) => void;
}) {
  const isApprovalRequest = post.kind === "approval_request";
  const isApprovalDecision = post.kind === "approval_decision";
  const Icon =
    post.author_kind === "agent"
      ? Bot
      : post.author_kind === "human"
      ? User
      : isApprovalRequest
      ? Bell
      : MessageSquare;

  const authorLabel =
    post.author_slug != null
      ? `${post.author_kind} @${post.author_slug}`
      : post.author_kind;

  return (
    <div
      className={cn(
        "border rounded-md p-3 bg-cs-card",
        isApprovalRequest
          ? "border-yellow-500/50 bg-yellow-500/5"
          : isApprovalDecision
          ? "border-cs-accent/30"
          : "border-cs-border"
      )}
    >
      <div className="flex items-start gap-2">
        <Icon
          size={16}
          className={cn(
            "shrink-0 mt-0.5",
            isApprovalRequest ? "text-yellow-400" : "text-cs-muted"
          )}
        />
        <div className="flex-1 min-w-0">
          <div className="flex items-center justify-between text-xs text-cs-muted mb-1">
            <span>
              {authorLabel}
              {" · "}
              <span className="font-mono">{post.kind}</span>
            </span>
            <span title={post.created_at}>{formatTime(post.created_at)}</span>
          </div>
          <p className="text-sm whitespace-pre-wrap break-words">{post.text}</p>
          {isApprovalRequest ? (
            <div className="flex gap-2 mt-2">
              <button
                onClick={() => onDecide(post.id, true)}
                className="flex items-center gap-1 px-3 py-1 text-xs rounded-md border border-green-500/50 text-green-300 hover:bg-green-500/10"
              >
                <Check size={12} /> Approve
              </button>
              <button
                onClick={() => onDecide(post.id, false)}
                className="flex items-center gap-1 px-3 py-1 text-xs rounded-md border border-red-500/50 text-red-300 hover:bg-red-500/10"
              >
                <X size={12} /> Deny
              </button>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

function formatTime(rfc3339: string): string {
  try {
    const d = new Date(rfc3339);
    if (Number.isNaN(d.getTime())) return rfc3339;
    return d.toLocaleString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  } catch {
    return rfc3339;
  }
}
