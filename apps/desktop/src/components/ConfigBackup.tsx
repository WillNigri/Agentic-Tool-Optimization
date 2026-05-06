import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Download, Upload, Loader2, AlertCircle, CheckCircle2, FileJson } from "lucide-react";
import { save, open } from "@tauri-apps/plugin-dialog";
import { writeTextFile, readTextFile } from "@tauri-apps/plugin-fs";
import {
  exportConfiguration,
  importConfiguration,
  type ImportSummary,
} from "@/lib/configBackup";

// v1.4.0 Polish-T4 — Settings → Backup tab.
//
// Export writes a single .json snapshot to disk. Import reads one back and
// re-inserts rows with INSERT OR REPLACE. Secrets / LLM key VALUES are
// excluded by design — only metadata travels — so the user re-enters them on
// the new machine. We surface that gap in the import summary.

const TABLE_LABELS: Record<keyof ImportSummary, string> = {
  agents: "Agents",
  agentVariables: "Agent variables",
  agentHooks: "Agent hooks",
  agentGroups: "Agent groups",
  agentGroupMembers: "Group memberships",
  projects: "Projects",
  envVars: "Environment vars",
  modelConfigs: "Model configs",
  secretsMeta: "Secrets (names only)",
  llmApiKeysMeta: "API keys (metadata only)",
  settings: "App settings",
};

export default function ConfigBackup() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [exporting, setExporting] = useState(false);
  const [importing, setImporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lastSummary, setLastSummary] = useState<ImportSummary | null>(null);
  const [lastExportPath, setLastExportPath] = useState<string | null>(null);

  const handleExport = async () => {
    if (exporting) return;
    setError(null);
    setExporting(true);
    try {
      const backup = await exportConfiguration();
      const filePath = await save({
        title: "Save ATO configuration backup",
        defaultPath: `ato-backup-${new Date().toISOString().slice(0, 10)}.json`,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!filePath) {
        // User cancelled.
        setExporting(false);
        return;
      }
      await writeTextFile(filePath, JSON.stringify(backup, null, 2));
      setLastExportPath(filePath);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setExporting(false);
    }
  };

  const handleImport = async () => {
    if (importing) return;
    setError(null);
    setLastSummary(null);
    setImporting(true);
    try {
      const filePath = await open({
        title: "Pick an ATO backup .json",
        multiple: false,
        directory: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!filePath || Array.isArray(filePath)) {
        setImporting(false);
        return;
      }
      const text = await readTextFile(filePath);
      const summary = await importConfiguration(text);
      setLastSummary(summary);
      // Refetch every query that backs config-driven UI.
      void queryClient.invalidateQueries();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setImporting(false);
    }
  };

  return (
    <div className="space-y-5">
      <div>
        <h2 className="text-xl font-semibold text-cs-text mb-1">
          {t("settings.backup.title", "Configuration backup")}
        </h2>
        <p className="text-sm text-cs-muted">
          {t(
            "settings.backup.subtitle",
            "Export your local config so you can move to another machine or roll back. Plain JSON — diff-friendly, version-control-friendly, no surprises."
          )}
        </p>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <div className="rounded-lg border border-cs-border bg-cs-card p-4 space-y-3">
          <div className="flex items-center gap-2">
            <Download size={16} className="text-cs-accent" />
            <h3 className="text-sm font-medium text-cs-text">
              {t("settings.backup.exportTitle", "Export")}
            </h3>
          </div>
          <p className="text-xs text-cs-muted">
            {t(
              "settings.backup.exportBody",
              "Snapshots your agents, hooks, variables, groups, projects, env vars and model configs. Secret values stay in your keychain — only their names travel."
            )}
          </p>
          <button
            type="button"
            onClick={handleExport}
            disabled={exporting}
            className="inline-flex items-center gap-2 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover disabled:opacity-50"
          >
            {exporting ? <Loader2 size={12} className="animate-spin" /> : <Download size={12} />}
            {exporting
              ? t("settings.backup.exporting", "Exporting…")
              : t("settings.backup.exportCta", "Export to JSON")}
          </button>
          {lastExportPath && (
            <div className="flex items-start gap-2 rounded-md border border-cs-accent/30 bg-cs-accent/5 p-2">
              <CheckCircle2 size={12} className="text-cs-accent shrink-0 mt-0.5" />
              <div className="min-w-0">
                <p className="text-[11px] text-cs-text">
                  {t("settings.backup.exportSaved", "Saved")}
                </p>
                <p className="text-[10px] text-cs-muted font-mono break-all">{lastExportPath}</p>
              </div>
            </div>
          )}
        </div>

        <div className="rounded-lg border border-cs-border bg-cs-card p-4 space-y-3">
          <div className="flex items-center gap-2">
            <Upload size={16} className="text-cs-accent" />
            <h3 className="text-sm font-medium text-cs-text">
              {t("settings.backup.importTitle", "Import")}
            </h3>
          </div>
          <p className="text-xs text-cs-muted">
            {t(
              "settings.backup.importBody",
              "Restores rows via INSERT OR REPLACE — existing entries with the same id are overwritten. You'll need to re-enter API keys and secret values."
            )}
          </p>
          <button
            type="button"
            onClick={handleImport}
            disabled={importing}
            className="inline-flex items-center gap-2 rounded-md border border-cs-border bg-cs-bg-raised px-3 py-1.5 text-xs font-medium text-cs-text hover:border-cs-accent/40 disabled:opacity-50"
          >
            {importing ? <Loader2 size={12} className="animate-spin" /> : <FileJson size={12} />}
            {importing
              ? t("settings.backup.importing", "Importing…")
              : t("settings.backup.importCta", "Pick a backup .json")}
          </button>
        </div>
      </div>

      {error && (
        <div className="flex items-start gap-2 rounded-md border border-cs-danger/40 bg-cs-danger/10 p-3">
          <AlertCircle size={14} className="text-cs-danger shrink-0 mt-0.5" />
          <span className="text-xs text-cs-text">{error}</span>
        </div>
      )}

      {lastSummary && (
        <div className="rounded-lg border border-cs-accent/30 bg-cs-accent/5 p-4 space-y-2">
          <div className="flex items-center gap-2">
            <CheckCircle2 size={16} className="text-cs-accent" />
            <h3 className="text-sm font-medium text-cs-text">
              {t("settings.backup.imported", "Imported")}
            </h3>
          </div>
          <table className="w-full text-xs">
            <tbody>
              {(Object.keys(TABLE_LABELS) as Array<keyof ImportSummary>).map((key) => (
                <tr key={key} className="border-t border-cs-border/40 first:border-t-0">
                  <td className="py-1.5 text-cs-muted">{TABLE_LABELS[key]}</td>
                  <td className="py-1.5 text-right font-mono text-cs-text">{lastSummary[key]}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {(lastSummary.secretsMeta > 0 || lastSummary.llmApiKeysMeta > 0) && (
            <p className="text-[11px] text-cs-muted italic">
              {t(
                "settings.backup.secretsReminder",
                "Secret and API-key values were not in the backup. Re-enter them under Settings → Secrets / API Keys."
              )}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
