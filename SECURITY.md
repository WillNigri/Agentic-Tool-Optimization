# Security Policy

ATO is local-first software that handles API keys, prompts, and
remote-runtime credentials. We take vulnerability reports seriously
and respond fast.

## Reporting a vulnerability

**Do not open a public issue.** Send the report by one of:

- Email: `security@agentictool.ai`
- Encrypted PGP / Signal — request a key by emailing the address
  above with the subject `SECURITY KEY REQUEST`
- GitHub private advisory: <https://github.com/WillNigri/Agentic-Tool-Optimization/security/advisories/new>

Include:

1. **What you found** — affected version, file paths, the
   misbehavior in concrete terms.
2. **How to reproduce** — minimal steps, ideally with `ato` CLI
   commands or a screenshot of the desktop state.
3. **Impact you can imagine** — even if you're not sure it's
   exploitable, tell us your worst-case theory.
4. **Whether you'd like attribution** in the fix's release notes
   (we'll honor "anonymous", a handle, or a real name + link).

We acknowledge every report within **72 hours**. A non-critical fix
typically ships within 7-14 days; a critical fix is faster and may
roll out as an off-cycle patch.

## Supported versions

We patch the latest minor release. Pre-release branches (e.g.
`phase-7-mesh`) are not patched on their own — fixes land in the
next merged release.

| Version | Patches accepted |
|---|---|
| `2.4.x` | ✅ |
| < `2.4.0` | ❌ — upgrade |

## Scope

In scope for a security report:

- **OSS desktop app + CLI** (this repo): API-key storage, prompt
  injection vectors, command-injection in dispatch / SSH / mesh
  paths, Tauri capability misuse, frontend XSS, auto-updater
  trust chain.
- **Mesh / Phase 7** (`phase-7-mesh` branch): daemon-to-daemon
  protocol, invite-code pairing, signature verification, replay
  defenses.
- **Brew distribution** (`WillNigri/homebrew-ato`): cask
  integrity, signing chain.
- **Public website** (`agentictool.ai` repo): content + form
  handling.

Out of scope:

- **Cloud product** (`ato-cloud`, closed-source): report via the
  same email with `[ato-cloud]` in the subject so it routes to
  the right reviewer.
- **Vendor APIs** (Anthropic, OpenAI, Google, MiniMax, etc.) — go
  to the vendor.
- **Self-inflicted misconfig** — e.g., chmodding `~/.ato` to
  `777` yourself, then complaining the DB is readable. We chmod
  the DB to `0600` on startup; if you flip it, that's on you.

## What we promise

- **No legal action** for good-faith research against your own
  install / a sandbox.
- **No bounty fund yet** — we're a small team. Severe reports get
  swag, a thanks line in the release notes, and a real
  conversation with maintainers. As the project grows we'll
  formalize this.
- **Coordinated disclosure** — we publish the CVE + patch notes
  together, never name the researcher without their consent, and
  give you a draft to review 24h before going public.

## Security postures we already maintain

- API keys in `~/.ato/local.db` are AES-256-GCM encrypted under a
  master key kept in the OS keychain (macOS Keychain / Linux
  Secret Service / Windows Credential Manager). The DB file
  itself is chmod 0600 on Unix.
- All non-trivial PRs run multi-LLM review via `ato review
  --consensus` (see [CONTRIBUTING.md](CONTRIBUTING.md)) — the
  output is in the PR description.
- The Tauri auto-updater is signed; pubkey rotation is documented
  in the release notes when it happens.
- The mesh daemon (Phase 7) binds 127.0.0.1 by default; LAN
  exposure is opt-in.

Past security audits are kept in our internal archive (not redistributed with the OSS repo); contact the maintainers if you're evaluating ATO for a security-sensitive deployment.
