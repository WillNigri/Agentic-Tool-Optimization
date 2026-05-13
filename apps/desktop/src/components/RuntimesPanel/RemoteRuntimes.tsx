// v2.3.52 — Settings → Runtimes → Remote panel.
//
// GUI for managing Phase 6.x-J SSH-backed remote runtimes. Lists
// registered remotes, lets the user add one (with an SSH key picker
// suggesting ~/.ssh/* candidates), and unregisters. Mirrors the CLI's
// `ato runtimes add-remote / list-remote / remove-remote` surface
// through three Tauri commands that shell out to the same canonical
// implementation.
//
// Driven by feedback from @iamknownasfesal on 2026-05-13: asking
// users to terminal-only to wire SSH keys when every other runtime
// config is GUI-first was unnecessary friction.

import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import {
  ServerCog,
  Plus,
  Trash2,
  X,
  Loader2,
  KeyRound,
  Info,
} from "lucide-react";
import { cn } from "@/lib/utils";

interface RemoteRuntimeRow {
  slug: string;
  host: string;
  port: number;
  sshUser: string | null;
  keyPath: string | null;
  runtime: string;
  binaryPath: string;
  extraArgs: string | null;
  createdAt: string;
}

// Runtimes that can plausibly be invoked over SSH. The CLI accepts
// anything in this list; restricting the dropdown keeps the user
// from picking an API-provider slug (minimax/grok/etc.) that
// wouldn't make sense as a remote shell-spawn target.
const REMOTE_RUNTIME_OPTIONS = ["claude", "codex", "gemini", "hermes", "openclaw"];

