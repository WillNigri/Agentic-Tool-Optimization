// Phase 7.0 step 5 — Settings → Mesh panel.
//
// Visual surface for invite-code pairing between two ATO daemons.
// Three stacked sections:
//
//   1. Discovered peers — read-only list of daemons the local
//      mDNS browser has seen on the LAN. Selecting one pre-fills
//      the host/port + the pinned peer_id when the user clicks
//      "Pair with this peer".
//   2. Paired peers — the trust list (mesh_peers rows). Each row
//      exposes a remove button; removing only deletes the local
//      trust mark, the remote daemon is not notified.
//   3. Open invites — codes this machine has issued and not yet
//      consumed. Lists code, expiry, age. Display-only for now;
//      we don't expose a revoke action because the underlying SQL
//      is a hard DELETE and we want the audit row to persist.
//
// All write paths go through the same Tauri commands the CLI
// uses, so the GUI cannot bypass the issuer-pubkey + peer_id-pin
// checks that the daemon-side handler enforces.

import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import {
  Network,
  Plus,
  Trash2,
  Copy,
  Check,
  Loader2,
  ShieldCheck,
  Radio,
  Ticket,
  Info,
  X,
} from "lucide-react";
import { cn } from "@/lib/utils";

interface DiscoveredPeer {
  peerId: string;
  name: string;
  addr: string;
  version: string | null;
  lastSeenAt: string;
}

interface PairedPeer {
  peerId: string;
  name: string;
  pairedAt: string;
  lastSeenAt: string | null;
  notes: string | null;
}

interface InviteRow {
  code: string;
  issuedAt: string;
  expiresAt: string;
  consumed: boolean;
  issuerPubkey: string | null;
}

interface ConsumeResult {
  peerId: string;
  publicKeyB64: string;
  machineName: string;
}

function relTime(iso: string): string {
  const t = new Date(iso).getTime();
  const delta = Math.max(0, Date.now() - t);
  const s = Math.floor(delta / 1000);
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.floor(h / 24);
  return `${d}d ago`;
}

