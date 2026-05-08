import { create } from "zustand";
import type { Section } from "@/components/Sidebar";

// v1.5.0 — Cross-cutting UI state.
//
// Lifted out of Dashboard / Home / AgentsSection so the demo runner (and any
// other coordinator) can drive section navigation, open the Create Agent
// wizard, pre-pick a template, etc., from anywhere. Components subscribe and
// react.

export type WizardPath = "guided" | "quick" | "templates";

interface UiState {
  /** Active sidebar section. */
  section: Section;
  setSection: (s: Section) => void;

  /** Whether the Create Agent wizard is open. */
  createAgentOpen: boolean;
  /** When opening, which path to land on (Templates / Guided / Quick). */
  createAgentPath: WizardPath;
  /** Optional template id to auto-pick after the wizard opens. The wizard
   *  consumes this once and clears it. */
  createAgentTemplateId: string | null;

  openCreateAgent: (path?: WizardPath, templateId?: string | null) => void;
  closeCreateAgent: () => void;
  consumeTemplateId: () => string | null;

  /** External overrides for SectionTabs sub-tabs. Keyed by storageKey
   *  (e.g. "ato.subtab.agents"). Lets the demo runner switch sub-tabs
   *  without touching localStorage races. */
  subTabs: Record<string, string | null>;
  setSubTab: (storageKey: string, tabId: string | null) => void;

  /** Optional: groups list internal "selected group slug" — lets the demo
   *  runner open a specific group's detail view. GroupsList consumes this. */
  selectedGroupSlug: string | null;
  selectGroupSlug: (slug: string | null) => void;

  /** v2.0.0 — when set, MyAgentsList opens the agent detail overlay for
   *  this slug as soon as it mounts (or the moment the agent shows up in
   *  list results). Optional `tab` lets the wizard's "Set up Knowledge"
   *  CTA land the user directly on the Knowledge tab instead of Variables.
   *  Consumed once by MyAgentsList, then cleared. */
  pendingOpenAgentSlug: string | null;
  pendingOpenAgentTab: string | null;
  openAgentDetail: (slug: string, tab?: string | null) => void;
  consumePendingOpenAgent: () => { slug: string | null; tab: string | null };
}

export const useUiStore = create<UiState>((set, get) => ({
  section: "home",
  createAgentOpen: false,
  createAgentPath: "guided",
  createAgentTemplateId: null,

  setSection: (s) => set({ section: s }),

  openCreateAgent: (path = "guided", templateId = null) =>
    set({
      createAgentOpen: true,
      createAgentPath: path,
      createAgentTemplateId: templateId,
    }),

  closeCreateAgent: () =>
    set({
      createAgentOpen: false,
      createAgentTemplateId: null,
    }),

  consumeTemplateId: () => {
    const id = get().createAgentTemplateId;
    if (id) set({ createAgentTemplateId: null });
    return id;
  },

  subTabs: {},
  setSubTab: (storageKey, tabId) =>
    set((s) => ({ subTabs: { ...s.subTabs, [storageKey]: tabId } })),

  selectedGroupSlug: null,
  selectGroupSlug: (slug) => set({ selectedGroupSlug: slug }),

  pendingOpenAgentSlug: null,
  pendingOpenAgentTab: null,
  openAgentDetail: (slug, tab = null) =>
    set({ pendingOpenAgentSlug: slug, pendingOpenAgentTab: tab }),
  consumePendingOpenAgent: () => {
    const slug = get().pendingOpenAgentSlug;
    const tab = get().pendingOpenAgentTab;
    if (slug) set({ pendingOpenAgentSlug: null, pendingOpenAgentTab: null });
    return { slug, tab };
  },
}));
