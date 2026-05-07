import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import i18n from "@/i18n";
import type { AgentRuntime } from "@/lib/agents";
import type { Section } from "@/components/Sidebar";
import { useUiStore, type WizardPath } from "@/stores/useUiStore";
import type { QuickDraft } from "@/lib/agentDraft";

interface CreateAgentSpec {
  displayName: string;
  runtime: AgentRuntime;
  description?: string;
  systemPrompt?: string;
  model?: string;
  goal?: string;
  permissions?: string[];
  skills?: string[];
  mcps?: string[];
  /** v2.0.0 — defaults to 'internal'; pass 'external' to seed an
   *  external/customer-facing agent for the v2 demo segment. */
  kind?: "internal" | "external";
}

interface CreateGroupSpec {
  displayName: string;
  runtime: AgentRuntime;
  description?: string;
  childSlugs: string[];
  routerRules?: { keywords: string[]; then: string }[];
  /** "routed" (default) or "sequential" (automation pipeline). */
  dispatchKind?: "routed" | "sequential";
}

// v1.5.0 — Demo mode runtime.
//
// Drives the whole app through a scripted sequence so the user can record a
// canonical demo without touching anything. The runner can navigate sections,
// open the Create Agent wizard with a pre-picked template, type into the chat,
// dispatch, swap runtimes, and dwell on each scene with a subtitle.
//
// PromptBar / Dashboard / TerminalPane subscribe to the relevant fields:
//   - section navigation goes through useUiStore.setSection
//   - wizard control goes through useUiStore.openCreateAgent / closeCreateAgent
//   - chat-pane state (input, runtime, send, new thread) is driven via the
//     `pending*` fields on this store
//
// Steps run in order. The runner advances when each step's "done" signal
// arrives (typing complete, dispatch returned, timer fired).

