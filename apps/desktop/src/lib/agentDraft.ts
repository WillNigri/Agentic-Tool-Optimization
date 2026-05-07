import type { AgentRuntime } from "@/lib/tauri-api";

// v1.3.0 T3.b — Agent wizard draft persistence (localStorage).
// One draft per path (guided | quick). Saved on each change, cleared on
// successful create. Future: move to ~/.ato/agent-drafts/<id>.json so the cron
// daemon can pick them up too.

export type WizardPath = "guided" | "quick";

export interface QuickDraft {
  name: string;
  runtime: AgentRuntime;
  model: string;
  description: string;
  systemPrompt: string;
  projectId: string | null;
  skills: string[];
  mcps: string[];
  /** Files the agent should always be able to read. Saved as F2 Context
   *  Hooks (kind: "file") on agent creation — content gets injected into a
   *  <context> block on every turn, NOT into the system prompt. */
  contextFiles: string[];
}

export interface GuidedDraft {
  goal: string;
  submittedGoal: string;
}

const QUICK_KEY = "ato.agent-draft.quick.v1";
const GUIDED_KEY = "ato.agent-draft.guided.v1";

export function loadQuickDraft(): QuickDraft | null {
  try {
    const raw = localStorage.getItem(QUICK_KEY);
    if (!raw) return null;
    return JSON.parse(raw) as QuickDraft;
  } catch {
    return null;
  }
}

export function saveQuickDraft(draft: QuickDraft): void {
  try {
    localStorage.setItem(QUICK_KEY, JSON.stringify(draft));
  } catch {
    // ignore quota errors
  }
}

export function clearQuickDraft(): void {
  try {
    localStorage.removeItem(QUICK_KEY);
  } catch {
    // ignore
  }
}

export function loadGuidedDraft(): GuidedDraft | null {
  try {
    const raw = localStorage.getItem(GUIDED_KEY);
    if (!raw) return null;
    return JSON.parse(raw) as GuidedDraft;
  } catch {
    return null;
  }
}

export function saveGuidedDraft(draft: GuidedDraft): void {
  try {
    localStorage.setItem(GUIDED_KEY, JSON.stringify(draft));
  } catch {
    // ignore
  }
}

export function clearGuidedDraft(): void {
  try {
    localStorage.removeItem(GUIDED_KEY);
  } catch {
    // ignore
  }
}
