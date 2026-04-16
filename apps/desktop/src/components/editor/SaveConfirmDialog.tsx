import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, Check, Loader2, Shield, X } from "lucide-react";
import { cn } from "@/lib/utils";
import {
  previewWriteAgentConfigFile,
  type DiffLine,
  type ValidationResult,
  type WritePreview,
} from "@/lib/api";

interface SaveConfirmDialogProps {
  open: boolean;
  filePath: string;
  newContent: string;
  onConfirm: () => void;
  onCancel: () => void;
  saving?: boolean;
}

const GLOBAL_CONFIG_PATHS = [
  "/.claude/settings.json",
  "/.claude/settings.local.json",
  "/.codex/config.toml",
  "/.hermes/config.yaml",
  "/.openclaw/openclaw.json",
];

function isGlobalConfig(path: string): boolean {
  return GLOBAL_CONFIG_PATHS.some((suffix) => path.endsWith(suffix));
}

export default function SaveConfirmDialog({
  open,
  filePath,
  newContent,
  onConfirm,
  onCancel,
  saving,
}: SaveConfirmDialogProps) {
  const { t } = useTranslation();
  const [preview, setPreview] = useState<WritePreview | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmed, setConfirmed] = useState(false);

  const globalScope = isGlobalConfig(filePath);

  useEffect(() => {
    if (!open) return;
    setConfirmed(false);
    setPreview(null);
    setError(null);
    setLoading(true);
    previewWriteAgentConfigFile(filePath, newContent)
      .then((p) => setPreview(p))
      .catch((e) => setError(e instanceof Error ? e.message : String(e)))
      .finally(() => setLoading(false));
  }, [open, filePath, newContent]);

  if (!open) return null;

  const validation: ValidationResult | null = preview?.validation ?? null;
  const hasValidationErrors = validation ? !validation.valid : false;
  const canConfirm =
    !loading &&
    !!preview &&
    !hasValidationErrors &&
    (!globalScope || confirmed);

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 p-4">
      <div className="w-full max-w-2xl max-h-[85vh] flex flex-col rounded-xl border border-cs-border bg-cs-card shadow-2xl">
        {/* Header */}
        <div className="flex items-start justify-between gap-3 border-b border-cs-border px-5 py-4">
          <div className="flex items-start gap-3">
            <div className={cn(
              "mt-0.5 rounded-md p-1.5",
              globalScope ? "bg-yellow-500/10 text-yellow-400" : "bg-cs-accent/10 text-cs-accent"
            )}>
              {globalScope ? <AlertTriangle size={16} /> : <Shield size={16} />}
            </div>
            <div>
              <h3 className="text-sm font-semibold">
                {globalScope ? t("editor.reviewGlobalChange", "Review global config change") : t("editor.confirmSave", "Confirm save")}
              </h3>
              <p className="mt-0.5 text-[11px] text-cs-muted font-mono truncate max-w-[480px]">
                {filePath}
              </p>
            </div>
          </div>
          <button
            onClick={onCancel}
            disabled={saving}
            className="rounded p-1 text-cs-muted transition-colors hover:bg-cs-border hover:text-cs-text disabled:opacity-50"
          >
            <X size={16} />
          </button>
        </div>

        {/* Body */}
        <div className="flex-1 overflow-y-auto px-5 py-4">
          {loading && (
            <div className="flex items-center justify-center py-10 text-cs-muted text-xs gap-2">
              <Loader2 size={14} className="animate-spin" /> {t("editor.computingDiff", "Computing diff…")}
            </div>
          )}

          {error && (
            <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-300">
              {error}
            </div>
          )}

          {preview && (
            <>
              <div className="mb-3 flex flex-wrap items-center gap-2 text-[11px]">
                <span className="rounded bg-green-500/10 px-2 py-0.5 text-green-400 font-mono">
                  +{preview.addedLines}
                </span>
                <span className="rounded bg-red-500/10 px-2 py-0.5 text-red-400 font-mono">
                  −{preview.removedLines}
                </span>
                <span className="text-cs-muted font-mono">
                  {preview.currentHash.slice(0, 8)} → {preview.newHash.slice(0, 8)}
                </span>
                {preview.addedLines === 0 && preview.removedLines === 0 && (
                  <span className="text-cs-muted italic">No changes</span>
                )}
              </div>

              {hasValidationErrors && (
                <div className="mb-3 rounded-lg border border-red-500/30 bg-red-500/5 px-3 py-2">
                  <p className="mb-1.5 text-xs font-semibold text-red-300">{t("editor.schemaFailed", "Schema validation failed")}</p>
                  <ul className="space-y-1 text-[11px] text-red-300/90">
                    {validation!.errors.map((err, i) => (
                      <li key={i} className="font-mono">
                        <span className="opacity-60">{err.field}:</span> {err.message}
                      </li>
                    ))}
                  </ul>
                </div>
              )}

              {validation?.valid && (
                <div className="mb-3 flex items-center gap-1.5 text-[11px] text-cs-success">
                  <Check size={12} /> {t("editor.schemaPass", "Schema validation passed")}
                </div>
              )}

              {preview.diff.length === 0 ? (
                <div className="rounded-lg border border-cs-border bg-cs-bg/50 px-3 py-6 text-center text-xs text-cs-muted">
                  {t("editor.noChanges", "Files are identical.")}
                </div>
              ) : (
                <div className="rounded-lg border border-cs-border bg-cs-bg overflow-hidden">
                  <pre className="max-h-80 overflow-y-auto p-0 text-[11px] font-mono leading-relaxed">
                    {preview.diff.map((line, idx) => (
                      <DiffRow key={idx} line={line} />
                    ))}
                  </pre>
                </div>
              )}
            </>
          )}
        </div>

        {/* Footer */}
        <div className="border-t border-cs-border px-5 py-3">
          {globalScope && !hasValidationErrors && (
            <label className="mb-3 flex items-start gap-2 text-[11px] text-cs-muted cursor-pointer">
              <input
                type="checkbox"
                checked={confirmed}
                onChange={(e) => setConfirmed(e.target.checked)}
                className="mt-0.5 accent-yellow-400"
              />
              <span>
                I understand this edits a <span className="text-yellow-400">global config</span> that
                affects all projects using this runtime.
              </span>
            </label>
          )}

          <div className="flex items-center justify-end gap-2">
            <button
              onClick={onCancel}
              disabled={saving}
              className="rounded-md px-3 py-1.5 text-xs text-cs-muted transition-colors hover:bg-cs-border hover:text-cs-text disabled:opacity-50"
            >
              Cancel
            </button>
            <button
              onClick={onConfirm}
              disabled={!canConfirm || saving}
              className={cn(
                "flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium transition-colors",
                canConfirm
                  ? "bg-cs-accent text-cs-bg hover:bg-cs-accent/90"
                  : "bg-cs-border/50 text-cs-muted cursor-not-allowed"
              )}
            >
              {saving ? (
                <>
                  <Loader2 size={12} className="animate-spin" /> Saving…
                </>
              ) : (
                <>
                  <Check size={12} /> Confirm save
                </>
              )}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function DiffRow({ line }: { line: DiffLine }) {
  const base = "flex gap-2 px-3 py-0.5";
  if (line.kind === "add") {
    return (
      <div className={cn(base, "bg-green-500/10 text-green-300")}>
        <span className="w-8 shrink-0 select-none text-right opacity-50">{line.newLine ?? ""}</span>
        <span className="w-3 shrink-0 select-none">+</span>
        <span className="flex-1 whitespace-pre-wrap break-words">{line.text || "\u00A0"}</span>
      </div>
    );
  }
  if (line.kind === "remove") {
    return (
      <div className={cn(base, "bg-red-500/10 text-red-300")}>
        <span className="w-8 shrink-0 select-none text-right opacity-50">{line.oldLine ?? ""}</span>
        <span className="w-3 shrink-0 select-none">−</span>
        <span className="flex-1 whitespace-pre-wrap break-words">{line.text || "\u00A0"}</span>
      </div>
    );
  }
  return (
    <div className={cn(base, "text-cs-muted")}>
      <span className="w-8 shrink-0 select-none text-right opacity-40">{line.oldLine ?? ""}</span>
      <span className="w-3 shrink-0 select-none opacity-40"> </span>
      <span className="flex-1 whitespace-pre-wrap break-words">{line.text || "\u00A0"}</span>
    </div>
  );
}
