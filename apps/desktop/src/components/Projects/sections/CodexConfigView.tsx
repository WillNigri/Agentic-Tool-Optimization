import { useState, useEffect } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { Settings, Save, Edit3, X, Loader2, ExternalLink } from "lucide-react";
import { readAgentConfigFile, writeTomlConfig } from "@/lib/api";
import SectionShell, { EmptyRow } from "./SectionShell";
import { cn } from "@/lib/utils";

interface CodexConfigViewProps {
  configPath: string;
  onOpenRaw: (path: string) => void;
}

interface ConfigFields {
  model: string;
  temperature: string;
  maxTokens: string;
  env: Record<string, string>;
  sandbox: Record<string, string>;
}

function parseConfigToFields(parsed: unknown): ConfigFields {
  const obj = parsed as Record<string, unknown> ?? {};
  const model = (obj as Record<string, unknown>)?.model as Record<string, unknown> ?? {};
  const sandbox = (obj as Record<string, unknown>)?.sandbox as Record<string, unknown> ?? {};
  const env = (obj as Record<string, unknown>)?.env as Record<string, string> ?? {};

  return {
    model: String(model?.name ?? model?.model ?? ""),
    temperature: String(model?.temperature ?? ""),
    maxTokens: String(model?.max_tokens ?? model?.maxTokens ?? ""),
    env: typeof env === "object" ? env : {},
    sandbox: Object.fromEntries(
      Object.entries(sandbox).map(([k, v]) => [k, String(v)])
    ),
  };
}

function fieldsToConfigJson(fields: ConfigFields): Record<string, unknown> {
  const config: Record<string, unknown> = {};
  if (fields.model || fields.temperature || fields.maxTokens) {
    const model: Record<string, unknown> = {};
    if (fields.model) model.name = fields.model;
    if (fields.temperature) model.temperature = parseFloat(fields.temperature) || 0;
    if (fields.maxTokens) model.max_tokens = parseInt(fields.maxTokens) || 4096;
    config.model = model;
  }
  if (Object.keys(fields.env).length > 0) {
    config.env = fields.env;
  }
  if (Object.keys(fields.sandbox).length > 0) {
    const sb: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(fields.sandbox)) {
      if (v === "true") sb[k] = true;
      else if (v === "false") sb[k] = false;
      else if (/^\d+$/.test(v)) sb[k] = parseInt(v);
      else sb[k] = v;
    }
    config.sandbox = sb;
  }
  return config;
}

