# Phase 7 — Bi-directional ATO daemon mesh

**Status**: planning (locked 2026-05-13)
**Predecessor**: Phase 6.x-J (one-way SSH adapter)

## Why

Phase 6.x-J answers "my laptop calls my server" — the laptop initiates over SSH, server runs the runtime, response comes back synchronously. That covers ~80% of the laptop→server use case but leaves a real gap:

> "agent PC → agent Server, server finish → agent PC, not in an infinite loop though, they need to have a task/goal together to finish" — @iamknownasfesal, 2026-05-13

The missing piece is **server-initiated communication back to the laptop**: long-running jobs on a server that need to notify the laptop when done, with results. Phase 6.x-J can't do this — SSH is request/response, the server has no path back to the laptop after the connection closes.

## Packaging decision (locked 2026-05-13)

Two-tier shape to align with ATO's existing free/Pro pricing:

| Tier | Scope | Use case |
|------|-------|----------|
| **Phase 7.0 — free, LAN-only** | mDNS discovery + invite-code pairing on the same network. Server can post completion notifications back to laptop. No bi-directional dispatch in v1. | Home / office setups where laptop + server are reachable on the same LAN (Tailscale, local VPN, same WiFi). |
| **Phase 7.1+ — Pro/Team tier on ato-cloud** | Cloud relay daemon for NAT traversal across networks. Full bi-directional dispatch (server can ask laptop to fire any allowed runtime). Multi-machine topologies. | Production setups where the server is in a cloud DC and the laptop is on a coffee-shop network. The "real" daemon mesh. |

This packaging matters because:
1. Free users get a real working bi-directional path on LAN — not a teaser
2. The Pro upgrade is "stop fighting your firewall + unlock the full mesh"
3. ato-cloud already exists as the paid backend; adding the relay is incremental work
4. Honest scope: not promising "global mesh from day one"

## Phase 7.0 — Free LAN tier

### Goals

1. Server-side ATO daemon can send a `post_completion(session_id, status, payload)` message to the laptop's ATO daemon.
2. The laptop receives it and writes a turn to `session_turns` + an event to `events_log` — same audit surface as a local dispatch.
3. Discovery is automatic on LAN; manual pairing fallback for cases mDNS doesn't find the peer.
4. Auth is per-machine: peers exchange Ed25519 public keys at pairing time; only trusted peers can post.

### Non-goals (deliberate)

- ❌ Server-initiated **dispatch** (asking the laptop to run a new runtime) — Phase 7.1.
- ❌ NAT traversal / cross-network — Phase 7.1.
- ❌ Multi-laptop / multi-server topologies — Phase 7.1.
- ❌ Replay queue when laptop is offline — best-effort send + drop in 7.0.

### Architecture

```
┌─────────────────────────┐         ┌─────────────────────────┐
│  Laptop                 │         │  Server                 │
│  ┌───────────────────┐  │         │  ┌───────────────────┐  │
│  │ ATO desktop / CLI │  │         │  │ ATO CLI           │  │
│  └────────┬──────────┘  │         │  └────────┬──────────┘  │
│           │             │         │           │             │
│  ┌────────▼──────────┐  │ mDNS    │  ┌────────▼──────────┐  │
│  │ ato daemon        │◄─┼─────────┼─►│ ato daemon        │  │
│  │ (background)      │  │  WS+JSON│  │ (background)      │  │
│  │ ~/.ato/daemon/    │  │  -RPC   │  │ ~/.ato/daemon/    │  │
│  └───────────────────┘  │         │  └───────────────────┘  │
└─────────────────────────┘         └─────────────────────────┘
```

Each machine runs `ato daemon` as a background process (launchd / systemd / Windows service). The daemon:
- Listens on a Unix socket for local commands (`ato daemon status`, etc.)
- Listens on a TCP port for peer connections (WS+JSON-RPC)
- Broadcasts its presence via mDNS as `_ato._tcp.local`
- Holds an Ed25519 keypair persisted at `~/.ato/daemon/keys/`

### Wire protocol

WebSocket + JSON-RPC 2.0. Single method in v1:

```json
{
  "jsonrpc": "2.0",
  "id": "<uuid>",
  "method": "post_completion",
  "params": {
    "from_peer_id": "<sha256 of sender's pubkey>",
    "from_machine_name": "human-readable label",
    "session_id": "<uuid of session on the recipient>",
    "status": "success" | "error",
    "summary": "short human-readable line",
    "payload": { /* arbitrary JSON, capped at 64KB */ },
    "occurred_at": "<RFC3339>"
  }
}
```

