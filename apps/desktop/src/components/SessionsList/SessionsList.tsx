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
  Bot,
  User as UserIcon,
  Loader2,
  Sparkles,
  Plus,
  Send,
  GitBranch,
  X,
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
  clusterEvalRuns,
  type SessionListRow,
} from "./_helpers";
import {
  ChatCard,
  WarRoomCard,
  SingleRunCard,
  SessionCard,
} from "./SessionCards";
import SharedDetailView, {
  type SharedResourceKind,
} from "@/components/TeamWorkspaces/SharedDetailView";
// teamfilter (#1) — cloud-shared rows. getTeams returns the teams the
// user belongs to (or throws for free/unauthenticated tiers — we swallow
// that so the merge is a no-op); the three getShared* calls return the
// snapshots shared into each team.
import {
  getTeams,
  getSharedSessions,
  getSharedWarRooms,
  getSharedChats,
  getTeamMembers,
  type TeamMemberSimple,
} from "@/lib/cloud-api";
// v2.10.0 PR-1 (UI) — eval-cluster card. Imported as a default-style
// named import so re-exports from ./SessionCards/index don't have to
// land in lockstep; the cluster card is opt-in render-path only.
import { EvalClusterCard } from "./SessionCards/EvalClusterCard";

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
// teamfilter (#1) — "team" is a peer of the existing kind chips. Picking
// it fetches cloud-shared rows and scopes the feed to them (same model as
// "sessions"/"chats" scoping to a single rowKind).
type KindFilter =
  | "all"
  | "sessions"
  | "single_runs"
  | "war_rooms"
  | "chats"
  | "team";

// teamfilter (#1) — rowKinds that came from the cloud share feed.
const TEAM_SHARED_KINDS = new Set<SessionListRow["rowKind"]>([
  "team_shared_session",
  "team_shared_war_room",
  "team_shared_chat",
]);

// teamfilter (#1) — safely read a string field off an unknown snapshot
// payload. The shared-* endpoints return `snapshot: unknown`; we only
// surface a handful of human-readable fields and never trust the shape.
function snapshotString(
  snapshot: unknown,
  ...keys: string[]
): string | null {
  if (!snapshot || typeof snapshot !== "object") return null;
  const obj = snapshot as Record<string, unknown>;
  for (const k of keys) {
    const v = obj[k];
    if (typeof v === "string" && v.trim().length > 0) return v;
  }
  return null;
}

