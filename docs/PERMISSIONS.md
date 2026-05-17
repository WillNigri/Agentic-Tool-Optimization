# ATO Permissions Ladder

> **Principle: ask for the minimum permissions, only when the user needs them.** A new user downloading the app should be able to *open it and look around* without granting anything. The first permission prompt should fire at the moment the user invokes a feature that strictly requires it — paired with clear context for what's being asked and why.

This doc is the contract for what permissions ATO requests, when, and from which surface (CLI / Desktop / MCP). It's load-bearing for future feature work: a new permission only ships if it earns a row here.

## The ladder (today)

| Permission | What it grants | When ATO asks | Surfaces affected | Required for |
|---|---|---|---|---|
| **macOS Keychain Access** | Read/write `ato-desktop / master_key_v1` — the AES-256-GCM master key used to decrypt stored API keys | First time the user dispatches against a metered API runtime AND has a stored API key (not env var). Lazy — never on app startup. | CLI, Desktop | Stored API key decryption for `anthropic / google / openai / minimax / grok / deepseek / qwen / openrouter / together` |
| **Local filesystem `~/.ato/`** | Read/write the user's own data directory (SQLite, JSONL logs, agent files) | Implicit — macOS doesn't prompt for an app writing in its own user-scoped directory | CLI, Desktop | Every core operation |
| **Network access (HTTPS outbound)** | Reach provider APIs + GitHub release manifests | Implicit on macOS — no prompt for outbound HTTPS from an app the user launched | CLI, Desktop | API-key dispatches, optional cloud sync, auto-updater |
| **Notifications** | macOS Notification Center alerts (cron-job results, regression detection) | When the user creates their first scheduled job that opts into notifications. Never on startup. | Desktop | Cron-job alerts (opt-in feature) |
| **Login items / auto-start** | Background launch on login (for cron monitor) | When the user explicitly enables the cron monitor's "Run at login" toggle | Desktop | Cron jobs surviving reboot (opt-in) |
| **Accessibility / Automation** | Send key events / read another app's UI | NEVER — ATO does not ask for these. | — | Not used by any feature |
| **Camera / Microphone / Screen recording** | Media capture | NEVER — ATO does not ask for these. | — | Not used by any feature |
| **Full Disk Access** | Read anywhere on disk | NEVER — ATO does not ask for this. | — | Not used by any feature |

The keychain prompt is the only OS-level dialog a user should ever see, and only if they've stored an API key (not env var) AND they're running a metered API dispatch.

## What "lazy" looks like in code

- **Never** call `encryption::master_key()` on app startup, on module init, or in a `Default::new()` somewhere. Only call it from the dispatch path's `resolve_api_key()`, and only after the env var path has been checked first.
- **Never** subscribe to notifications, watch filesystem events outside `~/.ato/`, or open WebSocket connections at startup. Subscribe when the user enables the feature.
- **Within a single process**, cache the result of any permission-bearing call (e.g., master key) in a `OnceLock` so we don't re-trigger prompts on subsequent uses. ATO ships this for the master key today (commit shipping with `docs(permissions)`).

## Dev-mode escape hatch

Unsigned local builds (`cargo build --release` on a dev machine) produce a fresh code signature on every rebuild. macOS keychain ACL is bound to signature, so even "Always Allow" doesn't survive the next rebuild — the dialog comes back. For contributors hitting this:

```bash
# Set once in your shell rc. Bypasses keychain entirely for the CLI
# and for the desktop's tauri dev process. Production users on signed
# releases NEVER set this — they go through the normal keychain path.
export ATO_MASTER_KEY_B64="<32 random bytes, base64>"
# To generate one:  openssl rand -base64 32
```

The bypass is by design and intentionally documented:

- **Security model unchanged.** Env vars are user-scope; the keychain is user-scope. Anyone with user-level access can read either. The dev bypass doesn't widen the attack surface — it just trades the dialog for the env var.
- **Production never sets this.** Apple-signed releases ship without the env var. The keychain path is the default; the dev bypass only fires when the contributor explicitly opts in.
- **Document on first contributor PR.** The bypass should appear in `CONTRIBUTING.md` (TODO) so new contributors don't get blocked on the keychain dialog.

## Adding a new permission

If a future feature wants to add a permission ATO doesn't currently request, the PR must:

1. **Justify the need.** What feature is it for? Can the feature ship without the permission (e.g., via a different code path)?
2. **Add a row to the table above.** Permission name, what it grants, exact moment of the prompt, surfaces affected, required-for.
3. **Default OFF.** New permissions ship behind an opt-in toggle the user has to enable. The first prompt fires the first time the toggle is flipped, not at install.
4. **Pair the prompt with a one-line context message** explaining why ATO needs it. Generic system prompts (`"ato wants notification permission"`) without explanation are the anti-pattern this doc exists to prevent.
5. **Get a war-room review** if the permission is one of the high-friction ones (Full Disk, Accessibility, Automation, Camera, Mic, Screen Recording). Default disposition for those five is `NEVER` per the table.

## What we explicitly will NOT do

- Pre-emptively prompt for permissions on first launch.
- Bundle multiple permission requests into a single onboarding flow ("approve all five at once" — Mac App Store anti-pattern).
- Use Full Disk Access, Accessibility, or Automation. These are the heaviest-friction permissions on macOS; ATO's scope (multi-runtime AI dispatch + local audit trail) doesn't need any of them.
- Ship features that require new permissions without a war-room review of the trade-off.

## See also

- `apps/cli/src/encryption.rs` — master key fetch + cache + dev-mode bypass
- `apps/desktop/src-tauri/src/encryption.rs` — desktop mirror of the same logic
- `docs/SESSIONS.md` — how sessions work; the only data ATO captures is what the user dispatches
- `SECURITY.md` (root) — broader threat model (TODO if not yet written)
