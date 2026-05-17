# `commands.rs` Split Plan — maintenance sprint item 5

> **Goal:** split `apps/desktop/src-tauri/src/commands.rs` (17,317 lines, 237 `#[tauri::command]` fns) into per-domain files under `commands/` so future feature work doesn't fight a monolith on every change.
>
> **Constraint:** zero behavior change. Every command registered today must remain registered. Every call site that imports `commands::*` must still resolve.
>
> **Method:** incremental, one domain per PR, each PR runs the full release testing procedure including the dogfood of all feature surfaces.

---

## Inventory

237 `#[tauri::command]` functions in `commands.rs` today. Grouped by domain:

| Domain | Count | Examples |
|---|---|---|
| `agents` | 46 | `create_agent`, `prompt_agent`, `prompt_agent_with_history`, `list_agents`, `dispatch_to_group`, `read_agent_traces`, `get_agent_metrics`, `save_agent_variable`, `agent_evaluator_*` |
| `skills_mcps` | 23 | `get_local_skills`, `toggle_local_skill`, `create_skill`, `install_mcp_server`, `discover_mcp_server_tools`, `openclaw_list_skills` |
| `cron` | 17 | `list_cron_jobs`, `save_cron_job`, `trigger_cron_job`, `cron_os_scheduler_*`, `openclaw_cron_*` |
| `secrets_env` | 15 | `list_secrets`, `save_secret`, `list_llm_api_keys`, `save_llm_api_key`, `rotate_llm_api_key`, `list_env_vars` |
| `settings_config` | 13 | `get_local_config`, `record_local_config_change`, `list_backups`, `restore_backup`, `export_configuration`, `import_configuration`, `validate_settings_json`, `telemetry_*` |
| `runtimes` | 9 | `set_runtime_path`, `list_runtime_preferences`, `test_runtime_connection`, `list_available_runtimes` |
| `projects` | 8 | `discover_projects`, `list_projects`, `add_project`, `update_project`, `set_active_project` |
| `chat_threads` | 8 | `list_chat_threads`, `create_chat_thread`, `append_chat_message`, `search_chat_threads` |
| `hooks_evals` | 8 | `get_hooks`, `save_hook`, `register_workflow_webhook`, `evaluate_recent_traces` |
| `execution_logs` | 7 | `get_execution_logs`, `add_execution_log`, `link_execution_log_to_cloud_trace`, `start_replay`, `get_replay_job`, log-watcher trio |
| `live_health` | 7 | `get_health_status`, `record_health_check`, `start_health_poller`, `get_monitoring_snapshot` |
| `events_activity` | 7 | `track_event`, `get_queued_events`, `add_audit_log`, `get_audit_logs` |
| `recipes` | 7 | `recipes_list`, `recipes_create`, `recipes_install_template`, `recipes_set_enabled` |
| `auth_cloud` | 7 | `get_sync_status`, `set_sync_enabled`, `save_profile_snapshot`, `list_profile_snapshots` |
| `notifications` | 6 | `save_notification_channel`, `list_notification_channels`, `send_notification`, `test_notification_channel` |
| `context` | 5 | `get_context_estimate`, `get_context_for_runtime`, `list_context_files`, `read_context_file`, `write_context_file` |
| `workflows` | 5 | `list_workflows`, `save_workflow`, `load_workflow`, `delete_workflow`, `list_workflow_templates` |
| `models` | 4 | `list_ollama_models`, `list_model_configs`, `save_model_config`, `get_model_config` |
| `usage_billing` | 4 | `get_local_usage`, `get_daily_usage`, `get_burn_rate`, `compute_cost_recommendations_local` |
| `posts` | 5 | `posts_list`, `posts_create`, `posts_pending`, `posts_decide`, `posts_pending_count` |
| `knowledge` | 4 | `ingest_knowledge_text`, `delete_knowledge_chunk`, `delete_knowledge_source`, `retrieve_knowledge` |
| `sessions_dispatch` | 4 | `prompt_claude`, `prompt_api_provider`, `get_live_session_data`, `openclaw_list_sessions` |
| `files_paths` | 2 | `watch_project_files`, `import_env_file` |
| `analytics` | 4 | `get_analytics_summary`, `get_token_timeline`, `compute_regressions_local`, `compute_billing_surface_summary` |
| `external_deploy` | 1 | `get_project_bundle` |
| `security_policies` | 1 | `write_approval_policies` |
| `onboarding` | 1 | `get_onboarding_status` |

**Total: 237 commands → 25 proposed modules.** Average ~10 commands per file. The largest (`agents.rs`) is 46 commands, ~2000-3000 lines after extraction — still manageable.

---

## Revisions after codex-reviewer Round 1 (2026-05-17, session 6cf41892)

