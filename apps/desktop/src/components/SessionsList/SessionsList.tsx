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
  type SessionListRow,
} from "./_helpers";
import {
  ChatCard,
  WarRoomCard,
  SingleRunCard,
  SessionCard,
} from "./SessionCards";

// SessionListRow moved to ./_helpers.ts (2026-05-19) so SessionCards/
// can import it without reaching back through this file.

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
  // 2026-05-19 — subscribe to the pending VALUES (not the consume
  // functions) so the effect re-runs when the bottom-pane multi-launcher
  // flips them while SessionsList is already mounted. Previously the
  // effect deps were the stable consume function refs, so the effect
  // only ran once on mount; clicking Multi-turn session from the
  // chevron dropdown while on the Sessions tab silently set the flag
  // with no listener.
  const pendingOpenSessionKind = useUiStore((s) => s.pendingOpenSessionKind);
  const pendingOpenSessionId = useUiStore((s) => s.pendingOpenSessionId);
  const pendingOpenNewSession = useUiStore((s) => s.pendingOpenNewSession);
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
    if (pendingOpenSessionId && pendingOpenSessionKind) {
      const pending = consumePendingOpenSession();
      if (pending.id && pending.kind) {
        setOpenSelection({ kind: pending.kind, id: pending.id });
      }
    }
    if (pendingOpenNewSession) {
      if (consumePendingOpenNewSession()) {
        // 2026-05-19 — if a detail view is open (session / chat /
        // war-room / single-run transcript), the early-return at
        // line 347 prevents the `<NewSessionModal>` JSX from ever
        // rendering. Close the detail view first so the modal can
        // mount. War-room launcher doesn't hit this because the
        // FirstChatWizard is mounted globally in Dashboard.tsx.
        setOpenSelection(null);
        setShowNew(true);
      }
    }
  }, [
    pendingOpenSessionKind,
    pendingOpenSessionId,
    pendingOpenNewSession,
    consumePendingOpenSession,
    consumePendingOpenNewSession,
  ]);
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
  // 2026-05-19 — hide zero-turn rows from the inbox. The bottom-pane
  // chat creates a thread on focus, FirstChatWizard navigates here
  // before the dispatch lands, NewSessionModal commits the row before
  // the first turn is sent — every one of those paths leaves an empty
  // ghost row. Filtering here is the v2.7.6 quick fix; lazy row
  // creation at the write points is queued for v2.7.7.
  const nonEmptyData = sessionsQ.data
    ? sessionsQ.data.filter((s) => s.turnCount > 0)
    : undefined;
  const filteredSessions = nonEmptyData
    ? (() => {
        const metaMatched = filterSessions(
          nonEmptyData,
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
        return nonEmptyData.filter((s) => {
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
      {nonEmptyData && nonEmptyData.length > 0 && (
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
              const rowsForKindCount = nonEmptyData!.filter((s) =>
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
            const rowsForCategoryCount = nonEmptyData!.filter((s) =>
              rowMatchesFilters(s, filterFields, "category")
            );
            const rowsForTeamCount = nonEmptyData!.filter((s) =>
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
              const rowsForStatusCount = nonEmptyData!.filter((s) =>
                rowMatchesFilters(s, filterFields, "status")
              );
              // Lifecycle applies to sessions only — Open/Closed match
              // session rows by definition. "All" must count from the
              // same denominator so the math adds up (All = Open + Closed).
              // Pre-fix this was rowsForStatusCount.length which mixed in
              // single-runs / war-rooms / chats and showed e.g. All (49)
              // alongside Open (6) Closed (1). Will's dogfood 2026-05-21.
              const sessionRows = rowsForStatusCount.filter(
                (row) => row.rowKind === "session"
              );
              return (["all", "open", "closed"] as StatusFilter[]).map((s) => {
                const count =
                  s === "all"
                    ? sessionRows.length
                    : sessionRows.filter((row) => row.status === s).length;
              return (
                <button
                  key={s}
                  onClick={() => setStatusFilter(s)}
                  title={
                    s === "all"
                      ? "Show every session regardless of lifecycle. Non-session rows (single-runs, war-rooms, chats) are unaffected by this filter."
                      : `Show sessions whose lifecycle is "${s}". Non-session rows are hidden when a lifecycle is selected — they have no open/closed state.`
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
                {filteredSessions.length} of {nonEmptyData!.length} shown
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
                {nonEmptyData!.length} total ·{" "}
                {nonEmptyData!.filter((r) => r.rowKind === "session").length} sessions
                ·{" "}
                {nonEmptyData!.filter((r) => r.rowKind === "single_run").length} single-runs
                ·{" "}
                {nonEmptyData!.filter((r) => r.rowKind === "war_room").length} war rooms
                ·{" "}
                {nonEmptyData!.filter((r) => r.rowKind === "chat").length} chats.{" "}
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
      ) : !nonEmptyData || nonEmptyData.length === 0 ? (
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
            {nonEmptyData!.length} sessions.
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {filteredSessions.map((s) => {
            // 2026-05-19 elegance push — the four card variants moved to
            // SessionCards/ ({Chat,WarRoom,SingleRun,Session}Card.tsx).
            // Pick by rowKind; chat/war-room/single-run render before the
            // default session path so type-narrowing flows cleanly.
            if (s.rowKind === "chat") {
              return (
                <ChatCard
                  key={s.id}
                  session={s}
                  onOpen={() => setOpenSelection({ kind: "chat", id: s.id })}
                />
              );
            }
            if (s.rowKind === "war_room") {
              return (
                <WarRoomCard
                  key={s.id}
                  session={s}
                  onOpen={() =>
                    setOpenSelection({ kind: "war_room", id: s.id })
                  }
                />
              );
            }
            if (s.rowKind === "single_run") {
              return (
                <SingleRunCard
                  key={s.id}
                  session={s}
                  onOpen={() =>
                    setOpenSelection({ kind: "single_run", id: s.id })
                  }
                />
              );
            }
            return (
              <SessionCard
                key={s.id}
                session={s}
                onOpen={() => setOpenSelection({ kind: "session", id: s.id })}
                tagFilter={tagFilter}
                setTagFilter={setTagFilter}
              />
            );
          })}
        </div>
      )}
    </div>
  );
}
