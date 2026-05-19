import { useState, useRef, useEffect, useMemo, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Send,
  Bot,
  X,
  Loader2,
  ChevronUp,
  ChevronDown,
  Sparkles,
  Terminal,
  AlertCircle,
  Cpu,
  Server,
  Globe,
  MessageSquarePlus,
  MessageSquare,
  Swords,
  History,
  Paperclip,
  Trash2,
  FolderKanban,
  Check,
  Network,
  ArrowRight,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { listAgents, parseMemoryPolicy, type Agent } from "@/lib/agents";
import {
  promptAgentWithHistoryStream,
  promptAgentStream,
  type AgentMessage,
} from "@/lib/agentVariables";
import {
  appendChatMessage,
  createChatThread,
  defaultThreadTitle,
  deleteChatThread,
  getChatMessages,
  listChatThreads,
  renameChatThread,
  setChatThreadAgent,
  type ChatMessage,
  type ChatThread,
} from "@/lib/chatThreads";
import { useProjectStore } from "@/stores/useProjectStore";
import { useDemoStore } from "@/stores/useDemoStore";
import { useUiStore } from "@/stores/useUiStore";
import { listAgentGroups, dispatchToGroup, type AgentGroup } from "@/lib/agentGroups";
import { uploadAgentTrace, summarizePrompt } from "@/lib/agentTraceUpload";
import { estimateUsage } from "@/lib/pricing";
import type { AgentRuntime } from "@/components/cron/types";
import { RUNTIME_REGISTRY, type RuntimeId } from "@/lib/runtimes";
import ApprovalDialog, { extractSkillFromResponse } from "../ApprovalDialog";
import MarkdownContent from "../MarkdownContent";

import {
  AvailableRuntimeRow,
  MAX_ATTACHMENT_BYTES,
  RUNTIME_META,
  RUNTIME_OPTIONS,
  formatThreadAge,
  isProbablyBinary,
  messagesToAgentHistory,
  simulateMock,
  stitchThreadIntoPrompt,
} from "./_helpers";
import { ChatRow } from "./ChatRow";
import { RoomTypePicker } from "./RoomTypePicker";
import { useEnabledRuntimes } from "@/lib/enabledRuntimes";

const isTauri =
  typeof window !== "undefined" &&
  ("__TAURI__" in window || "__TAURI_INTERNALS__" in window);

// PromptBar/_helpers.ts owns RUNTIME_META, RUNTIME_OPTIONS,
// AvailableRuntimeRow, MAX_ATTACHMENT_BYTES, simulateMock,
// isProbablyBinary, messagesToAgentHistory, stitchThreadIntoPrompt.
// PromptBar/ChatRow.tsx owns the per-row JSX. This file is the
// orchestrator: state, dispatch, layout.

export default function PromptBar() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const activeProject = useProjectStore((s) => s.activeProject);
  const activeProjectId = activeProject?.id ?? null;

  // Path B (2026-05-18) — bottom pane is a multi-launcher. The
  // "+ New conversation" affordance opens three kinds: quick chat
  // (current behavior, stays here), multi-turn session (navigates to
  // the Sessions tab with NewSessionModal auto-opened), and war room
  // (opens the FirstChatWizard from any surface). Group dispatch is
  // already accessible via the existing agent picker — no separate
  // launcher option needed.
  const setSection = useUiStore((s) => s.setSection);
  const setSubTab = useUiStore((s) => s.setSubTab);
  const openNewSession = useUiStore((s) => s.openNewSession);
  const openFirstChat = useUiStore((s) => s.openFirstChat);

  const [input, setInput] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [expanded, setExpanded] = useState(false);
  // Live streaming buffer — what the runtime has emitted so far for the
  // in-flight dispatch. Cleared on done/error.
  const [streamingText, setStreamingText] = useState("");
  const [runtime, setRuntime] = useState<AgentRuntime>("claude");
  // 2026-05-19 elegance war-room call (claude + codex unanimous):
  // collapse the 3 independent picker booleans into one discriminated
  // union. Closes a latent bug — multiple `fixed inset-0 z-30`
  // backdrops could stack when 2 popovers were open at once, and the
  // wrong backdrop caught the close click. With a single source of
  // truth only one popover is ever open. Each toggle/open also
  // implicitly closes the others.
  const [openPicker, setOpenPicker] = useState<
    "runtime" | "agent" | "thread" | null
  >(null);
  const showRuntimePicker = openPicker === "runtime";
  const setShowRuntimePicker = (next: boolean | ((v: boolean) => boolean)) => {
    const nextValue =
      typeof next === "function" ? next(showRuntimePicker) : next;
    setOpenPicker(nextValue ? "runtime" : null);
  };
  // v2.7.7 — shared enabled-runtimes hook (was a local useState +
  // useEffect that re-fetched on every PromptBar mount). React Query
  // now caches by key `["enabled-runtimes"]` so PromptBar +
  // FirstChatWizard hit one fetch and invalidate together when a key
  // is added in Settings.
  const enabledRuntimesQuery = useEnabledRuntimes();
  const availableRuntimes: AvailableRuntimeRow[] | null =
    enabledRuntimesQuery.data ?? null;
  const [agentId, setAgentId] = useState<string | null>(null);
  const showAgentPicker = openPicker === "agent";
  const setShowAgentPicker = (next: boolean | ((v: boolean) => boolean)) => {
    const nextValue = typeof next === "function" ? next(showAgentPicker) : next;
    setOpenPicker(nextValue ? "agent" : null);
  };
  // Group dispatch — when set, prompt routes through the group's router
  // instead of going to a single agent. Mutually exclusive with agentId.
  const [groupSlug, setGroupSlug] = useState<string | null>(null);
  const showThreadPicker = openPicker === "thread";
  const setShowThreadPicker = (next: boolean | ((v: boolean) => boolean)) => {
    const nextValue =
      typeof next === "function" ? next(showThreadPicker) : next;
    setOpenPicker(nextValue ? "thread" : null);
  };
  const [currentThreadId, setCurrentThreadId] = useState<string | null>(null);
  const [renamingThread, setRenamingThread] = useState<{ id: string; title: string } | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const currentRuntime = RUNTIME_OPTIONS.find((r) => r.id === runtime)!;
  const RuntimeIcon = currentRuntime.icon;

  // ── Threads ────────────────────────────────────────────────────────────

  const threadsQuery = useQuery({
    queryKey: ["chat-threads", activeProjectId],
    queryFn: () => listChatThreads({ projectId: activeProjectId, limit: 50 }),
    enabled: isTauri,
    staleTime: 10_000,
  });

  // Pick the most-recent thread on first load. If none exist, currentThreadId
  // stays null and we'll auto-create one on first send.
  useEffect(() => {
    if (!isTauri) return;
    if (currentThreadId) return;
    if (threadsQuery.isLoading) return;
    // v2.1.7+ — honor a ⌘K palette handoff if the user clicked a
    // thread search hit. Take the hint once, then wipe it.
    try {
      const hinted = localStorage.getItem("ato.activeChatThreadId");
      if (hinted) {
        localStorage.removeItem("ato.activeChatThreadId");
        const exists = threadsQuery.data?.some((t) => t.id === hinted);
        if (exists) {
          setCurrentThreadId(hinted);
          return;
        }
      }
    } catch {
      // localStorage unavailable — fall through to default behavior.
    }
    const first = threadsQuery.data?.[0];
    if (first) setCurrentThreadId(first.id);
  }, [currentThreadId, threadsQuery.data, threadsQuery.isLoading]);

  // v2.1.7+ — also re-check the palette handoff hint while the chat
  // pane is already mounted. The first-load hook only runs once;
  // without this, a ⌘K thread click while already on Home wouldn't
  // switch threads.
  useEffect(() => {
    if (!isTauri) return;
    const onStorage = () => {
      try {
        const hinted = localStorage.getItem("ato.activeChatThreadId");
        if (!hinted) return;
        localStorage.removeItem("ato.activeChatThreadId");
        if (threadsQuery.data?.some((t) => t.id === hinted)) {
          setCurrentThreadId(hinted);
        }
      } catch {
        // best-effort
      }
    };
    // Same-window writes don't fire `storage` events, so we also poll
    // briefly after CommandPalette closes. Cheap: short interval, only
    // looking up one localStorage key.
    const id = window.setInterval(onStorage, 250);
    return () => window.clearInterval(id);
  }, [threadsQuery.data]);

  // Drop current thread if it's not in the active project's filtered list.
  // (Switching projects shouldn't strand you on a foreign thread.)
  useEffect(() => {
    if (!currentThreadId || !threadsQuery.data) return;
    if (!threadsQuery.data.some((t) => t.id === currentThreadId)) {
      setCurrentThreadId(threadsQuery.data[0]?.id ?? null);
    }
  }, [currentThreadId, threadsQuery.data]);

  const messagesQuery = useQuery({
    queryKey: ["chat-messages", currentThreadId],
    queryFn: () => (currentThreadId ? getChatMessages(currentThreadId) : Promise.resolve([])),
    enabled: !!currentThreadId && isTauri,
    staleTime: 5_000,
  });
  const messages = messagesQuery.data ?? [];

  const currentThread = useMemo(
    () => threadsQuery.data?.find((t) => t.id === currentThreadId) ?? null,
    [threadsQuery.data, currentThreadId]
  );

  // When the thread carries a sticky agent_id, hydrate the picker on switch.
  useEffect(() => {
    if (!currentThread) return;
    setAgentId(currentThread.agentId ?? null);
  }, [currentThread]);

  // ── Agents (filtered to runtime) ───────────────────────────────────────

  const { data: runtimeAgents = [] } = useQuery({
    queryKey: ["promptbar-agents", runtime],
    queryFn: () => listAgents({ runtime: runtime as Agent["runtime"] }),
    staleTime: 30_000,
    enabled: isTauri,
  });

  const { data: runtimeGroups = [] } = useQuery({
    queryKey: ["promptbar-groups", runtime],
    queryFn: () => listAgentGroups(runtime as AgentRuntime),
    staleTime: 30_000,
    enabled: isTauri,
  });

  const selectedAgent = useMemo(
    () => runtimeAgents.find((a) => a.id === agentId) ?? null,
    [runtimeAgents, agentId]
  );

  const selectedGroup = useMemo<AgentGroup | null>(
    () => runtimeGroups.find((g) => g.slug === groupSlug) ?? null,
    [runtimeGroups, groupSlug]
  );

  // Drop persisted agent if its runtime no longer matches.
  useEffect(() => {
    if (agentId && runtimeAgents.length > 0 && !selectedAgent) {
      setAgentId(null);
    }
  }, [agentId, runtimeAgents, selectedAgent]);

  // Persist agent selection as the thread's sticky default.
  const stickAgentToThread = useCallback(
    async (id: string | null) => {
      if (!currentThreadId) return;
      try {
        await setChatThreadAgent(currentThreadId, id);
      } catch {
        // Sticky default is convenience — don't block the UI.
      }
      void queryClient.invalidateQueries({ queryKey: ["chat-threads", activeProjectId] });
    },
    [currentThreadId, queryClient, activeProjectId]
  );

  // ── Demo mode plumbing ─────────────────────────────────────────────────

  const demoIsPlaying = useDemoStore((s) => s.isPlaying);
  const demoPendingRuntime = useDemoStore((s) => s.pendingRuntime);
  const demoPendingInputText = useDemoStore((s) => s.pendingInputText);
  const demoPendingSubmit = useDemoStore((s) => s.pendingSubmit);
  const demoPendingNewThread = useDemoStore((s) => s.pendingNewThread);
  const demoPendingSelectAgentSlug = useDemoStore((s) => s.pendingSelectAgentSlug);
  const demoPendingSelectGroupSlug = useDemoStore((s) => s.pendingSelectGroupSlug);
  const demoNotifyDispatchComplete = useDemoStore((s) => s.notifyDispatchComplete);

  // While the demo is playing, mirror its input text into the field.
  useEffect(() => {
    if (demoIsPlaying) {
      setInput(demoPendingInputText);
    }
  }, [demoIsPlaying, demoPendingInputText]);

  // Demo asked for a runtime swap → swap.
  useEffect(() => {
    if (demoPendingRuntime) {
      setRuntime(demoPendingRuntime);
    }
  }, [demoPendingRuntime]);

  // Demo asked us to pick an agent by slug → look it up in the runtime list
  // and set the agent picker. When the demo passes `null` (and is currently
  // playing) we DESELECT — that's how cross-runtime swaps work (the chat
  // panel goes from "agent X" → "no agent" so the runtime picker takes over).
  // The `demoIsPlaying` gate keeps the mount-time `null` from clearing a
  // user's manual selection.
  useEffect(() => {
    if (!demoIsPlaying) return;
    if (demoPendingSelectAgentSlug === null) {
      setAgentId(null);
      return;
    }
    const found = runtimeAgents.find((a) => a.slug === demoPendingSelectAgentSlug);
    if (found) {
      setAgentId(found.id);
      setGroupSlug(null);
    }
  }, [demoPendingSelectAgentSlug, runtimeAgents, demoIsPlaying]);

  // Demo asked us to pick a group → set the group picker. Same null-means-
  // deselect rule as the agent effect above; without this the chat panel
  // stayed routed through the previously-selected group even after the
  // demo issued `selectChatGroup: { slug: null }` (Beatriz feedback
  // 2026-05-07: summarize step kept dispatching through write-and-review
  // instead of going to a single Claude).
  useEffect(() => {
    if (!demoIsPlaying) return;
    if (demoPendingSelectGroupSlug === null) {
      setGroupSlug(null);
      return;
    }
    setGroupSlug(demoPendingSelectGroupSlug);
    setAgentId(null);
  }, [demoPendingSelectGroupSlug, demoIsPlaying]);

  // Demo asked for a new thread.
  useEffect(() => {
    if (demoPendingNewThread > 0 && isTauri) {
      void (async () => {
        const t = await createChatThread({
          title: "Demo · " + new Date().toLocaleTimeString(),
          projectId: activeProjectId,
          agentId: null,
        });
        setCurrentThreadId(t.id);
        setAgentId(null);
        setExpanded(true);
        void queryClient.invalidateQueries({ queryKey: ["chat-threads", activeProjectId] });
      })();
    }
    // We deliberately depend on the bumping counter, not deeper state.
  }, [demoPendingNewThread]);

  // Demo asked us to submit. The pendingSubmit counter only increments while
  // the demo is playing, so observing the count change is the trigger.
  const lastSeenSubmitRef = useRef(0);
  useEffect(() => {
    if (!demoIsPlaying) {
      lastSeenSubmitRef.current = demoPendingSubmit;
      return;
    }
    if (demoPendingSubmit > lastSeenSubmitRef.current) {
      lastSeenSubmitRef.current = demoPendingSubmit;
      // Fire handleSubmit on the next tick so the input state has settled.
      requestAnimationFrame(() => {
        const fakeEvent = { preventDefault: () => {} } as React.FormEvent;
        void handleSubmit(fakeEvent);
      });
    }
  }, [demoPendingSubmit, demoIsPlaying]);

  // When isLoading transitions from true → false during demo, signal the
  // runner that the dispatch is done so it can advance.
  const prevLoadingRef = useRef(false);
  useEffect(() => {
    const prev = prevLoadingRef.current;
    prevLoadingRef.current = isLoading;
    if (demoIsPlaying && prev && !isLoading) {
      demoNotifyDispatchComplete();
    }
  }, [isLoading, demoIsPlaying, demoNotifyDispatchComplete]);

  // ── Auto-scroll ────────────────────────────────────────────────────────
  // Scrolls on new messages AND while streaming so the bottom of the chat
  // follows the live-typing tokens. Also runs on isLoading flips so the
  // "thinking" placeholder is visible the moment it appears.

  useEffect(() => {
    if (expanded && messagesEndRef.current) {
      messagesEndRef.current.scrollIntoView({ behavior: "smooth", block: "end" });
    }
  }, [messages.length, expanded, streamingText, isLoading]);

  // ── Helpers ────────────────────────────────────────────────────────────

  /** Ensure a thread exists for the next send. Auto-titles from the first
   *  user message; carries the active project + sticky agent. */
  const ensureThread = useCallback(
    async (firstUserContent: string): Promise<ChatThread | null> => {
      if (currentThreadId) {
        return threadsQuery.data?.find((t) => t.id === currentThreadId) ?? null;
      }
      const newThread = await createChatThread({
        title: defaultThreadTitle(firstUserContent),
        projectId: activeProjectId,
        agentId,
      });
      setCurrentThreadId(newThread.id);
      void queryClient.invalidateQueries({ queryKey: ["chat-threads", activeProjectId] });
      return newThread;
    },
    [currentThreadId, threadsQuery.data, activeProjectId, agentId, queryClient]
  );

  const refetchMessages = useCallback(
    (threadId: string) => {
      void queryClient.invalidateQueries({ queryKey: ["chat-messages", threadId] });
    },
    [queryClient]
  );

  // ── Submit ─────────────────────────────────────────────────────────────

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!input.trim() || isLoading) return;

    const userContent = input.trim();
    setInput("");
    setExpanded(true);
    setIsLoading(true);

    if (!isTauri) {
      // Web mode: keep the simulated response, no persistence.
      setIsLoading(false);
      return;
    }

    try {
      const thread = await ensureThread(userContent);
      if (!thread) throw new Error("could-not-create-thread");

      // Persist the user message.
      await appendChatMessage({
        threadId: thread.id,
        role: "user",
        content: userContent,
        runtime,
        agentSlug: selectedAgent?.slug ?? null,
      });
      refetchMessages(thread.id);

      // Build the prompt + dispatch.
      const lower = userContent.toLowerCase();
      const isSkillRequest =
        lower.includes("skill") || lower.includes("create") || lower.includes("write");
      const prompt = isSkillRequest
        ? `IMPORTANT: You are running in --print mode without file write permissions. Do NOT attempt to create files or ask for permissions. Instead, return the complete file content in a markdown code block so the user can review and save it through the app. If asked to create a skill, return the full SKILL.md content with YAML frontmatter (name, description, allowed-tools) in a \`\`\`markdown code block.\n\nUser request: ${userContent}`
        : userContent;

      let response: string;
      let routedTo: string | null = null;
      let routingReason: string | null = null;
      // Sequential groups produce multiple messages — one per stage. We
      // collect them here and persist each as its own assistant bubble so
      // humans can follow the pipeline (and so each stage acts as a real
      // turn in the thread, which is how LLM-to-LLM relay works best).
      let pipelineStages: { agentSlug: string; runtime: string; response: string }[] = [];
      // v2.1.0+ — capture dispatch start time for the trace upload that
      // fires after every dispatch path. Without this, the no-agent and
      // agent-without-variables paths never uploaded to cloud, which
      // meant Compare/Pipelines panels stayed empty + replay had no
      // source data. Variables/hooks-attributed dispatches go through
      // agentVariables.ts which has its own upload; group dispatches
      // upload below; this captures the remaining two paths.
      const dispatchStartedAtIso = new Date().toISOString();
      const dispatchStartedAtMs = Date.now();
      setStreamingText("");
      try {
        // v2.3.26 Phase 6.x-C — API providers (MiniMax, Grok, ...)
        // take a non-streaming path: single HTTPS POST via the new
        // prompt_api_provider Tauri command. Skips agents / groups /
        // history (those are CLI-runtime concepts for now).
        //
        // MiniMax round-1 6.x-C: derive the list from
        // availableRuntimes instead of duplicating the backend
        // registry — single source of truth, no drift when a new
        // provider lands. Falls back to the static set in non-Tauri
        // contexts where availableRuntimes is null.
        const apiSlugs = availableRuntimes
          ? availableRuntimes.filter((r) => r.kind === "api").map((r) => r.slug)
          : ["minimax", "grok", "deepseek", "qwen", "openrouter"];
        if (apiSlugs.includes(runtime)) {
          if (!isTauri) {
            response = simulateMock(prompt);
          } else {
            const { invoke } = await import("@tauri-apps/api/core");
            const result = await invoke<{
              status: string;
              response: string | null;
              error_message: string | null;
              model: string;
              duration_ms: number;
            }>("prompt_api_provider", {
              runtime,
              prompt,
              model: null,
              agentSlug: selectedAgent?.slug ?? null,
            });
            if (result.status === "success" && result.response) {
              response = result.response;
            } else {
              throw new Error(
                result.error_message ?? `${runtime} dispatch failed`
              );
            }
          }
        } else if (selectedGroup) {
          // Group dispatch — router picks (routed) or pipeline runs all
          // children (sequential). Single round-trip; we still stitch
          // thread history so the dispatcher sees recent context.
          const history: AgentMessage[] = messagesToAgentHistory(messages);
          const stitched = stitchThreadIntoPrompt(history, prompt);
          const result = await dispatchToGroup({
            slug: selectedGroup.slug,
            prompt: stitched,
          });
          response = result.response;
          routedTo = result.routedTo;
          routingReason = result.routingReason;
          if (result.stages && result.stages.length > 1) {
            pipelineStages = result.stages.map((s) => ({
              agentSlug: s.agentSlug,
              runtime: s.runtime,
              response: s.response,
            }));
          }
          // v2.1.0 Phase 7 — Pipeline trace correlation. Emit one
          // trace per stage with a shared parent_run_id so the
          // pipeline visualizer can render Claude → Codex → Gemini
          // as a flow with per-stage timing + status. Single
          // dispatch (routed groups with 1 stage) gets the same
          // treatment so the UI is uniform.
          if (result.stages && result.stages.length > 0) {
            const parentRunId =
              typeof crypto !== "undefined" && "randomUUID" in crypto
                ? crypto.randomUUID()
                : `pipeline-${Date.now()}-${Math.random().toString(36).slice(2)}`;
            const promptSummary = summarizePrompt(prompt);
            for (const s of result.stages) {
              const startedAt =
                s.startedAt ?? new Date().toISOString();
              const durationMs = s.durationMs ?? 0;
              void uploadAgentTrace({
                agentSlug: s.agentSlug,
                runtime: s.runtime,
                startedAt,
                durationMs,
                ok: s.ok,
                error: s.error,
                source: "desktop:promptbar:pipeline",
                parentRunId,
                promptSummary,
                metadata: {
                  groupSlug: selectedGroup.slug,
                  routedTo: result.routedTo,
                  routingReason: result.routingReason,
                  stageIndex: result.stages.indexOf(s),
                  totalStages: result.stages.length,
                },
              });
            }
          }
        } else if (selectedAgent) {
          // Agent-attributed multi-turn streaming dispatch — full thread
          // history travels, plus the agent's variables / hooks / memory
          // policy / role models all fire.
          const history: AgentMessage[] = messagesToAgentHistory(messages);
          response = await promptAgentWithHistoryStream({
            agentId: selectedAgent.id,
            agentSlug: selectedAgent.slug,
            runtime,
            history,
            newPrompt: prompt,
            source: "desktop:promptbar:stream",
            onChunk: (text) => setStreamingText((prev) => prev + text),
          });
          // v2.1.0+ — agents with NO variables/hooks bypass the
          // agentVariables.ts upload path. Cover them here so every
          // single-agent dispatch lands a cloud trace.
          // v2.1.4+ — also estimate token usage + cost so Compare/
          // Cost recs/Replay panels show real numbers instead of "—".
          // Marked `costEstimated:true` in metadata so the UI can
          // render an "est." badge.
          {
            const usage = estimateUsage(runtime, selectedAgent.model ?? null, prompt, response);
            void uploadAgentTrace({
              agentSlug: selectedAgent.slug,
              runtime,
              startedAt: dispatchStartedAtIso,
              durationMs: Date.now() - dispatchStartedAtMs,
              ok: true,
              source: "desktop:promptbar:agent-stream",
              promptSummary: summarizePrompt(prompt),
              promptTokens: usage.promptTokens,
              responseTokens: usage.responseTokens,
              costUsd: usage.costUsd,
              metadata: {
                historyLength: history.length,
                streamed: true,
                costEstimated: true,
                modelUsed: usage.model,
              },
            });
          }
        } else {
          // No agent selected — but the thread is still a conversation.
          // Stitch the history into the prompt so cross-runtime swaps
          // mid-thread keep their context. The runtime sees one big prompt
          // with a framing instruction; this is the only honest way to do
          // multi-turn when we don't manage the runtime's session.
          const history: AgentMessage[] = messagesToAgentHistory(messages);
          const stitched = stitchThreadIntoPrompt(history, prompt);
          response = await promptAgentStream({
            runtime,
            prompt: stitched,
            onChunk: (text) => setStreamingText((prev) => prev + text),
          });
          // v2.1.0+ — no-agent path now uploads too. agent_slug uses
          // the runtime as a stable bucket so multiple no-agent
          // dispatches against the same runtime accumulate under one
          // entry in Compare/Pipelines instead of scattering across
          // different empty buckets.
          {
            const usage = estimateUsage(runtime, null, prompt, response);
            void uploadAgentTrace({
              agentSlug: runtime,
              runtime,
              startedAt: dispatchStartedAtIso,
              durationMs: Date.now() - dispatchStartedAtMs,
              ok: true,
              source: "desktop:promptbar:no-agent-stream",
              promptSummary: summarizePrompt(prompt),
              promptTokens: usage.promptTokens,
              responseTokens: usage.responseTokens,
              costUsd: usage.costUsd,
              metadata: {
                historyLength: history.length,
                streamed: true,
                noAgent: true,
                costEstimated: true,
                modelUsed: usage.model,
              },
            });
          }
        }
      } catch (dispatchErr) {
        // Upload a failure trace so the panels show ok_rate drops too,
        // not just successes. Re-throw so the outer catch handles UI.
        void uploadAgentTrace({
          agentSlug: selectedAgent?.slug ?? runtime,
          runtime,
          startedAt: dispatchStartedAtIso,
          durationMs: Date.now() - dispatchStartedAtMs,
          ok: false,
          error: dispatchErr instanceof Error ? dispatchErr.message : String(dispatchErr),
          source: selectedAgent
            ? "desktop:promptbar:agent-stream"
            : "desktop:promptbar:no-agent-stream",
          promptSummary: summarizePrompt(prompt),
          metadata: { streamed: true, noAgent: !selectedAgent },
        });
        throw dispatchErr;
      } finally {
        // Clear regardless of success/error so the placeholder doesn't
        // outlive the dispatch.
        setStreamingText("");
      }

      if (pipelineStages.length > 0) {
        // Sequential group: persist each stage as its own assistant bubble
        // so the conversation reads as Claude → Codex → … each with their
        // own runtime badge, a "via {group}" attribution, and a stage badge.
        // We deliberately stagger appends so the viewer SEES two messages
        // arrive — without the pause they blur together and auto-scroll
        // jumps straight to the bottom of the second one.
        for (let i = 0; i < pipelineStages.length; i++) {
          const stage = pipelineStages[i];
          const isLast = i === pipelineStages.length - 1;
          const detectedTools = Array.from(
            new Set(stage.response.match(/mcp__[a-z0-9_-]+__[a-z0-9_-]+/gi) ?? [])
          );
          const meta: Record<string, unknown> = {
            viaGroup: selectedGroup!.slug,
            routingReason,
            stagedFrom: pipelineStages[0].agentSlug,
            stageOf: pipelineStages.length,
            stageIndex: i,
          };
          if (detectedTools.length > 0) meta.toolsUsed = detectedTools;
          const appended = await appendChatMessage({
            threadId: thread.id,
            role: "assistant",
            content: stage.response,
            runtime: stage.runtime,
            agentSlug: stage.agentSlug,
            metadata: JSON.stringify(meta),
          });
          await refetchMessages(thread.id);

          // For non-final stages, scroll to the TOP of the just-appended
          // bubble (not the bottom) so the viewer sees a clear divider
          // before the next message arrives. Then dwell so the eye lands.
          // Anchor each stage's top to viewport top so the runtime badge +
          // stage pill are visible — this is the sequential-pipeline money
          // shot. Without this the auto-scroll-to-end hides the boundary
          // between stage 1 and stage 2.
          await new Promise((r) => setTimeout(r, 60)); // let DOM paint
          const el = document.querySelector<HTMLElement>(
            `[data-message-id="${appended.id}"]`
          );
          el?.scrollIntoView({ behavior: "smooth", block: "start" });
          if (!isLast) {
            // Demo dwell — give the viewer time to actually READ stage N's
            // output before stage N+1 lands. Previous 1500ms was too short:
            // Beatriz reported that the chat looked like ONLY the final
            // stage's LLM (Codex) had replied because Claude's bubble had
            // already scrolled out of view by the time Codex appeared.
            // 4000ms is enough for a glance at a 6–10 line code block.
            await new Promise((r) => setTimeout(r, 4000));
          }
        }
      } else {
        // Routed group OR single agent OR no agent: one message.
        const detectedTools = Array.from(
          new Set(response.match(/mcp__[a-z0-9_-]+__[a-z0-9_-]+/gi) ?? [])
        );
        const meta: Record<string, unknown> = {};
        if (routedTo) meta.routedTo = routedTo;
        if (routingReason) meta.routingReason = routingReason;
        if (selectedGroup) meta.viaGroup = selectedGroup.slug;
        if (detectedTools.length > 0) meta.toolsUsed = detectedTools;
        await appendChatMessage({
          threadId: thread.id,
          role: "assistant",
          content: response,
          runtime,
          agentSlug: routedTo ?? selectedAgent?.slug ?? null,
          metadata: Object.keys(meta).length > 0 ? JSON.stringify(meta) : null,
        });
        refetchMessages(thread.id);
      }
      void queryClient.invalidateQueries({ queryKey: ["chat-threads", activeProjectId] });
    } catch (err) {
      // Try to record the error in the thread, but don't loop if append fails.
      if (currentThreadId) {
        try {
          await appendChatMessage({
            threadId: currentThreadId,
            role: "error",
            content: err instanceof Error ? err.message : String(err),
            runtime,
          });
          refetchMessages(currentThreadId);
        } catch {
          // ignore
        }
      }
    } finally {
      setIsLoading(false);
    }
  };

  // ── File attachment ────────────────────────────────────────────────────

  const handleFile = async (file: File) => {
    if (!isTauri) return;
    if (file.size > MAX_ATTACHMENT_BYTES) {
      alert(
        t(
          "prompt.fileTooLarge",
          "File is too large ({{size}} bytes). Max {{max}} bytes.",
          { size: file.size, max: MAX_ATTACHMENT_BYTES }
        )
      );
      return;
    }
    let text: string;
    try {
      text = await file.text();
    } catch {
      alert(t("prompt.fileReadFailed", "Could not read file."));
      return;
    }
    if (isProbablyBinary(text)) {
      alert(t("prompt.fileBinaryRefused", "Binary files aren't supported as attachments."));
      return;
    }
    const thread = await ensureThread(`📎 ${file.name}`);
    if (!thread) return;
    const wrapped = `<attachment name="${file.name}">\n${text}\n</attachment>`;
    await appendChatMessage({
      threadId: thread.id,
      role: "attachment",
      content: wrapped,
      metadata: JSON.stringify({ filename: file.name, bytes: file.size }),
    });
    refetchMessages(thread.id);
    setExpanded(true);
  };

  const onPickFile = (e: React.ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0];
    if (f) void handleFile(f);
    e.target.value = "";
  };

  const onDropFile = async (e: React.DragEvent) => {
    e.preventDefault();
    const f = e.dataTransfer.files?.[0];
    if (f) await handleFile(f);
  };

  const onDragOver = (e: React.DragEvent) => {
    if (e.dataTransfer.types.includes("Files")) e.preventDefault();
  };

  // ── Thread actions ─────────────────────────────────────────────────────

  const newThread = async () => {
    const t = await createChatThread({
      title: defaultThreadTitle(""),
      projectId: activeProjectId,
      agentId,
    });
    setCurrentThreadId(t.id);
    void queryClient.invalidateQueries({ queryKey: ["chat-threads", activeProjectId] });
    setShowThreadPicker(false);
    setExpanded(false);
  };

  // Path B launchers — close the dropdown, route to the appropriate
  // surface. NewSession + WarRoom navigate away from the bottom pane
  // because their detail views live in the Sessions tab; QuickChat
  // (above) stays in place. Group dispatch is reached via the existing
  // agent picker so no fourth entry here.
  const launchNewSession = () => {
    setShowThreadPicker(false);
    setSection("runs");
    setSubTab("ato.subtab.runs", "sessions");
    openNewSession();
  };
  const launchWarRoom = () => {
    setShowThreadPicker(false);
    openFirstChat();
  };

  const removeThread = async (id: string) => {
    await deleteChatThread(id);
    if (currentThreadId === id) setCurrentThreadId(null);
    void queryClient.invalidateQueries({ queryKey: ["chat-threads", activeProjectId] });
  };

  const commitRename = async () => {
    if (!renamingThread) return;
    const trimmed = renamingThread.title.trim();
    if (trimmed) {
      await renameChatThread(renamingThread.id, trimmed);
      void queryClient.invalidateQueries({ queryKey: ["chat-threads", activeProjectId] });
    }
    setRenamingThread(null);
  };

  // ── Render ─────────────────────────────────────────────────────────────

  return (
    <div
      // PromptBar's parent (TerminalPane) constrains height to 320px. Without
      // `h-full flex flex-col` here, PromptBar's children (header + chat
      // history + form) grew naturally and the form got pushed below the
      // 320px ceiling — invisible to the user. Felipe + Beatriz both hit this
      // on Linux + macOS in v1.5.20: chat worked for the first message
      // (history was empty so the form was still in view) then "the input
      // area disappeared" once the chat history took its max-h-80.
      className="h-full flex flex-col border-t border-cs-border bg-cs-card"
      onDrop={onDropFile}
      onDragOver={onDragOver}
    >
      <input
        ref={fileInputRef}
        type="file"
        className="hidden"
        onChange={onPickFile}
        accept="text/*,.md,.json,.yaml,.yml,.toml,.csv,.tsv,.ts,.tsx,.js,.jsx,.py,.rs,.go"
      />

      {/* Thread header — always visible so threads are discoverable */}
      <header className="shrink-0 flex items-center gap-2 px-3 py-1.5 border-b border-cs-border/60 bg-cs-bg-raised/40">
        <div className="relative shrink-0">
          <button
            type="button"
            onClick={() => setShowThreadPicker((v) => !v)}
            className="inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-[11px] text-cs-text hover:bg-cs-border/40"
          >
            <History size={12} className="text-cs-muted" />
            <span className="font-medium truncate max-w-[180px]">
              {currentThread?.title ?? t("prompt.noThread", "(new conversation)")}
            </span>
            <ChevronDown size={10} className="text-cs-muted" />
          </button>

          {showThreadPicker && (
            <>
              <div className="fixed inset-0 z-30" onClick={() => setShowThreadPicker(false)} />
              <div className="absolute top-full left-0 mt-1 w-80 max-h-80 overflow-y-auto rounded-lg border border-cs-border bg-cs-card shadow-xl z-40">
                {/* 2026-05-19 — the three Start-new launchers (Quick
                    chat / Multi-turn session / War room) moved to the
                    inline "+ New ▾" picker on the input row. This
                    dropdown is now thread-history only — fast pickup
                    of recent conversations for power users. Whether
                    to truncate to last 3-4 is open; pending war-room
                    with the design seats. */}
                {/* 2026-05-19 truncation war-room (claude + codex
                    unanimous structural calls + CTO synthesis on cap=5
                    and one-line WhatsApp row): hard cap at last 5,
                    sorted by lastMessageAt desc. Drop msg-count + full
                    timestamp from the row; compact relative time on
                    the right. Footer link routes to Sessions for the
                    full archive. */}
                {(() => {
                  const allThreads = threadsQuery.data ?? [];
                  const sorted = [...allThreads].sort((a, b) => {
                    const aTs = a.lastMessageAt
                      ? new Date(a.lastMessageAt).getTime()
                      : 0;
                    const bTs = b.lastMessageAt
                      ? new Date(b.lastMessageAt).getTime()
                      : 0;
                    return bTs - aTs;
                  });
                  const capped = sorted.slice(0, 5);
                  const remaining = sorted.length - capped.length;
                  if (allThreads.length === 0) {
                    return (
                      <p className="px-3 py-3 text-[11px] text-cs-muted">
                        {t("prompt.noThreads", "No conversations yet.")}
                      </p>
                    );
                  }
                  return (
                    <>
                      {capped.map((thr) => {
                        const isCurrent = thr.id === currentThreadId;
                        const isRenaming = renamingThread?.id === thr.id;
                        return (
                          <div
                            key={thr.id}
                            className={cn(
                              "group flex items-center gap-2 px-3 py-1.5 transition-colors",
                              isCurrent ? "bg-cs-accent/5" : "hover:bg-cs-bg"
                            )}
                          >
                            {isRenaming ? (
                              <input
                                type="text"
                                value={renamingThread.title}
                                onChange={(e) =>
                                  setRenamingThread({ id: thr.id, title: e.target.value })
                                }
                                onKeyDown={(e) => {
                                  if (e.key === "Enter") void commitRename();
                                  if (e.key === "Escape") setRenamingThread(null);
                                }}
                                onBlur={() => void commitRename()}
                                autoFocus
                                className="flex-1 bg-cs-bg border border-cs-accent/40 rounded px-2 py-0.5 text-xs text-cs-text focus:outline-none"
                              />
                            ) : (
                              <button
                                type="button"
                                onClick={() => {
                                  setCurrentThreadId(thr.id);
                                  setShowThreadPicker(false);
                                  setExpanded(true);
                                }}
                                onDoubleClick={() =>
                                  setRenamingThread({ id: thr.id, title: thr.title })
                                }
                                title={`${thr.title} · ${thr.messageCount} msgs`}
                                className="flex-1 min-w-0 text-left text-xs flex items-center gap-2"
                              >
                                <span className={cn(
                                  "truncate flex-1 font-medium",
                                  isCurrent ? "text-cs-accent" : "text-cs-text"
                                )}>
                                  {thr.title}
                                </span>
                                <span className="text-[10px] text-cs-muted shrink-0">
                                  {formatThreadAge(thr.lastMessageAt)}
                                </span>
                              </button>
                            )}
                            <button
                              type="button"
                              onClick={(e) => {
                                e.stopPropagation();
                                void removeThread(thr.id);
                              }}
                              className="opacity-0 group-hover:opacity-100 text-cs-muted hover:text-cs-danger shrink-0 p-1"
                              aria-label={t("common.delete", "Delete")}
                            >
                              <Trash2 size={10} />
                            </button>
                          </div>
                        );
                      })}
                      {remaining > 0 && (
                        <button
                          type="button"
                          onClick={() => {
                            setShowThreadPicker(false);
                            setSection("runs");
                            setSubTab("ato.subtab.runs", "sessions");
                          }}
                          className="w-full border-t border-cs-border px-3 py-2 text-[11px] text-cs-muted hover:text-cs-accent hover:bg-cs-bg flex items-center justify-center gap-1"
                        >
                          {t("prompt.seeAllThreads", {
                            count: allThreads.length,
                            defaultValue: "See all {{count}} conversations →",
                          })}
                        </button>
                      )}
                    </>
                  );
                })()}
              </div>
            </>
          )}
        </div>

        <div className="flex-1" />

        {activeProject && (
          <span className="inline-flex items-center gap-1 rounded-md bg-cs-bg px-2 py-0.5 text-[10px] text-cs-muted">
            <FolderKanban size={10} />
            {activeProject.name}
          </span>
        )}

        <button
          type="button"
          onClick={newThread}
          className="inline-flex items-center gap-1 rounded-md px-1.5 py-1 text-[10px] text-cs-muted hover:text-cs-accent"
          title={t("prompt.newThreadTitle", "New conversation")}
        >
          <MessageSquarePlus size={11} />
        </button>
      </header>

      {/* Chat history — flex-1 with min-h-0 so it shares the parent's
          height with header + form instead of overflowing them. The
          previous `max-h-80` capped at 320px which equaled the entire
          parent height, pushing the form offscreen. */}
      {expanded && messages.length > 0 && (
        <div className="flex-1 min-h-0 overflow-y-auto border-b border-cs-border">
          <div className="p-3 space-y-3">
            {messages.map((msg) => (
              <ChatRow key={msg.id} msg={msg} />
            ))}
            {isLoading && (
              <div className="flex gap-2.5">
                <div
                  className="w-6 h-6 rounded-md flex items-center justify-center shrink-0"
                  style={{
                    background: `${currentRuntime.color}15`,
                    border: `1px solid ${currentRuntime.color}30`,
                  }}
                >
                  {streamingText ? (
                    <RuntimeIcon size={12} style={{ color: currentRuntime.color }} />
                  ) : (
                    <Loader2
                      size={12}
                      style={{ color: currentRuntime.color }}
                      className="animate-spin"
                    />
                  )}
                </div>
                <div className="rounded-lg px-3 py-2 bg-cs-bg border border-cs-border max-w-[85%]">
                  {streamingText ? (
                    <div className="relative">
                      <MarkdownContent content={streamingText} />
                      <span className="inline-block w-1.5 h-3 bg-cs-accent ml-0.5 animate-pulse align-middle" />
                    </div>
                  ) : (
                    <span className="text-xs text-cs-muted">
                      {selectedGroup
                        ? t("prompt.routingThroughGroup", "Routing through {{group}}…", {
                            group: selectedGroup.slug,
                          })
                        : selectedAgent
                        ? t("prompt.thinkingWithAgent", "Thinking — @{{agent}}…", {
                            agent: selectedAgent.slug,
                          })
                        : t("prompt.thinkingPlain", "Thinking…")}
                    </span>
                  )}
                </div>
              </div>
            )}
            <div ref={messagesEndRef} />
          </div>
        </div>
      )}

      {/* Multi-turn status banner */}
      {selectedAgent && messages.length > 0 && (() => {
        const policy = parseMemoryPolicy(selectedAgent);
        const willSummarize = messages.length > policy.summarizeAfter;
        const within = policy.summarizeAfter - messages.length;
        if (!willSummarize && within > 5) return null;
        return (
          <div
            className={cn(
              "px-3 py-1 text-[10px] border-t",
              willSummarize
                ? "border-cs-accent/30 bg-cs-accent/5 text-cs-accent"
                : "border-cs-border bg-cs-bg-raised text-cs-muted"
            )}
          >
            {willSummarize
              ? t(
                  "prompt.willSummarize",
                  "Next message: {{n}} prior turns will be summarized; last {{k}} kept verbatim.",
                  {
                    n: messages.length - policy.keepLastK,
                    k: policy.keepLastK,
                  }
                )
              : t("prompt.nearSummarize", "{{n}} more turns until summarization fires.", {
                  n: within,
                })}
          </div>
        );
      })()}

      {/* Input bar */}
      <form onSubmit={handleSubmit} className="shrink-0 flex items-center gap-2 px-3 py-2.5">
        <button
          type="button"
          onClick={() => messages.length > 0 && setExpanded(!expanded)}
          className={cn(
            "p-1.5 rounded transition-colors shrink-0",
            messages.length > 0
              ? "text-cs-accent hover:bg-cs-accent/10"
              : "text-cs-muted/30 cursor-default"
          )}
        >
          {expanded ? <ChevronDown size={14} /> : <ChevronUp size={14} />}
        </button>

        {/* Room-type launcher — extracted to RoomTypePicker.tsx per the
            2026-05-19 elegance war-room. Sits on the input row so
            Quick chat / Session / War room are discoverable without
            hunting for the chevron in the thread header. */}
        <RoomTypePicker
          onQuickChat={() => inputRef.current?.focus()}
          onNewSession={launchNewSession}
          onWarRoom={launchWarRoom}
        />

        {/* Runtime selector */}
        <div className="relative shrink-0">
          <button
            type="button"
            onClick={() => setShowRuntimePicker(!showRuntimePicker)}
            data-demo-id="runtime-picker"
            className="flex items-center gap-1 px-2 py-1.5 rounded-lg border border-cs-border hover:border-opacity-60 transition-colors"
            style={{ borderColor: `${currentRuntime.color}40` }}
          >
            <RuntimeIcon size={12} style={{ color: currentRuntime.color }} />
            <span
              className="text-[10px] font-medium"
              style={{ color: currentRuntime.color }}
            >
              {currentRuntime.label}
            </span>
          </button>

          {showRuntimePicker && (
            <>
              <div
                className="fixed inset-0 z-30"
                onClick={() => setShowRuntimePicker(false)}
              />
              <div className="absolute bottom-full left-0 mb-1 w-44 rounded-lg border border-cs-border bg-cs-card shadow-xl z-40 overflow-hidden">
                {(() => {
                  // Use the queried list when available, else the
                  // legacy 4-CLI hardcoded list. Filter to available
                  // rows; render API providers as a separate group
                  // with a clear "CLI dispatch only" hint.
                  const rows: AvailableRuntimeRow[] = availableRuntimes
                    ? availableRuntimes.filter((r) => r.available)
                    : RUNTIME_OPTIONS.map((o) => ({
                        slug: o.id,
                        label: o.label,
                        kind: "cli" as const,
                        available: true,
                        reason: "ok",
                      }));
                  const cliRows = rows.filter((r) => r.kind === "cli");
                  const apiRows = rows.filter((r) => r.kind === "api");
                  const renderRow = (r: AvailableRuntimeRow) => {
                    const meta = RUNTIME_META[r.slug] ?? {
                      label: r.label,
                      icon: Globe,
                      color: "#888",
                    };
                    const Icon = meta.icon;
                    const isApi = r.kind === "api";
                    // v2.3.26 Phase 6.x-C: GUI dispatch for API
                    // providers now wired via prompt_api_provider
                    // Tauri command. The "API" tag stays as a label
                    // (different billing model — flat-rate
                    // subscription via key) but the runtime is
                    // fully clickable + dispatchable.
                    return (
                      <button
                        key={r.slug}
                        type="button"
                        onClick={() => {
                          setRuntime(r.slug as AgentRuntime);
                          setShowRuntimePicker(false);
                        }}
                        title={
                          isApi
                            ? `${meta.label} — API provider (subscription via API key)`
                            : meta.label
                        }
                        className={cn(
                          "w-full flex items-center gap-2 px-3 py-2 text-xs transition-colors",
                          runtime === r.slug ? "bg-cs-accent/5" : "hover:bg-cs-bg"
                        )}
                      >
                        <Icon size={12} style={{ color: meta.color }} />
                        <span
                          className="flex-1 text-left"
                          style={{ color: runtime === r.slug ? meta.color : undefined }}
                        >
                          {meta.label}
                        </span>
                        {isApi ? (
                          <span className="text-[9px] uppercase tracking-wide text-cs-muted">
                            API
                          </span>
                        ) : null}
                      </button>
                    );
                  };
                  return (
                    <>
                      {cliRows.map(renderRow)}
                      {apiRows.length > 0 ? (
                        <div className="px-3 pt-2 pb-1 text-[9px] uppercase tracking-wide text-cs-muted border-t border-cs-border">
                          API providers
                        </div>
                      ) : null}
                      {apiRows.map(renderRow)}
                    </>
                  );
                })()}
              </div>
            </>
          )}
        </div>

        {/* Agent / Group selector */}
        <div className="relative shrink-0">
          <button
            type="button"
            onClick={() => setShowAgentPicker((v) => !v)}
            data-demo-id="agent-picker"
            className={cn(
              "flex items-center gap-1 px-2 py-1.5 rounded-lg border transition-colors",
              selectedAgent || selectedGroup
                ? "border-cs-accent/40 bg-cs-accent/5"
                : "border-cs-border hover:border-cs-border/80"
            )}
            title={t("prompt.agentPickerTitle", "Pick an agent or group")}
          >
            {selectedGroup ? (
              <Network size={12} className="text-cs-accent" />
            ) : (
              <Bot size={12} className={selectedAgent ? "text-cs-accent" : "text-cs-muted"} />
            )}
            <span
              className={cn(
                "text-[10px] font-medium font-mono",
                selectedAgent || selectedGroup ? "text-cs-accent" : "text-cs-muted"
              )}
            >
              {selectedGroup
                ? `${selectedGroup.slug}/`
                : selectedAgent
                ? `@${selectedAgent.slug}`
                : t("prompt.noAgent", "no agent")}
            </span>
          </button>

          {showAgentPicker && (
            <>
              <div className="fixed inset-0 z-30" onClick={() => setShowAgentPicker(false)} />
              <div className="absolute bottom-full left-0 mb-1 w-72 max-h-80 overflow-y-auto rounded-lg border border-cs-border bg-cs-card shadow-xl z-40">
                {/* No-agent / single-shot row */}
                <button
                  type="button"
                  onClick={() => {
                    setAgentId(null);
                    setGroupSlug(null);
                    setShowAgentPicker(false);
                    void stickAgentToThread(null);
                  }}
                  className={cn(
                    "w-full flex items-center gap-2 px-3 py-2 text-xs transition-colors border-b border-cs-border",
                    !agentId && !groupSlug
                      ? "bg-cs-accent/5 text-cs-accent"
                      : "text-cs-muted hover:bg-cs-bg"
                  )}
                >
                  {!agentId && !groupSlug ? <Check size={11} /> : <X size={11} />}
                  <span>{t("prompt.noAgent", "no agent")}</span>
                  <span className="ml-auto text-[9px] text-cs-muted">single-shot</span>
                </button>

                {/* Groups section — when selected, prompt routes through the
                    group's router. Shown above individual agents because
                    they're the more powerful primitive. */}
                {runtimeGroups.length > 0 && (
                  <>
                    <div className="px-3 py-1.5 text-[9px] uppercase tracking-wider text-cs-muted bg-cs-bg-raised/40 border-b border-cs-border">
                      {t("prompt.groupsHeader", "Groups · routed dispatch")}
                    </div>
                    {runtimeGroups.map((g) => {
                      const isActive = groupSlug === g.slug;
                      const childCount = g.members.filter((m) => m.role === "child").length;
                      return (
                        <button
                          key={g.id}
                          type="button"
                          onClick={() => {
                            setGroupSlug(g.slug);
                            setAgentId(null);
                            setShowAgentPicker(false);
                            void stickAgentToThread(null);
                          }}
                          className={cn(
                            "w-full flex items-start gap-2 px-3 py-2 text-xs transition-colors text-left border-b border-cs-border/40",
                            isActive ? "bg-cs-accent/5" : "hover:bg-cs-bg"
                          )}
                        >
                          <Network
                            size={11}
                            className={cn(
                              "shrink-0 mt-0.5",
                              isActive ? "text-cs-accent" : "text-cs-muted"
                            )}
                          />
                          <div className="min-w-0 flex-1">
                            <code
                              className={cn(
                                "font-mono truncate",
                                isActive ? "text-cs-accent" : "text-cs-text"
                              )}
                            >
                              {g.slug}
                            </code>
                            <p className="text-[9px] text-cs-muted truncate">
                              {t("prompt.groupChildren", "{{n}} children · router routes per prompt", {
                                n: childCount,
                              })}
                            </p>
                          </div>
                        </button>
                      );
                    })}
                  </>
                )}

                {/* Individual agents */}
                {runtimeAgents.length > 0 && (
                  <div className="px-3 py-1.5 text-[9px] uppercase tracking-wider text-cs-muted bg-cs-bg-raised/40 border-b border-cs-border">
                    {t("prompt.agentsHeader", "Agents")}
                  </div>
                )}
                {runtimeAgents.length === 0 && runtimeGroups.length === 0 ? (
                  <p className="px-3 py-3 text-[11px] text-cs-muted">
                    {t("prompt.noAgentsForRuntime", "No agents created for {{runtime}} yet.", {
                      runtime,
                    })}
                  </p>
                ) : (
                  runtimeAgents.map((a) => {
                    const policy = parseMemoryPolicy(a);
                    return (
                      <button
                        key={a.id}
                        type="button"
                        onClick={() => {
                          setAgentId(a.id);
                          setGroupSlug(null);
                          setShowAgentPicker(false);
                          void stickAgentToThread(a.id);
                        }}
                        className={cn(
                          "w-full flex items-start gap-2 px-3 py-2 text-xs transition-colors text-left",
                          agentId === a.id ? "bg-cs-accent/5" : "hover:bg-cs-bg"
                        )}
                      >
                        <Bot
                          size={11}
                          className={cn(
                            "shrink-0 mt-0.5",
                            agentId === a.id ? "text-cs-accent" : "text-cs-muted"
                          )}
                        />
                        <div className="min-w-0 flex-1">
                          <code
                            className={cn(
                              "font-mono truncate",
                              agentId === a.id ? "text-cs-accent" : "text-cs-text"
                            )}
                          >
                            @{a.slug}
                          </code>
                          <p className="text-[9px] text-cs-muted truncate">
                            {t(
                              "prompt.summarizesAfter",
                              "summarizes after {{n}} msgs · keeps last {{k}}",
                              {
                                n: policy.summarizeAfter,
                                k: policy.keepLastK,
                              }
                            )}
                          </p>
                        </div>
                      </button>
                    );
                  })
                )}
              </div>
            </>
          )}
        </div>

        {/* File attachment */}
        <button
          type="button"
          onClick={() => fileInputRef.current?.click()}
          className="p-1.5 rounded text-cs-muted hover:text-cs-accent hover:bg-cs-border/40 shrink-0"
          title={t("prompt.attachFile", "Attach a text file")}
          disabled={!isTauri}
        >
          <Paperclip size={14} />
        </button>

        <div className="flex-1 relative">
          <Sparkles
            size={14}
            className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted"
          />
          <input
            ref={inputRef}
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder={
              isTauri
                ? t("prompt.placeholderReal", {
                    runtime: currentRuntime.label,
                    defaultValue: "Ask {{runtime}} anything…",
                  })
                : t("prompt.placeholder")
            }
            className="w-full bg-cs-bg border border-cs-border rounded-lg pl-9 pr-3 py-2 text-sm text-cs-text placeholder:text-cs-muted/60 focus:outline-none focus:border-cs-accent/50 font-mono"
            disabled={isLoading}
          />
        </div>

        <button
          type="submit"
          disabled={!input.trim() || isLoading}
          className="p-2 rounded-lg text-cs-bg hover:opacity-90 transition-colors disabled:opacity-30 disabled:cursor-not-allowed shrink-0"
          style={{ background: currentRuntime.color }}
        >
          <Send size={14} />
        </button>
      </form>
    </div>
  );
}

