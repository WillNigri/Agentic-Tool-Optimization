import { create } from "zustand";
import type {
  AgentConfigFile,
  ParsedConfigFile,
  AgentPermission,
  AgentContextPreview,
  AgentConfigRuntime,
} from "@/lib/tauri-api";

export type RuntimeFilter = "all" | AgentConfigRuntime;

interface AgentConfigStore {
  // Discovered config files
  configFiles: AgentConfigFile[];
  selectedFilePath: string | null;
  isLoading: boolean;
  error: string | null;

  // Parsed content (in-memory editing)
  editingContent: ParsedConfigFile | null;
  originalContent: string | null;
  dirty: boolean;

  // Runtime filter
  activeRuntime: RuntimeFilter;

  // Permissions (aggregated from all sources)
  permissions: AgentPermission[];

  // Context preview
  contextPreview: AgentContextPreview | null;

  // Actions
  setConfigFiles: (files: AgentConfigFile[]) => void;
  selectFile: (path: string | null) => void;
  setEditingContent: (content: ParsedConfigFile | null) => void;
  setOriginalContent: (content: string | null) => void;
  setDirty: (dirty: boolean) => void;
  setActiveRuntime: (runtime: RuntimeFilter) => void;
  setPermissions: (permissions: AgentPermission[]) => void;
  setContextPreview: (preview: AgentContextPreview | null) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;

  // Update content in place
  updateRawContent: (newContent: string) => void;

  // Computed getters
  getFilteredFiles: () => AgentConfigFile[];
  getFilesByScope: (scope: "global" | "project") => AgentConfigFile[];
  getSelectedFile: () => AgentConfigFile | null;
}

export const useAgentConfigStore = create<AgentConfigStore>((set, get) => ({
  // Initial state
  configFiles: [],
  selectedFilePath: null,
  isLoading: false,
  error: null,
  editingContent: null,
  originalContent: null,
  dirty: false,
  activeRuntime: "all",
  permissions: [],
  contextPreview: null,

  // Actions
  setConfigFiles: (files) => set({ configFiles: files }),

  selectFile: (path) =>
    set({
      selectedFilePath: path,
      editingContent: null,
      originalContent: null,
      dirty: false,
    }),

  setEditingContent: (content) => set({ editingContent: content }),

  setOriginalContent: (content) => set({ originalContent: content }),

  setDirty: (dirty) => set({ dirty }),

  setActiveRuntime: (runtime) => set({ activeRuntime: runtime }),

  setPermissions: (permissions) => set({ permissions }),

  setContextPreview: (preview) => set({ contextPreview: preview }),

  setLoading: (loading) => set({ isLoading: loading }),

  setError: (error) => set({ error }),

  updateRawContent: (newContent) => {
    const { editingContent, originalContent } = get();
    if (editingContent) {
      set({
        editingContent: {
          ...editingContent,
          raw: newContent,
        },
        dirty: newContent !== originalContent,
      });
    }
  },

  // Computed getters
  getFilteredFiles: () => {
    const { configFiles, activeRuntime } = get();
    if (activeRuntime === "all") {
      return configFiles;
    }
    return configFiles.filter(
      (f) => f.runtime === activeRuntime || f.runtime === "shared"
    );
  },

  getFilesByScope: (scope) => {
    const filtered = get().getFilteredFiles();
    return filtered.filter((f) => f.scope === scope);
  },

  getSelectedFile: () => {
    const { configFiles, selectedFilePath } = get();
    if (!selectedFilePath) return null;
    return configFiles.find((f) => f.path === selectedFilePath) || null;
  },
}));