export type DemoStep =
  | { kind: "subtitle"; text: string; textKey?: string; durationMs?: number }
  | { kind: "wait"; ms: number }
  | { kind: "navigate"; section: Section }
  | { kind: "openWizard"; path?: WizardPath; templateId?: string | null }
  | { kind: "closeWizard" }
  | { kind: "newThread" }
  | { kind: "setRuntime"; runtime: AgentRuntime }
  | { kind: "type"; text: string; charsPerSec?: number }
  | { kind: "send" } // depends on PromptBar to dispatch + signal back when streaming done
  | { kind: "stop" }
  // Animated agent-form typing — drives QuickPath's draft fields one
  // character at a time so creation feels human.
  | { kind: "typeAgentField"; field: keyof QuickDraft; text: string; charsPerSec?: number }
  | { kind: "setAgentField"; field: keyof QuickDraft; value: QuickDraft[keyof QuickDraft] }
  | { kind: "submitAgentForm" }
  // Direct backend creation — fast, for the supporting agents and group.
  | { kind: "createAgent"; spec: CreateAgentSpec }
  | { kind: "createGroup"; spec: CreateGroupSpec }
  // Pick an agent in the chat pane by slug (sets PromptBar's agent picker).
  | { kind: "selectAgent"; slug: string | null }
  // Pick a group in the chat pane by slug. Mutually exclusive with selectAgent.
  | { kind: "selectChatGroup"; slug: string | null }
  // Collapse / expand the bottom Chat pane. Used to give sections more
  // vertical space while the demo is touring them.
  | { kind: "setChatPaneOpen"; open: boolean }
  // Pulse a UI element with a glowing ring so the recording shows what's
  // about to be "clicked". Match against `data-demo-id="..."` on the DOM
  // element. Multiple highlights stack — auto-clear after durationMs.
  | { kind: "highlight"; id: string; durationMs?: number }
  // Scroll a data-demo-id element into view (smooth scroll). Used before
  // highlighting so the viewer actually sees the target — Save buttons in
  // long modals get pushed below the fold otherwise.
  | { kind: "scrollIntoView"; id: string; block?: "center" | "start" | "end" | "nearest" }
  // Click a data-demo-id element. Used to drive UI affordances whose state
  // lives inside a component (not in a store) — e.g., the cron Calendar/List
  // toggle, which is local React state.
  | { kind: "clickByDemoId"; id: string }
  // Drive the open Group form field-by-field with delays so the viewer
  // sees the same "watching it being built" UX as the agent flow. Bridge
  // lives in `pendingGroupAutoFill` — GroupDetail subscribes and animates.
  | {
      kind: "autoFillGroupForm";
      spec: {
        displayName: string;
        description?: string;
        runtime?: AgentRuntime;
        dispatchKind: "routed" | "sequential";
        childSlugs: string[];
        routerRule?: { keywords: string[]; thenSlug: string };
      };
    }
  // Guided path — animate typing into the goal input + submit. The wizard
  // opens its own conversation flow which we don't try to drive end-to-end;
  // we just kick it off so the user sees the chat-style creation surface.
  | { kind: "typeGuidedGoal"; text: string; charsPerSec?: number }
  | { kind: "submitGuidedGoal" }
  // Seed a cron job programmatically — demoes the scheduling story.
  | {
      kind: "createCronJob";
      name: string;
      description: string;
      schedule: string;
      runtime: AgentRuntime;
      prompt: string;
      /** When set, the cron dispatches via the agent's full context path
       *  (variables / hooks / memory) instead of a raw runtime+prompt. */
      agentSlug?: string;
      /** When set, the cron dispatches via the group (routed or pipeline). */
      groupSlug?: string;
    }
  // Switch a section's sub-tab (e.g. Agents → Groups). storageKey matches
  // the prop SectionTabs is mounted with.
  | { kind: "setSubTab"; storageKey: string; tabId: string }
  // Open a specific group's detail view by slug.
  | { kind: "selectGroup"; slug: string | null }
  // Wipe specific agents + groups so a re-run starts clean. Best-effort
  // (silently skips anything that doesn't exist).
  | {
      kind: "cleanup";
      agentSlugs?: string[];
      groupSlugs?: string[];
      runtime?: AgentRuntime;
    };

export interface DemoScript {
  id: string;
  label: string;
  /** Used as overlay caption while playing the script's hero step. */
  shortDescription: string;
  steps: DemoStep[];
}

interface DemoState {
  isPlaying: boolean;
  /** When true, the runner blocks before each step until pause is cleared.
   *  Lets viewers stop on a long subtitle to read it without restarting. */
  isPaused: boolean;
  scriptId: string | null;
  stepIndex: number;
  caption: string | null;
  /** Drives PromptBar: the runtime to switch to. Cleared after the bar reacts. */
  pendingRuntime: AgentRuntime | null;
  /** Drives PromptBar: text to display in the input. Animated character-by-character
   *  by the runner; PromptBar just renders whatever is set. */
  pendingInputText: string;
  /** Tells PromptBar to fire its submit handler. Cleared after dispatch starts. */
  pendingSubmit: number; // increment-as-trigger
  /** Tells PromptBar to start a new thread. Cleared after the bar reacts. */
  pendingNewThread: number;
  /** Drives QuickPath: a partial draft to merge. Used for animated typing
   *  AND outright field sets. The runner clears it after each step so
   *  QuickPath only re-merges on actual changes. */
  pendingAgentFormPatch: { seq: number; patch: Partial<QuickDraft> } | null;
  /** Tells QuickPath to submit its form. */
  pendingAgentFormSubmit: number;
  /** Drives PromptBar: select this agent slug if it exists. */
  pendingSelectAgentSlug: string | null;
  /** Drives PromptBar: select this group slug if it exists (chat picker). */
  pendingSelectGroupSlug: string | null;
  /** Drives TerminalPane: explicit chat-pane-open flag for the demo. When
   *  true forces open + Chat tab; when false forces collapsed; null means
   *  TerminalPane uses its default behavior. */
  pendingChatPaneOpen: boolean | null;
  /** Set of data-demo-ids currently highlighted. HighlightOverlay reads
   *  this and pulses the matching DOM elements. */
  highlightIds: string[];
  /** Drives GuidedPath: text to display in the goal input. Animated by the
   *  runner; GuidedPath just renders. */
  pendingGuidedGoal: string;
  /** Drives GuidedPath: bump to fire submit. */
  pendingGuidedSubmit: number;
  /** Drives GroupDetail: when set, GroupDetail animates each field with a
   *  brief delay between them so the demo viewer sees the form being built
   *  the same way QuickPath shows it. */
  pendingGroupAutoFill: {
    seq: number;
    spec: {
      displayName: string;
      description?: string;
      runtime?: AgentRuntime;
      dispatchKind: "routed" | "sequential";
      childSlugs: string[];
      routerRule?: { keywords: string[]; thenSlug: string };
    };
  } | null;

