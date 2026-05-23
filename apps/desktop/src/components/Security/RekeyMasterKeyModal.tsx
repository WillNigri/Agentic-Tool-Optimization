// Rekey modal — PR-5 of master_key_v2.
//
// Hostile textarea + Submit per the war-room locked UX for v2.0.
// Tooltip surfaces the exact shell command users can paste from
// (matching what PR-6's `ato master-key export` will eventually
// emit). v2.1 socket-handshake replaces this — see design doc.
//
// Tauri command contract (PR-4 rekey.rs):
//   Ok(RekeyResult { rowsRekeyed, v2KeychainAccount,
//                     v1KeychainDeleted, retiredAt })
//   Err(String) — RekeyError::Display rendered for the user.

import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { KeyRound, Loader2, X, ShieldCheck, Copy, Check, Terminal as TerminalIcon } from "lucide-react";

interface RekeyResult {
  rowsRekeyed: number;
  v2KeychainAccount: string;
  v1KeychainDeleted: boolean;
  retiredAt: string;
}

interface Props {
  onClose: () => void;
  onSuccess: () => void;
}

export default function RekeyMasterKeyModal({ onClose, onSuccess }: Props) {
  const [oldKey, setOldKey] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<RekeyResult | null>(null);
  const [cmdCopied, setCmdCopied] = useState(false);

  const MACOS_CMD = "security find-generic-password -s ato-desktop -a master_key_v1 -w";
  const copyCmd = () => {
    navigator.clipboard.writeText(MACOS_CMD).then(() => {
      setCmdCopied(true);
      setTimeout(() => setCmdCopied(false), 2000);
    });
  };

  const handleSubmit = async () => {
    if (submitting || !oldKey.trim()) return;
    setSubmitting(true);
    setError(null);
    try {
      const r = await invoke<RekeyResult>("rekey_master_key", {
        oldKeyB64: oldKey.trim(),
      });
      setResult(r);
      // Give the user 1.5s to read the success card before dismiss.
      setTimeout(onSuccess, 1500);
    } catch (e) {
      setError(String(e));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50"
      onClick={(e) => {
        if (e.target === e.currentTarget && !submitting) onClose();
      }}
    >
      <div className="w-[600px] max-w-[90vw] rounded-lg border border-cs-border bg-cs-card p-6 shadow-2xl">
        <div className="flex items-start justify-between mb-4">
          <div className="flex items-center gap-2">
            <KeyRound className="text-cs-accent" size={20} />
            <h2 className="text-lg font-medium text-cs-text">
              Re-key master encryption key
            </h2>
          </div>
          <button
            onClick={onClose}
            disabled={submitting}
            aria-label="Close"
            className="text-cs-muted hover:text-cs-text disabled:opacity-30"
          >
            <X size={18} />
          </button>
        </div>

        {!result ? (
          <>
            <div className="text-sm text-cs-muted mb-4 space-y-3">
              <p>
                ATO needs the OLD master key (the one stored on your
                Mac's Keychain) to re-encrypt your saved API keys
                under a new identity. The whole flow is one atomic
                transaction — your stored keys never get orphaned.
              </p>

              <div className="rounded-md border border-cs-border bg-cs-bg/50 p-3 space-y-2">
                <div className="text-xs font-medium text-cs-text mb-1">
                  How to get the old key on macOS:
                </div>
                <ol className="text-xs space-y-2 list-decimal list-inside text-cs-muted">
                  <li>
                    Open the <strong className="text-cs-text">Terminal</strong> app
                    (⌘+Space, type "Terminal", press Enter).
                  </li>
                  <li className="space-y-1.5">
                    <span>Paste this command and press Enter:</span>
                    <div className="flex items-stretch gap-1 not-italic">
                      <code className="flex-1 bg-cs-bg px-2 py-1.5 rounded-l text-[11px] font-mono text-cs-text break-all">
                        {MACOS_CMD}
                      </code>
                      <button
                        type="button"
                        onClick={copyCmd}
                        className="px-2 rounded-r border-l border-cs-border bg-cs-bg hover:bg-cs-card flex items-center gap-1 text-[11px] text-cs-muted hover:text-cs-text"
                        title="Copy command"
                      >
                        {cmdCopied ? (
                          <>
                            <Check size={12} className="text-cs-accent" /> copied
                          </>
                        ) : (
                          <>
                            <Copy size={12} /> copy
                          </>
                        )}
                      </button>
                    </div>
                    <div className="text-[10px] text-cs-muted pl-1 flex items-start gap-1">
                      <TerminalIcon size={11} className="mt-0.5 shrink-0" />
                      <span>
                        macOS will prompt you to Allow access — click
                        Always Allow (it's reading the key ATO itself
                        stored there).
                      </span>
                    </div>
                  </li>
                  <li>
                    Terminal prints a long base64 string. Select the
                    whole line and copy it (⌘+C).
                  </li>
                  <li>
                    Paste it into the box below and click <strong className="text-cs-text">Re-key now</strong>.
                  </li>
                </ol>
              </div>

              <details className="text-xs">
                <summary className="cursor-pointer text-cs-muted hover:text-cs-text">
                  I already rotated the macOS keychain — the old key is gone →
                </summary>
                <div className="mt-2 pl-3 border-l-2 border-amber-500/30 text-cs-muted space-y-1">
                  <p>
                    If you've already wiped the Keychain entry, the
                    old key is unrecoverable and ATO can't decrypt
                    your stored API keys.
                  </p>
                  <p>
                    Recovery: close this dialog, go to{" "}
                    <strong className="text-cs-text">Settings → API Keys</strong>,
                    delete the orphaned rows, and re-enter the keys
                    fresh. They'll encrypt under the new identity
                    automatically.
                  </p>
                </div>
              </details>
            </div>
            <textarea
              value={oldKey}
              onChange={(e) => setOldKey(e.target.value)}
              disabled={submitting}
              placeholder="Paste base64-encoded 32-byte key here…"
              rows={4}
              className="w-full px-3 py-2 text-xs font-mono bg-cs-bg border border-cs-border rounded resize-none focus:outline-none focus:border-cs-accent disabled:opacity-50"
              autoFocus
            />
            {error && (
              <div
                role="alert"
                className="mt-3 px-3 py-2 text-xs rounded bg-red-500/10 border border-red-500/30 text-red-200"
              >
                {error}
              </div>
            )}
            <div className="mt-5 flex items-center justify-end gap-2">
              <button
                onClick={onClose}
                disabled={submitting}
                className="px-3 py-1.5 text-sm text-cs-muted hover:text-cs-text disabled:opacity-30"
              >
                Cancel
              </button>
              <button
                onClick={handleSubmit}
                disabled={submitting || !oldKey.trim()}
                className="px-4 py-1.5 text-sm font-medium rounded bg-cs-accent text-cs-bg hover:bg-cs-accent/90 disabled:opacity-30 flex items-center gap-2"
              >
                {submitting ? (
                  <>
                    <Loader2 size={14} className="animate-spin" />
                    Re-keying…
                  </>
                ) : (
                  "Re-key now"
                )}
              </button>
            </div>
          </>
        ) : (
          <div role="status" className="space-y-3">
            <div className="flex items-center gap-2 text-cs-accent">
              <ShieldCheck size={20} />
              <span className="font-medium">Re-key complete</span>
            </div>
            <dl className="text-xs space-y-1 text-cs-muted">
              <div className="flex justify-between">
                <dt>Rows re-encrypted</dt>
                <dd className="text-cs-text">{result.rowsRekeyed}</dd>
              </div>
              <div className="flex justify-between">
                <dt>New keychain account</dt>
                <dd className="text-cs-text font-mono">{result.v2KeychainAccount}</dd>
              </div>
              <div className="flex justify-between">
                <dt>Old keychain entry deleted</dt>
                <dd className="text-cs-text">
                  {result.v1KeychainDeleted ? "yes" : "no (orphan — non-fatal)"}
                </dd>
              </div>
            </dl>
          </div>
        )}
      </div>
    </div>
  );
}