Codex flagged 4 real issues in the original plan. Each applied:

1. **`sessions_dispatch` merged into `agents`.** The dispatch pipeline (prompt history, hook loading, streaming, traces, evaluators, cron headless dispatch) is one subsystem in the current code — splitting them creates a fake boundary that helpers would have to cross every line. Final layout has 24 modules instead of 25.

2. **`hooks_evals` split into two.** Workflow hooks/webhooks and agent hooks/evaluators are different domains:
   - `workflow_webhooks.rs` — `get_hooks`, `save_hook`, `delete_hook`, `register_workflow_webhook`, `list_workflow_webhooks`, `delete_workflow_webhook`, `toggle_workflow_webhook`
   - `agent_hooks_evals.rs` — `evaluate_recent_traces` + the agent-hook commands currently grouped under `agents` actually belong here too (`list_agent_hooks`, `save_agent_hook`, `delete_agent_hook`)

3. **`shared.rs` extracted FIRST (PR 1), not last (PR 7).** Cross-cutting types/structs (`AgentMessage`, `DispatchResult`, `ReplayJob`, hook loaders, active-runs bookkeeping, log attribution) used across agents / execution_logs / cron / analytics. Waiting until PR 7 means every intermediate PR copies these or imports them via a half-baked path. Better to land them as a stable foundation up front.

4. **PR sequencing tightened — never bundle multiple domains in one PR.** The original PR 3 / PR 4 / PR 5 bundles are gone. New rule: each PR moves ONE domain, with the full release procedure pass. More PRs but smaller, lower-risk, faster to review.

5. **Failure mode codex named that I missed:** *"compile-green, dogfood-green, but semantic drift in duplicated helper logic."* Two copies of the dispatch pipeline that differ in subtle ways. Mitigation: `shared.rs` lands first, every helper that touches >1 domain goes there, dispatch path tested end-to-end after every move.

## Proposed file structure

```
apps/desktop/src-tauri/src/commands/
├── mod.rs                       — module declarations + re-exports
├── agents.rs                    — 46 cmds (the largest)
├── skills_mcps.rs               — 23
├── cron.rs                      — 17
├── secrets_env.rs               — 15
├── settings_config.rs           — 13
├── runtimes.rs                  — 9
├── projects.rs                  — 8
├── chat_threads.rs              — 8
├── hooks_evals.rs               — 8
├── execution_logs.rs            — 7
├── live_health.rs               — 7
├── events_activity.rs           — 7
├── recipes.rs                   — 7
├── auth_cloud.rs                — 7
├── notifications.rs             — 6
├── context.rs                   — 5
├── workflows.rs                 — 5
├── posts.rs                     — 5
├── models.rs                    — 4
├── usage_billing.rs             — 4
├── knowledge.rs                 — 4
├── sessions_dispatch.rs         — 4
├── analytics.rs                 — 4
├── files_paths.rs               — 2
├── external_deploy.rs           — 1
├── security_policies.rs         — 1
├── onboarding.rs                — 1
└── helpers.rs                   — non-#[tauri::command] helpers that multiple modules use
```

Existing `commands/mod.rs` is the CLI's, which is fine (different crate). The desktop side adds its own `commands.rs` → `commands/mod.rs` migration.

---

## Migration strategy — incremental, one PR per domain (REVISED post codex)

This is NOT one big-bang refactor. Each extraction is a separate PR going through the full release testing procedure. Per codex Round 1 feedback: **never bundle multiple domains in one PR**.

**Sequence (foundation first, then small → large):**