// Part C — enriched teamSharedRow: accepts optional rich fields from the
// cloud list endpoint (card_title, summary, tags, category, coordinator,
// shared_by_name, shared_by_email) so the rich card variants render fully.
// FIX 4 — also accepts seat_count/seat_runtimes (war-rooms) and
// turn_count (sessions) so the reused cards show REAL metadata.
function teamSharedRow(args: {
  teamId: string;
  teamName: string;
  rowKind: Extract<SessionListRow["rowKind"], `team_shared_${string}`>;
  originalId: string;
  sharedByUserId: string;
  sharedAt: string;
  snapshot: unknown;
  // Part C — optional rich fields from the cloud list endpoint
  cardTitle?: string | null;
  richSummary?: string | null;
  richTags?: string[] | null;
  richCategory?: string | null;
  coordinator?: string | null;
  sharedByName?: string | null;
  sharedByEmail?: string | null;
  // Part F — anchor runtime for chats
  anchorRt?: string | null;
  richRuntime?: string | null;
  // FIX 4 — real seat / turn metadata
  seatCount?: string | null;
  seatRuntimes?: string[] | null;
  turnCount?: number | null;
}): SessionListRow {
  const {
    teamId,
    teamName,
    rowKind,
    originalId,
    sharedByUserId,
    sharedAt,
    snapshot,
    cardTitle,
    richSummary,
    richTags,
    richCategory,
    coordinator,
    sharedByName,
    sharedByEmail,
    anchorRt,
    richRuntime,
    seatCount,
    seatRuntimes,
    turnCount,
  } = args;

  // Resolve display label: prefer resolved name/email over truncated userId.
  const sharedByLabel =
    sharedByName ??
    (sharedByEmail ? sharedByEmail.split("@")[0] : null) ??
    (sharedByUserId ? sharedByUserId.slice(0, 8) : null);

  // Resolve runtime: prefer the explicit richRuntime (from list endpoint),
  // then snapshot fields.
  const runtime =
    richRuntime ??
    snapshotString(snapshot, "runtime", "anchorRuntime") ??
    "claude";

  // Resolve anchor runtime: explicit field first, then snapshot.
  const anchorRuntime =
    anchorRt ??
    snapshotString(snapshot, "anchorRuntime") ??
    null;

  // Title resolution: prefer card_title (coordinator-distilled), then
  // snapshot autoTitle/title, then snapshot summary as last resort.
  const title =
    cardTitle ??
    snapshotString(snapshot, "autoTitle", "title", "summary");

  // Summary resolution: prefer cloud-provided, then snapshot.
  const summary =
    richSummary ??
    snapshotString(snapshot, "summary", "lastAssistantPreview");

  // Tags: prefer cloud-provided array; snapshot fallback is a no-op
  // since snapshots don't always carry tags as arrays.
  // FIX 5 — guard: node-pg may return JSONB tags as a non-array object;
  // fall back to [] to prevent cards' tags.map(...) from throwing.
  const tags: string[] = Array.isArray(richTags) ? richTags : [];

  // Category: prefer cloud-provided.
  const category = richCategory ?? null;

  // Coordinator runtime: prefer explicit field, then snapshot.
  const coordinatorRuntime =
    coordinator ??
    snapshotString(snapshot, "coordinator", "coordinatorRuntime") ??
    null;

  // FIX 4 — resolve runtimesUsed from seat_runtimes (war-rooms).
  // Filter falsy entries so empty strings from the backend don't render
  // as blank badges. For sessions/chats, fall back to [] so other card
  // variants are unaffected.
  const runtimesUsed: string[] =
    Array.isArray(seatRuntimes) ? seatRuntimes.filter(Boolean) : [];

  // FIX 4 — turnCount: use backend value when present; for chats leave
  // it 0/undefined so the ChatCard omits the "N msgs" counter rather
  // than showing a fake "1 msg". For war-rooms we don't show turnCount
  // (WarRoomCard uses runtimesUsed.length as participantCount instead).
  const resolvedTurnCount: number =
    typeof turnCount === "number" && turnCount >= 0 ? turnCount : 0;

  return {
    id: `shared:${teamId}:${originalId}`,
    runtime,
    agentSlug: null,
    title,
    createdAt: sharedAt,
    lastUsedAt: sharedAt,
    // FIX 4 — session: real turn_count from backend; war-room/chat: 0
    // so cards that don't show turnCount (WarRoomCard) are unaffected,
    // and ChatCard with 0 omits the "N msgs" counter cleanly.
    turnCount: resolvedTurnCount,
    // FIX 4 — war-room: real seat runtimes so WarRoomCard's
    // participantCount (= runtimesUsed.length) reflects real data.
    runtimesUsed,
    agentsUsed: [],
    totalCostUsd: null,
    lastAssistantPreview: null,
    // Part C — shared snapshots are closed conversations.
    status: "closed",
    closedAt: sharedAt,
    autoTitle: cardTitle ?? snapshotString(snapshot, "autoTitle"),
    summary,
    tags,
    projectId: null,
    projectName: null,
    category,
    team: teamName,
    coordinatorRuntime,
    humanComment: null,
    anchorRuntime,
    rowKind,
    sharedAt,
    sharedByLabel,
    sharedTeamId: teamId,
    sharedTeamName: teamName,
  };
}

