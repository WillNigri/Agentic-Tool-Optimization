import { useEffect, useMemo, useState } from "react";
import { Check, Copy, ExternalLink, Loader2, Terminal } from "lucide-react";

// UpdateBanner — Felipe P2 (S4, 2026-05-18).
//
// When ATO is installed via `.deb` (or Snap), Tauri's in-process
// updater can't replace /usr/bin/ato-desktop — it's root-owned and
// dpkg-tracked, so the swap fails with EACCES and the "Update now"
// button silently does nothing. Felipe spent six days on v2.4.8
// before realizing v2.7.7 had shipped.
//
// This banner detects the install method via the
// `get_install_method` Tauri command and, when an update is
// available on a `.deb` install, renders a copy-pasteable
// `wget … && sudo apt install -y /tmp/ato.deb` command instead of
// the usual install-and-relaunch button. For other install methods
// it returns null so the existing auto-update flow continues to
// handle the normal cases.
//
// TODO(driver-wiring): this component is currently unmounted. The
// existing checkForUpdates() flow lives in apps/desktop/src/App.tsx
// (outside this session's write set per ownership rules). Desired
// swap semantics: when `get_install_method()` returns `deb` or
// `snap`, suppress the existing `ask()` dialog at App.tsx:17-25 and
// render <UpdateBanner /> in its place (e.g. as a toast / modal /
// top-of-Dashboard banner). For all other methods, fall through to
// the current `ask` + `downloadAndInstall` + `relaunch` flow
// unchanged. The banner itself self-renders null in the
// fall-through cases, so a naïve "always render <UpdateBanner />
// alongside ask()" wiring is *almost* right but double-prompts on
// deb/snap — explicit short-circuit is the cleaner shape.

const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type InstallMethod = "deb" | "appimage" | "snap" | "unknown" | "nonlinux";

type UpdateInfo = {
  version: string;
  currentVersion: string;
};

type BannerState =
  | { kind: "loading" }
  | { kind: "uptodate" }
  | { kind: "available"; update: UpdateInfo; method: InstallMethod }
  | { kind: "error"; message: string };

// Defense-in-depth against a compromised updater manifest (the
// Tauri updater signs it, but a leaked key would otherwise let an
// attacker pipe shell metacharacters straight into the rendered
// command). Mirrors installer_detect::is_safe_version (Rust).
function isSafeVersion(v: string): boolean {
  if (!v || v.length > 64) return false;
  return /^[A-Za-z0-9.\-+]+$/.test(v);
}