export default function CodexConfigView({ configPath, onOpenRaw }: CodexConfigViewProps) {
  const queryClient = useQueryClient();
  const [editing, setEditing] = useState(false);
  const [fields, setFields] = useState<ConfigFields>({ model: "", temperature: "", maxTokens: "", env: {}, sandbox: {} });
  const [newEnvKey, setNewEnvKey] = useState("");

  const { data, isLoading } = useQuery({
    queryKey: ["config-file", configPath],
    queryFn: () => readAgentConfigFile(configPath),
    staleTime: 10_000,
  });

  useEffect(() => {
    if (data?.content) {
      setFields(parseConfigToFields(data.content));
    }
  }, [data]);

  const saveMutation = useMutation({
    mutationFn: () => writeTomlConfig(configPath, fieldsToConfigJson(fields)),
    onSuccess: () => {
      setEditing(false);
      queryClient.invalidateQueries({ queryKey: ["config-file", configPath] });
      queryClient.invalidateQueries({ queryKey: ["project-bundle"] });
    },
  });

  if (isLoading) {
    return <div className="flex items-center gap-2 py-4 text-xs text-cs-muted"><Loader2 size={12} className="animate-spin" /> Loading config…</div>;
  }

  if (!data) {
    return <EmptyRow message="config.toml not found." />;
  }

  return (
    <SectionShell
      icon={Settings}
      title="Codex Configuration"
      subtitle="config.toml — model, environment, and sandbox settings"
      actions={!editing ? (
        <div className="flex items-center gap-2">
          <button onClick={() => setEditing(true)} className="flex items-center gap-1 text-[10px] text-cs-muted hover:text-cs-accent"><Edit3 size={10} /> Edit</button>
          <button onClick={() => onOpenRaw(configPath)} className="flex items-center gap-1 text-[10px] text-cs-muted hover:text-cs-accent"><ExternalLink size={10} /> Raw TOML</button>
        </div>
      ) : undefined}
    >
      <div className="space-y-4">
        {/* Model settings */}
        <div>
          <h4 className="mb-2 text-[10px] font-medium text-cs-muted uppercase tracking-wide">Model</h4>
          <div className="grid grid-cols-3 gap-2">
            <FormField label="Model name" value={fields.model} editing={editing} onChange={(v) => setFields({ ...fields, model: v })} placeholder="gpt-4o" />
            <FormField label="Temperature" value={fields.temperature} editing={editing} onChange={(v) => setFields({ ...fields, temperature: v })} placeholder="0.7" type="number" />
            <FormField label="Max tokens" value={fields.maxTokens} editing={editing} onChange={(v) => setFields({ ...fields, maxTokens: v })} placeholder="4096" type="number" />
          </div>
        </div>

        {/* Env vars */}
        <div>
          <h4 className="mb-2 text-[10px] font-medium text-cs-muted uppercase tracking-wide">
            Environment ({Object.keys(fields.env).length})
          </h4>
          {Object.keys(fields.env).length === 0 && !editing ? (
            <p className="text-[11px] text-cs-muted">No environment variables set.</p>
          ) : (
            <div className="space-y-1">
              {Object.entries(fields.env).map(([key, val]) => (
                <div key={key} className="flex items-center gap-2 rounded-md border border-cs-border/60 bg-cs-bg/40 px-2 py-1">
                  <code className="text-[11px] font-mono text-cs-accent w-32 shrink-0 truncate">{key}</code>
                  {editing ? (
                    <>
                      <input
                        value={val}
                        onChange={(e) => setFields({ ...fields, env: { ...fields.env, [key]: e.target.value } })}
                        className="flex-1 rounded border border-cs-border bg-cs-bg px-2 py-0.5 text-[11px] focus:outline-none focus:border-cs-accent"
                      />
                      <button
                        onClick={() => {
                          const newEnv = { ...fields.env };
                          delete newEnv[key];
                          setFields({ ...fields, env: newEnv });
                        }}
                        className="text-red-400 text-[10px] hover:bg-red-500/10 rounded px-1"
                      >
                        ✕
                      </button>
                    </>
                  ) : (
                    <span className="text-[11px] font-mono truncate text-cs-muted">{val}</span>
                  )}
                </div>
              ))}
              {editing && (
                <div className="flex items-center gap-2 mt-1">
                  <input
                    value={newEnvKey}
                    onChange={(e) => setNewEnvKey(e.target.value)}
                    placeholder="NEW_VAR"
                    className="w-32 rounded border border-cs-border bg-cs-bg px-2 py-0.5 text-[11px] font-mono focus:outline-none focus:border-cs-accent"
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && newEnvKey.trim()) {
                        setFields({ ...fields, env: { ...fields.env, [newEnvKey.trim()]: "" } });
                        setNewEnvKey("");
                      }
                    }}
                  />
                  <button
                    onClick={() => { if (newEnvKey.trim()) { setFields({ ...fields, env: { ...fields.env, [newEnvKey.trim()]: "" } }); setNewEnvKey(""); } }}
                    className="text-[10px] text-cs-accent hover:bg-cs-accent/10 rounded px-2 py-0.5"
                  >
                    + Add
                  </button>
                </div>
              )}
            </div>
          )}
        </div>

        {/* Edit controls */}
        {editing && (
          <div className="flex items-center justify-end gap-2 pt-2 border-t border-cs-border/60">
            <button onClick={() => { setEditing(false); if (data?.content) setFields(parseConfigToFields(data.content)); }} className="px-3 py-1 rounded text-xs text-cs-muted hover:bg-cs-border">
              <X size={11} className="inline mr-1" />Cancel
            </button>
            <button
              onClick={() => saveMutation.mutate()}
              disabled={saveMutation.isPending}
              className="flex items-center gap-1 px-3 py-1 rounded text-xs font-medium bg-cs-accent text-cs-bg hover:bg-cs-accent/90 disabled:opacity-50"
            >
              {saveMutation.isPending ? <Loader2 size={11} className="animate-spin" /> : <Save size={11} />} Save as TOML
            </button>
          </div>
        )}
        {saveMutation.isError && (
          <p className="text-[11px] text-red-300">{saveMutation.error instanceof Error ? saveMutation.error.message : "Save failed"}</p>
        )}
      </div>
    </SectionShell>
  );
}

function FormField({ label, value, editing, onChange, placeholder, type = "text" }: {
  label: string; value: string; editing: boolean; onChange: (v: string) => void; placeholder?: string; type?: string;
}) {
  return (
    <div className="rounded-md border border-cs-border/60 bg-cs-bg/40 px-3 py-2">
      <div className="mb-1 text-[10px] text-cs-muted uppercase tracking-wide">{label}</div>
      {editing ? (
        <input
          type={type}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          className="w-full rounded border border-cs-border bg-cs-bg px-2 py-0.5 text-xs focus:outline-none focus:border-cs-accent"
        />
      ) : (
        <div className={cn("text-sm font-mono", value ? "" : "text-cs-muted/50")}>{value || "—"}</div>
      )}
    </div>
  );
}