// Part D/E — carries team-share annotation for the owner-dedupe and
// the teamShare prop on the rich cards.
//
// FIX 3 — one conversation can be shared into multiple teams; we now
// store a TeamShareInfo per (teamId, originalId) pair, keyed by
// "teamId:rowKind:originalId" in the shareInfoMap.  The per-local-row
// annotation (localRowShareAnnotations) stores the FIRST share found
// for convenience (card shows one team badge + "(+N)" when N>1).
interface TeamShareInfo {
  teamId: string;
  teamName: string;
  sharedByLabel: string | null;
  isOwner: boolean;
  members: { userId: string; name: string | null; email: string }[];
  originalId: string;
  rowKind: "team_shared_session" | "team_shared_war_room" | "team_shared_chat";
}

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
  // FIX 2 — set of local row ids that are annotated with a team share
  // (owner sees their local card under the Team filter). Passed through
  // from SessionsList so the predicate can admit annotated local rows.
  annotatedLocalIds?: Set<string>,
): boolean {
  if (skip !== "kind") {
    if (f.kind === "sessions" && s.rowKind !== "session") return false;
    if (f.kind === "single_runs" && s.rowKind !== "single_run") return false;
    if (f.kind === "war_rooms" && s.rowKind !== "war_room") return false;
    if (f.kind === "chats" && s.rowKind !== "chat") return false;
    // teamfilter (#1) — "team" scopes to cloud-shared rows; every other
    // kind excludes them so a shared row never leaks into "all" etc.
    //
    // FIX 2 — ALSO admit local rows that have a teamShare annotation:
    // when the owner has shared a war_room/session/chat, we suppress the
    // duplicate shared row and show the annotated local card instead.
    // Without this exception the owner sees ZERO cards for a shared+local
    // item (shared row dropped by dedupe, local row excluded here).
    if (f.kind === "team" && !TEAM_SHARED_KINDS.has(s.rowKind)) {
      // Allow annotated local rows through.
      if (!annotatedLocalIds?.has(s.id)) return false;
    }
    if (f.kind !== "team" && TEAM_SHARED_KINDS.has(s.rowKind)) return false;
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
  annotatedLocalIds?: Set<string>,
): SessionListRow[] {
  const trimmed = query.trim().toLowerCase();
  const tokens = trimmed.length === 0 ? [] : trimmed.split(/\s+/);
  const f: FilterFields = { status, kind, category, team, tag };
  return sessions.filter((s) => {
    if (!rowMatchesFilters(s, f, undefined, annotatedLocalIds)) return false;
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
  // teamfilter (#1) — a cloud-shared row. Carries the synthetic
  // `shared:<teamId>:<originalId>` id and which shared kind it is so the
  // placeholder (and #6's real view) can route. Read-only.
  | {
      kind: "team_shared";
      id: string;
      sharedKind: "session" | "war_room" | "chat";
    }
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

  const teamSharedQ = useQuery<{ rows: SessionListRow[]; shareInfoMap: Map<string, TeamShareInfo> }>({
    queryKey: ["team-shared-rows"],
    enabled: kindFilter === "team",
    staleTime: 30_000,
    queryFn: async () => {
      const teams = await getTeams().catch(() => []);
      const shareInfoMap = new Map<string, TeamShareInfo>();
      const perTeam = await Promise.all(
        teams.map(async (t) => {
          const [sessions, warRooms, chats, members] = await Promise.all([
            getSharedSessions(t.id).catch(() => []),
            getSharedWarRooms(t.id).catch(() => []),
            getSharedChats(t.id).catch(() => []),
            getTeamMembers(t.id).catch(() => [] as TeamMemberSimple[]),
          ]);
          const memberList = members.map((m) => ({
            userId: m.user_id,
            name: m.name,
            email: m.email,
          }));
          // FIX 3 — key the shareInfoMap by "teamId:rowKind:originalId" so
          // the same conversation shared into two different teams gets two
          // distinct entries instead of the second overwriting the first.
          const rows: SessionListRow[] = [
            ...sessions.map((x) => {
              const row = teamSharedRow({
                teamId: t.id,
                teamName: t.name,
                rowKind: "team_shared_session",
                originalId: x.session_id,
                sharedByUserId: x.shared_by_user_id,
                sharedAt: x.shared_at,
                snapshot: x.snapshot,
                cardTitle: x.card_title,
                richSummary: x.summary,
                richTags: x.tags,
                richCategory: x.category,
                coordinator: x.coordinator,
                sharedByName: x.shared_by_name,
                sharedByEmail: x.shared_by_email,
                richRuntime: x.runtime,
                // FIX 4 — sessions carry turn_count from the list endpoint.
                turnCount: x.turn_count ?? null,
              });
              // FIX 3 — key includes teamId to prevent cross-team collision.
              shareInfoMap.set(`${t.id}:team_shared_session:${x.session_id}`, {
                teamId: t.id,
                teamName: t.name,
                sharedByLabel: row.sharedByLabel ?? null,
                isOwner: false, // resolved in Part E
                members: memberList,
                originalId: x.session_id,
                rowKind: "team_shared_session",
              });
              return row;
            }),
            ...warRooms.map((x) => {
              const row = teamSharedRow({
                teamId: t.id,
                teamName: t.name,
                rowKind: "team_shared_war_room",
                originalId: x.war_room_id,
                sharedByUserId: x.shared_by_user_id,
                sharedAt: x.shared_at,
                snapshot: x.snapshot,
                cardTitle: x.card_title,
                richSummary: x.summary,
                richTags: x.tags,
                richCategory: x.category,
                coordinator: x.coordinator,
                sharedByName: x.shared_by_name,
                sharedByEmail: x.shared_by_email,
                // FIX 4 — war-rooms: real seat metadata from list endpoint.
                seatCount: x.seat_count ?? null,
                seatRuntimes: x.seat_runtimes ?? null,
              });
              // FIX 3 — key includes teamId.
              shareInfoMap.set(`${t.id}:team_shared_war_room:${x.war_room_id}`, {
                teamId: t.id,
                teamName: t.name,
                sharedByLabel: row.sharedByLabel ?? null,
                isOwner: false,
                members: memberList,
                originalId: x.war_room_id,
                rowKind: "team_shared_war_room",
              });
              return row;
            }),
            ...chats.map((x) => {
              const row = teamSharedRow({
                teamId: t.id,
                teamName: t.name,
                rowKind: "team_shared_chat",
                originalId: x.chat_thread_id,
                sharedByUserId: x.shared_by_user_id,
                sharedAt: x.shared_at,
                snapshot: x.snapshot,
                cardTitle: x.card_title,
                richSummary: x.summary,
                richTags: x.tags,
                richCategory: x.category,
                coordinator: x.coordinator,
                sharedByName: x.shared_by_name,
                sharedByEmail: x.shared_by_email,
                richRuntime: x.runtime,
                anchorRt: x.runtime, // chats: runtime is the anchor
                // FIX 4 — chats: leave turnCount absent (0); ChatCard
                // renders "0 msgs" which we suppress below.
              });
              // FIX 3 — key includes teamId.
              shareInfoMap.set(`${t.id}:team_shared_chat:${x.chat_thread_id}`, {
                teamId: t.id,
                teamName: t.name,
                sharedByLabel: row.sharedByLabel ?? null,
                isOwner: false,
                members: memberList,
                originalId: x.chat_thread_id,
                rowKind: "team_shared_chat",
              });
              return row;
            }),
          ];
          return rows;
        }),
      );
      const rows = perTeam
        .flat()
        .sort((a, b) => (b.sharedAt ?? "").localeCompare(a.sharedAt ?? ""));
      return { rows, shareInfoMap };
    },
  });
  const teamRows = teamSharedQ.data?.rows ?? [];
  const teamShareInfoMap = teamSharedQ.data?.shareInfoMap ?? new Map<string, TeamShareInfo>();

  // FIX 1/3 — helper: derive the shareInfoMap lookup key from a shared row's
  // synthetic id ("shared:<teamId>:<originalId>") and its rowKind.
  // The map is now keyed by "teamId:rowKind:originalId" (FIX 3).
  function shareInfoKey(row: SessionListRow): string {
    // id shape: "shared:<teamId>:<originalId>"
    const [, teamId, ...rest] = row.id.split(":");
    const originalId = rest.join(":");
    return `${teamId}:${row.rowKind}:${originalId}`;
  }

  // FIX 1 — The old code built localToShareMap from teamShareInfoMap entries
  // (the shared rows themselves) then checked `!localToShareMap.has(localKey)`
  // — since the map was populated from the same set, the predicate was ALWAYS
  // false and every shared row was dropped → empty Team feed.
  //
  // Correct approach: source the key-set from the LOCAL rows (clusteredData).
  // We compute this lazily below, after clusteredData is available.
  // Placeholder — will be populated after clusteredData is known.
  const localKeys = new Set<string>();

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
  // v2.10.0 PR-1 (UI) — collapse consecutive same-prompt single_runs
  // into eval_cluster synthetic rows. Closes the "Runs tab is drowning
  // in 150 identical SINGLE RUN cards from one methodology eval" bug.
  // Applied BEFORE filtering so the eval_cluster row's metadata
  // (runtime, category, etc) drives the filter chips — otherwise
  // filtering would hide the cluster but show its members.
  const clusteredData = nonEmptyData
    ? clusterEvalRuns(nonEmptyData)
    : undefined;
  // Part E — FIX 1: Build the local key-set from clusteredData (local rows),
  // NOT from teamShareInfoMap.  A local key has the shape
  // "localRowKind:localRowId" where localRowKind is the non-prefixed kind
  // (war_room / session / chat).  We compare it against every share entry's
  // "localKind:originalId" to decide whether the owner has a local copy.
  //
  // FIX 2 — also populate localRowShareAnnotations: the owner's local card
  // gets a teamShare annotation so it survives the "team" kind filter and
  // renders with the TEAM badge.  The corresponding shared row is suppressed
  // (dedupe keeps exactly one card).
  //
  // Keyed by local row id for O(1) lookup at render time.
  const localRowShareAnnotations = new Map<string, TeamShareInfo>();
  if (clusteredData) {
    for (const row of clusteredData) {
      // Only local (non-shared) rows can have a local copy.
      if (TEAM_SHARED_KINDS.has(row.rowKind)) continue;
      const localKey = `${row.rowKind}:${row.id}`;
      localKeys.add(localKey);
    }
    // Now walk every share entry; if the owner has a matching local row,
    // annotate that local row so it gets a TEAM badge under the Team filter.
    for (const [, info] of teamShareInfoMap) {
      const localKind =
        info.rowKind === "team_shared_war_room"
          ? "war_room"
          : info.rowKind === "team_shared_session"
            ? "session"
            : "chat";
      const localKey = `${localKind}:${info.originalId}`;
      if (localKeys.has(localKey)) {
        // Owner has local copy — annotate the local row.
        // If already annotated (shared into >1 team), keep the first
        // annotation; the card will pick it up and can show "+N" later.
        if (!localRowShareAnnotations.has(info.originalId)) {
          localRowShareAnnotations.set(info.originalId, { ...info, isOwner: true });
        }
      }
    }
  }

  // FIX 1 — dedupe: keep a shared row ONLY when the owner does NOT have a
  // local copy (i.e. the local key is absent from localKeys).  Recipients
  // (no local copy) always see the shared row.  Owners see the annotated
  // local row instead (one card, one badge).
  const deduplicatedTeamRows = teamRows.filter((r) => {
    const info = teamShareInfoMap.get(shareInfoKey(r));
    if (!info) return true; // no info → keep (defensive)
    const localKind =
      info.rowKind === "team_shared_war_room"
        ? "war_room"
        : info.rowKind === "team_shared_session"
          ? "session"
          : "chat";
    const localKey = `${localKind}:${info.originalId}`;
    // Drop this shared row only when a matching LOCAL row exists (owner).
    // Recipients (no local row) always keep it.
    return !localKeys.has(localKey);
  });

  // teamfilter (#1) — merge cloud-shared rows into the local feed when the
  // Team chip is active. deduplicatedTeamRows is pre-sorted shared_at DESC
  // and prepended; duplicate rows where the owner has the local copy are
  // filtered out in favor of the annotated local card.
  const sourceRows =
    clusteredData && kindFilter === "team"
      ? [...deduplicatedTeamRows, ...clusteredData]
      : clusteredData;
  // FIX 2 — build the annotated-ids set once for O(1) lookup in filters.
  const annotatedLocalIdSet = new Set(localRowShareAnnotations.keys());

  const filteredSessions = sourceRows
    ? (() => {
        const metaMatched = filterSessions(
          sourceRows,
          searchQuery,
          statusFilter,
          kindFilter,
          categoryFilter,
          teamFilter,
          tagFilter,
          annotatedLocalIdSet,
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
        return sourceRows.filter((s) => {
          if (!rowMatchesFilters(s, f, undefined, annotatedLocalIdSet)) return false;
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
  // teamfilter — render the real cross-machine transcript for a cloud-shared
  // row. The cloud detail endpoint now returns the snapshot (ato-cloud #29),
  // so SharedDetailView replaces the old read-only placeholder. id shape:
  // `shared:<teamId>:<originalId>`; `war_room` → `war-room` for the
  // SharedResourceKind contract.
  if (openSelection?.kind === "team_shared") {
    const [, sharedTeamId, ...rest] = openSelection.id.split(":");
    const originalId = rest.join(":");
    const resourceKind: SharedResourceKind =
      openSelection.sharedKind === "war_room"
        ? "war-room"
        : openSelection.sharedKind;
    return (
      <SharedDetailView
        resourceKind={resourceKind}
        teamId={sharedTeamId}
        resourceId={originalId}
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
                ["team", "Team"],
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
                          : k === "chats"
                            ? rowsForKindCount.filter((row) => row.rowKind === "chat").length
                            // teamfilter (#1) — local rows never include
                            // shared kinds; the Team count comes from the
                            // cloud query (0 until the chip is first picked).
                            : teamRows.length;
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
          {/* 2026-06-14 fix — status chips (All / Open / Closed) were
              lost in an IA refactor; the filter state existed but no
              UI control flipped it. Mirrors the kind-chip pattern and
              uses the same counting strategy via filterFields. Single-
              runs have no lifecycle so they're excluded when Open or
              Closed is selected. */}
          <div className="flex items-center gap-2 text-xs">
            {(() => {
              const statusChips: { key: StatusFilter; label: string }[] = [
                { key: "all", label: "All" },
                { key: "open", label: "Open" },
                { key: "closed", label: "Closed" },
              ];
              return statusChips.map(({ key, label }) => {
                // Count rows that would be visible if statusFilter === key,
                // holding the other filters constant. The actual filter
                // logic is at line ~140 (status: 'open' / 'closed' only
                // matches rows with that lifecycle; single_runs are
                // excluded from those buckets).
                const count = nonEmptyData!.filter((row) => {
                  if (key === "open" && row.status !== "open") return false;
                  if (key === "closed" && row.status !== "closed") return false;
                  return true;
                }).length;
                return (
                  <button
                    key={key}
                    onClick={() => setStatusFilter(key)}
                    className={cn(
                      "px-2 py-1 rounded-md border transition-colors",
                      statusFilter === key
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
          {/* v2.7.13 — lifecycle filter row REMOVED. Will dogfood
              2026-05-21: it was redundant with the kind chips above
              (All / Sessions / Single runs / War rooms / Chats) once
              war-rooms + chats became closeable in their own surface.
              Filtering by lifecycle is best done from the conversation
              detail (click in → see Closed badge) or via the kind chip
              (Sessions → most are open, only a few are closed). The
              "X of Y shown" indicator survives via the breakdown row
              below when a filter narrows the list. */}
          {(searchQuery ||
            statusFilter !== "all" ||
            kindFilter !== "all" ||
            categoryFilter !== null ||
            teamFilter !== null ||
            tagFilter !== null) && (
            <div className="flex items-center text-xs">
              <span className="text-cs-muted ml-auto">
                {filteredSessions.length} of {nonEmptyData!.length} shown
              </span>
            </div>
          )}
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
                {nonEmptyData!.filter((r) => r.rowKind === "chat").length} chats.
              </div>
            )}
        </div>
      )}

      {sessionsQ.isLoading ||
      (kindFilter === "team" && teamSharedQ.isLoading) ? (
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
        kindFilter === "team" ? (
          // teamfilter (#1) — distinct empty state for the Team feed so a
          // free-tier user (no teams → no rows) or a team with nothing
          // shared yet doesn't read it as "search returned nothing."
          <div className="text-center py-12 text-cs-muted">
            <MessagesSquare size={36} className="mx-auto mb-3 opacity-50" />
            <p>Nothing shared with your teams yet.</p>
            <p className="text-xs mt-2 max-w-md mx-auto">
              Conversations a teammate shares into a team workspace show up
              here. Share one from any conversation's “Share with team” menu.
            </p>
          </div>
        ) : (
          <div className="text-center py-12 text-cs-muted">
            <Search size={36} className="mx-auto mb-3 opacity-50" />
            <p>No sessions match your search.</p>
            <p className="text-xs mt-2">
              Try a different word, or clear the filter to see all{" "}
              {nonEmptyData!.length} sessions.
            </p>
          </div>
        )
      ) : (
        <div className="space-y-2">
          {filteredSessions.map((s) => {
            // 2026-05-19 elegance push — the four card variants moved to
            // SessionCards/ ({Chat,WarRoom,SingleRun,Session}Card.tsx).
            // Pick by rowKind; chat/war-room/single-run render before the
            // default session path so type-narrowing flows cleanly.
            if (s.rowKind === "chat") {
              const ownerShareInfo = localRowShareAnnotations.get(s.id);
              return (
                <ChatCard
                  key={s.id}
                  session={s}
                  onOpen={() => setOpenSelection({ kind: "chat", id: s.id })}
                  teamShare={ownerShareInfo ? {
                    teamName: ownerShareInfo.teamName,
                    sharedByLabel: null,
                    isOwner: true,
                    members: ownerShareInfo.members,
                  } : undefined}
                />
              );
            }
            // teamfilter (#1) — cloud-shared rows. Part D: route through
            // rich card variants (WarRoomCard / ChatCard / SessionCard) with
            // a teamShare annotation banner instead of the old TeamSharedCard.
            if (
              s.rowKind === "team_shared_session" ||
              s.rowKind === "team_shared_war_room" ||
              s.rowKind === "team_shared_chat"
            ) {
              const sharedKind =
                s.rowKind === "team_shared_session"
                  ? "session"
                  : s.rowKind === "team_shared_war_room"
                    ? "war_room"
                    : "chat";
              // FIX 3 — map is now keyed by "teamId:rowKind:originalId";
              // use shareInfoKey() to derive the correct key from the row.
              const info = teamShareInfoMap.get(shareInfoKey(s));
              const teamShareProp = info
                ? {
                    teamName: info.teamName,
                    sharedByLabel: info.sharedByLabel,
                    isOwner: info.isOwner,
                    members: info.members,
                  }
                : {
                    teamName: s.sharedTeamName ?? null,
                    sharedByLabel: s.sharedByLabel ?? null,
                    isOwner: false,
                    members: [],
                  };
              const openHandler = () =>
                setOpenSelection({ kind: "team_shared", id: s.id, sharedKind });
              if (s.rowKind === "team_shared_war_room") {
                return (
                  <WarRoomCard
                    key={s.id}
                    session={s}
                    onOpen={openHandler}
                    teamShare={teamShareProp}
                  />
                );
              }
              if (s.rowKind === "team_shared_chat") {
                return (
                  <ChatCard
                    key={s.id}
                    session={s}
                    onOpen={openHandler}
                    teamShare={teamShareProp}
                  />
                );
              }
              // team_shared_session
              return (
                <SessionCard
                  key={s.id}
                  session={s}
                  onOpen={openHandler}
                  tagFilter={tagFilter}
                  setTagFilter={setTagFilter}
                  teamShare={teamShareProp}
                />
              );
            }
            if (s.rowKind === "war_room") {
              const ownerShareInfo = localRowShareAnnotations.get(s.id);
              return (
                <WarRoomCard
                  key={s.id}
                  session={s}
                  onOpen={() =>
                    setOpenSelection({ kind: "war_room", id: s.id })
                  }
                  teamShare={ownerShareInfo ? {
                    teamName: ownerShareInfo.teamName,
                    sharedByLabel: null,
                    isOwner: true,
                    members: ownerShareInfo.members,
                  } : undefined}
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
            // v2.10.0 PR-1 (UI) — collapsed cluster of same-prompt
            // single_runs. Renders one card with an aggregate; expand
            // reveals individual SingleRunCard children that drill
            // into their own receipts.
            if (s.rowKind === "eval_cluster") {
              return (
                <EvalClusterCard
                  key={s.id}
                  session={s}
                  onOpenMember={(memberId) =>
                    setOpenSelection({ kind: "single_run", id: memberId })
                  }
                />
              );
            }
            const ownerShareInfoSess = localRowShareAnnotations.get(s.id);
            return (
              <SessionCard
                key={s.id}
                session={s}
                onOpen={() => setOpenSelection({ kind: "session", id: s.id })}
                tagFilter={tagFilter}
                setTagFilter={setTagFilter}
                teamShare={ownerShareInfoSess ? {
                  teamName: ownerShareInfoSess.teamName,
                  sharedByLabel: null,
                  isOwner: true,
                  members: ownerShareInfoSess.members,
                } : undefined}
              />
            );
          })}
        </div>
      )}
    </div>
  );
}
