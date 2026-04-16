import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Variable,
  Plus,
  Trash2,
  Edit2,
  Check,
  X,
  Loader2,
  Upload,
  Download,
  RefreshCw,
  FolderOpen,
} from "lucide-react";
import { cn } from "@/lib/utils";
import {
  listEnvVars,
  saveEnvVar,
  updateEnvVar,
  deleteEnvVar,
  importEnvFile,
  listProjects,
  type EnvVar,
  type Project,
} from "@/lib/api";

const RUNTIMES = [
  { value: "", label: "All Runtimes" },
  { value: "claude", label: "Claude" },
  { value: "codex", label: "Codex" },
  { value: "hermes", label: "Hermes" },
  { value: "openclaw", label: "OpenClaw" },
];

export default function EnvManager() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [showAdd, setShowAdd] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editKey, setEditKey] = useState("");
  const [editValue, setEditValue] = useState("");

  // Filters
  const [filterProject, setFilterProject] = useState<string>("");
  const [filterRuntime, setFilterRuntime] = useState<string>("");

  // Form state
  const [newKey, setNewKey] = useState("");
  const [newValue, setNewValue] = useState("");
  const [newRuntime, setNewRuntime] = useState("");
  const [newProjectId, setNewProjectId] = useState("");

  // Fetch projects for filtering
  const { data: projects = [] } = useQuery<Project[]>({
    queryKey: ["projects"],
    queryFn: listProjects,
  });

  // Fetch env vars
  const { data: envVars = [], isLoading, refetch } = useQuery({
    queryKey: ["env-vars", filterProject, filterRuntime],
    queryFn: () => listEnvVars(filterProject || undefined, filterRuntime || undefined),
  });

  // Save mutation
  const saveMutation = useMutation({
    mutationFn: () => saveEnvVar(newKey, newValue, newProjectId || undefined, newRuntime || undefined),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["env-vars"] });
      setShowAdd(false);
      setNewKey("");
      setNewValue("");
      setNewRuntime("");
      setNewProjectId("");
    },
  });

  // Update mutation
  const updateMutation = useMutation({
    mutationFn: ({ id, key, value }: { id: string; key?: string; value?: string }) =>
      updateEnvVar(id, key, value),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["env-vars"] });
      setEditingId(null);
    },
  });

  // Delete mutation
  const deleteMutation = useMutation({
    mutationFn: deleteEnvVar,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["env-vars"] });
    },
  });

  // Import mutation
  const importMutation = useMutation({
    mutationFn: (filePath: string) => importEnvFile(filePath, filterProject || undefined, filterRuntime || undefined),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["env-vars"] });
    },
  });

  const startEdit = (envVar: EnvVar) => {
    setEditingId(envVar.id);
    setEditKey(envVar.key);
    setEditValue(envVar.value);
  };

  const saveEdit = () => {
    if (editingId) {
      updateMutation.mutate({ id: editingId, key: editKey, value: editValue });
    }
  };

  const handleImport = async () => {
    // In a real app, we'd use a file picker dialog
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".env,.env.*";
    input.onchange = async (e) => {
      const file = (e.target as HTMLInputElement).files?.[0];
      if (file) {
        // Note: In Tauri, we'd use the dialog plugin instead
        // For now, just show the path concept
        alert("In production, this would import from: " + file.name);
      }
    };
    input.click();
  };

  const exportEnvFile = () => {
    const content = envVars.map((ev) => `${ev.key}=${ev.value}`).join("\n");
    const blob = new Blob([content], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = ".env";
    a.click();
    URL.revokeObjectURL(url);
  };

  const getProjectName = (projectId?: string) => {
    if (!projectId) return "Global";
    return projects.find((p) => p.id === projectId)?.name || projectId;
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="animate-spin text-cs-accent" size={32} />
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold flex items-center gap-2">
            <Variable className="text-cs-accent" size={24} />
            {t("env.title", "Environment Variables")}
          </h2>
          <p className="text-sm text-cs-muted mt-1">
            {t("env.subtitle", "Manage environment variables per project and runtime")}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={handleImport}
            className="flex items-center gap-2 px-3 py-2 rounded-md border border-cs-border text-sm hover:bg-cs-border/50 transition-colors"
          >
            <Upload size={14} />
            Import .env
          </button>
          <button
            onClick={exportEnvFile}
            disabled={envVars.length === 0}
            className="flex items-center gap-2 px-3 py-2 rounded-md border border-cs-border text-sm hover:bg-cs-border/50 transition-colors disabled:opacity-50"
          >
            <Download size={14} />
            Export
          </button>
          <button
            onClick={() => refetch()}
            className="p-2 rounded-md border border-cs-border hover:bg-cs-border/50 transition-colors"
          >
            <RefreshCw size={16} />
          </button>
          <button
            onClick={() => setShowAdd(true)}
            className="flex items-center gap-2 px-3 py-2 rounded-md bg-cs-accent text-black text-sm font-medium hover:bg-cs-accent/90 transition-colors"
          >
            <Plus size={16} />
            Add Variable
          </button>
        </div>
      </div>

      {/* Filters */}
      <div className="flex items-center gap-4">
        <div className="flex items-center gap-2">
          <FolderOpen size={14} className="text-cs-muted" />
          <select
            value={filterProject}
            onChange={(e) => setFilterProject(e.target.value)}
            className="bg-cs-card border border-cs-border rounded-md px-3 py-1.5 text-sm focus:outline-none focus:border-cs-accent"
          >
            <option value="">All Projects</option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </div>
        <select
          value={filterRuntime}
          onChange={(e) => setFilterRuntime(e.target.value)}
          className="bg-cs-card border border-cs-border rounded-md px-3 py-1.5 text-sm focus:outline-none focus:border-cs-accent"
        >
          {RUNTIMES.map((rt) => (
            <option key={rt.value} value={rt.value}>
              {rt.label}
            </option>
          ))}
        </select>
        <span className="text-xs text-cs-muted">
          {envVars.length} variable{envVars.length !== 1 ? "s" : ""}
        </span>
      </div>

      {/* Add form */}
      {showAdd && (
        <div className="border border-cs-border rounded-lg p-4 bg-cs-card">
          <h3 className="font-medium mb-4">Add Environment Variable</h3>
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="block text-sm font-medium mb-1.5">Key</label>
              <input
                type="text"
                placeholder="e.g., DATABASE_URL"
                value={newKey}
                onChange={(e) => setNewKey(e.target.value.toUpperCase().replace(/[^A-Z0-9_]/g, "_"))}
                className="w-full px-3 py-2 rounded-md border border-cs-border bg-cs-bg text-sm font-mono focus:outline-none focus:border-cs-accent"
              />
            </div>
            <div>
              <label className="block text-sm font-medium mb-1.5">Value</label>
              <input
                type="text"
                placeholder="Enter value"
                value={newValue}
                onChange={(e) => setNewValue(e.target.value)}
                className="w-full px-3 py-2 rounded-md border border-cs-border bg-cs-bg text-sm focus:outline-none focus:border-cs-accent"
              />
            </div>
            <div>
              <label className="block text-sm font-medium mb-1.5">Project</label>
              <select
                value={newProjectId}
                onChange={(e) => setNewProjectId(e.target.value)}
                className="w-full px-3 py-2 rounded-md border border-cs-border bg-cs-bg text-sm focus:outline-none focus:border-cs-accent"
              >
                <option value="">Global</option>
                {projects.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
            </div>
            <div>
              <label className="block text-sm font-medium mb-1.5">Runtime</label>
              <select
                value={newRuntime}
                onChange={(e) => setNewRuntime(e.target.value)}
                className="w-full px-3 py-2 rounded-md border border-cs-border bg-cs-bg text-sm focus:outline-none focus:border-cs-accent"
              >
                {RUNTIMES.map((rt) => (
                  <option key={rt.value} value={rt.value}>
                    {rt.label}
                  </option>
                ))}
              </select>
            </div>
          </div>
          <div className="flex justify-end gap-2 mt-4">
            <button
              onClick={() => setShowAdd(false)}
              className="px-4 py-2 rounded-md text-sm hover:bg-cs-border transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={() => saveMutation.mutate()}
              disabled={!newKey.trim() || !newValue.trim() || saveMutation.isPending}
              className="flex items-center gap-2 px-4 py-2 rounded-md bg-cs-accent text-black text-sm font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50"
            >
              {saveMutation.isPending && <Loader2 size={14} className="animate-spin" />}
              Add Variable
            </button>
          </div>
        </div>
      )}

      {/* Variables list */}
      {envVars.length === 0 ? (
        <div className="text-center py-12 text-cs-muted">
          <Variable size={48} className="mx-auto mb-4 opacity-50" />
          <p>No environment variables</p>
          <p className="text-sm mt-1">Add variables or import from a .env file</p>
        </div>
      ) : (
        <div className="border border-cs-border rounded-lg overflow-hidden">
          <table className="w-full">
            <thead className="bg-cs-card border-b border-cs-border">
              <tr>
                <th className="text-left px-4 py-3 text-sm font-medium text-cs-muted">Key</th>
                <th className="text-left px-4 py-3 text-sm font-medium text-cs-muted">Value</th>
                <th className="text-left px-4 py-3 text-sm font-medium text-cs-muted">Project</th>
                <th className="text-left px-4 py-3 text-sm font-medium text-cs-muted">Runtime</th>
                <th className="text-right px-4 py-3 text-sm font-medium text-cs-muted">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-cs-border">
              {envVars.map((envVar) => (
                <tr key={envVar.id} className="hover:bg-cs-card/50">
                  <td className="px-4 py-3">
                    {editingId === envVar.id ? (
                      <input
                        type="text"
                        value={editKey}
                        onChange={(e) => setEditKey(e.target.value)}
                        className="px-2 py-1 rounded border border-cs-border bg-cs-bg text-sm font-mono w-full focus:outline-none focus:border-cs-accent"
                      />
                    ) : (
                      <code className="text-sm font-mono text-cs-accent">{envVar.key}</code>
                    )}
                  </td>
                  <td className="px-4 py-3">
                    {editingId === envVar.id ? (
                      <input
                        type="text"
                        value={editValue}
                        onChange={(e) => setEditValue(e.target.value)}
                        className="px-2 py-1 rounded border border-cs-border bg-cs-bg text-sm w-full focus:outline-none focus:border-cs-accent"
                      />
                    ) : (
                      <span className="text-sm truncate max-w-[200px] inline-block">
                        {envVar.value.length > 30 ? envVar.value.slice(0, 30) + "..." : envVar.value}
                      </span>
                    )}
                  </td>
                  <td className="px-4 py-3 text-sm text-cs-muted">
                    {getProjectName(envVar.projectId)}
                  </td>
                  <td className="px-4 py-3 text-sm text-cs-muted capitalize">
                    {envVar.runtime || "All"}
                  </td>
                  <td className="px-4 py-3 text-right">
                    {editingId === envVar.id ? (
                      <div className="flex items-center justify-end gap-1">
                        <button
                          onClick={saveEdit}
                          className="p-1.5 rounded hover:bg-cs-border text-green-400"
                        >
                          <Check size={14} />
                        </button>
                        <button
                          onClick={() => setEditingId(null)}
                          className="p-1.5 rounded hover:bg-cs-border text-red-400"
                        >
                          <X size={14} />
                        </button>
                      </div>
                    ) : (
                      <div className="flex items-center justify-end gap-1">
                        <button
                          onClick={() => startEdit(envVar)}
                          className="p-1.5 rounded hover:bg-cs-border transition-colors"
                        >
                          <Edit2 size={14} />
                        </button>
                        <button
                          onClick={() => deleteMutation.mutate(envVar.id)}
                          className="p-1.5 rounded hover:bg-cs-border text-red-400 transition-colors"
                        >
                          <Trash2 size={14} />
                        </button>
                      </div>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
