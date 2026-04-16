import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { Project } from "@/lib/tauri-api";

export type ProjectSection =
  | "memory"
  | "skills"
  | "subagents"
  | "commands"
  | "hooks"
  | "permissions"
  | "mcp"
  | "sandbox"
  | "policies";

interface ProjectStore {
  activeProject: Project | null;
  selectedSection: ProjectSection;
  sidebarExpanded: boolean;
  setActiveProject: (project: Project | null) => void;
  setSelectedSection: (section: ProjectSection) => void;
  clearActiveProject: () => void;
  toggleSidebarExpanded: () => void;
  setSidebarExpanded: (value: boolean) => void;
}

export const useProjectStore = create<ProjectStore>()(
  persist(
    (set) => ({
      activeProject: null,
      selectedSection: "memory",
      sidebarExpanded: false,
      setActiveProject: (project) => set({ activeProject: project, selectedSection: "memory" }),
      setSelectedSection: (section) => set({ selectedSection: section }),
      clearActiveProject: () => set({ activeProject: null }),
      toggleSidebarExpanded: () => set((s) => ({ sidebarExpanded: !s.sidebarExpanded })),
      setSidebarExpanded: (value) => set({ sidebarExpanded: value }),
    }),
    {
      name: "ato-project-store",
      partialize: (state) => ({ sidebarExpanded: state.sidebarExpanded }),
    }
  )
);