Recipient validates:
1. Sender's `from_peer_id` is in the local `mesh_peers` allowlist
2. Signature on the message body verifies against the stored pubkey
3. `session_id` exists locally (otherwise drop — out of scope completion)

On accept: writes a turn into `session_turns` with `role='assistant'`, `runtime=<mesh_peer_slug>`, `text=summary`. Also writes a `peer_completion` event into `events_log` so ops recipes can react.

### Discovery

**mDNS** as the default. Each daemon registers `_ato._tcp.local` with TXT records:
- `peer_id=<sha256(pubkey)>`
- `name=<machine-friendly-label>`
- `version=<ato version>`

Daemons receiving a discovery hit DO NOT auto-trust. Discovery only populates a "discovered peers" list in the GUI; trust requires explicit pairing.

**Invite-code fallback** for when mDNS doesn't find the peer (typical on isolated VLANs):

```
laptop$ ato mesh invite
> Share this code with the other machine within 5 minutes:
>   ATO-INVITE: kx-9f3a-2b1c-8d77
> Listening on 192.168.1.42:7755 ...

server$ ato mesh join ATO-INVITE:kx-9f3a-2b1c-8d77 --laptop-host 192.168.1.42
> Connecting to 192.168.1.42:7755 ...
> Pairing handshake complete. Peer added: laptop@WillsMacBook
```

Invite codes are short-lived (5min), single-use, and gate the keypair exchange.

### Authentication

Ed25519 keypair per machine, generated at first `ato daemon start`. Stored at `~/.ato/daemon/keys/private.pem` (0600) and `~/.ato/daemon/keys/public.pem`.

`mesh_peers` SQLite table:
```sql
CREATE TABLE mesh_peers (
    peer_id      TEXT PRIMARY KEY,   -- sha256(public_key)
    public_key   TEXT NOT NULL,      -- base64-encoded Ed25519 pubkey
    name         TEXT NOT NULL,      -- human-readable label
    paired_at    TEXT NOT NULL,      -- when this peer was trusted
    last_seen_at TEXT,
    notes        TEXT
);
```

Every JSON-RPC message includes a `signature` field — Ed25519 signature over the body using the sender's private key. Recipient verifies before processing.

### ACL (v1: ultra-narrow)

Only one method is exposed: `post_completion`. The server CANNOT make the laptop do anything; it can only tell the laptop "I finished, here's the result."

This is intentional. The full "server dispatches into laptop" surface is the Pro-tier 7.1 unlock — a bigger trust + cost story.

### Schema additions

```sql
-- v2.4.0 Phase 7.0 — mesh peers
CREATE TABLE mesh_peers (
    peer_id      TEXT PRIMARY KEY,
    public_key   TEXT NOT NULL,
    name         TEXT NOT NULL,
    paired_at    TEXT NOT NULL,
    last_seen_at TEXT,
    notes        TEXT
);

-- v2.4.0 Phase 7.0 — pending invites (short-lived)
CREATE TABLE mesh_invites (
    code         TEXT PRIMARY KEY,
    issued_at    TEXT NOT NULL,
    expires_at   TEXT NOT NULL,
    consumed     INTEGER NOT NULL DEFAULT 0
);

-- session_turns gets a sender_peer_id column so a turn from a
-- remote peer is distinguishable from a locally-dispatched turn.
ALTER TABLE session_turns ADD COLUMN sender_peer_id TEXT;
```

### CLI surface

```
ato daemon start           # spawn the background daemon (launchd / systemd / svc)
ato daemon stop
ato daemon status

ato mesh invite            # issue a short-lived invite code
ato mesh join <code> --laptop-host <ip>
ato mesh peers             # list paired peers
ato mesh unpair <peer-id>  # remove a peer

ato mesh post-completion --to <peer-id> --session <id> --status success --summary "deploy done"
# Used inside scripts on the server side to notify the laptop. Behind
# the scenes this hits the local daemon, which routes to the peer's
# daemon over the established WS connection.
```

### GUI surface

