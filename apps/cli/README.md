# `ato` — ATO CLI

Local-first CLI for ATO. Talks to `~/.ato/local.db` directly. Designed to be driven by humans (with `--human`) and AI coding agents (default JSON output).

See [`../../AGENTS.md`](../../AGENTS.md) for the full command surface, MCP-tool equivalents, and example recipes a coding agent would use.

## Install

### Build from source (today)

```bash
cd apps/cli
cargo build --release
cp target/release/ato /usr/local/bin/   # or ~/.local/bin/ — anywhere on PATH
ato --version
```

### Pre-built (shipped in v2.3.0)

The `ato` binary now ships as a Tauri sidecar inside every desktop bundle (DMG, AppImage, NSIS installer, .deb). After installing the desktop app:

```bash
# One-time PATH setup
ato setup-path
```

This symlinks the bundled CLI to `/usr/local/bin/ato` (or `~/.local/bin/ato` if `/usr/local/bin` isn't writable). Idempotent — running it again is a no-op once set up. On macOS, the bundled binary lives at `/Applications/ATO.app/Contents/Resources/binaries/ato-<arch>`; the first `setup-path` invocation typically looks like:

```bash
/Applications/ATO.app/Contents/Resources/binaries/ato-aarch64-apple-darwin setup-path
```

After that one command, plain `ato <command>` works from any shell.

## Quick check

```bash
$ ato --version
ato 2.3.0-dev

$ ato dispatches recent --limit 5 --human
5 dispatches:

  [success] 2026-05-10T15:33:27 codex (3821b0c4, 12171ms, —)
  [success] 2026-05-10T15:30:52 claude (dc01aae1, 18897ms, —)
  ...
```

## Output

JSON by default (agent-friendly). Pass `--human` for terminal output, `--quiet` to suppress non-essential output, `--db <path>` to point at a non-default SQLite file.

## Commands (Phase 1 shipped)

**Observation** (read-only):
- `ato dispatches recent [--limit N] [--runtime X] [--status Y]`
- `ato runs live`
- `ato runs get <id>`
- `ato config-changes list --agent <slug> [--since 7d]`
- `ato files-touched <run-id>`
- `ato replays for-trace <trace-id>`

**Operations**:
- `ato dispatch <runtime> "<prompt>" [--model M] [--agent S]`
- `ato replay start <source-id> --runtime <target> [--model M]`
- `ato replay get <job-id> [--wait]`
- `ato compare <id-a> <id-b>`

**Authoring**:
- `ato skills draft --from-replay <job-id> [--out path]`

## Coming next (Phase 2+)

- `ato regressions list` (Phase 2 — local-mode regressions)
- `ato cost recommendations` (Phase 2)
- `ato kill <run-id>` (Phase 1.x)
- `ato agents create | update <slug>` (Phase 1.x)
- `ato events watch [--type T]` (Phase 4 — once event bus exists)
- `ato events post --type X --payload '{...}'` (Phase 4)
- `ato recipes create | list | enable | disable` (Phase 4)

See `apps/cli/src/main.rs` for current subcommand definitions and `../../AGENTS.md` for the agent-facing reference.

## Architecture

- Pure Rust binary (no Tauri deps)
- Talks to `~/.ato/local.db` (SQLite) directly via `rusqlite`
- Read commands open the DB read-only; write commands re-open with write privileges
- Dispatch + replay shell out to the runtime CLIs on `PATH` (`claude`, `codex`, `gemini`, etc.), capture stdout/stderr, persist to `execution_logs` + `replay_jobs` tables
- The desktop GUI writes the *same* tables, so anything you do in the CLI shows up in the GUI on next refresh and vice-versa
- Pricing helpers mirror `apps/desktop/src/lib/pricing.ts` and `apps/desktop/src-tauri/src/commands.rs` — keep all three in sync when adding models

## Why the CLI doesn't talk to the running desktop

By design. The CLI works without the GUI being open. Both processes touch the same SQLite file; SQLite's journal mode handles concurrent reads cleanly. The desktop's in-memory `active_runs` registry mirrors to a `live_runs` table so the CLI can see it without IPC.

## License

MIT (same as the repo).