  play(script: DemoScript): Promise<void>;
  stop(): void;
  /** Toggle pause/resume — bound to Tab in DemoOverlay. */
  togglePause(): void;
  /** PromptBar calls this when the in-flight dispatch's stream completes. */
  notifyDispatchComplete(): void;
  /** QuickPath calls this when its createAgent mutation succeeds. */
  notifyAgentCreated(): void;
}

let dispatchResolvers: Array<() => void> = [];
let agentCreatedResolvers: Array<() => void> = [];
// Pending sleeps that are blocked because the demo is paused. togglePause
// drains this list to resume; stop() drains it to abort.
let pauseResolvers: Array<() => void> = [];
let formPatchSeq = 0;

export const useDemoStore = create<DemoState>((set, get) => ({
  isPlaying: false,
  isPaused: false,
  scriptId: null,
  stepIndex: -1,
  caption: null,
  pendingRuntime: null,
  pendingInputText: "",
  pendingSubmit: 0,
  pendingNewThread: 0,
  pendingAgentFormPatch: null,
  pendingAgentFormSubmit: 0,
  pendingSelectAgentSlug: null,
  pendingSelectGroupSlug: null,
  pendingChatPaneOpen: null,
  highlightIds: [],
  pendingGuidedGoal: "",
  pendingGuidedSubmit: 0,
  pendingGroupAutoFill: null,

  play: async (script) => {
    if (get().isPlaying) return;
    set({
      isPlaying: true,
      scriptId: script.id,
      stepIndex: -1,
      caption: null,
      pendingInputText: "",
    });

    for (let i = 0; i < script.steps.length; i++) {
      if (!get().isPlaying) break;
      const step = script.steps[i];
      set({ stepIndex: i });

      switch (step.kind) {
        case "subtitle": {
          // Translate when a textKey is provided; fall back to the English
          // text otherwise. New subtitles can ship without a key and just
          // render in English until translations land.
          const caption = step.textKey
            ? i18n.t(step.textKey, { defaultValue: step.text })
            : step.text;
          set({ caption });
          await sleep(step.durationMs ?? 1800);
          // Auto-clear so subtitles don't bleed into typing animations.
          // Persistent narration would override the visual focus on what
          // the demo is actually doing.
          set({ caption: null });
          break;
        }
        case "wait":
          await sleep(step.ms);
          break;
        case "navigate":
          // Pulse the sidebar item that's about to be selected.
          flashHighlight(set, get, `nav-${step.section}`, 1000);
          await sleep(150);
          useUiStore.getState().setSection(step.section);
          await sleep(450);
          break;
        case "openWizard":
          flashHighlight(set, get, `wizard-path-${step.path ?? "guided"}`, 900);
          await sleep(150);
          useUiStore
            .getState()
            .openCreateAgent(step.path ?? "templates", step.templateId ?? null);
          await sleep(550);
          break;
        case "closeWizard":
          useUiStore.getState().closeCreateAgent();
          await sleep(350);
          break;
        case "newThread":
          set((s) => ({ pendingNewThread: s.pendingNewThread + 1, pendingInputText: "" }));
          await sleep(450); // give react-query + PromptBar a tick to switch threads
          break;
        case "setRuntime":
          set({ pendingRuntime: step.runtime });
          await sleep(150);
          // Pulse the picker pill AFTER the swap so the user sees the new
          // colored runtime label flash.
          flashHighlight(set, get, "runtime-picker", 1100);
          await sleep(550);
          set({ pendingRuntime: null });
          break;
        case "type": {
          const cps = step.charsPerSec ?? 22;
          const delay = Math.max(20, 1000 / cps);
          let buf = "";
          for (const ch of step.text) {
            if (!get().isPlaying) break;
            buf += ch;
            set({ pendingInputText: buf });
            await sleep(delay);
          }
          break;
        }
        case "send": {
          const waitForDispatch = new Promise<void>((resolve) => {
            dispatchResolvers.push(resolve);
          });
          set((s) => ({ pendingSubmit: s.pendingSubmit + 1 }));
          // Hard ceiling so a stuck CLI can't lock the script. 3 minutes —
          // group dispatch is non-streaming and `claude --print` on long
          // prompts (history-stitched + framing) can take 60-90s. Streaming
          // dispatches always notify well before this fires.
          await Promise.race([waitForDispatch, sleep(180_000)]);
          // Drain any leftover resolver slots.
          dispatchResolvers = dispatchResolvers.filter((r) => r !== undefined);
          set({ pendingInputText: "" });
          break;
        }
        case "typeAgentField": {
          // Animate one character at a time so the form looks human.
          const cps = step.charsPerSec ?? 26;
          const delay = Math.max(20, 1000 / cps);
          let buf = "";
          for (const ch of step.text) {
            if (!get().isPlaying) break;
            buf += ch;
            formPatchSeq += 1;
            set({
              pendingAgentFormPatch: {
                seq: formPatchSeq,
                patch: { [step.field]: buf } as Partial<QuickDraft>,
              },
            });
            await sleep(delay);
          }
          break;
        }
        case "setAgentField": {
          formPatchSeq += 1;
          set({
            pendingAgentFormPatch: {
              seq: formPatchSeq,
              patch: { [step.field]: step.value } as Partial<QuickDraft>,
            },
          });
          await sleep(150);
          break;
        }
        case "submitAgentForm": {
          const waitForCreate = new Promise<void>((resolve) => {
            agentCreatedResolvers.push(resolve);
          });
          set((s) => ({ pendingAgentFormSubmit: s.pendingAgentFormSubmit + 1 }));
          await Promise.race([waitForCreate, sleep(15_000)]);
          agentCreatedResolvers = [];
          break;
        }
        case "createAgent": {
          try {
            await invoke("create_agent", {
              displayName: step.spec.displayName,
              runtime: step.spec.runtime,
              description: step.spec.description ?? null,
              model: step.spec.model ?? null,
              projectId: null,
              systemPrompt: step.spec.systemPrompt ?? null,
              permissions: step.spec.permissions ?? null,
              skills: step.spec.skills ?? null,
              mcps: step.spec.mcps ?? null,
              goal: step.spec.goal ?? null,
              writeFile: step.spec.kind !== "external", // external skips the on-disk file
              kind: step.spec.kind ?? "internal",
            });
          } catch {
            // Agent may already exist from a prior demo run — that's fine.
          }
          await sleep(250);
          break;
        }
        case "createGroup": {
          try {
            const members = step.spec.childSlugs.map((slug, i) => ({
              agentSlug: slug,
              role: "child" as const,
              position: i,
            }));
            const routerConfig = {
              rules: (step.spec.routerRules ?? []).map((r) => ({
                if: { keyword: r.keywords },
                then: r.then,
              })),
              llmFallback: { enabled: true, model: undefined },
            };
            await invoke("create_agent_group", {
              displayName: step.spec.displayName,
              runtime: step.spec.runtime,
              description: step.spec.description ?? null,
              routerConfigJson: JSON.stringify(routerConfig),
              members,
              dispatchKind: step.spec.dispatchKind ?? null,
            });
          } catch {
            // Group may already exist — fine.
          }
          await sleep(250);
          break;
        }
        case "selectAgent":
          set({ pendingSelectAgentSlug: step.slug });
          await sleep(150);
          if (step.slug) flashHighlight(set, get, "agent-picker", 1100);
          await sleep(400);
          set({ pendingSelectAgentSlug: null });
          break;
        case "selectChatGroup":
          set({ pendingSelectGroupSlug: step.slug });
          await sleep(150);
          if (step.slug) flashHighlight(set, get, "agent-picker", 1100);
          await sleep(400);
          set({ pendingSelectGroupSlug: null });
          break;
        case "setChatPaneOpen":
          set({ pendingChatPaneOpen: step.open });
          await sleep(300);
          break;
        case "typeGuidedGoal": {
          const cps = step.charsPerSec ?? 24;
          const delay = Math.max(20, 1000 / cps);
          let buf = "";
          for (const ch of step.text) {
            if (!get().isPlaying) break;
            buf += ch;
            set({ pendingGuidedGoal: buf });
            await sleep(delay);
          }
          break;
        }
        case "submitGuidedGoal":
          set((s) => ({ pendingGuidedSubmit: s.pendingGuidedSubmit + 1 }));
          await sleep(400);
          break;
        case "createCronJob": {
          try {
            const id = crypto.randomUUID();
            const now = new Date().toISOString();
            const job: Record<string, unknown> = {
              id,
              name: step.name,
              description: step.description,
              schedule: step.schedule,
              runtime: step.runtime,
              prompt: step.prompt,
              enabled: true,
              status: "healthy" as const,
              createdAt: now,
              updatedAt: now,
            };
            // The demo's seeded cron points at the security-reviewer agent
            // we just created, so the run inherits that agent's system
            // prompt + variables instead of being a raw runtime+prompt.
            if ((step as { agentSlug?: string }).agentSlug) {
              job.agentSlug = (step as { agentSlug?: string }).agentSlug;
            }
            if ((step as { groupSlug?: string }).groupSlug) {
              job.groupSlug = (step as { groupSlug?: string }).groupSlug;
            }
            await invoke("save_cron_job", { job: JSON.stringify(job) });
          } catch {
            // Cron may not be configured locally — that's fine for the demo.
          }
          await sleep(400);
          break;
        }
        case "setSubTab":
          flashHighlight(set, get, `subtab-${step.storageKey}-${step.tabId}`, 900);
          await sleep(150);
          useUiStore.getState().setSubTab(step.storageKey, step.tabId);
          await sleep(400);
          break;
        case "highlight":
          flashHighlight(set, get, step.id, step.durationMs ?? 1200);
          await sleep((step.durationMs ?? 1200) + 100);
          break;
        case "scrollIntoView": {
          const el = document.querySelector<HTMLElement>(`[data-demo-id="${step.id}"]`);
          if (el) {
            el.scrollIntoView({ behavior: "smooth", block: step.block ?? "center" });
            await sleep(700);
          }
          break;
        }
        case "clickByDemoId": {
          const el = document.querySelector<HTMLElement>(`[data-demo-id="${step.id}"]`);
          if (el) {
            flashHighlight(set, get, step.id, 800);
            el.click();
            await sleep(500);
          }
          break;
        }
        case "autoFillGroupForm": {
          // Bump seq so the receiver re-runs even if spec is structurally
          // similar to a prior auto-fill.
          set((s) => ({
            pendingGroupAutoFill: {
              seq: (s.pendingGroupAutoFill?.seq ?? 0) + 1,
              spec: step.spec,
            },
          }));
          // Give GroupDetail's effect time to consume + animate. The
          // animation itself happens inside the component (~3s for a
          // typical 5-field form); we wait long enough that the demo
          // doesn't race ahead of what the viewer sees.
          await sleep(3500);
          break;
        }
        case "selectGroup":
          useUiStore.getState().selectGroupSlug(step.slug);
          await sleep(400);
          break;
        case "cleanup": {
          // Best-effort: list, find by slug+runtime, delete.
          try {
            interface AgentRow { id: string; slug: string; runtime: string }
            interface GroupRow { id: string; slug: string }
            const agents = (await invoke<AgentRow[]>("list_agents", { runtime: null })) ?? [];
            for (const slug of step.agentSlugs ?? []) {
              const match = agents.find(
                (a) =>
                  a.slug === slug &&
                  (!step.runtime || a.runtime === step.runtime)
              );
              if (match) {
                try {
                  await invoke("delete_agent", { id: match.id, deleteFile: true });
                } catch {
                  // ignore individual failures
                }
              }
            }
            const groups = (await invoke<GroupRow[]>("list_agent_groups", { runtime: null })) ?? [];
            for (const slug of step.groupSlugs ?? []) {
              const match = groups.find((g) => g.slug === slug);
              if (match) {
                try {
                  await invoke("delete_agent_group", { id: match.id });
                } catch {
                  // ignore
                }
              }
            }
          } catch {
            // Fully silent — cleanup is best-effort.
          }
          await sleep(300);
          break;
        }
        case "stop":
          break;
      }
    }

    set({
      isPlaying: false,
      scriptId: null,
      stepIndex: -1,
      caption: null,
      pendingInputText: "",
      pendingRuntime: null,
    });
  },

  stop: () => {
    dispatchResolvers.forEach((r) => r());
    dispatchResolvers = [];
    agentCreatedResolvers.forEach((r) => r());
    agentCreatedResolvers = [];
    pauseResolvers.forEach((r) => r());
    pauseResolvers = [];
    set({
      isPlaying: false,
      isPaused: false,
      scriptId: null,
      stepIndex: -1,
      caption: null,
      pendingInputText: "",
      pendingRuntime: null,
      pendingAgentFormPatch: null,
      pendingSelectAgentSlug: null,
      pendingSelectGroupSlug: null,
      pendingChatPaneOpen: null,
      highlightIds: [],
    });
  },

  togglePause: () => {
    const willPause = !get().isPaused;
    set({ isPaused: willPause });
    if (!willPause) {
      // Wake any sleeps blocked on the pause flag.
      pauseResolvers.forEach((r) => r());
      pauseResolvers = [];
    }
  },

  notifyDispatchComplete: () => {
    const next = dispatchResolvers.shift();
    next?.();
  },

  notifyAgentCreated: () => {
    const next = agentCreatedResolvers.shift();
    next?.();
  },
}));

async function sleep(ms: number): Promise<void> {
  // If paused, block until togglePause clears it. Then dwell for the
  // requested duration so timing relative to the user's resume point is
  // preserved.
  if (useDemoStore.getState().isPaused) {
    await new Promise<void>((resolve) => {
      pauseResolvers.push(resolve);
    });
  }
  await new Promise<void>((resolve) => setTimeout(resolve, ms));
}

/** Add an id to highlightIds, then remove it after `durationMs`. The
 *  HighlightOverlay component watches this list and pulses any element
 *  whose `data-demo-id` matches. Multiple ids stack. */
function flashHighlight(
  set: (partial: Partial<DemoState> | ((s: DemoState) => Partial<DemoState>)) => void,
  get: () => DemoState,
  id: string,
  durationMs: number,
) {
  set((s) => ({ highlightIds: [...s.highlightIds, id] }));
  setTimeout(() => {
    if (!get().isPlaying) return;
    set((s) => ({ highlightIds: s.highlightIds.filter((x) => x !== id) }));
  }, durationMs);
}