Settings → Runtimes → **Mesh** (new tab next to the existing Remote tab):
- Daemon status (running / stopped / error) with start/stop buttons
- "Discovered peers (LAN)" list — auto-populated from mDNS
- "Paired peers" list — peers in the allowlist
- "Pair a new peer" → generates an invite code modal OR pastes a received code
- Recent completion notifications visible inline (mirrors what's in `events_log`)

### Implementation order

1. **Daemon binary** (`apps/cli/src/bin/daemon.rs` or a separate crate): Tokio-based, listens on Unix socket + TCP. Hot-reload of `mesh_peers` from SQLite.
2. **Schema migrations** in `apps/desktop/src-tauri/src/lib.rs`.
3. **Ed25519 keygen + signing** — `ed25519-dalek` crate.
4. **mDNS** — `mdns-sd` crate. Broadcast + discovery.
5. **WS+JSON-RPC server** — `tokio-tungstenite` + handwritten dispatcher (one method, no need for a full JSON-RPC framework).
6. **CLI subcommands** — `ato daemon`, `ato mesh`.
7. **GUI Mesh tab** — list peers, pair, view completions.
8. **launchd / systemd / Windows service installers** — `ato daemon install` writes the appropriate service file and registers it.

### Risks / open questions

- **Daemon lifecycle on macOS**: launchd is the right path but signing-cert revocations (the Phase 6.x-I incident) could brick the daemon binary. Mitigation: ship as part of the existing ATO bundle which already navigates the cert story.
- **What happens when the laptop is closed?** v1: drop the message + log. The server's daemon will retry on next reconnect but with no replay buffer, messages from offline windows are lost. Acceptable for v1 because completion notifications are usually about ephemeral runs; the audit row still exists on the server.
- **Multi-laptop topologies**: if I have my desktop AND my laptop both paired to the same server, who gets the completion notification? v1: the most-recently-seen peer wins. v1.1 adds explicit routing.
- **Daemon CPU/memory overhead**: target <20MB RSS idle. mdns-sd + tokio-tungstenite at idle should easily fit.

---

## Phase 7.1+ — Paid cloud-relay tier

### What unlocks

1. **NAT traversal**: cloud relay daemon at `mesh.ato-cloud.com` (or wherever ato-cloud lives) acts as a rendezvous point. Peers behind asymmetric NAT can still talk.
2. **Full bi-directional dispatch**: server can call `dispatch_on_peer(peer_id, runtime, prompt, session_id, ...)` — the laptop runs the runtime and returns the response. This requires a per-peer ACL ("server X can ask laptop Y to run claude but not codex") which doesn't exist in 7.0.
3. **Multi-machine sessions**: a single session has turns landing from 3+ machines in real time, all bridged through the relay.
4. **Persistent peer presence**: relay holds a buffer of completion messages while a peer is offline; replays on reconnect.
5. **Observability dashboard**: see mesh health, message flow, peer status in one ato-cloud panel.

### Pricing point

Fits cleanly into the existing Pro/Team ladder:

- **Free desktop**: Phase 7.0 (LAN-only mesh)
- **Pro (paid)**: Phase 7.1 cloud relay, single-user mesh, up to N peers
- **Team (paid)**: shared mesh across team members, ACLs, audit log of cross-machine dispatches

Pricing exact-numbers TBD when 7.1 actually ships; the packaging shape is what's locked.

### Architecture sketch

```
┌──────────┐                                 ┌──────────┐
│  Peer A  │◄────WS─── ato-cloud relay ─WS──►│  Peer B  │
│ (laptop) │           (mesh.ato-cloud)      │ (server) │
└──────────┘                                 └──────────┘
```

The relay holds:
- Authenticated WS connection per online peer
- Message queue per offline peer (TTL: 24h)
- ACL table per peer (`can_post_completion`, `can_dispatch`, per-runtime allowlist)
- Audit log of all relayed messages

The relay does NOT see message contents — bodies are end-to-end encrypted between peers using their Ed25519 keys + Curve25519 for the symmetric session key. The relay routes ciphertext + headers.

### Out of scope for 7.1+ (real Phase 8 territory)

- Self-hosted relay (running your own relay box). Would be nice but a big maintenance story.
- Federated relays. Cloud-only for v1.
- Edge cases like: server has 3 paired laptops, dispatches "@laptop" — which one? Routing policy TBD.

---

## Decision log

| Decision | Date | Notes |
|---|---|---|
| Free tier = LAN-only via mDNS | 2026-05-13 | Avoids cloud dep for the basic use case |
| Paid tier = cloud relay for NAT + full bi-dir | 2026-05-13 | Real upgrade value; aligns with existing tier ladder |
| v7.0 ACL = completion notifications only | 2026-05-13 | Trust + scope minimization in v1 |
| Ed25519 for peer identity | 2026-05-13 | Standard, small, well-supported in Rust |
| WS + JSON-RPC for wire protocol | 2026-05-13 | Matches existing JSON shapes; no new toolchain |
| Daemon binary separate from `ato` CLI | 2026-05-13 | Different lifecycle; `ato daemon start` spawns it |