export default function RemoteRuntimes() {
  const queryClient = useQueryClient();
  const [showAdd, setShowAdd] = useState(false);
  const [removing, setRemoving] = useState<string | null>(null);

  const remotesQ = useQuery<RemoteRuntimeRow[]>({
    queryKey: ["remote-runtimes"],
    queryFn: () => invoke<RemoteRuntimeRow[]>("list_remote_runtimes"),
    refetchInterval: 60_000,
  });

  const handleRemove = async (slug: string) => {
    if (
      !window.confirm(
        `Remove remote runtime "${slug}"? Future dispatches against this slug will fall back to the local resolution chain.`,
      )
    ) {
      return;
    }
    setRemoving(slug);
    try {
      await invoke("remove_remote_runtime", { name: slug });
      await queryClient.invalidateQueries({ queryKey: ["remote-runtimes"] });
    } catch (e) {
      window.alert(`Remove failed: ${e}`);
    } finally {
      setRemoving(null);
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h3 className="text-base font-semibold flex items-center gap-2">
            <ServerCog size={18} className="text-cs-accent" />
            Remote runtimes
          </h3>
          <p className="mt-1 text-xs text-cs-muted max-w-2xl">
            Register an SSH host running Claude / Codex / Gemini / Hermes / OpenClaw, then dispatch
            against it like a local runtime —{" "}
            <code className="bg-cs-card px-1 rounded">
              ato dispatch &lt;slug&gt; "&hellip;"
            </code>{" "}
            routes over SSH and the response lands in your local execution_logs / Live tab.
            One-way today (your machine initiates the call); reverse direction is on the roadmap.
          </p>
        </div>
        <button
          onClick={() => setShowAdd(true)}
          className="flex items-center gap-2 px-3 py-2 rounded-md bg-cs-accent text-cs-bg text-sm font-medium hover:opacity-90"
        >
          <Plus size={14} /> Add remote
        </button>
      </div>

      {showAdd && (
        <AddRemoteModal
          onClose={() => setShowAdd(false)}
          onAdded={async () => {
            setShowAdd(false);
            await queryClient.invalidateQueries({ queryKey: ["remote-runtimes"] });
          }}
        />
      )}

      {remotesQ.isLoading ? (
        <div className="flex items-center justify-center h-24">
          <Loader2 size={20} className="animate-spin text-cs-muted" />
        </div>
      ) : !remotesQ.data || remotesQ.data.length === 0 ? (
        <div className="border border-dashed border-cs-border rounded-lg bg-cs-card p-6 text-sm text-cs-muted">
          <p>No remote runtimes registered yet.</p>
          <p className="mt-2 text-xs">
            Click <strong>Add remote</strong> above, or run from the terminal:{" "}
            <code className="bg-cs-bg px-1.5 py-0.5 rounded text-cs-text">
              ato runtimes add-remote --name claude-server --host you@server --runtime claude
              --key-path ~/.ssh/id_rsa
            </code>
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {remotesQ.data.map((r) => (
            <div
              key={r.slug}
              className="border border-cs-border rounded-lg bg-cs-card p-3"
            >
              <div className="flex items-center gap-3 flex-wrap">
                <code className="text-sm font-medium text-cs-accent">{r.slug}</code>
                <span className="text-xs px-1.5 py-0.5 rounded bg-cs-border/40 text-cs-text">
                  {r.runtime}
                </span>
                <span className="text-xs text-cs-muted truncate">
                  ssh {r.sshUser ? `${r.sshUser}@` : ""}
                  {r.host}
                  {r.port !== 22 ? `:${r.port}` : ""}
                </span>
                <button
                  onClick={() => handleRemove(r.slug)}
                  disabled={removing === r.slug}
                  className="ml-auto flex items-center gap-1 text-xs text-cs-muted hover:text-cs-danger disabled:opacity-50"
                  title="Remove this remote"
                >
                  <Trash2 size={12} /> remove
                </button>
              </div>
              <div className="mt-2 grid grid-cols-2 gap-x-4 gap-y-1 text-[10px] text-cs-muted">
                <div>
                  binary: <code className="text-cs-text">{r.binaryPath}</code>
                </div>
                <div>
                  key:{" "}
                  <code className="text-cs-text">
                    {r.keyPath || "(ssh-agent / default)"}
                  </code>
                </div>
                {r.extraArgs && (
                  <div className="col-span-2">
                    extra args: <code className="text-cs-text">{r.extraArgs}</code>
                  </div>
                )}
                <div className="col-span-2">
                  registered {new Date(r.createdAt).toLocaleString()}
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function AddRemoteModal({
  onClose,
  onAdded,
}: {
  onClose: () => void;
  onAdded: () => Promise<void>;
}) {
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState(22);
  const [user, setUser] = useState("");
  const [runtime, setRuntime] = useState("claude");
  const [keyPath, setKeyPath] = useState("");
  const [binaryPath, setBinaryPath] = useState("");
  const [extraArgs, setExtraArgs] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // SSH key candidates from ~/.ssh — suggested rather than enforced.
  // The user can also type a path freehand for keys in other locations.
  const keysQ = useQuery<string[]>({
    queryKey: ["ssh-key-candidates"],
    queryFn: () => invoke<string[]>("list_ssh_key_candidates"),
  });

  // Accept `user@host` shorthand in the host field: split into the
  // user and host inputs so the underlying command stays clean. The
  // CLI handles this too, but doing it here keeps the saved row
  // readable in the list view.
  const normalize = () => {
    if (host.includes("@") && !user.trim()) {
      const [u, h] = host.split("@", 2);
      return { user: u, host: h };
    }
    return { user: user.trim(), host: host.trim() };
  };

  const handleAdd = async () => {
    setError(null);
    const { user: u, host: h } = normalize();
    if (!name.trim()) return setError("Name is required.");
    if (!h) return setError("Host is required.");
    if (!/^[a-zA-Z0-9._-]+$/.test(name.trim())) {
      return setError("Name must be alphanumeric / dashes / underscores / dots.");
    }
    setSubmitting(true);
    try {
      await invoke("add_remote_runtime", {
        name: name.trim(),
        host: h,
        runtime,
        port,
        user: u || null,
        keyPath: keyPath.trim() || null,
        binaryPath: binaryPath.trim() || null,
        extraArgs: extraArgs.trim() || null,
      });
      await onAdded();
    } catch (e) {
      setError(String(e));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={onClose}
    >
      <div
        className="relative bg-cs-card border border-cs-border rounded-lg p-6 w-full max-w-lg space-y-4 max-h-[90vh] overflow-y-auto"
        onClick={(e) => e.stopPropagation()}
      >
        <button
          onClick={onClose}
          className="absolute top-3 right-3 text-cs-muted hover:text-cs-text"
          aria-label="close"
        >
          <X size={16} />
        </button>
        <h3 className="text-lg font-semibold text-cs-text flex items-center gap-2">
          <ServerCog size={18} className="text-cs-accent" />
          Add remote runtime
        </h3>

        <div className="space-y-3">
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="text-xs text-cs-muted uppercase font-medium">Slug</label>
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="claude-server"
                disabled={submitting}
                className="mt-1 w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent font-mono"
              />
              <div className="mt-1 text-[10px] text-cs-muted">
                Used as <code>ato dispatch &lt;slug&gt; "&hellip;"</code>.
              </div>
            </div>
            <div>
              <label className="text-xs text-cs-muted uppercase font-medium">Runtime</label>
              <select
                value={runtime}
                onChange={(e) => setRuntime(e.target.value)}
                disabled={submitting}
                className="mt-1 w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent"
              >
                {REMOTE_RUNTIME_OPTIONS.map((r) => (
                  <option key={r} value={r}>
                    {r}
                  </option>
                ))}
              </select>
            </div>
          </div>

          <div className="grid grid-cols-3 gap-3">
            <div className="col-span-2">
              <label className="text-xs text-cs-muted uppercase font-medium">Host</label>
              <input
                type="text"
                value={host}
                onChange={(e) => setHost(e.target.value)}
                placeholder="you@server.example.com"
                disabled={submitting}
                className="mt-1 w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent font-mono"
              />
              <div className="mt-1 text-[10px] text-cs-muted">
                <code>user@host</code> shorthand is accepted; the User field below overrides if
                set.
              </div>
            </div>
            <div>
              <label className="text-xs text-cs-muted uppercase font-medium">Port</label>
              <input
                type="number"
                min={1}
                max={65535}
                value={port}
                onChange={(e) =>
                  setPort(Math.max(1, Math.min(65535, parseInt(e.target.value || "22", 10))))
                }
                disabled={submitting}
                className="mt-1 w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent"
              />
            </div>
          </div>

          <div>
            <label className="text-xs text-cs-muted uppercase font-medium">User (optional)</label>
            <input
              type="text"
              value={user}
              onChange={(e) => setUser(e.target.value)}
              placeholder="ubuntu"
              disabled={submitting}
              className="mt-1 w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent font-mono"
            />
          </div>

          <div>
            <label className="text-xs text-cs-muted uppercase font-medium flex items-center gap-1">
              <KeyRound size={12} /> SSH private key (optional)
            </label>
            <div className="mt-1 flex gap-2">
              <input
                type="text"
                value={keyPath}
                onChange={(e) => setKeyPath(e.target.value)}
                placeholder="~/.ssh/id_ed25519"
                disabled={submitting}
                className="flex-1 bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent font-mono"
              />
              {keysQ.data && keysQ.data.length > 0 && (
                <select
                  value=""
                  onChange={(e) => {
                    if (e.target.value) setKeyPath(e.target.value);
                  }}
                  disabled={submitting}
                  className="bg-cs-bg border border-cs-border rounded-md px-2 py-2 text-xs focus:outline-none focus:border-cs-accent"
                  title="Pick from keys found in ~/.ssh"
                >
                  <option value="">~/.ssh…</option>
                  {keysQ.data.map((k) => (
                    <option key={k} value={k}>
                      {k.split("/").pop()}
                    </option>
                  ))}
                </select>
              )}
            </div>
            <div className="mt-1 text-[10px] text-cs-muted flex items-start gap-1">
              <Info size={10} className="mt-[1px] shrink-0" />
              <span>
                Leave empty to use ssh-agent / default keys. The path is passed to{" "}
                <code>ssh -i &lt;path&gt;</code> at dispatch time; no key contents are read or
                stored by ATO.
              </span>
            </div>
          </div>

          <details className="text-xs">
            <summary className="cursor-pointer text-cs-muted hover:text-cs-text select-none">
              Advanced
            </summary>
            <div className="mt-2 space-y-3">
              <div>
                <label className="text-xs text-cs-muted uppercase font-medium">
                  Binary path on remote (optional)
                </label>
                <input
                  type="text"
                  value={binaryPath}
                  onChange={(e) => setBinaryPath(e.target.value)}
                  placeholder={`(defaults to "${runtime}" on the remote's PATH)`}
                  disabled={submitting}
                  className="mt-1 w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent font-mono"
                />
              </div>
              <div>
                <label className="text-xs text-cs-muted uppercase font-medium">
                  Extra args (optional)
                </label>
                <input
                  type="text"
                  value={extraArgs}
                  onChange={(e) => setExtraArgs(e.target.value)}
                  placeholder="--no-update-check"
                  disabled={submitting}
                  className="mt-1 w-full bg-cs-bg border border-cs-border rounded-md px-3 py-2 text-sm focus:outline-none focus:border-cs-accent font-mono"
                />
                <div className="mt-1 text-[10px] text-cs-muted">
                  Appended verbatim to every dispatch.
                </div>
              </div>
            </div>
          </details>
        </div>

        {error && <div className="text-xs text-cs-danger">{error}</div>}

        <div className="flex items-center justify-end gap-2">
          <button
            onClick={onClose}
            disabled={submitting}
            className="px-3 py-2 rounded-md border border-cs-border text-sm hover:bg-cs-border/30"
          >
            Cancel
          </button>
          <button
            onClick={handleAdd}
            disabled={submitting}
            className={cn(
              "flex items-center gap-2 px-3 py-2 rounded-md text-sm font-medium",
              "bg-cs-accent text-cs-bg hover:opacity-90 disabled:opacity-40 disabled:cursor-not-allowed",
            )}
          >
            {submitting ? <Loader2 size={14} className="animate-spin" /> : <Plus size={14} />}
            Add remote
          </button>
        </div>
      </div>
    </div>
  );
}
