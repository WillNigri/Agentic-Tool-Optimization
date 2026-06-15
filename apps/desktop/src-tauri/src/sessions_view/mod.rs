// v2.3.42 — Sessions view for the GUI.
//
// Phase 6 sessions (Slice A + A.2 + B) are CLI-first today: they live
// in the `sessions` + `session_turns` tables and the CLI exposes
// `ato sessions ...` and `ato bridge`. The desktop GUI never had a
// first-class surface for them — they only appeared incidentally
// under Execution Logs after v2.3.41's grouping.
//
// This module adds two Tauri commands:
//   - list_sessions_full   — overview rows for the Sessions tab list
//   - get_session_transcript — every turn for one session, ordered
//
// Both are read-only. Continuing a session from the GUI uses the
// existing prompt_agent flow with an extra session_id parameter
// (wired separately so this module stays narrowly scoped to the
// view layer).

// Shared structs only — read paths import what they need from `read.rs`,
// write paths from `write.rs`. The Mutex+HashMap+Serialize trio below
// support `CloseInflight` + the three session-shape structs.
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;

/// v2.6 Slice C — tracks the PIDs of in-flight `ato sessions close`
/// subprocesses so the frontend's Cancel button can interrupt them.
/// Keyed by session_id because a user can only close one session at
/// a time per session (a second close on the same session is refused
/// by the CLI's idempotency guard anyway). The Child is dropped after
/// wait_with_output returns; the PID entry is removed in the same
/// scope. SIGTERM lets the subprocess unwind cleanly — reqwest
/// drops the in-flight HTTP request, the UPDATE never runs, and the
/// session stays 'open'.
pub struct CloseInflight(pub Mutex<HashMap<String, u32>>);

/// v2.7.14 — Map keys are namespaced by conversation kind so two
/// types that happen to share an ID (impossible today since each
/// table mints its own UUID, but defense-in-depth) can't collide on
/// the same map entry. Format: `"<kind>/<id>"` where kind is
/// `"session"`, `"war_room"`, or `"chat"`.
pub fn inflight_key(kind: &str, id: &str) -> String {
    format!("{}/{}", kind, id)
}

/// Kinds that participate in the close-inflight map. Iterated by
/// `cancel_close_session` to find a running close for a given id
/// regardless of conversation type (the frontend's Cancel button
/// passes only the id, not the kind).
pub const INFLIGHT_KINDS: &[&str] = &["session", "war_room", "chat"];

