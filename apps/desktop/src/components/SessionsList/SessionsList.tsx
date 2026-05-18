// v2.3.42 — Sessions tab in Runs.
//
// First-class GUI surface for Phase 6 sessions: list every conversation
// in the local DB, click to open a chat-style transcript, see which
// runtimes participated. Sessions were CLI-only until now (Slice A
// + A.2 + B in v2.3.31–33); v2.3.41 added incidental grouping in
// Execution Logs but didn't make sessions browsable on their own.
//
// Pure read view for v1 — opening a chat input for continue/bridge
// from the GUI is the next slice (involves wiring prompt_agent
// with --session). Document linked in the empty state directs the
// user to the CLI as the interim path.

import { useState, useEffect, useRef } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  MessagesSquare,
  ArrowLeft,
  Bot,
  User as UserIcon,
  Loader2,
  Sparkles,
  Plus,
  Send,
  GitBranch,
  X,
  Lock,
  Unlock,
  Tag,
  Search,
} from "lucide-react";
import { cn } from "@/lib/utils";
import SingleRunDetailView from "./SingleRunDetailView";
import WarRoomDetailView from "./WarRoomDetailView";
import ChatThreadDetailView from "./ChatThreadDetailView";
import SessionTranscriptView from "./SessionTranscriptView";
import NewSessionModal from "./NewSessionModal";
import { useProjectStore } from "@/stores/useProjectStore";
import { useUiStore } from "@/stores/useUiStore";
import {
  runtimeBadge,
  formatTime,
  personaDisplay,
  personaBadge,
  runtimeDisplay,
  RUNTIME_COLORS,
} from "./_helpers";

interface SessionListRow {
  id: string;
  runtime: string;
  agentSlug: string | null;
  title: string | null;
  createdAt: string;
  lastUsedAt: string;
  turnCount: number;
  runtimesUsed: string[];
  // 2026-05-16 — distinct agent slugs that appeared on assistant turns
  // in this session, in first-spoken order. Empty when every dispatch
  // was a generalist (no --agent). Drives the persona-badge cluster
  // on the SessionsList card.
  agentsUsed: string[];
  // 2026-05-16 — session-total cost USD. NULL when no execution_logs
  // rows reference this session (pre-session-id-on-logs sessions). 0.0
  // when there are rows but all were on subscription (no metered cost).
  totalCostUsd: number | null;
  lastAssistantPreview: string | null;
  // v2.6 Slice C — lifecycle + coordinator-generated metadata.
  // PR 5a (2026-05-17) broadened the `status` literal from "open" |
  // "closed" to a plain string so single-run rows (which carry the
  // execution_log status like "success" / "error") flow through the
  // same shape. UI code that checks lifecycle still tests for "open"
  // / "closed" explicitly; everything else falls through to the
  // single-run card variant.
  status: string;
  closedAt: string | null;
  autoTitle: string | null;
  summary: string | null;
  tags: string[];
  projectId: string | null;
  // PR 15 (2026-05-18) — human-readable project name resolved by
  // the Tauri LEFT JOIN against the projects table. Prefer this
  // for display; projectId stays the canonical identifier. NULL
  // when projectId is NULL or when the join doesn't find a row
  // (project deleted, session retains the snapshot id).
  projectName: string | null;
  // 2026-05-17 — Sessions UX polish PR 2 + 4. category is a
  // controlled-vocab work-band tag (Business / Marketing / Dev /
  // Frontend / etc.); team is a free-form owner label. Both are
  // populated by the coordinator at close (PR 3); NULL on
  // pre-PR-2 rows AND on single-run rows (taxonomy is a session-only
  // concern — a single dispatch isn't worth taxonomizing).
  category: string | null;
  team: string | null;
  // 2026-05-17 — Sessions UX polish PR 5a. Discriminator between
  // real multi-turn sessions (from the `sessions` table) and
  // single-run dispatches (from `execution_logs` with
  // session_id IS NULL — what the History tab was the only surface
  // for before PR 5 collapsed them into one unified feed). The
  // frontend uses this to pick the card variant + the
  // click-into-detail route in PR 5b/5c.
  // 2026-05-18 — Path A consolidation: chat threads from the
  // bottom-pane Chat tab land in this feed as a fourth kind so the
  // Sessions tab is one inbox for every conversation type. The bottom
  // pane keeps writing to its own `chat_threads` table for now;
  // sessions_view's list_sessions_inner UNIONs the two on read.
  rowKind: "session" | "single_run" | "war_room" | "chat";
}

// 2026-05-18 — elegance push #2. Shared types + helpers
// (SessionTurn / SessionCostBreakdown / SessionTranscript /
// CloseSessionResult / RUNTIME_DISPLAY / runtimeDisplay /
// inferCoordinatorTarget / avatarInitials / NEW_SESSION_RUNTIMES)
// moved to `_helpers.ts` so SessionTranscriptView + NewSessionModal
// (now in their own files) can import them without re-declaring.
// SessionsList itself only still needs runtimeDisplay (used by the
// chat + single-run card variants for the "runtime:" meta line).

type StatusFilter = "all" | "open" | "closed";

// PR 6 (2026-05-17) — category filter vocab. Mirrors
// `ALLOWED_CATEGORIES` in apps/cli/src/commands/sessions.rs and the
// SQL CHECK in apps/desktop/src-tauri/src/lib.rs. The drift-killer
// unit test on the Rust side catches CHECK ↔ vocab mismatches at
// build time; this UI list is a cosmetic affordance — if the
// backend ever adds an 11th category before this list catches up,
// the existing rows just won't appear in the dropdown until it
// does. Capitalized exactly as stored so the equality check below
// works without normalization.
const CATEGORY_VOCAB = [
  "Business",
  "Marketing",
  "Dev",
  "Frontend",
  "Backend",
  "Design",
  "Security",
  "Compliance",
  "Ops",
  "Other",
] as const;

// PR 5b (2026-05-17) — kind filter: ALL shows both real sessions and
// single-run dispatches; SESSIONS shows only multi-turn
// rooms from the `sessions` table; SINGLE_RUNS shows only standalone
// `execution_logs` rows (what the History tab used to be the only
// surface for). Keeps the status filter (open/closed) orthogonal —
// status applies to sessions; single-runs carry execution_log status
// values (success/error/unknown) so they pass through the "all"
// status bucket only. PR 5c dropped the History tab and added the
// single-run click-into-detail panel — see SingleRunDetailView.tsx.
type KindFilter = "all" | "sessions" | "single_runs" | "war_rooms" | "chats";

