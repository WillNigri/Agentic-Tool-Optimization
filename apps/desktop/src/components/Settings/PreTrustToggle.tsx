// Felipe P3 (v2.7.10) — "Trust ATO-registered projects" toggle.
//
// Claude Code prompts "trust this folder?" on every dispatch because its
// own trust state lives in a different SQLite under a different macOS
// identity than the ATO desktop. When this toggle is ON (the default),
// the Rust dispatcher (prompt_agent_inner claude branch in
// apps/desktop/src-tauri/src/commands/mod.rs) appends
// `--dangerously-skip-permissions` to the claude CLI args for any
// workspace whose path matches a row in the local `projects` table
// (single trailing slash is normalized).
//
// Scope honesty (war-room review 2026-05-20): `--dangerously-skip-permissions`
// is broader than the per-folder trust prompt — claude also uses it to
// suppress per-tool approval prompts. The helper text below makes this
// explicit so the user's consent is real, not framed solely as "trust
// this folder?".
//
// Storage is the JSON sidecar `~/.ato/settings.json` (not the SQLite
// `settings` table) because this session can not add a new Tauri get/set
// command for the bool. The Rust side reads the same file on every
// dispatch (see read_trust_registered_projects in mod.rs). Default ON
// applies whenever the file is missing, malformed, or the key is absent.
//
// We deliberately do NOT call `mkdir` on `~/.ato/` — the capabilities
// manifest does not include `fs:allow-mkdir` and adding it is out of
// this session's owned-files scope. The directory always exists by the
// time this component renders: it is created when the Tauri backend
// opens the local SQLite DB on first launch, and this Settings tab is
// reachable only AFTER the backend has initialized.
//
// WIRE-UP (TODO follow-up, owned by a different session): import this
// into apps/desktop/src/pages/sections/SettingsSection.tsx alongside
// RuntimesPanel — e.g. as a new "Trust" tab, or appended to the
// Runtimes tab's Claude card. Self-contained until then.

import { useEffect, useState } from "react";
import { homeDir } from "@tauri-apps/api/path";
import {
  readTextFile,
  writeTextFile,
  exists,
} from "@tauri-apps/plugin-fs";

const SETTINGS_KEY = "trust_registered_projects";
const SETTINGS_REL_PATH = ".ato/settings.json";

type LoadState = "loading" | "ready" | "error";

async function settingsAbsPath(): Promise<string> {
  const home = await homeDir();
  // homeDir() may or may not include a trailing slash on macOS; normalize.
  const base = home.endsWith("/") ? home.slice(0, -1) : home;
  return `${base}/${SETTINGS_REL_PATH}`;
}

async function readToggle(): Promise<boolean> {
  try {
    const path = await settingsAbsPath();
    if (!(await exists(path))) return true;
    const raw = await readTextFile(path);
    const json = JSON.parse(raw);
    if (typeof json !== "object" || json === null) return true;
    const v = (json as Record<string, unknown>)[SETTINGS_KEY];
    return typeof v === "boolean" ? v : true;
  } catch {
    // Missing / malformed / unreadable — default ON. Matches the Rust
    // contract in read_trust_registered_projects (mod.rs).
    return true;
  }
}

async function writeToggle(next: boolean): Promise<void> {
  const path = await settingsAbsPath();
  let merged: Record<string, unknown> = {};
  try {
    if (await exists(path)) {
      const existing = JSON.parse(await readTextFile(path));
      if (existing && typeof existing === "object") {
        merged = existing as Record<string, unknown>;
      }
    }
  } catch {
    // Corrupt JSON — overwrite with a fresh object rather than refuse
    // to update. The other keys in this file are not load-bearing and
    // the user's intent is clear (flip the toggle).
    merged = {};
  }
  merged[SETTINGS_KEY] = next;
  await writeTextFile(path, JSON.stringify(merged, null, 2));
}

export default function PreTrustToggle() {
  const [state, setState] = useState<LoadState>("loading");
  const [enabled, setEnabled] = useState<boolean>(true);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    readToggle().then((v) => {
      if (cancelled) return;
      setEnabled(v);
      setState("ready");
    }).catch(() => {
      if (cancelled) return;
      setState("error");
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const onChange = async (next: boolean) => {
    setEnabled(next);
    setSaveError(null);
    try {
      await writeToggle(next);
    } catch (err) {
      setSaveError(err instanceof Error ? err.message : String(err));
      // Revert optimistic update so UI matches disk on failure.
      setEnabled(!next);
    }
  };

  const disabled = state !== "ready";

  return (
    <div className="rounded-lg border border-cs-border bg-cs-card p-4 space-y-2">
      <label className="flex items-start gap-3 cursor-pointer">
        <input
          type="checkbox"
          checked={enabled}
          disabled={disabled}
          onChange={(e) => onChange(e.target.checked)}
          className="mt-1 h-4 w-4 accent-cs-accent"
        />
        <div className="space-y-1">
          <div className="text-sm font-medium text-cs-text">
            Trust ATO-registered projects
          </div>
          <div className="text-xs text-cs-muted leading-relaxed">
            When ON, Claude Code dispatches into a workspace that you have
            registered as a project (Settings → Projects) pass
            <code className="mx-1 px-1 py-0.5 rounded bg-cs-bg text-cs-accent font-mono text-[10px]">
              --dangerously-skip-permissions
            </code>
            . This skips Claude&apos;s &ldquo;trust this folder?&rdquo;
            prompt <em>and</em> the per-tool approval prompts (Bash, Edit,
            Write, …) inside that workspace. Turn OFF if you want Claude
            to ask before each tool use. Matching is by exact path
            (trailing slash normalized); sub-paths under a registered
            project are not covered.
          </div>
        </div>
      </label>
      {saveError && (
        <div className="text-[11px] text-cs-danger pl-7">
          Could not save: {saveError}
        </div>
      )}
    </div>
  );
}