impl CloseInflight {
    pub fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionListRow {
    pub id: String,
    pub runtime: String,
    pub agent_slug: Option<String>,
    pub title: Option<String>,
    pub created_at: String,
    pub last_used_at: String,
    pub turn_count: i64,
    /// Distinct runtimes that appear in this session's turns. For a
    /// claude-only session this is `["claude"]`. For a Slice B
    /// cross-runtime conversation it's e.g. `["claude", "minimax"]`.
    /// Drives the runtime badges in the list UI.
    pub runtimes_used: Vec<String>,
    /// 2026-05-16 — distinct agent slugs that appear on the assistant
    /// turns of this session. Empty when every turn was a generalist
    /// dispatch (no `--agent` flag). For a war-room session it's e.g.
    /// `["positioning", "devex", "ceo", "designer", "office-hours"]`.
    /// Drives the persona-badge cluster on the SessionsList card.
    pub agents_used: Vec<String>,
    /// 2026-05-16 — session-total cost in USD, summed across every
    /// successful execution_logs row tied to this session_id. Renders
    /// as a small "$0.0644" pill on the card next to the turn count so
    /// users can scan cost per session without opening it. NULL when
    /// no execution_logs rows reference the session (older sessions
    /// pre-session_id-on-execution-logs).
    pub total_cost_usd: Option<f64>,
    /// Last (assistant) turn's text, truncated. Gives the user a
    /// "what was this conversation about" preview without expanding.
    pub last_assistant_preview: Option<String>,
    // v2.6 Slice C — lifecycle + coordinator-generated metadata.
    // `status` is "open" or "closed". `auto_title` is preferred over
    // the user-supplied `title` in the list when present (it's the
    // coordinator's distilled label). `summary`, `tags`, and
    // `project_id` are populated on close and refreshed on each
    // subsequent close after a reopen.
    pub status: String,
    pub closed_at: Option<String>,
    pub auto_title: Option<String>,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub project_id: Option<String>,
    /// PR 15 (2026-05-18) — human-readable project name resolved at
    /// query time via LEFT JOIN against the projects table. The
    /// frontend prefers this for display; project_id stays the
    /// canonical identifier. NULL when project_id is NULL or when
    /// the join doesn't find a row (project deleted but session
    /// retains the snapshot id).
    pub project_name: Option<String>,
    /// 2026-05-17 — Sessions UX polish PR 2 + 4. Controlled-vocab tag
    /// for the work band (Business / Marketing / Dev / Frontend / etc.)
    /// + free-form team label. NULL on pre-PR-2 rows; populated by the
    /// coordinator at close in PR 3.
    pub category: Option<String>,
    pub team: Option<String>,
    /// v2.7.13 — the LLM runtime that summarized this conversation
    /// on its most recent close. NULL when never closed. Drives the
    /// COORD badge on the list card so a glance shows which model
    /// produced the auto-title / summary. Sessions populate from
    /// sessions.coordinator_runtime (added in v2.7.12); war rooms +
    /// chats populate from their own lifecycle rows (v2.7.13).
    pub coordinator_runtime: Option<String>,
    /// v2.7.14 — the stable anchor runtime for chats (the WhatsApp-row
    /// "this chat is with claude" identity). Distinct from `runtime`
    /// which CAN flip to the latest assistant message's runtime as a
    /// fallback. NULL for sessions / war-rooms / single-runs (the
    /// anchor concept only applies to chat threads today). Lets the
    /// UI render a dedicated anchor badge that doesn't flicker when a
    /// thread hops runtimes mid-conversation.
    pub anchor_runtime: Option<String>,
    /// v2.7.13 — the human's free-form note attached at close time.
    /// Exposed here so list cards (which today don't drill into the
    /// detail view) can still surface it. NULL when no comment.
    pub human_comment: Option<String>,
    /// 2026-05-17 — Sessions UX polish PR 5a. Discriminator between
    /// real sessions (multi-turn, from the `sessions` table) and
    /// "single_run" single-shot dispatches (one row in `execution_logs`
    /// with `session_id IS NULL`). The History tab today shows the
    /// latter as a flat list; PR 5 collapses both into one Sessions
    /// feed (WhatsApp-style — group chats and single chats in one
    /// inbox). The frontend uses this discriminator to pick the card
    /// variant + the click-into-detail route (full transcript for
    /// `"session"`, single-turn detail for `"single_run"`). Codex
    /// Round-1 #2: bool would be too weak for routing/caching — a
    /// typed string keeps future variants open ("scheduled-run",
    /// "automation-step", etc.) without another migration.
    pub row_kind: String,
    /// Attribution PR (2026-06-13) — initiator provenance surfaced so
    /// the SessionsList card can render an InitiatorBadge. NULL on rows
    /// dispatched before the attribution mission backfilled the columns.
    pub initiator_kind: Option<String>,
    pub client_surface: Option<String>,
    pub initiator_id: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionTurn {
    pub turn_index: i64,
    pub role: String,
    pub text: String,
    pub runtime: String,
    pub created_at: String,
    /// 2026-05-16 — agent slug captured when the dispatching turn was
    /// fired with `--agent <slug>`. NULL means a generalist dispatch
    /// (raw model priors, no persona overlay). Drives the persona role
    /// label in the chat-bubble UI.
    pub agent_slug: Option<String>,
    /// Attribution PR (2026-06-13) — per-turn initiator provenance so
    /// each chat bubble can render an InitiatorBadge. NULL on turns
    /// written before the attribution mission backfilled the columns.
    pub initiator_kind: Option<String>,
    pub client_surface: Option<String>,
    pub initiator_id: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionTranscript {
    pub id: String,
    pub runtime: String,
    pub agent_slug: Option<String>,
    pub title: Option<String>,
    pub turns: Vec<SessionTurn>,
    // v2.6 Slice C — coordinator metadata, same fields as the list row.
    pub status: String,
    pub closed_at: Option<String>,
    pub auto_title: Option<String>,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub project_id: Option<String>,
    /// v2.7.12 — free-form note the human attached at close time.
    /// Renders in the closed-session summary card alongside the
    /// coordinator's auto-generated summary. NULL when no comment
    /// was added (or the session pre-dates the column).
    pub human_comment: Option<String>,
    /// Attribution PR (2026-06-13) — initiator provenance for the
    /// session header badge. NULL on sessions created before the
    /// attribution mission backfilled the columns.
    pub initiator_kind: Option<String>,
    pub client_surface: Option<String>,
    pub initiator_id: Option<String>,
}

// 2026-05-19 elegance war-room split (was 1635-line sessions_view.rs).
// Read paths in `read.rs`, write/CLI-spawn paths in `write.rs`.
// External callers (lib.rs + remote_runtimes_view.rs comments +
// ratchet_view.rs comments) reach commands via `sessions_view::<name>`
// — the re-exports below keep that path unchanged.

pub mod read;
pub mod write;

// Wildcard re-export so Tauri's macro-generated `__cmd__<name>`
// siblings come along — `generate_handler!` in lib.rs resolves
// `sessions_view::<name>` and needs both the function AND its
// generated companion in this namespace.
pub use read::*;
pub use write::*;
