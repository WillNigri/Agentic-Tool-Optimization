import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, RefreshCw, ExternalLink, Check } from "lucide-react";

// v1.5.21 — Settings → About panel.
//
// People kept asking "am I on the latest?" — there was no version visible
// anywhere in the app, so the only way to check was a terminal `defaults
// read`. This panel surfaces the installed version + a manual "Check for
// updates" button that fires Tauri's updater on demand. The auto-check
// already runs at app launch (App.tsx); this is for the case where the
// user wants to verify mid-session.

const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type CheckState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "uptodate" }
  | { kind: "available"; version: string }
  | { kind: "installing" }
  | { kind: "error"; message: string };

export default function AboutPanel() {
  const { t } = useTranslation();
  const [version, setVersion] = useState<string | null>(null);
  const [tauriVersion, setTauriVersion] = useState<string | null>(null);
  const [check, setCheck] = useState<CheckState>({ kind: "idle" });

  useEffect(() => {
    if (!isTauri) {
      // Browser dev mode — fall back to package.json version baked at build
      // time. import.meta.env's VITE_APP_VERSION isn't set today; we'd need
      // to wire it through vite.config.ts. For now show "dev".
      setVersion("dev");
      return;
    }
    void (async () => {
      try {
        const { getVersion, getTauriVersion } = await import("@tauri-apps/api/app");
        const [v, tv] = await Promise.all([getVersion(), getTauriVersion()]);
        setVersion(v);
        setTauriVersion(tv);
      } catch {
        setVersion("?");
      }
    })();
  }, []);

  const onCheckForUpdates = async () => {
    if (!isTauri) return;
    setCheck({ kind: "checking" });
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      if (!update) {
        setCheck({ kind: "uptodate" });
        return;
      }
      setCheck({ kind: "available", version: update.version });
    } catch (err) {
      setCheck({ kind: "error", message: err instanceof Error ? err.message : String(err) });
    }
  };

  const onInstallUpdate = async () => {
    setCheck({ kind: "installing" });
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      if (!update) {
        setCheck({ kind: "uptodate" });
        return;
      }
      await update.downloadAndInstall();
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (err) {
      setCheck({ kind: "error", message: err instanceof Error ? err.message : String(err) });
    }
  };

  return (
    <div className="space-y-6">
      <section>
        <h3 className="text-xs font-semibold uppercase tracking-wide text-cs-muted mb-3">
          {t("about.versionTitle", "Version")}
        </h3>
        <div className="rounded-lg border border-cs-border bg-cs-bg-raised p-4 space-y-3">
          <Row
            label={t("about.atoVersion", "ATO version")}
            value={version === null ? <Loader2 size={11} className="animate-spin text-cs-muted" /> : <code className="font-mono text-cs-text">{version}</code>}
          />
          {tauriVersion && (
            <Row
              label={t("about.tauriVersion", "Tauri runtime")}
              value={<code className="font-mono text-cs-muted">{tauriVersion}</code>}
            />
          )}
          <Row
            label={t("about.platform", "Platform")}
            value={<code className="font-mono text-cs-muted">{typeof navigator !== "undefined" ? navigator.platform : "—"}</code>}
          />
        </div>
      </section>

      {isTauri && (
        <section>
          <h3 className="text-xs font-semibold uppercase tracking-wide text-cs-muted mb-3">
            {t("about.updatesTitle", "Updates")}
          </h3>
          <div className="rounded-lg border border-cs-border bg-cs-bg-raised p-4 space-y-3">
            <p className="text-xs text-cs-muted">
              {t(
                "about.updatesHint",
                "ATO checks for updates automatically at launch. You can also check manually here.",
              )}
            </p>

            {check.kind === "available" ? (
              <div className="flex items-center justify-between gap-3 rounded-md border border-cs-accent/40 bg-cs-accent/10 px-3 py-2">
                <span className="text-xs text-cs-accent">
                  {t("about.updateAvailable", "v{{v}} is available — click to install + relaunch.", { v: check.version })}
                </span>
                <button
                  type="button"
                  onClick={onInstallUpdate}
                  className="inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover"
                >
                  {t("about.installUpdate", "Update now")}
                </button>
              </div>
            ) : check.kind === "installing" ? (
              <div className="flex items-center gap-2 text-xs text-cs-muted">
                <Loader2 size={12} className="animate-spin" />
                {t("about.installing", "Downloading + applying update…")}
              </div>
            ) : check.kind === "uptodate" ? (
              <div className="flex items-center gap-2 text-xs text-cs-accent">
                <Check size={12} />
                {t("about.uptodate", "You're on the latest version.")}
              </div>
            ) : check.kind === "error" ? (
              <div className="rounded-md border border-cs-danger/40 bg-cs-danger/10 px-3 py-2 text-xs text-cs-text">
                {t("about.checkFailed", "Update check failed:")}{" "}
                <code className="font-mono">{check.message}</code>
              </div>
            ) : null}

            <button
              type="button"
              onClick={onCheckForUpdates}
              disabled={check.kind === "checking" || check.kind === "installing"}
              className="inline-flex items-center gap-1.5 rounded-md border border-cs-border bg-cs-bg px-3 py-1.5 text-xs font-medium text-cs-text hover:bg-cs-border/30 disabled:opacity-50"
            >
              {check.kind === "checking" ? <Loader2 size={11} className="animate-spin" /> : <RefreshCw size={11} />}
              {t("about.checkNow", "Check for updates")}
            </button>
          </div>
        </section>
      )}

      <section>
        <h3 className="text-xs font-semibold uppercase tracking-wide text-cs-muted mb-3">
          {t("about.linksTitle", "Links")}
        </h3>
        <div className="rounded-lg border border-cs-border bg-cs-bg-raised p-4 space-y-2">
          <Link href="https://agentictool.ai" label={t("about.website", "Website")} />
          <Link href="https://github.com/WillNigri/Agentic-Tool-Optimization/releases" label={t("about.releases", "Releases & changelog")} />
          <Link href="https://github.com/WillNigri/Agentic-Tool-Optimization/issues" label={t("about.issues", "Report a bug")} />
        </div>
      </section>
    </div>
  );
}

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-3">
      <span className="text-xs text-cs-muted">{label}</span>
      <span className="text-xs">{value}</span>
    </div>
  );
}

function Link({ href, label }: { href: string; label: string }) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      className="flex items-center gap-1.5 text-xs text-cs-accent hover:underline"
    >
      <ExternalLink size={11} />
      {label}
    </a>
  );
}