/// Case-insensitive substring search across every human-readable
/// field on a session row plus the 8-char id prefix. Returns the
/// filtered list; tokens that look like a single word are matched
/// individually so users can type "review consensus" and find rows
/// where both words appear somewhere.
// PR 6 — single source of truth for the row predicate. The main
// filter chain and the per-dropdown count chains all run through
// this, optionally with one field skipped (so the dropdowns show
// "what selecting this option would yield given every OTHER active
// filter"). Codex Round-1 #3: extracting this prevents the three
// places from drifting when the next filter lands.
interface FilterFields {
  status: StatusFilter;
  kind: KindFilter;
  category: string | null;
  team: string | null;
  tag: string | null;
}
function rowMatchesFilters(
  s: SessionListRow,
  f: FilterFields,
  skip?: keyof FilterFields,
): boolean {
  if (skip !== "kind") {
    if (f.kind === "sessions" && s.rowKind !== "session") return false;
    if (f.kind === "single_runs" && s.rowKind !== "single_run") return false;
    if (f.kind === "war_rooms" && s.rowKind !== "war_room") return false;
    if (f.kind === "chats" && s.rowKind !== "chat") return false;
  }
  if (skip !== "status") {
    if (f.status === "open" && s.status !== "open") return false;
    if (f.status === "closed" && s.status !== "closed") return false;
  }
  // PR 6 — taxonomy filters. Category + team only exist on real
  // sessions (PR 3 closure path); a category or team filter therefore
  // implicitly scopes to sessions, dropping single-runs.
  if (skip !== "category" && f.category !== null && s.category !== f.category)
    return false;
  if (skip !== "team" && f.team !== null && s.team !== f.team) return false;
  // Tag filter: row matches when it carries the exact tag string.
  // Tags are sanitized to kebab-case at close time, so equality is
  // the right comparison (no case folding needed).
  if (skip !== "tag" && f.tag !== null && !s.tags.includes(f.tag)) return false;
  return true;
}

function filterSessions(
  sessions: SessionListRow[],
  query: string,
  status: StatusFilter,
  kind: KindFilter,
  category: string | null,
  team: string | null,
  tag: string | null,
): SessionListRow[] {
  const trimmed = query.trim().toLowerCase();
  const tokens = trimmed.length === 0 ? [] : trimmed.split(/\s+/);
  const f: FilterFields = { status, kind, category, team, tag };
  return sessions.filter((s) => {
    if (!rowMatchesFilters(s, f)) return false;
    if (tokens.length === 0) return true;
    // Build a single haystack string per row so each token can run a
    // cheap String.includes. Avoids re-allocating arrays inside the
    // inner loop. Limited to the fields a human would actually type.
    const haystack = [
      s.autoTitle ?? "",
      s.title ?? "",
      s.summary ?? "",
      s.lastAssistantPreview ?? "",
      s.agentSlug ?? "",
      s.tags.join(" "),
      s.runtimesUsed.join(" "),
      s.runtime,
      s.id.slice(0, 8),
    ]
      .join(" ")
      .toLowerCase();
    return tokens.every((t) => haystack.includes(t));
  });
}

// PR 5c — what's currently open in the detail view. Encoding the
// kind alongside the id keeps the render branch unambiguous: a
// session uuid and an execution_log uuid live in the same string
// space, so the discriminator is required for routing. `null`
// means the list is showing.
type OpenSelection =
  | { kind: "session"; id: string }
  | { kind: "single_run"; id: string }
  | { kind: "war_room"; id: string }
  | { kind: "chat"; id: string }
  | null;