1. **PR 1 — Foundation: `shared.rs`**. Extract cross-cutting types (`AgentMessage`, `DispatchResult`, `ReplayJob`, etc.) and helpers that more than one domain uses. NO commands moved yet. Lets every subsequent PR import a stable foundation rather than duplicating.
2. **PR 2 — `models.rs` (4 cmds)** — proof of pattern with the smallest domain.
3. **PR 3 — `usage_billing.rs` (4 cmds)**.
4. **PR 4 — `knowledge.rs` (4 cmds)**.
5. **PR 5 — `posts.rs` (5 cmds)**.
6. **PR 6 — `analytics.rs` (4 cmds)**.
7. **PR 7 — `files_paths.rs` (2 cmds)**.
8. **PR 8 — `external_deploy.rs` (1 cmd)**.
9. **PR 9 — `security_policies.rs` (1 cmd)**.
10. **PR 10 — `onboarding.rs` (1 cmd)**.
11. **PR 11 — `context.rs` (5 cmds)**.
12. **PR 12 — `workflows.rs` (5 cmds)**.
13. **PR 13 — `workflow_webhooks.rs` (7 cmds)** — workflow side of the old hooks_evals split.
14. **PR 14 — `notifications.rs` (6 cmds)**.
15. **PR 15 — `chat_threads.rs` (8 cmds)**.
16. **PR 16 — `projects.rs` (8 cmds)**.
17. **PR 17 — `agent_hooks_evals.rs` (~5 cmds)** — agent-evaluator + agent-hook commands.
18. **PR 18 — `live_health.rs` (7 cmds)**.
19. **PR 19 — `events_activity.rs` (7 cmds)**.
20. **PR 20 — `recipes.rs` (7 cmds)**.
21. **PR 21 — `auth_cloud.rs` (7 cmds)**.
22. **PR 22 — `execution_logs.rs` (7 cmds)**.
23. **PR 23 — `runtimes.rs` (9 cmds)**.
24. **PR 24 — `settings_config.rs` (13 cmds)**.
25. **PR 25 — `secrets_env.rs` (15 cmds)**.
26. **PR 26 — `cron.rs` (17 cmds)**.
27. **PR 27 — `skills_mcps.rs` (23 cmds)**.
28. **PR 28 — `agents.rs` (50 cmds, includes the merged sessions_dispatch commands)** — the elephant. Last + biggest, so all helper-extraction patterns are settled.
29. **PR 29 — cleanup: anything remaining in old `commands.rs` is non-command utility code; finalize `helpers.rs` and either delete the old file or reduce to a stub re-export.

Each PR runs the full release testing procedure (sections 3-7). Doing this across multiple sessions / days is fine — the sequencing is preserved by the PR number.

Each PR runs:
- §3 Shared crates contract — `cargo test` on all packages
- §4 Build matrix — `cargo check` + `cargo build --release` desktop and CLI; `tsc --noEmit`
- §5A Dogfood setup — fresh session
- §5B Mechanical smoke — exercise EVERY feature surface in section 2 of the procedure that touches the moved commands
- §5C War-room — codex-reviewer + pr-reviewer on the diff
- §6 Regression — full regression suite green

**Failure mode the procedure prevents:** missing a command from the `invoke_handler!` registration. The dogfood pass exercises every UI surface; if a command is missing, the UI errors at runtime and the dogfood catches it.

---

## Non-command code in `commands.rs`

Beyond the 237 `#[tauri::command]` functions, `commands.rs` contains:

- Struct definitions (request/response shapes for each command — these move with their command)
- Helper functions (non-command) called by multiple commands — these go to `helpers.rs` OR move with their primary caller if only one
- Module-level constants (`DEFAULT_*`, `MAX_*`) — these go to `helpers.rs` or the relevant module
- Free-standing utility functions (`truncate_for_log`, `home_dir`, etc.) — `helpers.rs` if cross-cutting

A grep before extraction identifies cross-cutting helpers; they move to `helpers.rs` in PR 7.

---

## Risks + mitigations

| Risk | Mitigation |
|---|---|
| Missing a command from `invoke_handler!` after move | The dogfood pass (§5B) exercises every UI feature. A missing registration → UI errors → caught. |
| Helper function lost in the move (deleted instead of moved to helpers.rs) | `cargo check` after each PR — compiler catches every call site. |
| Visibility scope change (`pub` vs `pub(crate)` vs private) | Default to `pub` when migrating; tighten later as a follow-up. |
| Struct definitions duplicated by accident | Move each struct exactly once; if two commands share a struct, move it to `helpers.rs`. |
| Order-dependent code (some commands call others via Tauri internal dispatch) | None observed today, but worth grepping for `tauri::AppHandle::run_command` or similar. |
| Compile time regression (many small files vs one big file) | Modern Rust handles this fine; if measurable, accept the trade-off — IDE / review speed wins. |

---

## How the war-room reviews this plan (BEFORE any code moves)

This doc is the input to a war-room review. The seats to consult:

- **codex-reviewer** — catches architectural smells, calls out non-obvious coupling. Asked to flag any domain we've split wrong, any cross-cutting concern we've missed, any sequencing risk.
- **pr-reviewer** — reviews this plan as if it were a PR. Asked to commit to one of: `[APPROVE]` (start PR 1), `[REFINE: …]` (apply changes first), `[DISSENT]` (propose a different approach).

War-room session id will be captured here once it runs.

---

## Success criteria

- `apps/desktop/src-tauri/src/commands.rs` → either deleted or reduced to a stub that re-exports `commands/*`
- Every `#[tauri::command]` registered in `lib.rs::invoke_handler!` still resolves
- Full release testing procedure passes for each PR
- Total compile time post-split: same or better (Rust handles many small files fine)
- New contributors can find any command by domain in < 30 seconds (vs `grep` through 17K lines today)