function expiresIn(iso: string): string {
  const t = new Date(iso).getTime();
  const delta = t - Date.now();
  if (delta <= 0) return "expired";
  const s = Math.floor(delta / 1000);
  if (s < 60) return `${s}s left`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m left`;
  const h = Math.floor(m / 60);
  return `${h}h left`;
}

function shortPeer(id: string): string {
  return id.length > 12 ? `${id.slice(0, 8)}…${id.slice(-4)}` : id;
}

export default function MeshPanel() {
  const queryClient = useQueryClient();
  const [creatingInvite, setCreatingInvite] = useState(false);
  const [showPair, setShowPair] = useState(false);
  const [pairPrefill, setPairPrefill] = useState<DiscoveredPeer | null>(null);
  const [copiedCode, setCopiedCode] = useState<string | null>(null);

  const discoveredQ = useQuery<DiscoveredPeer[]>({
    queryKey: ["mesh-discovered"],
    queryFn: () => invoke<DiscoveredPeer[]>("mesh_list_discovered"),
    refetchInterval: 5_000,
  });

  const peersQ = useQuery<PairedPeer[]>({
    queryKey: ["mesh-peers"],
    queryFn: () => invoke<PairedPeer[]>("mesh_list_peers"),
    refetchInterval: 15_000,
  });

  const invitesQ = useQuery<InviteRow[]>({
    queryKey: ["mesh-invites"],
    queryFn: () => invoke<InviteRow[]>("mesh_list_invites", { includeAll: false }),
    refetchInterval: 10_000,
  });

  const createInvite = async () => {
    setCreatingInvite(true);
    try {
      await invoke<InviteRow>("mesh_create_invite", { expiresMinutes: 15 });
      await queryClient.invalidateQueries({ queryKey: ["mesh-invites"] });
    } catch (e) {
      window.alert(`Could not create invite: ${e}`);
    } finally {
      setCreatingInvite(false);
    }
  };

  const removePeer = async (peerId: string, name: string) => {
    if (
      !window.confirm(
        `Remove paired peer "${name}" (${shortPeer(peerId)})?\n\nThis only deletes the local trust record. The remote daemon is not notified and could re-pair if you accept a fresh invite from them.`,
      )
    ) {
      return;
    }
    try {
      await invoke("mesh_remove_peer", { peerId });
      await queryClient.invalidateQueries({ queryKey: ["mesh-peers"] });
    } catch (e) {
      window.alert(`Remove failed: ${e}`);
    }
  };

  const copyCode = async (code: string) => {
    try {
      await navigator.clipboard.writeText(code);
      setCopiedCode(code);
      window.setTimeout(() => setCopiedCode((c) => (c === code ? null : c)), 2000);
    } catch {
      // clipboard failed silently — user can read the code visually.
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h3 className="text-base font-semibold flex items-center gap-2">
            <Network size={18} className="text-cs-accent" />
            Mesh — paired daemons
          </h3>
          <p className="text-sm text-cs-muted mt-1 max-w-2xl">
            Pair this machine with another ATO daemon over your LAN. Pairing
            uses one-time invite codes; both sides verify the other's Ed25519
            peer_id before trust is recorded. No data leaves your network.
          </p>
        </div>
        <div className="flex gap-2">
          <button
            onClick={createInvite}
            disabled={creatingInvite}
            className={cn(
              "inline-flex items-center gap-2 px-3 py-2 text-sm rounded-md border border-cs-accent/40 text-cs-accent hover:bg-cs-accent/10 disabled:opacity-50",
            )}
          >
            {creatingInvite ? (
              <Loader2 size={14} className="animate-spin" />
            ) : (
              <Ticket size={14} />
            )}
            New invite
          </button>
          <button
            onClick={() => {
              setPairPrefill(null);
              setShowPair(true);
            }}
            className="inline-flex items-center gap-2 px-3 py-2 text-sm rounded-md bg-cs-accent text-black hover:bg-cs-accent/90"
          >
            <Plus size={14} />
            Pair with peer
          </button>
        </div>
      </div>

      {/* Open invites */}
      <section>
        <h4 className="text-sm font-medium text-cs-text mb-2 flex items-center gap-2">
          <Ticket size={14} className="text-cs-muted" />
          Open invites
        </h4>
        {invitesQ.isLoading ? (
          <div className="text-sm text-cs-muted">Loading…</div>
        ) : !invitesQ.data || invitesQ.data.length === 0 ? (
          <div className="text-sm text-cs-muted bg-cs-surface/40 border border-cs-border rounded-md p-3">
            No open invites. Click <strong>New invite</strong> to generate one,
            then run <code className="text-cs-accent">ato mesh invite consume</code> on
            the other machine.
          </div>
        ) : (
          <ul className="space-y-2">
            {invitesQ.data.map((inv) => (
              <li
                key={inv.code}
                className="flex items-center justify-between gap-3 bg-cs-surface/40 border border-cs-border rounded-md px-3 py-2"
              >
                <div className="flex items-center gap-3 min-w-0">
                  <code className="font-mono text-sm text-cs-accent">{inv.code}</code>
                  <span className="text-xs text-cs-muted">
                    {expiresIn(inv.expiresAt)} · issued {relTime(inv.issuedAt)}
                  </span>
                </div>
                <button
                  onClick={() => copyCode(inv.code)}
                  className="inline-flex items-center gap-1 text-xs text-cs-muted hover:text-cs-text"
                  title="Copy code"
                >
                  {copiedCode === inv.code ? <Check size={12} /> : <Copy size={12} />}
                  {copiedCode === inv.code ? "Copied" : "Copy"}
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      {/* Paired peers */}
      <section>
        <h4 className="text-sm font-medium text-cs-text mb-2 flex items-center gap-2">
          <ShieldCheck size={14} className="text-cs-muted" />
          Paired peers ({peersQ.data?.length ?? 0})
        </h4>
        {peersQ.isLoading ? (
          <div className="text-sm text-cs-muted">Loading…</div>
        ) : !peersQ.data || peersQ.data.length === 0 ? (
          <div className="text-sm text-cs-muted bg-cs-surface/40 border border-cs-border rounded-md p-3">
            No paired peers yet.
          </div>
        ) : (
          <ul className="space-y-2">
            {peersQ.data.map((p) => (
              <li
                key={p.peerId}
                className="flex items-center justify-between gap-3 bg-cs-surface/40 border border-cs-border rounded-md px-3 py-2"
              >
                <div className="min-w-0 flex-1">
                  <div className="text-sm font-medium text-cs-text">{p.name}</div>
                  <div className="text-xs text-cs-muted font-mono">
                    {shortPeer(p.peerId)} · paired {relTime(p.pairedAt)}
                  </div>
                </div>
                <button
                  onClick={() => removePeer(p.peerId, p.name)}
                  className="text-cs-muted hover:text-red-400 p-1"
                  title="Remove peer"
                >
                  <Trash2 size={14} />
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      {/* Discovered (LAN mDNS) */}
      <section>
        <h4 className="text-sm font-medium text-cs-text mb-2 flex items-center gap-2">
          <Radio size={14} className="text-cs-muted" />
          Discovered on LAN ({discoveredQ.data?.length ?? 0})
        </h4>
        {discoveredQ.isLoading ? (
          <div className="text-sm text-cs-muted">Loading…</div>
        ) : !discoveredQ.data || discoveredQ.data.length === 0 ? (
          <div className="text-sm text-cs-muted bg-cs-surface/40 border border-cs-border rounded-md p-3 flex items-start gap-2">
            <Info size={14} className="text-cs-muted mt-0.5 shrink-0" />
            <span>
              No daemons advertising on your LAN. Start the daemon on the other
              machine with <code className="text-cs-accent">ato daemon start</code>{" "}
              and make sure both are on the same network.
            </span>
          </div>
        ) : (
          <ul className="space-y-2">
            {discoveredQ.data.map((d) => {
              const alreadyPaired = peersQ.data?.some((p) => p.peerId === d.peerId);
              return (
                <li
                  key={d.peerId}
                  className="flex items-center justify-between gap-3 bg-cs-surface/40 border border-cs-border rounded-md px-3 py-2"
                >
                  <div className="min-w-0 flex-1">
                    <div className="text-sm font-medium text-cs-text flex items-center gap-2">
                      {d.name}
                      {alreadyPaired && (
                        <span className="text-xs text-cs-accent bg-cs-accent/10 px-1.5 py-0.5 rounded">
                          paired
                        </span>
                      )}
                    </div>
                    <div className="text-xs text-cs-muted font-mono">
                      {d.addr} · {shortPeer(d.peerId)}
                      {d.version ? ` · v${d.version}` : ""} · seen {relTime(d.lastSeenAt)}
                    </div>
                  </div>
                  {!alreadyPaired && (
                    <button
                      onClick={() => {
                        setPairPrefill(d);
                        setShowPair(true);
                      }}
                      className="text-xs text-cs-accent hover:underline"
                    >
                      Pair…
                    </button>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </section>

      {showPair && (
        <PairModal
          prefill={pairPrefill}
          onClose={() => setShowPair(false)}
          onPaired={async () => {
            setShowPair(false);
            await queryClient.invalidateQueries({ queryKey: ["mesh-peers"] });
          }}
        />
      )}
    </div>
  );
}

interface PairModalProps {
  prefill: DiscoveredPeer | null;
  onClose: () => void;
  onPaired: () => void;
}

// Crockford base32 invite code shape (mirror of CLI's
// validate_code_format). Matches ATO-XXXX-XXXX-XXXX where each X is
// a Crockford base32 char (no I, L, O, U). Mirrored here so the
// modal can reject malformed input before paying for the subprocess
// roundtrip + an ugly stderr error string. (claude #6)
const INVITE_CODE_RE = /^ATO-[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}$/;

// Pull host + port out of a mDNS-stored address. IPv6 addresses get
// stored bare ("fe80::1:7474"), so splitting on the first `:` would
// chop the address; use the last colon as the host/port separator
// and strip any brackets. (claude #4)
function splitHostPort(addr: string): { host: string; port: number } {
  const idx = addr.lastIndexOf(":");
  if (idx <= 0 || idx === addr.length - 1) {
    return { host: addr, port: 7474 };
  }
  const rawHost = addr.slice(0, idx);
  const portStr = addr.slice(idx + 1);
  const port = parseInt(portStr, 10);
  const host = rawHost.replace(/^\[|\]$/g, "");
  return {
    host,
    port: Number.isNaN(port) ? 7474 : port,
  };
}

function PairModal({ prefill, onClose, onPaired }: PairModalProps) {
  const [code, setCode] = useState("");
  const initial = prefill ? splitHostPort(prefill.addr) : { host: "", port: 7474 };
  const [host, setHost] = useState<string>(initial.host);
  const [port, setPort] = useState<number>(initial.port);
  const [peerId, setPeerId] = useState(prefill?.peerId ?? "");
  const [note, setNote] = useState("");
  const [pairing, setPairing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    setError(null);
    const trimmedCode = code.trim().toUpperCase();
    const trimmedHost = host.trim();
    const trimmedPeer = peerId.trim().toLowerCase();
    if (!trimmedCode || !trimmedHost || !trimmedPeer) {
      setError("Code, host, and peer ID are required.");
      return;
    }
    if (!INVITE_CODE_RE.test(trimmedCode)) {
      setError("Invite code must be ATO-XXXX-XXXX-XXXX (Crockford base32 — no I, L, O, U).");
      return;
    }
    if (trimmedPeer.length !== 64 || !/^[0-9a-f]+$/.test(trimmedPeer)) {
      setError("Peer ID must be 64 lowercase hex characters.");
      return;
    }
    setPairing(true);
    try {
      const result = await invoke<ConsumeResult>("mesh_consume_invite", {
        code: trimmedCode,
        host: trimmedHost,
        port,
        expectPeerId: trimmedPeer,
        note: note.trim() || null,
      });
      window.alert(
        `Paired with ${result.machineName}\n\npeer_id: ${result.peerId}`,
      );
      onPaired();
    } catch (e) {
      setError(String(e));
    } finally {
      setPairing(false);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/60 z-50 flex items-center justify-center p-4">
      <div className="bg-cs-surface border border-cs-border rounded-lg w-full max-w-lg">
        <div className="flex items-center justify-between p-4 border-b border-cs-border">
          <h3 className="text-base font-semibold flex items-center gap-2">
            <Plus size={16} className="text-cs-accent" />
            Pair with a peer
          </h3>
          <button onClick={onClose} className="text-cs-muted hover:text-cs-text">
            <X size={16} />
          </button>
        </div>
        <div className="p-4 space-y-3">
          <div>
            <label className="text-xs text-cs-muted block mb-1">Invite code</label>
            <input
              type="text"
              value={code}
              onChange={(e) => setCode(e.target.value)}
              placeholder="ATO-XXXX-XXXX-XXXX"
              className="w-full bg-cs-bg border border-cs-border rounded px-2 py-1.5 text-sm font-mono"
              autoFocus
            />
          </div>
          <div className="grid grid-cols-3 gap-2">
            <div className="col-span-2">
              <label className="text-xs text-cs-muted block mb-1">Host</label>
              <input
                type="text"
                value={host}
                onChange={(e) => setHost(e.target.value)}
                placeholder="192.168.1.42"
                className="w-full bg-cs-bg border border-cs-border rounded px-2 py-1.5 text-sm font-mono"
              />
            </div>
            <div>
              <label className="text-xs text-cs-muted block mb-1">Port</label>
              <input
                type="number"
                value={port}
                onChange={(e) => setPort(parseInt(e.target.value, 10) || 7474)}
                className="w-full bg-cs-bg border border-cs-border rounded px-2 py-1.5 text-sm font-mono"
              />
            </div>
          </div>
          <div>
            <label className="text-xs text-cs-muted block mb-1">
              Expected peer ID{" "}
              <span className="text-cs-muted/70">(64 hex chars)</span>
            </label>
            <input
              type="text"
              value={peerId}
              onChange={(e) => setPeerId(e.target.value)}
              placeholder="sha256 of the remote daemon's public key"
              className="w-full bg-cs-bg border border-cs-border rounded px-2 py-1.5 text-sm font-mono"
            />
            <p className="text-xs text-cs-muted mt-1">
              Run <code className="text-cs-accent">ato daemon status</code> on the
              other machine to read its peer ID. Pinning it prevents
              man-in-the-middle pairing.
            </p>
          </div>
          <div>
            <label className="text-xs text-cs-muted block mb-1">Note (optional)</label>
            <input
              type="text"
              value={note}
              onChange={(e) => setNote(e.target.value)}
              placeholder="e.g. Will's MacBook"
              className="w-full bg-cs-bg border border-cs-border rounded px-2 py-1.5 text-sm"
            />
          </div>
          {error && (
            <div className="text-xs text-red-400 bg-red-500/10 border border-red-500/30 rounded px-2 py-1.5">
              {error}
            </div>
          )}
        </div>
        <div className="flex items-center justify-end gap-2 p-4 border-t border-cs-border">
          <button
            onClick={onClose}
            className="px-3 py-1.5 text-sm rounded-md text-cs-muted hover:text-cs-text"
            disabled={pairing}
          >
            Cancel
          </button>
          <button
            onClick={submit}
            disabled={pairing}
            className="inline-flex items-center gap-2 px-3 py-1.5 text-sm rounded-md bg-cs-accent text-black hover:bg-cs-accent/90 disabled:opacity-50"
          >
            {pairing && <Loader2 size={14} className="animate-spin" />}
            Pair
          </button>
        </div>
      </div>
    </div>
  );
}