export default function SessionsList() {
  const [openSelection, setOpenSelection] = useState<OpenSelection>(null);
  const consumePendingOpenSession = useUiStore(
    (s) => s.consumePendingOpenSession
  );
  const consumePendingOpenNewSession = useUiStore(
    (s) => s.consumePendingOpenNewSession
  );
  const [showNew, setShowNew] = useState(false);
  // PR-C First-Chat Wizard (2026-05-18) — drain any pending detail
  // request on mount. The wizard sets this before navigating to the
  // Sessions tab so the user lands directly on the new war-room
  // instead of the list. Consumed once so a later navigation doesn't
  // re-open a stale id.
  //
  // Path B (2026-05-18) — also drain the "open New session modal"
  // flag set by the bottom-pane multi-launcher. Same one-shot pattern:
  // consume on mount, modal opens, future navigations don't re-trigger.
  useEffect(() => {
    const pending = consumePendingOpenSession();
    if (pending.id && pending.kind) {
      setOpenSelection({ kind: pending.kind, id: pending.id });
    }
    if (consumePendingOpenNewSession()) {
      setShowNew(true);
    }
  }, [consumePendingOpenSession, consumePendingOpenNewSession]);
  const [searchQuery, setSearchQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [kindFilter, setKindFilter] = useState<KindFilter>("all");
  // PR 6 — taxonomy filters. null means "no scoping"; a non-null
  // string filters to rows that exactly match. Category is gated by
  // the controlled vocab (PR 3); team is free-form so the dropdown
  // options are derived from distinct values in the loaded data.
  // Tag filter is set by clicking a tag chip on any card.
  const [categoryFilter, setCategoryFilter] = useState<string | null>(null);
  const [teamFilter, setTeamFilter] = useState<string | null>(null);
  const [tagFilter, setTagFilter] = useState<string | null>(null);
  // PR 12 (codex Round-1 #3 hoist) — single source of truth for the
  // active filter set, used by every count IIFE in the toolbar. Each
  // chip / dropdown count IIFE re-uses this with one field skipped
  // via `rowMatchesFilters(s, filterFields, "<field>")`. Adding a
  // new filter field means updating this one literal; the chips
  // can't drift.
  const filterFields: FilterFields = {
    status: statusFilter,
    kind: kindFilter,
    category: categoryFilter,
    team: teamFilter,
    tag: tagFilter,
  };
  // PR 9 — collapse the taxonomy row into a `Filters ▾` disclosure
  // (designer Round-1: filter chrome stack was dense — kind chips +
  // lifecycle chips + taxonomy row + tag chips = 4 mechanisms above
  // the list). Default closed; auto-open ONLY on the 0→>0
  // transition (codex Round-1 #1 fix: previous shape kept reopening
  // the disclosure on every render while a filter was active, so a
  // user couldn't close it without first clearing the filter). The
  // ref tracks the previous count so we can detect the transition.
  const taxonomyActiveCount =
    (categoryFilter !== null ? 1 : 0) +
    (teamFilter !== null ? 1 : 0) +
    (tagFilter !== null ? 1 : 0);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const prevTaxonomyCountRef = useRef(taxonomyActiveCount);
  useEffect(() => {
    const prev = prevTaxonomyCountRef.current;
    if (prev === 0 && taxonomyActiveCount > 0) {
      // Fresh filter activation (e.g., user clicked a tag chip on a
      // card). Reveal the disclosure so the active-tag pill is
      // visible and clearable.
      setFiltersOpen(true);
    }
    // Intentionally NOT auto-closing on the >0→0 transition — the
    // user may want to set another filter without re-clicking the
    // toggle.
    prevTaxonomyCountRef.current = taxonomyActiveCount;
  }, [taxonomyActiveCount]);
  // Debounce the search input by 300ms before firing the backend turn
  // search. Typing fast shouldn't fire a query on every keystroke —
  // the metadata filter runs instantly, the content search lags
  // slightly. State only updates when the user has stopped for the
  // debounce window.
  const [debouncedQuery, setDebouncedQuery] = useState("");
  useEffect(() => {
    const id = window.setTimeout(() => setDebouncedQuery(searchQuery), 300);
    return () => window.clearTimeout(id);
  }, [searchQuery]);

  const sessionsQ = useQuery<SessionListRow[]>({
    queryKey: ["sessions-full"],
    queryFn: () => invoke<SessionListRow[]>("list_sessions_full", { limit: 50 }),
    staleTime: 30_000,
    refetchInterval: 30_000,
  });

  // Backend search across turn text. Returns the set of session ids
  // whose turns contain all the search tokens. Combined with the
  // metadata filter (client-side) via union: a row matches if it
  // either matches the metadata or appears in the content-match set.
  // Min length 2 (after trim) so a 1-char query doesn't hammer the
  // DB with a LIKE that matches almost every session.
  const contentSearchEnabled = debouncedQuery.trim().length >= 2;
  const turnSearchQ = useQuery<string[]>({
    queryKey: ["session-turn-search", debouncedQuery],
    queryFn: () =>
      invoke<string[]>("search_session_turns", { query: debouncedQuery }),
    enabled: contentSearchEnabled,
    staleTime: 30_000,
  });
  const contentMatchIds = new Set(
    contentSearchEnabled ? turnSearchQ.data ?? [] : [],
  );

  // Union of metadata matches and content-match ids. When the query
  // is empty, contentMatchIds is empty and the filter is metadata-only
  // (which itself is empty-query => "all").
  const filteredSessions = sessionsQ.data
    ? (() => {
        const metaMatched = filterSessions(
          sessionsQ.data,
          searchQuery,
          statusFilter,
          kindFilter,
          categoryFilter,
          teamFilter,
          tagFilter,
        );
        if (!contentSearchEnabled || contentMatchIds.size === 0) {
          return metaMatched;
        }
        // Build a set of metadata-matched ids and union with content
        // matches (also gated by status filter to keep the chips
        // honest).
        const metaIds = new Set(metaMatched.map((s) => s.id));
        const f: FilterFields = {
          status: statusFilter,
          kind: kindFilter,
          category: categoryFilter,
          team: teamFilter,
          tag: tagFilter,
        };
        return sessionsQ.data.filter((s) => {
          if (!rowMatchesFilters(s, f)) return false;
          return metaIds.has(s.id) || contentMatchIds.has(s.id);
        });
      })()
    : [];

  if (openSelection?.kind === "session") {
    return (
      <SessionTranscriptView
        sessionId={openSelection.id}
        onBack={() => setOpenSelection(null)}
      />
    );
  }
  if (openSelection?.kind === "single_run") {
    return (
      <SingleRunDetailView
        logId={openSelection.id}
        onBack={() => setOpenSelection(null)}
      />
    );
  }
  if (openSelection?.kind === "war_room") {
    return (
      <WarRoomDetailView
        warRoomId={openSelection.id}
        onBack={() => setOpenSelection(null)}
      />
    );
  }
  if (openSelection?.kind === "chat") {
    return (
      <ChatThreadDetailView
        threadId={openSelection.id}
        onBack={() => setOpenSelection(null)}
      />
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold flex items-center gap-2">
            <MessagesSquare className="text-cs-accent" size={24} />
            Sessions
          </h2>
          <p className="text-sm text-cs-muted mt-1">
            One inbox for every conversation — multi-turn sessions, single-shot
            runs, war rooms, and bottom-pane chats. Click any card to read or
            continue. Start new ones from the bottom pane.
          </p>
        </div>
        <button
          onClick={() => setShowNew(true)}
          className="flex items-center gap-2 px-3 py-2 rounded-md bg-cs-accent text-cs-bg text-sm font-medium hover:opacity-90"
        >
          <Plus size={14} />
          New session
        </button>
      </div>

      {showNew && (
        <NewSessionModal
          onClose={() => setShowNew(false)}
          onCreated={(id) => {
            setShowNew(false);
            setOpenSelection({ kind: "session", id });
          }}
        />
      )}

      {/* Search + status filter. The input matches case-insensitively
          across title, summary, tags, agent slug, runtime names, and
          the 8-char id prefix — every field a human would type. The
          three status chips below let you scope to open or closed
          sessions; "All" is the default so the list looks the same as
          before by default. */}
      {sessionsQ.data && sessionsQ.data.length > 0 && (
        <div className="space-y-2">
          <div className="relative">
            <Search
              size={14}
              className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted pointer-events-none"
            />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search titles, summaries, tags, and words inside the conversations…"
              className="w-full bg-cs-card border border-cs-border rounded-md pl-9 pr-9 py-2 text-sm focus:outline-none focus:border-cs-accent placeholder:text-cs-muted"
            />
            {searchQuery && (
              <button
                onClick={() => setSearchQuery("")}
                className="absolute right-2 top-1/2 -translate-y-1/2 text-cs-muted hover:text-cs-text"
                aria-label="clear search"
              >
                {turnSearchQ.isFetching ? (
                  <Loader2 size={14} className="animate-spin" />
                ) : (
                  <X size={14} />
                )}
              </button>
            )}
          </div>
          {/* PR 5b — kind filter chips. WhatsApp-feed model: "All"
              shows both real sessions and single-run dispatches (the
              data the History tab was the only surface for);
              "Sessions" scopes to multi-turn rooms; "Single runs"
              scopes to standalone dispatches. The status chips below
              (Open / Closed) apply to sessions only — single-runs don't
              have a lifecycle and are filtered out when a status is
              selected. */}
          <div className="flex items-center gap-2 text-xs">
            {(() => {
              // PR 12 — kind chips count contextually via the hoisted
              // `filterFields` (codex Round-1 #3).
              const rowsForKindCount = sessionsQ.data!.filter((s) =>
                rowMatchesFilters(s, filterFields, "kind")
              );
              return ([
                ["all", "All"],
                ["sessions", "Sessions"],
                ["single_runs", "Single runs"],
                ["war_rooms", "War rooms"],
                ["chats", "Chats"],
              ] as [KindFilter, string][]).map(([k, label]) => {
                const count =
                  k === "all"
                    ? rowsForKindCount.length
                    : k === "sessions"
                      ? rowsForKindCount.filter((row) => row.rowKind === "session").length
                      : k === "single_runs"
                        ? rowsForKindCount.filter((row) => row.rowKind === "single_run").length
                        : k === "war_rooms"
                          ? rowsForKindCount.filter((row) => row.rowKind === "war_room").length
                          : rowsForKindCount.filter((row) => row.rowKind === "chat").length;
                return (
                <button
                  key={k}
                  onClick={() => setKindFilter(k)}
                  className={cn(
                    "px-2 py-1 rounded-md border transition-colors",
                    kindFilter === k
                      ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                      : "border-cs-border bg-cs-card text-cs-muted hover:text-cs-text"
                  )}
                >
                  {label}
                  <span className="ml-1 opacity-60">({count})</span>
                </button>
              );
              });
            })()}
          </div>
          {/* PR 6 — taxonomy filters: category dropdown + team
              dropdown. Both implicitly scope to sessions (single-runs
              have neither). Counts shown next to each option reflect
              how many rows match the value under the currently-active
              kind/status/tag filters, so the dropdown stays honest
              about what's actually selectable. */}
          {(() => {
            // Per-dropdown counts reuse the hoisted `filterFields`
            // (PR 12 codex Round-1 #3): every count IIFE in the
            // toolbar pulls from the same source-of-truth set.
            const rowsForCategoryCount = sessionsQ.data!.filter((s) =>
              rowMatchesFilters(s, filterFields, "category")
            );
            const rowsForTeamCount = sessionsQ.data!.filter((s) =>
              rowMatchesFilters(s, filterFields, "team")
            );
            // Team options are free-form, so derive from distinct
            // values present in the relevant subset. Codex Round-1 #4:
            // also include the currently-selected team even when its
            // count is 0 under the other active filters, so the select
            // doesn't render an "invalid" value (selected but absent
            // from options). Sorted for stable dropdown order.
            const teamOptionSet = new Set(
              rowsForTeamCount
                .map((s) => s.team)
                .filter((t): t is string => typeof t === "string" && t.length > 0)
            );
            if (teamFilter !== null) teamOptionSet.add(teamFilter);
            const teamOptions = Array.from(teamOptionSet).sort();
            return (
              <div className="space-y-2">
                {/* PR 9+10 — replace the "Taxonomy" label with a
                    Filters ▾ disclosure. "Taxonomy" was internal
                    vocabulary (positioning Round-1); "Filters" is
                    what users actually search for. Closed by
                    default; auto-opens when any filter activates. */}
                <button
                  type="button"
                  onClick={() => setFiltersOpen((v) => !v)}
                  className="flex items-center gap-2 text-xs text-cs-muted hover:text-cs-text"
                  aria-expanded={filtersOpen}
                  aria-controls="sessions-filters-region"
                >
                  <span className="text-[10px] uppercase tracking-wider font-medium">
                    Filters
                  </span>
                  {taxonomyActiveCount > 0 && (
                    <span className="px-1.5 py-0.5 rounded text-[10px] font-medium bg-cs-accent/15 text-cs-accent">
                      {taxonomyActiveCount} active
                    </span>
                  )}
                  <span className="text-[10px]">{filtersOpen ? "▾" : "▸"}</span>
                </button>
                {filtersOpen && (
              <div
                id="sessions-filters-region"
                role="region"
                aria-label="Session filters"
                className="flex items-center gap-2 text-xs flex-wrap"
              >
                <label className="flex items-center gap-1">
                  <span className="text-cs-muted">category:</span>
                  <select
                    value={categoryFilter ?? ""}
                    onChange={(e) =>
                      setCategoryFilter(e.target.value === "" ? null : e.target.value)
                    }
                    className="bg-cs-card border border-cs-border rounded-md px-2 py-1 text-xs focus:outline-none focus:border-cs-accent"
                  >
                    <option value="">All</option>
                    {CATEGORY_VOCAB.map((c) => {
                      const n = rowsForCategoryCount.filter((s) => s.category === c).length;
                      return (
                        <option key={c} value={c} disabled={n === 0 && categoryFilter !== c}>
                          {c} ({n})
                        </option>
                      );
                    })}
                  </select>
                </label>
                <label className="flex items-center gap-1">
                  <span className="text-cs-muted">team:</span>
                  <select
                    value={teamFilter ?? ""}
                    onChange={(e) =>
                      setTeamFilter(e.target.value === "" ? null : e.target.value)
                    }
                    className="bg-cs-card border border-cs-border rounded-md px-2 py-1 text-xs focus:outline-none focus:border-cs-accent"
                    disabled={teamOptions.length === 0}
                  >
                    <option value="">All</option>
                    {teamOptions.map((t) => {
                      const n = rowsForTeamCount.filter((s) => s.team === t).length;
                      return (
                        <option key={t} value={t}>
                          {t} ({n})
                        </option>
                      );
                    })}
                  </select>
                </label>
                {tagFilter !== null && (
                  <span
                    className="flex items-center gap-1 px-2 py-1 rounded-md border border-cs-accent bg-cs-accent/10 text-cs-accent"
                    title="Active tag filter — click X to clear"
                  >
                    tag: <span className="font-mono">{tagFilter}</span>
                    <button
                      type="button"
                      onClick={() => setTagFilter(null)}
                      className="ml-1 hover:opacity-70"
                      aria-label="clear tag filter"
                    >
                      <X size={10} />
                    </button>
                  </span>
                )}
                {(categoryFilter || teamFilter || tagFilter) && (
                  <button
                    type="button"
                    onClick={() => {
                      setCategoryFilter(null);
                      setTeamFilter(null);
                      setTagFilter(null);
                    }}
                    className="text-cs-muted hover:text-cs-text underline-offset-2 hover:underline"
                  >
                    clear filters
                  </button>
                )}
              </div>
                )}
              </div>
            );
          })()}
          {/* Status chips — lifecycle filter that applies to SESSIONS
              ONLY. Single-runs carry `execution_logs.status` values
              ("success"/"error"/"unknown") which aren't open/closed,
              so they're hidden when this filter is anything other
              than "all" (codex Round-1 #5: rename/scope copy so the
              labels don't silently mean "sessions only"). The
              "(sessions)" tooltip + the row label below the chips
              make the scope explicit. */}
          <div className="flex items-center gap-2 text-xs">
            <span className="text-[10px] uppercase tracking-wider text-cs-muted font-medium">
              Lifecycle
              <span className="ml-1 opacity-60 normal-case lowercase">
                (sessions only)
              </span>
            </span>
            {(() => {
              // PR 12 — lifecycle chips count contextually via the
              // hoisted `filterFields` (codex Round-1 #3).
              const rowsForStatusCount = sessionsQ.data!.filter((s) =>
                rowMatchesFilters(s, filterFields, "status")
              );
              return (["all", "open", "closed"] as StatusFilter[]).map((s) => {
                const count =
                  s === "all"
                    ? rowsForStatusCount.length
                    : rowsForStatusCount.filter((row) => row.status === s).length;
              return (
                <button
                  key={s}
                  onClick={() => setStatusFilter(s)}
                  title={
                    s === "all"
                      ? "Show every row (sessions + single-runs)"
                      : `Show sessions whose lifecycle is "${s}". Single-runs are hidden here — they have no open/closed lifecycle.`
                  }
                  className={cn(
                    "px-2 py-1 rounded-md border capitalize transition-colors",
                    statusFilter === s
                      ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                      : "border-cs-border bg-cs-card text-cs-muted hover:text-cs-text"
                  )}
                >
                  {s}
                  <span className="ml-1 opacity-60">({count})</span>
                </button>
              );
              });
            })()}
            {(searchQuery ||
              statusFilter !== "all" ||
              kindFilter !== "all" ||
              categoryFilter !== null ||
              teamFilter !== null ||
              tagFilter !== null) && (
              <span className="text-cs-muted ml-auto">
                {filteredSessions.length} of {sessionsQ.data.length} shown
              </span>
            )}
          </div>
          {/* PR 7 (2026-05-17) — lifecycle chip count reconciliation
              footer. The lifecycle chips' "All (N)" counts single-runs
              too, but "Open + Closed" only sums sessions, so the
              numbers don't naturally add up to N. A small breakdown
              line below removes the visual mystery: "N total · S
              sessions · E single-runs" matches what "All" actually
              contains. Hidden when the user has narrowed via search
              or any filter to avoid double-redundancy with the "X of
              Y shown" line above. (pr-reviewer Round-2 nit on PR 5c.) */}
          {!searchQuery &&
            statusFilter === "all" &&
            kindFilter === "all" &&
            categoryFilter === null &&
            teamFilter === null &&
            tagFilter === null && (
              <div className="text-[10px] text-cs-muted opacity-70">
                {sessionsQ.data!.length} total ·{" "}
                {sessionsQ.data!.filter((r) => r.rowKind === "session").length} sessions
                ·{" "}
                {sessionsQ.data!.filter((r) => r.rowKind === "single_run").length} single-runs
                ·{" "}
                {sessionsQ.data!.filter((r) => r.rowKind === "war_room").length} war rooms
                ·{" "}
                {sessionsQ.data!.filter((r) => r.rowKind === "chat").length} chats.{" "}
                <span className="opacity-80">
                  Open / Closed lifecycle applies to sessions only.
                </span>
              </div>
            )}
        </div>
      )}

      {sessionsQ.isLoading ? (
        <div className="flex items-center justify-center h-32">
          <Loader2 className="animate-spin text-cs-accent" size={28} />
        </div>
      ) : !sessionsQ.data || sessionsQ.data.length === 0 ? (
        <div className="text-center py-12 text-cs-muted">
          <MessagesSquare size={48} className="mx-auto mb-4 opacity-50" />
          <p className="text-cs-text text-base">Compare any AI · keep the receipts.</p>
          <p className="text-sm mt-3 max-w-md mx-auto">
            This is your inbox for every conversation — quick chats, multi-turn
            sessions, war rooms, and single-shot dispatches. Start one from the
            bottom pane's{" "}
            <span className="text-cs-accent">+ New</span> menu, or from the CLI:
            {" "}
            <code className="bg-cs-card px-1.5 py-0.5 rounded text-cs-text">
              ato dispatch claude "hello"
            </code>
            .
          </p>
          <p className="text-xs mt-2 max-w-md mx-auto opacity-70">
            Cross-runtime turns: dispatch a different runtime with{" "}
            <code className="bg-cs-card/60 px-1 rounded">--session &lt;id&gt;</code>{" "}
            and the same session picks them up.
          </p>
        </div>
      ) : filteredSessions.length === 0 ? (
        <div className="text-center py-12 text-cs-muted">
          <Search size={36} className="mx-auto mb-3 opacity-50" />
          <p>No sessions match your search.</p>
          <p className="text-xs mt-2">
            Try a different word, or clear the filter to see all{" "}
            {sessionsQ.data.length} sessions.
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {filteredSessions.map((s) => {
            // PR 5b — single-run card variant. A single-run dispatch
            // (`rowKind === "single_run"`) gets a lighter card: one
            // runtime badge, persona (if any), the prompt prefix as
            // title, response preview, cost, timestamp. No Coord/+
            // group (only one runtime spoke), no closed-lock (no
            // lifecycle), no category/team (taxonomy is a session-only
            // concern). Click-into-detail lands in PR 5c — until then
            // single-run cards render as non-interactive `div`s with a
            // tooltip explaining that the History tab still has the
            // full detail.
            // PR 14b — war-room synthetic card. Groups N single-runs
            // sharing a war_room_id into one card. Renders before the
            // single-run branch so the type-narrowing flows cleanly.
            // PR 14c — clickable: routes to WarRoomDetailView which
            // lists the constituent dispatches per seat.
            // 2026-05-18 — Path A consolidation: chat threads from the
            // bottom-pane Chat tab. Renders before session/war-room
            // branches so type-narrowing flows cleanly. The card mirrors
            // the war-room/single-run/session card shape (kind pill at
            // position 0, runtime badge, right-aligned meta cluster,
            // title on its own row, body preview) so the four variants
            // feel like one feed at a 60px scan.
            //
            // Click-into-detail: today, opening the chat surfaces the
            // assistant-side reply preview only (we have no full chat
            // transcript view yet — the bottom pane is the live surface).
            // Path B (multi-launcher refactor) will give chat threads a
            // proper detail view; for now, clicking just keeps the user
            // on Sessions with the row marked open. Per "ship as is"
            // (Path A is the cheap UNION), this is fine — users still
            // SEE their chats in the inbox, which is what was missing.
            if (s.rowKind === "chat") {
              return (
                <button
                  key={s.id}
                  onClick={() =>
                    setOpenSelection({ kind: "chat", id: s.id })
                  }
                  title={`Chat thread ${s.id}`}
                  className="w-full text-left border rounded-lg p-4 transition-colors border-cs-border/60 bg-cs-card/60 hover:border-cs-accent/40"
                >
                  <div className="flex items-center gap-3 flex-wrap">
                    <span
                      aria-label="chat"
                      className="px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide bg-cs-muted/15 text-cs-muted"
                      title="Bottom-pane chat thread. One-on-one conversation, can hop runtimes per message."
                    >
                      🗨 chat
                    </span>
                    <span
                      className={cn(runtimeBadge(s.runtime))}
                      title={`Most recent runtime: ${runtimeDisplay(s.runtime)}`}
                    >
                      {s.runtime}
                    </span>
                    <div className="ml-auto inline-flex items-center gap-3 text-xs text-cs-muted">
                      <span>
                        {s.turnCount} msg{s.turnCount !== 1 ? "s" : ""}
                      </span>
                      <span>{formatTime(s.lastUsedAt)}</span>
                    </div>
                  </div>
                  <div className="mt-2 text-sm font-medium text-cs-text truncate">
                    {s.title || (
                      <span className="text-cs-muted italic font-normal">
                        untitled chat
                      </span>
                    )}
                  </div>
                  <div className="mt-1 flex items-center flex-wrap gap-x-3 gap-y-1 text-[11px] text-cs-muted">
                    <span>
                      runtime:{" "}
                      <span className="text-cs-text">{runtimeDisplay(s.runtime)}</span>
                    </span>
                    <span>
                      kind: <span className="text-cs-text">bottom-pane chat</span>
                    </span>
                  </div>
                  {s.lastAssistantPreview && (
                    <div className="mt-2 text-xs text-cs-muted line-clamp-2">
                      {s.lastAssistantPreview}
                    </div>
                  )}
                </button>
              );
            }
            if (s.rowKind === "war_room") {
              const participantCount = s.runtimesUsed.length;
              return (
                <button
                  key={s.id}
                  onClick={() =>
                    setOpenSelection({ kind: "war_room", id: s.id })
                  }
                  title={`War room ${s.id}`}
                  className="w-full text-left border rounded-lg p-4 transition-colors border-cs-border/60 bg-cs-card/60 hover:border-cs-accent/40"
                >
                  <div className="flex items-center gap-3 flex-wrap">
                    <span
                      aria-label="war room"
                      className="px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide bg-cs-accent/15 text-cs-accent"
                      title={`War room ${s.id.slice(0, 8)} — ${participantCount} parallel seats`}
                    >
                      ⚔ war room
                    </span>
                    {/* Participant runtime badges. All co-equal — no
                        Coord/+ split since R1-parallel war-rooms are
                        peers by design. */}
                    <div className="flex items-center gap-1">
                      <span className="text-[10px] uppercase tracking-wider text-cs-muted font-medium">
                        seats
                      </span>
                      {s.runtimesUsed.map((r) => (
                        <span
                          key={r}
                          className={cn(runtimeBadge(r))}
                          title={`Participant runtime: ${r}`}
                        >
                          {r}
                        </span>
                      ))}
                    </div>
                    {s.agentsUsed.length > 0 && (
                      <div className="flex items-center gap-1">
                        {s.agentsUsed.map((slug) => (
                          <span
                            key={slug}
                            className={personaBadge()}
                            title={`Persona: ${personaDisplay(slug)}`}
                          >
                            {personaDisplay(slug)}
                          </span>
                        ))}
                      </div>
                    )}
                    {/* PR 17 follow-up — right-side meta cluster.
                        Title moved to its own row below (line below)
                        so it doesn't get pushed by chip overflow. */}
                    <div className="ml-auto inline-flex items-center gap-3 text-xs text-cs-muted">
                      <span>
                        {participantCount} seat
                        {participantCount !== 1 ? "s" : ""}
                      </span>
                      {s.totalCostUsd !== null && s.totalCostUsd > 0 && (
                        <span className="font-mono">
                          ${s.totalCostUsd.toFixed(4)}
                        </span>
                      )}
                      <span>{formatTime(s.lastUsedAt)}</span>
                    </div>
                  </div>
                  <div className="mt-2 text-sm font-medium text-cs-text truncate">
                    {s.title || (
                      <span className="text-cs-muted italic font-normal">
                        untitled war room
                      </span>
                    )}
                  </div>
                  {/* PR 17 — meta line on the war-room card so it has
                      parity with the session card's coordinator/team
                      line. Shows the seat-count × round-count summary;
                      the war-room title (first round's prompt prefix)
                      already appears in the chip row above. */}
                  <div className="mt-1 flex items-center flex-wrap gap-x-3 gap-y-1 text-[11px] text-cs-muted">
                    <span>
                      seats:{" "}
                      <span className="text-cs-text">{participantCount}</span>
                    </span>
                    <span>
                      kind:{" "}
                      <span className="text-cs-text">parallel</span> · each seat
                      fires independently within a round; across rounds every
                      seat sees the full peer transcript
                    </span>
                  </div>
                  {/* PR 17 — body preview: first-round prompt. Matches
                      single-run + session cards which both surface a
                      body line, so the Sessions feed feels uniform. */}
                  {s.title && (
                    <div className="mt-2 text-xs text-cs-muted line-clamp-2">
                      {s.title}
                    </div>
                  )}
                </button>
              );
            }
            if (s.rowKind === "single_run") {
              const promptPreview = s.title ?? "(no prompt recorded)";
              const responsePreview = s.lastAssistantPreview;
              const isErr = s.status !== "success";
              return (
                <button
                  key={s.id}
                  onClick={() =>
                    setOpenSelection({ kind: "single_run", id: s.id })
                  }
                  title="Open the full prompt + response for this single-run dispatch."
                  className={cn(
                    "w-full text-left border rounded-lg p-4 transition-colors",
                    isErr
                      ? "border-cs-danger/40 bg-cs-card/40 hover:border-cs-danger/60"
                      : "border-cs-border/60 bg-cs-card/60 hover:border-cs-accent/40"
                  )}
                >
                  <div className="flex items-center gap-3 flex-wrap">
                    {/* PR 17 — leading kind marker pill for parity
                        with war-room (⚔ WAR ROOM) and session
                        (💬 SESSION). Replaces the previous "small
                        glyph + mid-row 'single run' text pill" combo
                        with a single position-0 pill. Status (error
                        vs success) still encoded via the pill bg
                        (danger tint on error) so the same pill does
                        double duty. */}
                    <span
                      aria-label="single run"
                      className={cn(
                        "px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide",
                        isErr
                          ? "bg-cs-danger/15 text-cs-danger"
                          : "bg-cs-muted/15 text-cs-muted"
                      )}
                      title={`Single run · ${s.status}`}
                    >
                      ⚡ single run
                    </span>
                    <span
                      className={cn(runtimeBadge(s.runtime))}
                      title={`Runtime: ${runtimeDisplay(s.runtime)}`}
                    >
                      {s.runtime}
                    </span>
                    {s.agentSlug && (
                      <span
                        className={personaBadge()}
                        title={`Persona seat: ${personaDisplay(s.agentSlug)}`}
                      >
                        {personaDisplay(s.agentSlug)}
                      </span>
                    )}
                    {/* PR 17 follow-up — right-side meta cluster.
                        Title moved to its own row below at uniform
                        text-sm font-medium (was previously
                        text-xs font-mono — typography drift). */}
                    <div className="ml-auto inline-flex items-center gap-3 text-xs text-cs-muted">
                      {s.totalCostUsd !== null && s.totalCostUsd > 0 && (
                        <span
                          className="font-mono"
                          title="Estimated cost from execution_logs.cost_usd_estimated."
                        >
                          ${s.totalCostUsd.toFixed(4)}
                        </span>
                      )}
                      <span>{formatTime(s.lastUsedAt)}</span>
                    </div>
                  </div>
                  <div className="mt-2 text-sm font-medium text-cs-text truncate">
                    {promptPreview}
                  </div>
                  {/* PR 17 — meta line on the single-run card for parity
                      with session (coord/team) and war-room (seats × rounds)
                      meta lines. Tells the reader what they're about to
                      open without having to parse the chip row. */}
                  <div className="mt-1 flex items-center flex-wrap gap-x-3 gap-y-1 text-[11px] text-cs-muted">
                    <span>
                      runtime:{" "}
                      <span className="text-cs-text">{runtimeDisplay(s.runtime)}</span>
                    </span>
                    {s.agentSlug && (
                      <span>
                        persona:{" "}
                        <span className="text-cs-accent">
                          {personaDisplay(s.agentSlug)}
                        </span>
                      </span>
                    )}
                    <span>
                      kind: <span className="text-cs-text">single dispatch</span>
                    </span>
                  </div>
                  {responsePreview && (
                    <div className="mt-2 text-xs text-cs-muted line-clamp-2 font-mono">
                      {responsePreview}
                    </div>
                  )}
                </button>
              );
            }
            // Real-session card path below — unchanged from PR 4.
            // Prefer the coordinator-generated auto_title when present
            // (it's distilled from the actual conversation); fall back
            // to the user-supplied title, then to a muted "untitled".
            const displayTitle = s.autoTitle || s.title;
            // For closed sessions, the summary is a better preview than
            // the last assistant turn (which is often a tool result or
            // mid-thought fragment). For open sessions, keep the live
            // last-turn preview so users see what's happening now.
            const previewText =
              s.status === "closed" && s.summary
                ? s.summary
                : s.lastAssistantPreview;
            return (
              <button
                key={s.id}
                onClick={() => setOpenSelection({ kind: "session", id: s.id })}
                title={`Session ${s.id}`}
                className={cn(
                  "w-full text-left border rounded-lg transition-colors p-4",
                  s.status === "closed"
                    ? "border-cs-border/60 bg-cs-card/60 hover:border-cs-accent/40"
                    : "border-cs-border bg-cs-card hover:border-cs-accent/40 hover:bg-cs-border/20"
                )}
              >
                <div className="flex items-center gap-3 flex-wrap">
                  {/* PR 17 (2026-05-18) — kind marker on session cards
                      for parity with war-room (⚔) + single-run (⚡)
                      cards. Without this, the Sessions feed had three
                      card variants with different "what is this"
                      affordances; the kind marker is the visual hook
                      that says "I'm a session" at a 60px scan. */}
                  <span
                    aria-label="session"
                    className="px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide bg-cs-muted/15 text-cs-muted"
                    title="Sequential multi-turn conversation. Each new turn sees prior turns via history replay."
                  >
                    💬 session
                  </span>
                  {/* 2026-05-17 — coordinator vs participants split into
                      explicit labelled groups. Previously rendered as a
                      single badge cluster with a ★ prefix on the
                      coordinator — too easy to miss when the card has 4+
                      runtime badges. Now: "Coord" label + ring-accented
                      coordinator badge, separator, "+" label + dimmed
                      participant badges. The session's anchor runtime is
                      always shown as coordinator even if no turns have
                      been recorded yet. */}
                  <div className="flex items-center gap-1">
                    <span className="text-[10px] uppercase tracking-wider text-cs-muted font-medium">
                      Coord
                    </span>
                    <span
                      className={cn(
                        runtimeBadge(s.runtime),
                        "ring-1 ring-cs-accent/70"
                      )}
                      title={`Coordinator runtime: ${runtimeDisplay(s.runtime)} — orchestrated this session`}
                    >
                      {s.runtime}
                    </span>
                  </div>
                  {(() => {
                    const participants = s.runtimesUsed.filter(
                      (r) => r !== s.runtime
                    );
                    if (participants.length === 0) return null;
                    return (
                      <div className="flex items-center gap-1">
                        <span className="text-[10px] uppercase tracking-wider text-cs-muted font-medium">
                          +
                        </span>
                        {participants.map((r) => (
                          <span
                            key={r}
                            className={cn(runtimeBadge(r), "opacity-75")}
                            title={`Participant runtime: ${runtimeDisplay(r)} — contributed turns to this session`}
                          >
                            {r}
                          </span>
                        ))}
                      </div>
                    );
                  })()}
                  {/* 2026-05-16 — persona cluster. Renders the distinct
                      seat slugs that spoke in this session, in first-
                      spoken order. Empty (so the cluster is hidden) for
                      generalist-only sessions. */}
                  {s.agentsUsed.length > 0 && (
                    <div className="flex items-center gap-1">
                      {s.agentsUsed.map((slug) => (
                        <span
                          key={slug}
                          className={personaBadge()}
                          title={`Persona seat: ${personaDisplay(slug)}`}
                        >
                          {personaDisplay(slug)}
                        </span>
                      ))}
                    </div>
                  )}
                  {s.status === "closed" && (
                    <span
                      className="flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium uppercase bg-cs-muted/20 text-cs-muted"
                      title={
                        s.closedAt
                          ? `Closed ${formatTime(s.closedAt)}`
                          : "Closed"
                      }
                    >
                      <Lock size={10} /> closed
                    </span>
                  )}
                  {/* 2026-05-17 — Sessions UX polish PR 4. Category
                      badge in the cs-accent (cyan) color sits between
                      the closed-lock and the title so the work-band
                      reads at a glance (Dev / Marketing / Backend /
                      etc.). Hidden when NULL (pre-PR-2 rows or rows
                      that closed without category — PR 3 will warn at
                      close time). */}
                  {s.category && (
                    <span
                      className="px-1.5 py-0.5 rounded text-[10px] font-medium uppercase bg-cs-accent/15 text-cs-accent"
                      title={`Category: ${s.category} — populated by the coordinator at close`}
                    >
                      {s.category}
                    </span>
                  )}
                  {/* PR 17 follow-up — right-side meta cluster.
                      Title moved to its own row below so chip
                      overflow doesn't push the title's vertical
                      rhythm out of sync with cards that have
                      fewer chips. */}
                  <div className="ml-auto inline-flex items-center gap-3 text-xs text-cs-muted">
                    <span>
                      {s.turnCount} turn{s.turnCount !== 1 ? "s" : ""}
                    </span>
                    {s.totalCostUsd !== null && s.totalCostUsd > 0 && (
                      <span
                        className="font-mono"
                        title="Estimated session cost (sum of execution_logs). Open the session to see the per-runtime breakdown including which rows are metered API vs subscription-estimate."
                      >
                        ${s.totalCostUsd.toFixed(4)}
                      </span>
                    )}
                    <span>{formatTime(s.lastUsedAt)}</span>
                  </div>
                </div>
                <div className="mt-2 text-sm font-medium text-cs-text truncate">
                  {displayTitle || (
                    <span className="text-cs-muted italic font-normal">
                      untitled session
                    </span>
                  )}
                </div>
                {/* 2026-05-16 — coordinator + project line. Coordinator
                    is the session's anchor runtime (where the session
                    was created). The session-level agent slug (when
                    set) is the agent the SESSION was anchored to —
                    separate from the per-turn personas in the cluster
                    above. Project shows which project the conversation
                    is scoped to. */}
                <div className="mt-1 flex items-center flex-wrap gap-x-3 gap-y-1 text-[11px] text-cs-muted">
                  <span>
                    coordinator:{" "}
                    <span className="text-cs-text">
                      {runtimeDisplay(s.runtime)}
                    </span>
                    {s.agentSlug && (
                      <>
                        {" / "}
                        <span className="text-cs-accent">
                          {personaDisplay(s.agentSlug)}
                        </span>
                      </>
                    )}
                  </span>
                  {s.projectId && (
                    <span>
                      project:{" "}
                      {/* PR 15 — prefer the resolved human name from
                          the projects JOIN; fall back to the 8-char id
                          prefix when the name doesn't resolve (project
                          deleted but session retains the snapshot
                          id). Hover-title shows the full id either way
                          so the canonical reference is one mouse-over
                          away. */}
                      <span
                        className="text-cs-text font-mono"
                        title={`project_id: ${s.projectId}`}
                      >
                        {s.projectName ?? s.projectId.slice(0, 8)}
                      </span>
                    </span>
                  )}
                  {/* 2026-05-17 — Sessions UX polish PR 4. Team is a
                      free-form owner/band label (founder / frontend /
                      backend / ops / etc.). Sits alongside project so
                      the metadata line reads "coordinator · project ·
                      team" left-to-right. Hidden when NULL. */}
                  {s.team && (
                    <span>
                      team:{" "}
                      <span className="text-cs-text font-mono">{s.team}</span>
                    </span>
                  )}
                </div>
                {previewText && (
                  <div className="mt-2 text-xs text-cs-muted line-clamp-2">
                    {previewText}
                  </div>
                )}
                {s.tags.length > 0 && (
                  <div className="mt-2 flex items-center gap-1 flex-wrap">
                    <Tag size={10} className="text-cs-muted" />
                    {s.tags.map((tag) => (
                      // PR 6 — tag chips become click-to-filter. The
                      // outer card is a button (opens the session), so
                      // a nested button is invalid HTML; use a span
                      // with role=button + onClick. stopPropagation so
                      // the click sets the filter without also opening
                      // the session detail.
                      <span
                        key={tag}
                        role="button"
                        tabIndex={0}
                        aria-pressed={tagFilter === tag}
                        onClick={(e) => {
                          e.stopPropagation();
                          // Toggle: clicking the already-active tag
                          // clears the filter (matches the aria-pressed
                          // toggle semantics).
                          setTagFilter(tagFilter === tag ? null : tag);
                        }}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" || e.key === " ") {
                            e.preventDefault();
                            e.stopPropagation();
                            setTagFilter(tagFilter === tag ? null : tag);
                          }
                        }}
                        title={
                          tagFilter === tag
                            ? `Clear tag filter (currently "${tag}")`
                            : `Filter to sessions tagged "${tag}"`
                        }
                        className={cn(
                          // PR 9 — pressed-state designer note: the
                          // color-only delta (bg + text inversion)
                          // fails for users with color vision
                          // deficiency. Add font-weight + letter-
                          // spacing so the active tag also reads as
                          // "different" by typography even if the
                          // color contrast collapses. tracking-wide ≈
                          // 0.025em letter-spacing; font-bold = 700
                          // vs the default 500.
                          "px-1.5 py-0.5 rounded text-[10px] cursor-pointer transition-colors",
                          "focus:outline-none focus-visible:ring-2 focus-visible:ring-cs-accent focus-visible:ring-offset-1 focus-visible:ring-offset-cs-bg",
                          tagFilter === tag
                            ? "bg-cs-accent text-cs-bg ring-1 ring-cs-accent font-bold tracking-wide"
                            : "bg-cs-accent/10 text-cs-accent hover:bg-cs-accent/20 font-medium"
                        )}
                      >
                        {tag}
                      </span>
                    ))}
                  </div>
                )}
                {/* PR 17 (2026-05-18) — raw UUID moved to a hover-
                    only tooltip on the card. Will flagged the
                    Sessions feed for visual inconsistency: war-room
                    + single-run cards don't show their UUID; only
                    sessions did. Consistency wins; the UUID is one
                    mouse-over away when needed. */}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
