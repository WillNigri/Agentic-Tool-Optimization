import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Folder, FolderPlus, X, KeyRound, Database } from "lucide-react";
import { useProjectStore } from "@/stores/useProjectStore";
import { detectPlaceholders, type McpRegistryEntry } from "@/lib/mcpRegistry";

// v1.3.0 — Inline form for MCP install-time options (T4.b).
// Filesystem MCP: pick directories the agent can read/write.
// Postgres / SQLite: enter the DATABASE_URL / DB_PATH.
// Anything else with $VAR placeholders: prompt for the value.
//
// Surfaces only when the entry has detectable placeholders. Returns the
// resolved values map to the parent, which passes it to installMcpToRuntime().

export type OptionValues = Record<string, string | string[]>;

interface Props {
  entry: McpRegistryEntry;
  values: OptionValues;
  onChange: (values: OptionValues) => void;
}

const PATH_HEURISTIC = /(PATH|DIR|FOLDER|ROOT)$/;
const URL_HEURISTIC = /(URL|URI|ENDPOINT|HOST)$/;

function fieldKindFor(name: string): "paths" | "url" | "text" {
  if (PATH_HEURISTIC.test(name)) return "paths";
  if (URL_HEURISTIC.test(name)) return "url";
  return "text";
}

function fieldLabelFor(name: string, t: (k: string, defaultValue?: string) => string): string {
  if (name === "PROJECT_PATH") return t("mcpInstall.allowedPaths", "Folders the agent can access");
  if (name === "DATABASE_URL") return t("mcpInstall.databaseUrl", "Database URL");
  if (name === "DB_PATH") return t("mcpInstall.dbPath", "Database file path");
  return name;
}

export default function McpInstallOptions({ entry, values, onChange }: Props) {
  const { t } = useTranslation();
  const placeholders = detectPlaceholders(entry);
  const activeProject = useProjectStore((s) => s.activeProject);

  // Seed defaults the first time we see this entry.
  useEffect(() => {
    if (placeholders.length === 0) return;
    let next = values;
    let changed = false;
    for (const p of placeholders) {
      if (next[p] !== undefined) continue;
      const kind = fieldKindFor(p);
      if (kind === "paths") {
        next = {
          ...next,
          [p]: activeProject?.path ? [activeProject.path] : [],
        };
        changed = true;
      } else {
        next = { ...next, [p]: "" };
        changed = true;
      }
    }
    if (changed) onChange(next);
  // intentional one-shot per entry
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entry.id]);

  if (placeholders.length === 0) return null;

  return (
    <div className="mt-2 rounded-md border border-cs-border bg-cs-bg p-3 space-y-3">
      {placeholders.map((p) => {
        const kind = fieldKindFor(p);
        if (kind === "paths") {
          return (
            <PathsField
              key={p}
              name={p}
              label={fieldLabelFor(p, t as (k: string, d?: string) => string)}
              value={Array.isArray(values[p]) ? (values[p] as string[]) : []}
              onChange={(paths) => onChange({ ...values, [p]: paths })}
            />
          );
        }
        return (
          <TextField
            key={p}
            name={p}
            label={fieldLabelFor(p, t as (k: string, d?: string) => string)}
            kind={kind}
            value={typeof values[p] === "string" ? (values[p] as string) : ""}
            onChange={(v) => onChange({ ...values, [p]: v })}
          />
        );
      })}
    </div>
  );
}

function PathsField({
  name,
  label,
  value,
  onChange,
}: {
  name: string;
  label: string;
  value: string[];
  onChange: (paths: string[]) => void;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);

  const pickFolder = async () => {
    setBusy(true);
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const result = await open({
        directory: true,
        multiple: false,
        title: t("mcpInstall.pickFolderTitle", "Choose a folder the agent can access"),
      });
      if (result && typeof result === "string" && !value.includes(result)) {
        onChange([...value, result]);
      }
    } catch {
      // dialog cancelled / not Tauri
    } finally {
      setBusy(false);
    }
  };

  const remove = (path: string) => {
    onChange(value.filter((p) => p !== path));
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-1.5">
        <span className="text-[11px] uppercase tracking-wide text-cs-muted">
          {label}
          <code className="ml-1.5 text-[10px] text-cs-muted/60">${name}</code>
        </span>
        <span className="text-[10px] text-cs-muted">
          {value.length} {value.length === 1 ? t("mcpInstall.pathOne", "path") : t("mcpInstall.pathMany", "paths")}
        </span>
      </div>

      {value.length > 0 ? (
        <ul className="space-y-1 mb-1.5">
          {value.map((p) => (
            <li
              key={p}
              className="flex items-center gap-2 rounded border border-cs-border bg-cs-bg-raised px-2 py-1 text-xs"
            >
              <Folder size={11} className="text-cs-muted shrink-0" />
              <span className="font-mono text-cs-text truncate flex-1">{p}</span>
              <button
                type="button"
                onClick={() => remove(p)}
                aria-label={t("common.remove", "Remove")}
                className="text-cs-muted hover:text-cs-danger shrink-0"
              >
                <X size={11} />
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-[11px] text-cs-warning mb-1.5">
          ⚠ {t("mcpInstall.noPathsHint", "No paths picked — the agent won't be able to read or write any files.")}
        </p>
      )}

      <button
        type="button"
        onClick={pickFolder}
        disabled={busy}
        className="inline-flex items-center gap-1.5 rounded-md border border-cs-border bg-cs-bg-raised px-2.5 py-1 text-xs text-cs-text hover:border-cs-hover disabled:opacity-50"
      >
        <FolderPlus size={11} />
        {t("mcpInstall.addFolder", "Add folder")}
      </button>
    </div>
  );
}

function TextField({
  name,
  label,
  kind,
  value,
  onChange,
}: {
  name: string;
  label: string;
  kind: "url" | "text";
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <div>
      <div className="flex items-center justify-between mb-1">
        <span className="text-[11px] uppercase tracking-wide text-cs-muted">
          {label}
          <code className="ml-1.5 text-[10px] text-cs-muted/60">${name}</code>
        </span>
        {kind === "url" ? (
          <Database size={10} className="text-cs-muted" />
        ) : (
          <KeyRound size={10} className="text-cs-muted" />
        )}
      </div>
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={
          kind === "url" ? "postgres://user:pass@host:5432/db" : ""
        }
        className="w-full rounded-md border border-cs-border bg-cs-bg-raised px-2.5 py-1.5 text-xs text-cs-text font-mono focus:border-cs-accent focus:outline-none"
      />
    </div>
  );
}