function isSafeReleaseUrl(u: string): boolean {
  if (!u || u.length > 2048) return false;
  if (!/^https?:\/\//.test(u)) return false;
  // Glob chars (`*?[]`) are excluded too — under zsh's default
  // failglob, an unmatched glob in an argument is a hard error and
  // the command refuses to run; under bash it can quietly expand to
  // a local-file match that changes wget's argument out from under
  // us. Signed manifest URLs never legitimately contain these.
  return !/[\s`$"'\\;|&><(){}*?[\]]/.test(u);
}

// Mirrors installer_detect::manual_update_command (Rust) for the
// .deb / Snap cases the banner actually renders. Returns null when
// the install method has no out-of-band upgrade path — caller falls
// through to the regular updater UI.
//
// .deb form is `apt install ./local.deb` rather than `dpkg -r` +
// `dpkg -i`: skipping the remove step means a failed wget can't
// leave Felipe with no ATO at all on a flaky WSL2 network, and apt
// resolves any dependency drift that raw dpkg would leave dangling
// (war-room S4 2026-05-20 Q5).
export function manualUpdateCommand(
  method: InstallMethod,
  version: string,
  releaseUrl: string,
): string | null {
  if (!isSafeVersion(version)) return null;
  if (method === "deb") {
    if (!isSafeReleaseUrl(releaseUrl)) return null;
    return `wget ${releaseUrl} -O /tmp/ato.deb && sudo apt install -y /tmp/ato.deb`;
  }
  if (method === "snap") {
    return "sudo snap refresh ato";
  }
  return null;
}

// GitHub release asset URL for the .deb produced by tauri-bundler.
// productName=ATO + tauri-bundler's standard linux deb naming →
// `ATO_<version>_amd64.deb`. We only render this URL on x86_64 .deb
// installs (detect_install_method returns Unknown on other arches),
// so the hardcoded amd64 suffix is safe — we never reach this code
// path with an arm64 box.
export function debReleaseUrl(version: string): string {
  return `https://github.com/WillNigri/Agentic-Tool-Optimization/releases/download/v${version}/ATO_${version}_amd64.deb`;
}

export default function UpdateBanner() {
  const [state, setState] = useState<BannerState>({ kind: "loading" });
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!isTauri) {
      // Outside Tauri (dev server, vitest jsdom) there's no updater
      // and no install method — render nothing.
      setState({ kind: "uptodate" });
      return;
    }
    void (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const { check } = await import("@tauri-apps/plugin-updater");
        const [methodRaw, update] = await Promise.all([
          invoke<string>("get_install_method"),
          check(),
        ]);
        if (!update) {
          setState({ kind: "uptodate" });
          return;
        }
        setState({
          kind: "available",
          update: { version: update.version, currentVersion: update.currentVersion ?? "" },
          method: (methodRaw as InstallMethod) ?? "unknown",
        });
      } catch (err) {
        setState({
          kind: "error",
          message: err instanceof Error ? err.message : String(err),
        });
      }
    })();
  }, []);

  const manualCmd = useMemo(() => {
    if (state.kind !== "available") return null;
    return manualUpdateCommand(
      state.method,
      state.update.version,
      debReleaseUrl(state.update.version),
    );
  }, [state]);

  // Fall-through cases: when there's no update, when we errored,
  // or when this install method doesn't need the manual path
  // (macOS/Windows/AppImage/Unknown) → render nothing and let the
  // existing checkForUpdates() ask-dialog handle the prompt.
  if (state.kind !== "available" || manualCmd === null) {
    return null;
  }

  const onCopy = async () => {
    try {
      await navigator.clipboard.writeText(manualCmd);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard may be denied (sandboxed webview, permission
      // missing); leave `copied` false so the user sees the button
      // didn't latch and can select+copy the text manually.
    }
  };

  return (
    <div
      role="alert"
      aria-label="Update available"
      className="rounded-lg border border-cs-accent/40 bg-cs-accent/10 p-4 space-y-3"
    >
      <div className="flex items-start gap-2">
        <Terminal size={14} className="mt-0.5 shrink-0 text-cs-accent" />
        <div className="space-y-1">
          <p className="text-sm font-medium text-cs-text">
            ATO {state.update.version} is available
          </p>
          <p className="text-xs text-cs-muted">
            Your {state.method === "deb" ? ".deb package" : "Snap"} install needs
            a manual upgrade — the in-app updater can't replace a
            root-owned binary. Run this in a terminal:
          </p>
        </div>
      </div>

      <div className="relative">
        <pre className="overflow-x-auto rounded-md border border-cs-border bg-cs-bg p-3 pr-12 text-xs font-mono text-cs-text">
          <code>{manualCmd}</code>
        </pre>
        <button
          type="button"
          onClick={onCopy}
          aria-label="Copy update command"
          className="absolute top-2 right-2 inline-flex items-center gap-1 rounded-md border border-cs-border bg-cs-bg-raised px-2 py-1 text-xs text-cs-text hover:bg-cs-border/40"
        >
          {copied ? <Check size={11} /> : <Copy size={11} />}
          {copied ? "Copied" : "Copy"}
        </button>
      </div>

      <a
        href="https://github.com/WillNigri/Agentic-Tool-Optimization/releases/latest"
        target="_blank"
        rel="noopener noreferrer"
        className="inline-flex items-center gap-1 text-xs text-cs-accent hover:underline"
      >
        <ExternalLink size={11} />
        View the release on GitHub
      </a>
    </div>
  );
}

// Loading spinner export for callers that want to render a stub
// while detection is in flight, instead of waiting for the first
// render with content. Optional — most callers can ignore.
export function UpdateBannerSkeleton() {
  return (
    <div className="flex items-center gap-2 text-xs text-cs-muted">
      <Loader2 size={11} className="animate-spin" />
      Checking for updates…
    </div>
  );
}
