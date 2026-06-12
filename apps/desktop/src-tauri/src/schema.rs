// schema.rs — DB schema initialization.
//
// All CREATE TABLE / CREATE INDEX / CREATE VIEW / ALTER TABLE
// migrations live here. Called once at app startup from `lib::run`
// after the SQLite connection opens.
//
// 2026-05-19 elegance split — was a 983-line `pub fn init_database`
// sitting inside lib.rs, the single largest fn in the codebase.
// Extracted to keep lib.rs focused on the Tauri builder + handler
// registration. Future v2.8.0 work can sub-split this further (one
// migration per file) once the table count exceeds 50.

use rusqlite::Connection;

pub fn init_database(conn: &Connection) {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS skill_toggles (
            file_path TEXT PRIMARY KEY,
            enabled   INTEGER NOT NULL DEFAULT 1
        );
        -- v2.5.1 — per-runtime monitored toggle. The Insights → Health
        -- panel only renders cards for runtimes the user opted into
        -- monitoring. First launch seeds this table by detecting which
        -- runtimes are installed (via which_cli) so the user doesn't
        -- start with red cards for runtimes they've never touched.
        -- Adding a new runtime is just a row with monitored=1.
        CREATE TABLE IF NOT EXISTS runtime_preferences (
            runtime   TEXT PRIMARY KEY,
            monitored INTEGER NOT NULL DEFAULT 1,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS cron_alerts (
            id         TEXT PRIMARY KEY,
            job_id     TEXT NOT NULL,
            type       TEXT NOT NULL,
            message    TEXT NOT NULL,
            created_at TEXT NOT NULL,
            acknowledged INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS profile_snapshots (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            description TEXT,
            runtime     TEXT NOT NULL,
            files_json  TEXT NOT NULL,
            created_at  TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS projects (
            id           TEXT PRIMARY KEY,
            name         TEXT NOT NULL,
            path         TEXT NOT NULL UNIQUE,
            is_active    INTEGER NOT NULL DEFAULT 0,
            skill_count  INTEGER NOT NULL DEFAULT 0,
            last_accessed TEXT,
            created_at   TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS secrets (
            id           TEXT PRIMARY KEY,
            name         TEXT NOT NULL,
            key_type     TEXT NOT NULL,
            runtime      TEXT,
            project_id   TEXT,
            created_at   TEXT NOT NULL,
            updated_at   TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS env_vars (
            id           TEXT PRIMARY KEY,
            project_id   TEXT,
            runtime      TEXT,
            key          TEXT NOT NULL,
            value        TEXT NOT NULL,
            created_at   TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS model_configs (
            id           TEXT PRIMARY KEY,
            runtime      TEXT NOT NULL,
            project_id   TEXT,
            model_id     TEXT NOT NULL,
            max_tokens   INTEGER,
            temperature  REAL,
            created_at   TEXT NOT NULL,
            updated_at   TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS execution_logs (
            id           TEXT PRIMARY KEY,
            runtime      TEXT NOT NULL,
            prompt       TEXT,
            response     TEXT,
            tokens_in    INTEGER,
            tokens_out   INTEGER,
            duration_ms  INTEGER,
            status       TEXT NOT NULL,
            error_message TEXT,
            skill_name   TEXT,
            cloud_trace_id TEXT,
            created_at   TEXT NOT NULL
        );
        -- v2.1.0 Replay infra. One row per replay dispatch the user
        -- triggered. source_execution_log_id references the original
        -- prompt; status drives the polling UI. Response capped at
        -- 64KB for the same reason execution_logs.response is.
        CREATE TABLE IF NOT EXISTS replay_jobs (
            id                       TEXT PRIMARY KEY,
            source_execution_log_id  TEXT NOT NULL,
            source_cloud_trace_id    TEXT,
            source_runtime           TEXT NOT NULL,
            source_model             TEXT,
            target_runtime           TEXT NOT NULL,
            target_model             TEXT,
            status                   TEXT NOT NULL,
            response                 TEXT,
            duration_ms              INTEGER,
            error_message            TEXT,
            started_at               TEXT NOT NULL,
            finished_at              TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_replay_jobs_source
            ON replay_jobs(source_execution_log_id, started_at DESC);
        CREATE INDEX IF NOT EXISTS idx_replay_jobs_cloud_trace
            ON replay_jobs(source_cloud_trace_id, started_at DESC);
        CREATE TABLE IF NOT EXISTS health_checks (
            id           TEXT PRIMARY KEY,
            runtime      TEXT NOT NULL,
            status       TEXT NOT NULL,
            latency_ms   INTEGER,
            error_message TEXT,
            checked_at   TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS audit_logs (
            id            TEXT PRIMARY KEY,
            action        TEXT NOT NULL,
            resource_type TEXT NOT NULL,
            resource_id   TEXT,
            resource_name TEXT,
            details       TEXT,
            created_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_audit_logs_action ON audit_logs(action);
        CREATE INDEX IF NOT EXISTS idx_audit_logs_created ON audit_logs(created_at);
        CREATE INDEX IF NOT EXISTS idx_audit_logs_resource ON audit_logs(resource_type, resource_id);
        CREATE TABLE IF NOT EXISTS llm_api_keys (
            id            TEXT PRIMARY KEY,
            provider      TEXT NOT NULL,
            name          TEXT NOT NULL,
            key_preview   TEXT NOT NULL,
            encrypted_key TEXT NOT NULL,
            project_id    TEXT,
            runtime       TEXT,
            is_active     INTEGER NOT NULL DEFAULT 1,
            last_used     TEXT,
            usage_count   INTEGER NOT NULL DEFAULT 0,
            created_at    TEXT NOT NULL,
            updated_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_llm_keys_provider ON llm_api_keys(provider);
        CREATE INDEX IF NOT EXISTS idx_llm_keys_project ON llm_api_keys(project_id);
        -- v2.7.14 master_key_v2 PR-1 (ledger-only foundation). Tracks
        -- every master-key version this install has ever seen so
        -- later PRs can detect identity mismatches without orphaning
        -- ciphertexts. PR-1 ONLY creates the table + writes a v1 row
        -- on startup. By design the worst case is an extra row in a
        -- new table.
        CREATE TABLE IF NOT EXISTS master_key_ledger (
            version           TEXT PRIMARY KEY,
            keychain_account  TEXT NOT NULL,
            ciphertext_format TEXT NOT NULL,
            identity_probe    TEXT,
            source            TEXT NOT NULL DEFAULT 'keychain',
            created_at        TEXT NOT NULL,
            retired_at        TEXT,
            notes             TEXT,
            -- v2.14.3 — encrypted canary used by rekey to validate the
            -- old-key candidate before any destructive operation.
            -- Plaintext is a fixed string (CANARY_PLAINTEXT in encryption.rs);
            -- the column holds its ciphertext under THIS row master key.
            -- NULL on pre-2.14.3 rows; backfilled on first v2.14.3 launch.
            canary_ciphertext TEXT
        );
        CREATE TABLE IF NOT EXISTS agents (
            id            TEXT PRIMARY KEY,
            slug          TEXT NOT NULL,
            display_name  TEXT NOT NULL,
            description   TEXT,
            runtime       TEXT NOT NULL,
            model         TEXT,
            project_id    TEXT,
            system_prompt TEXT,
            permissions   TEXT,
            skills        TEXT,
            mcps          TEXT,
            goal          TEXT,
            file_path     TEXT,
            created_at    TEXT NOT NULL,
            last_used_at  TEXT,
            UNIQUE (runtime, slug)
        );
        CREATE INDEX IF NOT EXISTS idx_agents_runtime ON agents(runtime);
        CREATE INDEX IF NOT EXISTS idx_agents_last_used ON agents(last_used_at DESC);
        CREATE INDEX IF NOT EXISTS idx_agents_project ON agents(project_id);
        -- v1.4.0 — Production-Grade Agent Authoring (context engineering).
        -- F1: Dynamic prompts with variables. Each row is one named variable
        --     belonging to an agent, with a kind-specific resolver config.
        CREATE TABLE IF NOT EXISTS agent_variables (
            id          TEXT PRIMARY KEY,
            agent_id    TEXT NOT NULL,
            name        TEXT NOT NULL,
            kind        TEXT NOT NULL,            -- static | env | project-path | file | db-query | mcp-call | computed
            config_json TEXT NOT NULL,
            enabled     INTEGER NOT NULL DEFAULT 1,
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL,
            UNIQUE (agent_id, name)
        );
        CREATE INDEX IF NOT EXISTS idx_agent_vars_agent ON agent_variables(agent_id);
        -- v2.8.x P2 — Security AMEND (war-room 87E6CADF round 3, security-
        -- specialist NON-NEGOTIABLE): when variables.advanced re-tiered to
        -- Free, every user can now configure variables of kind 'file',
        -- 'db-query', or 'computed' that read local files / run code. A
        -- hostile MCP could spawn an agent that reads ~/.ssh/id_rsa and
        -- the file content gets inlined into the next LLM prompt — direct
        -- exfiltration. Mitigation: explicit per-variable consent grant
        -- required BEFORE the resolver will execute. Frontend prompts at
        -- save time with the exact path; user must acknowledge.
        --
        -- Scope semantics:
        --   'once'    — single resolve, then revoke (debug / one-shot use)
        --   'session' — until the app restart (not yet wired; future)
        --   'always'  — until user revokes from Settings → Permissions
        CREATE TABLE IF NOT EXISTS variable_consent_grants (
            id           TEXT PRIMARY KEY,
            variable_id  TEXT NOT NULL,
            scope        TEXT NOT NULL,        -- 'once' | 'session' | 'always'
            granted_at   TEXT NOT NULL,
            granted_resource TEXT NOT NULL,    -- the exact path/sql/expr at consent time
            revoked_at   TEXT,                  -- NULL = still active
            FOREIGN KEY (variable_id) REFERENCES agent_variables(id) ON DELETE CASCADE,
            UNIQUE (variable_id)
        );
        CREATE INDEX IF NOT EXISTS idx_consent_variable ON variable_consent_grants(variable_id);
        -- F2: Pre-call context hooks. Ordered list of resolvers that run
        --     before each LLM turn and inject results into the user message.
        CREATE TABLE IF NOT EXISTS agent_hooks (
            id          TEXT PRIMARY KEY,
            agent_id    TEXT NOT NULL,
            position    INTEGER NOT NULL,
            name        TEXT NOT NULL,
            kind        TEXT NOT NULL,            -- mcp-call | file | db-query | webhook | computed
            config_json TEXT NOT NULL,
            enabled     INTEGER NOT NULL DEFAULT 1,
            created_at  TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_agent_hooks_agent ON agent_hooks(agent_id, position);
        -- F4: Multi-agent groups (router + children).
        CREATE TABLE IF NOT EXISTS agent_groups (
            id            TEXT PRIMARY KEY,
            slug          TEXT NOT NULL UNIQUE,
            display_name  TEXT NOT NULL,
            description   TEXT,
            runtime       TEXT NOT NULL,
            router_config TEXT,
            file_path     TEXT,
            created_at    TEXT NOT NULL,
            last_used_at  TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_groups_runtime ON agent_groups(runtime);
        CREATE TABLE IF NOT EXISTS agent_group_members (
            group_id    TEXT NOT NULL,
            agent_id    TEXT NOT NULL,
            role        TEXT NOT NULL,             -- 'router' | 'child'
            position    INTEGER NOT NULL,
            PRIMARY KEY (group_id, agent_id),
            FOREIGN KEY (group_id) REFERENCES agent_groups(id) ON DELETE CASCADE,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_group_members_agent ON agent_group_members(agent_id);
        -- v1.4.0 Polish-T2 — Skill version history. We snapshot a SKILL.md's
        -- contents on edit so the user can scroll back through prior versions
        -- and restore one. Versions live in SQLite (not on disk) — they are
        -- recovery state, not a vcs.
        CREATE TABLE IF NOT EXISTS skill_versions (
            id            TEXT PRIMARY KEY,
            file_path     TEXT NOT NULL,
            content       TEXT NOT NULL,
            content_hash  TEXT NOT NULL,
            note          TEXT,
            created_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_skill_versions_path ON skill_versions(file_path, created_at DESC);
        -- v1.5.0 — Persistent chat threads. Makes the bottom Chat pane a
        -- destination instead of an ephemeral input. A thread isn't bound to
        -- a runtime: each message records which runtime answered it, so the
        -- same conversation can hop runtimes mid-flight. project_id is
        -- optional — threads can be global.
        CREATE TABLE IF NOT EXISTS chat_threads (
            id              TEXT PRIMARY KEY,
            title           TEXT NOT NULL,
            project_id      TEXT,
            agent_id        TEXT,                       -- last-used agent (sticky default)
            created_at      TEXT NOT NULL,
            last_message_at TEXT,
            message_count   INTEGER NOT NULL DEFAULT 0,
            archived        INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_chat_threads_project
            ON chat_threads(project_id, last_message_at DESC);
        CREATE INDEX IF NOT EXISTS idx_chat_threads_recent
            ON chat_threads(last_message_at DESC);

        CREATE TABLE IF NOT EXISTS chat_messages (
            id          TEXT PRIMARY KEY,
            thread_id   TEXT NOT NULL,
            role        TEXT NOT NULL,                  -- 'user' | 'assistant' | 'system' | 'attachment' | 'error'
            content     TEXT NOT NULL,
            runtime     TEXT,                           -- which runtime produced this turn (assistant only)
            agent_slug  TEXT,                           -- which agent (if any) handled the dispatch
            metadata    TEXT,                           -- JSON: file path for attachments, etc.
            created_at  TEXT NOT NULL,
            FOREIGN KEY (thread_id) REFERENCES chat_threads(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_chat_messages_thread
            ON chat_messages(thread_id, created_at ASC);
        ",
    )
    .expect("Failed to initialize database tables");

    // F3 + F5 — additive columns on the existing `agents` table. Wrapped in
    // separate calls so existing local.db files upgrade without complaint.
    // SQLite returns "duplicate column" if the column already exists; ignore.
    let _ = conn.execute("ALTER TABLE agents ADD COLUMN role_models_json TEXT", []);
    let _ = conn.execute("ALTER TABLE agents ADD COLUMN memory_policy_json TEXT", []);
    // v1.5.0 — dispatch kind on agent groups: "routed" (router picks one
    // child) vs "sequential" (children run in order, output of N is input
    // to N+1). Default keeps existing groups behaving as before.
    let _ = conn.execute(
        "ALTER TABLE agent_groups ADD COLUMN dispatch_kind TEXT NOT NULL DEFAULT 'routed'",
        [],
    );
    // v2.0.0 — Internal vs External agent kind.
    let _ = conn.execute(
        "ALTER TABLE agents ADD COLUMN kind TEXT NOT NULL DEFAULT 'internal'",
        [],
    );
    // v2.7.8 PR-6 — opt-in enforcement of agent.permissions.
    //
    // PR-2 wired the dispatch path to read agent.permissions and
    // translate them into runtime-native flags. Pre-v2.7.8 agents may
    // already have permissions populated (the wizard has been writing
    // them since v1.3.0) — turning enforcement on for them retroactively
    // would silently change dispatch behaviour for every existing agent
    // on upgrade. That's the migration trap claude flagged in the
    // 2026-05-20 war-room.
    //
    // Resolution: `permissions_migrated_at` is the explicit opt-in flag.
    // - NULL → dispatch path ignores stored permissions and uses
    //   pre-PR-2 hardcoded defaults. Backward compatibility preserved
    //   for every existing agent on day 1.
    // - Non-NULL → dispatch path reads + enforces stored permissions.
    //   Stamped at create_agent for any agent newly created on v2.7.8+
    //   (new agents have correct expectations from the wizard). Stamped
    //   when the user explicitly confirms migration via the next-edit
    //   toast for pre-existing agents.
    //
    // The migration timestamp also serves as audit evidence: when did
    // this agent's policy become enforceable?
    let _ = conn.execute(
        "ALTER TABLE agents ADD COLUMN permissions_migrated_at TEXT",
        [],
    );
    // v2.7.9 — Felipe P5. Optional prompt that fires automatically when
    // an agent is dispatched without one. Enables one-click "Run" for
    // monitoring agents (VPS health, telemetry) where the prompt is
    // always the same. NULL/empty preserves today's interactive
    // behavior. S9 wires the "use default when prompt is blank" branch
    // in prompt_agent_inner.
    let _ = conn.execute("ALTER TABLE agents ADD COLUMN default_prompt TEXT", []);
    // v2.1.0 — execution_logs links to its corresponding cloud
    // agent_traces row when the dispatch was uploaded. Powers replay
    // ("look up the local prompt for this cloud trace ID"). Existing
    // rows stay NULL and won't be replayable, which is honest — they
    // predate the link plumbing.
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN cloud_trace_id TEXT", []);
    // v2.0.0 Wave 2 — Local knowledge for external agents. Each row is one
    // chunk of text + its OpenAI text-embedding-3-small vector. Embedding
    // stored as a BLOB of f32 bytes (1536 floats = 6144 bytes per chunk).
    // Storage trade-off: keeping the embedding alongside the text means the
    // bundle inliner doesn't have to re-embed at deploy time, AND retrieval
    // testing in the UI is just a SELECT + cosine sim in Rust.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_knowledge_chunks (
            id          TEXT PRIMARY KEY,
            agent_id    TEXT NOT NULL,
            source      TEXT NOT NULL,
            content     TEXT NOT NULL,
            tokens      INTEGER NOT NULL,
            position    INTEGER NOT NULL,
            embedding   BLOB NOT NULL,
            embed_model TEXT NOT NULL,
            created_at  TEXT NOT NULL
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_kchunks_agent ON agent_knowledge_chunks(agent_id, position)",
        [],
    );
    // v2.0.0 Wave 4 — fire-mode for context hooks.
    // 'always'      = current behavior, hook fires every turn
    // 'keyword'     = fire only when user_prompt matches one of the
    //                 keywords stored in config_json.whenKeywords[]
    // 'llm-decides' = ask config_json.classifierModel "should this hook
    //                 fire?" given config_json.whenDescription
    let _ = conn.execute(
        "ALTER TABLE agent_hooks ADD COLUMN fire_mode TEXT NOT NULL DEFAULT 'always'",
        [],
    );
    // v2.2.0 — captured cost per dispatch. execution_logs already has
    // tokens_in / tokens_out; we add the computed USD value alongside so
    // panels can read a single column instead of recomputing on every
    // render. replay_jobs gets token + cost columns from scratch (the
    // table is v2.1.0 and was shipped without them).
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN cost_usd_estimated REAL", []);
    let _ = conn.execute("ALTER TABLE replay_jobs ADD COLUMN input_tokens INTEGER", []);
    let _ = conn.execute("ALTER TABLE replay_jobs ADD COLUMN output_tokens INTEGER", []);
    let _ = conn.execute("ALTER TABLE replay_jobs ADD COLUMN cost_usd_estimated REAL", []);
    // v2.3.2 — Phase 2: local-mode regressions + cost recommendations.
    // The cloud computes both over agent_traces × agent_config_changes;
    // for the offline-first surface we mirror enough locally to run
    // the same algorithm without a sign-in. Two additions to
    // execution_logs (agent_slug + model) make per-agent + per-model
    // aggregation possible; a new agent_config_changes table holds
    // the ledger.
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN agent_slug TEXT", []);
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN model TEXT", []);
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_execution_logs_agent_slug ON execution_logs(agent_slug, created_at DESC)",
        [],
    );
    // v2.3.41 — session_id on execution_logs lets the History panel
    // group multi-turn conversations under one collapsible header
    // instead of scattering them. NULL for standalone (non --session)
    // dispatches; populated by dispatch::run when --session is passed.
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN session_id TEXT", []);
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_execution_logs_session_id ON execution_logs(session_id, created_at ASC)",
        [],
    );
    // v2.4.5 — tool-call telemetry for Tier 2 review. Lets the GUI
    // distinguish "this reviewer verified findings via N tool calls"
    // from "prompt-only". tool_calls_summary is a JSON array of
    // {name, args_brief, is_error} so the Runs panel can render a
    // chronological list without re-parsing the response text.
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN tool_calls_count INTEGER", []);
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN tool_calls_summary TEXT", []);
    // 2026-05-14 — record which auth path the dispatch used so the
    // credit-burn meter can split "subscription" cost (counts against
    // Anthropic's Agent SDK credit pool starting June 15) from
    // "api_key" cost (billed directly to the user's API account).
    // Pre-migration rows have NULL; the analytics query treats NULL
    // as "unknown" rather than guessing.
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN auth_mode TEXT", []);
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_config_changes (
            id          TEXT PRIMARY KEY,
            agent_slug  TEXT NOT NULL,
            field       TEXT NOT NULL,
            old_value   TEXT,
            new_value   TEXT,
            actor       TEXT,
            changed_at  TEXT NOT NULL
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_config_changes_slug_time ON agent_config_changes(agent_slug, changed_at DESC)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_config_changes_field ON agent_config_changes(field, changed_at DESC)",
        [],
    );

    // v2.3.0 — live_runs SQLite mirror of the in-memory active_runs
    // registry. The registry stays authoritative; this mirror exists
    // so the `ato` CLI (a separate process) can read what's currently
    // running without IPC. Rows are best-effort INSERT'd by
    // active_runs::begin_run and DELETE'd by finish_run; if the writes
    // fail (DB locked, etc) the in-memory map is unaffected.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS live_runs (
            run_id      TEXT PRIMARY KEY,
            agent_slug  TEXT,
            runtime     TEXT NOT NULL,
            workspace   TEXT,
            source      TEXT,
            started_at  TEXT NOT NULL,
            status      TEXT NOT NULL DEFAULT 'running',
            child_pid   INTEGER
        )",
        [],
    );
    // Backfill for installs that already created live_runs before the
    // child_pid column existed.
    let _ = conn.execute("ALTER TABLE live_runs ADD COLUMN child_pid INTEGER", []);
    // v2.6 PR-A — observatory columns mirrored on live_runs so the chip
    // in the Live tab can render for active dispatches without a join.
    // Passive rows synthesized from the watcher are NOT written into
    // live_runs (they aren't kill-able processes); they only land in
    // execution_logs. Defaults make existing rows behave as before.
    let _ = conn.execute(
        "ALTER TABLE live_runs ADD COLUMN dispatch_kind TEXT NOT NULL DEFAULT 'active'",
        [],
    );
    let _ = conn.execute("ALTER TABLE live_runs ADD COLUMN billing_surface TEXT", []);
    // Clear stale rows from a previous desktop run. We're booting; if
    // any live_runs survived a prior crash, they're dead by definition.
    let _ = conn.execute("DELETE FROM live_runs", []);

    // v2.6 PR-A — observatory columns on execution_logs so passive
    // observations of foreign CLI sessions (claude code, codex, …) can
    // be persisted alongside ATO's own dispatches.
    //   dispatch_kind: 'active'  = ATO fired it
    //                  'passive_observation' = watcher saw it happen
    //   billing_surface: which auth path the upstream CLI used —
    //     claude_code_subscription / anthropic_api / codex_cli_subscription
    //     / openai_api / gemini_cli_subscription / gemini_api / ollama_local
    //     / unknown
    //   provider_session_id: upstream CLI's own session UUID; pairs
    //     with sequence_within_session for INSERT OR IGNORE dedup.
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN dispatch_kind TEXT NOT NULL DEFAULT 'active'",
        [],
    );
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN billing_surface TEXT", []);
    let _ = conn.execute("ALTER TABLE execution_logs ADD COLUMN provider_session_id TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN sequence_within_session INTEGER",
        [],
    );
    // Dedup unique index — partial so non-watcher rows (NULL session id)
    // don't conflict with each other.
    let _ = conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_execution_logs_session_seq \
            ON execution_logs(provider_session_id, sequence_within_session) \
            WHERE provider_session_id IS NOT NULL",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_execution_logs_dispatch_kind \
            ON execution_logs(dispatch_kind, created_at DESC)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_execution_logs_billing_surface \
            ON execution_logs(billing_surface, created_at DESC)",
        [],
    );

    // 2026-05-19 execution_logs war-room synthesis (docs/reviews/
    // execution-logs-war-room-2026-05-19.md): when v2.6 PR-A enables
    // passive-observation writes, legacy read paths must default-filter
    // to active rows. This view is the type-level handle for "real
    // dispatches only" — point new code at it and the passive vs
    // active distinction can never silently leak into Analytics /
    // Sessions feed / Regressions / Replay.
    //
    // Keep `execution_logs` as the source of truth (cheaper than
    // splitting tables); use this view for default reads, query
    // execution_logs directly when you specifically want both kinds
    // (`compute_billing_surface_summary` is the canonical example).
    let _ = conn.execute(
        "CREATE VIEW IF NOT EXISTS active_dispatches AS \
            SELECT * FROM execution_logs WHERE dispatch_kind = 'active'",
        [],
    );

    // v2.6 PR-A — watcher_state. One row per (source, file_path).
    // byte_offset = where the next read should start so re-ingest is
    // idempotent across desktop restarts. last_seq = the largest
    // sequence_within_session emitted from this file, so a hard crash
    // mid-line never re-emits the prior turn.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS watcher_state (
            source       TEXT NOT NULL,
            file_path    TEXT NOT NULL,
            byte_offset  INTEGER NOT NULL DEFAULT 0,
            last_seq     INTEGER NOT NULL DEFAULT 0,
            updated_at   TEXT NOT NULL,
            PRIMARY KEY (source, file_path)
        )",
        [],
    );

    // v2.3.7 Phase 4 — Ops recipes (user-authored trigger→action
    // workflows). trigger_config / action_config are TEXT (JSON
    // serialization of the typed enums in recipes.rs). Indexed by
    // trigger_type so the execution engine's dispatch path is O(log n)
    // when an event fires.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS ops_recipes (
            id              TEXT PRIMARY KEY,
            slug            TEXT NOT NULL UNIQUE,
            name            TEXT NOT NULL,
            description     TEXT,
            trigger_type    TEXT NOT NULL,
            trigger_config  TEXT NOT NULL,
            action_type     TEXT NOT NULL,
            action_config   TEXT NOT NULL,
            enabled         INTEGER NOT NULL DEFAULT 1,
            created_at      TEXT NOT NULL,
            updated_at      TEXT NOT NULL
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_ops_recipes_trigger ON ops_recipes(trigger_type, enabled)",
        [],
    );

    // v2.3.8 Phase 4.2 — Event audit log. Every event published on
    // events::bus is persisted here. Powers `ato events recent` and
    // gives the execution engine a deterministic re-read path when a
    // subscriber lagged (RecvError::Lagged). event_seq mirrors the
    // monotonic counter from events::bus::next_seq.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS events_log (
            event_seq   INTEGER PRIMARY KEY,
            event_type  TEXT NOT NULL,
            payload     TEXT NOT NULL,
            occurred_at TEXT NOT NULL
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_events_log_type_time ON events_log(event_type, occurred_at DESC)",
        [],
    );
    // v2.3.15 Phase 4.9 — composite index for `ato events watch
    // --type X` (codex 4.8 nit). The "type, occurred_at DESC" index
    // doesn't support the watch query shape (WHERE event_type = ? AND
    // event_seq > ? ORDER BY event_seq ASC) without an extra sort
    // step on large ledgers. (event_type, event_seq) does.
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_events_log_type_seq ON events_log(event_type, event_seq)",
        [],
    );

    // v2.3.8 Phase 4.2 — Recipe execution audit. Every action the
    // engine runs leaves a row here so users can see "what did my
    // recipes actually do, when, did they succeed?" The trigger payload
    // is captured so re-runs are reproducible if we ever build a
    // "replay this recipe run" tool.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS ops_recipe_runs (
            id              TEXT PRIMARY KEY,
            recipe_id       TEXT NOT NULL,
            recipe_slug     TEXT NOT NULL,
            event_seq       INTEGER NOT NULL,
            event_type      TEXT NOT NULL,
            event_payload   TEXT NOT NULL,
            action_type     TEXT NOT NULL,
            status          TEXT NOT NULL,
            result          TEXT,
            error_message   TEXT,
            started_at      TEXT NOT NULL,
            finished_at     TEXT
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_ops_recipe_runs_slug_time ON ops_recipe_runs(recipe_slug, started_at DESC)",
        [],
    );
    // v2.3.19 Phase 5.4 — RequestApproval support. recipe_runs with
    // a RequestApproval action park in status='awaiting_approval'
    // and store the ApprovalRequest post id; the resume watcher
    // updates `decision` + `decision_post_id` when an
    // ApprovalDecision post lands. Best-effort ALTER TABLE since
    // ADD COLUMN fails if the column already exists.
    let _ = conn.execute(
        "ALTER TABLE ops_recipe_runs ADD COLUMN awaiting_approval_request_post_id TEXT",
        [],
    );
    let _ = conn.execute("ALTER TABLE ops_recipe_runs ADD COLUMN decision TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE ops_recipe_runs ADD COLUMN decision_post_id TEXT",
        [],
    );
    // Indexed because the resume watcher scans by status every 5s.
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_ops_recipe_runs_status ON ops_recipe_runs(status)",
        [],
    );

    // v2.3.16 Phase 5.1 — Activity feed. A single chronological
    // stream where humans, agents, and the system post. NotifyHuman
    // recipe action writes here; users post via `ato posts add` or
    // the GUI; the system can auto-post when events fire.
    //
    // payload is optional structured JSON for approval kinds and
    // expanded agent responses. related_event_seq lets the GUI link
    // an event_notice post back to its events_log row.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS activity_posts (
            id                TEXT PRIMARY KEY,
            created_at        TEXT NOT NULL,
            author_kind       TEXT NOT NULL,
            author_slug       TEXT,
            kind              TEXT NOT NULL,
            text              TEXT NOT NULL,
            related_event_seq INTEGER,
            payload           TEXT
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_activity_posts_created_at ON activity_posts(created_at DESC)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_activity_posts_kind_created ON activity_posts(kind, created_at DESC)",
        [],
    );
    // v2.3.31 Phase 6 Slice A — sticky multi-turn sessions per runtime.
    // ATO assigns its own session id; the dispatch path passes it
    // through to the runtime CLI via --resume (claude) / similar.
    // runtime_session_id is the runtime's NATIVE token (captured from
    // claude's --output-format json metadata on the first dispatch);
    // the ATO id is a stable handle users + agents can refer to.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS sessions (
            id                  TEXT PRIMARY KEY,
            runtime             TEXT NOT NULL,
            agent_slug          TEXT,
            runtime_session_id  TEXT,
            title               TEXT,
            created_at          TEXT NOT NULL,
            last_used_at        TEXT NOT NULL,
            turn_count          INTEGER NOT NULL DEFAULT 0
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_runtime_lastused
            ON sessions(runtime, last_used_at DESC)",
        [],
    );
    // v2.6 Phase 6 Slice C — explicit session lifecycle (open → closed →
    // reopened). On close, the session's coordinator (the agent at
    // sessions.agent_slug, falling back to the anchor runtime) generates
    // a title, summary, topic tags, and an inferred project_id. Reopen
    // flips status back; the next close overwrites the summary with the
    // refreshed transcript. ALTER TABLE on each column individually so
    // older DBs upgrade in place; "duplicate column" errors are expected
    // on a fresh install where the columns already exist and are ignored.
    // Status is constrained to {'open', 'closed'} at the DB level so a
    // future write of a stray string (typo, branch like 'archived')
    // fails loudly rather than corrupting the invariant the UI relies
    // on. SQLite supports column-level CHECK on ADD COLUMN since 3.37.
    // Already-installed dev builds that added this column without the
    // CHECK silently fail the ALTER (duplicate column) and rely on the
    // application-layer enforcement in sessions.rs close/reopen.
    let _ = conn.execute(
        "ALTER TABLE sessions ADD COLUMN status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'closed'))",
        [],
    );
    let _ = conn.execute("ALTER TABLE sessions ADD COLUMN closed_at TEXT", []);
    let _ = conn.execute("ALTER TABLE sessions ADD COLUMN summary TEXT", []);
    let _ = conn.execute("ALTER TABLE sessions ADD COLUMN auto_title TEXT", []);
    let _ = conn.execute("ALTER TABLE sessions ADD COLUMN tags_json TEXT", []);
    let _ = conn.execute("ALTER TABLE sessions ADD COLUMN project_id TEXT", []);
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_project
            ON sessions(project_id, last_used_at DESC)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_status_lastused
            ON sessions(status, last_used_at DESC)",
        [],
    );
    // v2.7.3 — Sessions UX polish PR 2. Adds the closure-time taxonomy
    // fields the coordinator populates at `ato sessions close`. NULL
    // allowed on both so back-fill / older rows / forced-close paths
    // don't break. `category` is gated by CHECK to a controlled
    // vocabulary so UI filters can rely on it; `team` is free-form
    // because the multi-tenant story isn't locked yet (single-user
    // installs use it as "owner-band" labels — frontend / backend /
    // design / ops / etc., per Will's screenshot complaint that "all
    // sessions look the same"). Both columns surface in the UI in PR 3
    // and become required-at-close (warn, not hard fail) in PR 3 also.
    let _ = conn.execute(
        "ALTER TABLE sessions ADD COLUMN category TEXT CHECK (category IS NULL OR category IN \
         ('Business','Marketing','Dev','Frontend','Backend','Design','Security','Compliance','Ops','Other'))",
        [],
    );
    let _ = conn.execute("ALTER TABLE sessions ADD COLUMN team TEXT", []);
    // v2.7.12 — free-form human note attached at close time. Surfaced
    // in the closed-session summary card so the human's framing of the
    // conversation lives alongside the coordinator's auto-generated
    // summary. NULL when the user closed without adding a comment, or
    // when the close happened before this column existed.
    let _ = conn.execute("ALTER TABLE sessions ADD COLUMN human_comment TEXT", []);

    // v2.7.13 — war rooms become first-class closeable conversations.
    // Today they exist as a `war_room_id` grouping on `execution_logs`
    // (each LLM seat = one execution_logs row sharing the same id).
    // The `war_rooms` table is the lifecycle anchor: a row appears the
    // first time `ato war-rooms close <id>` runs (or `reopen` flips it
    // back to open). Sessions parity — same shape, same close-time
    // coordinator fields, same human_comment. No NOT NULL constraints
    // on the per-close fields so a partially-closed war room (close
    // failed mid-coordinator) doesn't poison the row.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS war_rooms (
            id                  TEXT PRIMARY KEY,
            status              TEXT NOT NULL DEFAULT 'open'
                                CHECK (status IN ('open', 'closed')),
            closed_at           TEXT,
            auto_title          TEXT,
            summary             TEXT,
            tags_json           TEXT,
            category            TEXT
                                CHECK (category IS NULL OR category IN
                                  ('Business','Marketing','Dev','Frontend','Backend',
                                   'Design','Security','Compliance','Ops','Other')),
            team                TEXT,
            project_id          TEXT,
            coordinator_runtime TEXT,
            coordinator_model   TEXT,
            human_comment       TEXT,
            duration_ms         INTEGER,
            created_at          TEXT NOT NULL,
            updated_at          TEXT
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_war_rooms_status_updated
            ON war_rooms(status, updated_at DESC)",
        [],
    );

    // v2.7.13 — chat lifecycle parity. `chat_threads` already had an
    // `archived INTEGER` flag that the UI hides closed-like threads
    // with; the new columns add real coordinator-generated summary +
    // sticky taxonomy + human_comment matching sessions/war-rooms.
    // archived stays untouched (UI hides; not a coordinator-driven
    // close). status defaults to 'open' so existing rows behave as
    // they did pre-migration.
    let _ = conn.execute(
        "ALTER TABLE chat_threads ADD COLUMN status TEXT NOT NULL DEFAULT 'open' \
         CHECK (status IN ('open', 'closed'))",
        [],
    );
    let _ = conn.execute("ALTER TABLE chat_threads ADD COLUMN closed_at TEXT", []);
    let _ = conn.execute("ALTER TABLE chat_threads ADD COLUMN auto_title TEXT", []);
    let _ = conn.execute("ALTER TABLE chat_threads ADD COLUMN summary TEXT", []);
    let _ = conn.execute("ALTER TABLE chat_threads ADD COLUMN tags_json TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE chat_threads ADD COLUMN category TEXT CHECK (category IS NULL OR category IN \
         ('Business','Marketing','Dev','Frontend','Backend','Design','Security','Compliance','Ops','Other'))",
        [],
    );
    let _ = conn.execute("ALTER TABLE chat_threads ADD COLUMN team TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE chat_threads ADD COLUMN coordinator_runtime TEXT",
        [],
    );
    let _ = conn.execute("ALTER TABLE chat_threads ADD COLUMN coordinator_model TEXT", []);
    let _ = conn.execute("ALTER TABLE chat_threads ADD COLUMN human_comment TEXT", []);
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_chat_threads_status_updated
            ON chat_threads(status, last_message_at DESC)",
        [],
    );

    // v2.7.14 (v2.8.0 ROADMAP item) — anchor_runtime: the runtime this
    // thread is "with." Chat threads today derive a runtime from the
    // most recent assistant message; that drifts when a thread hops
    // runtimes (e.g. a chat that started with claude but had two
    // gemini turns thrown in). The anchor is a STABLE identifier the
    // WhatsApp-style row UI can render an LLM icon for without flicker.
    //
    // Population strategy:
    //   - New chats: caller passes anchor_runtime at create time (e.g.
    //     "the agent attached to this thread runs on claude").
    //   - Existing chats: backfill below picks the first assistant
    //     turn's runtime (chronological "what was this chat originally
    //     with"). NULL when no assistant turn exists yet (a thread
    //     created but never replied-to).
    let _ = conn.execute("ALTER TABLE chat_threads ADD COLUMN anchor_runtime TEXT", []);
    // One-shot backfill: assign anchor_runtime to the runtime of the
    // FIRST assistant message in each thread that doesn't already have
    // one. Cheap O(N) — a UPDATE with a correlated subquery; SQLite
    // is happy doing this once per migration. The `WHERE … IS NULL`
    // guard makes the backfill idempotent across re-runs (won't
    // overwrite a value the caller already set).
    let _ = conn.execute(
        "UPDATE chat_threads
            SET anchor_runtime = (
                SELECT runtime
                  FROM chat_messages
                 WHERE thread_id = chat_threads.id
                   AND role = 'assistant'
                   AND runtime IS NOT NULL
                 ORDER BY created_at ASC
                 LIMIT 1
            )
          WHERE anchor_runtime IS NULL",
        [],
    );

    // v2.7.14 master_key_v2 PR-1 — additive foundation.
    //
    // 1. ALTER llm_api_keys: every ciphertext row declares which
    //    key_version encrypted it. Default 'v1' because pre-this-PR
    //    every row implicitly used v1 (the only key that existed).
    //    The `v1:` PREFIX on encrypted_key is the on-the-wire format
    //    tag; this column is the LEDGER tag — they should agree but
    //    are tracked separately so PR-4's atomic rekey can flip one
    //    row at a time without splitting the wire format.
    let _ = conn.execute(
        "ALTER TABLE llm_api_keys ADD COLUMN key_version TEXT NOT NULL DEFAULT 'v1'",
        [],
    );
    // 2. Seed the ledger with a v1 row if no row exists yet. IDEMPOTENT:
    //    INSERT OR IGNORE skips when the row's already there. The
    //    identity_probe stays NULL — PR-2 populates it once the
    //    per-OS probe computation lands. notes carries provenance so
    //    a future operator can tell "backfilled by the PR-1
    //    migration" from "written explicitly by a later code path."
    //
    //    DELIBERATELY does not check the keychain or derive any
    //    identity here — PR-1 is supposed to be additive only.
    //    Writing a backfill row that says "v1 exists; nothing more
    //    known about it" is the WHOLE behavior change of this PR.
    let now = chrono::Utc::now().to_rfc3339();
    let _ = conn.execute(
        "INSERT OR IGNORE INTO master_key_ledger (
            version, keychain_account, ciphertext_format, identity_probe,
            source, created_at, retired_at, notes
         ) VALUES (?1, ?2, ?3, NULL, 'keychain', ?4, NULL, ?5)",
        rusqlite::params![
            "v1",
            "master_key_v1",
            "aes-gcm-v1",
            now,
            "backfilled by schema.rs at app startup (master_key_v2 PR-1)"
        ],
    );

    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_category_lastused
            ON sessions(category, last_used_at DESC)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_team_lastused
            ON sessions(team, last_used_at DESC)",
        [],
    );
    // PR 14 (Sessions UX polish, 2026-05-18) — war-room cohesion.
    // Today's R1-parallel war-room methodology fires N standalone
    // dispatches (no --session) so they don't collide on
    // session_turns' PRIMARY KEY (session_id, turn_index). The
    // tradeoff was visual: N separate single-run cards instead of
    // one war-room. Fix: a shared `war_room_id` UUID tags
    // execution_logs rows that belong to the same parallel round.
    // The Sessions feed groups by it into a synthetic "war-room"
    // row that aggregates participating runtimes + personas +
    // cost. NULL on everything pre-PR-14 (no migration needed for
    // back-compat). Indexed for the group-by query.
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN war_room_id TEXT",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_execution_logs_war_room_id
            ON execution_logs(war_room_id, created_at DESC)
          WHERE war_room_id IS NOT NULL",
        [],
    );
    // PR 16 (2026-05-18) — war-rooms evolve from single-turn to
    // multi-turn. The PR 14 model was "single round = one user
    // prompt fans out to N seats, each replies independently."
    // That's now just round 1 of an arbitrarily-long war-room.
    //
    // The rules still hold within each round: seats fire in
    // parallel, none sees the others' replies before all return.
    // Between rounds, every seat (including their own prior reply)
    // sees the FULL transcript of all prior rounds. Round N's
    // user prompt is the user adding a new question with all R1..
    // R(N-1) replies as context.
    //
    // war_room_round is 1-indexed. NULL only on non-war-room rows.
    // Backfill rule: pre-PR-16 war-room rows had no round column —
    // they all become round 1 (the only round they had).
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN war_room_round INTEGER",
        [],
    );
    let _ = conn.execute(
        "UPDATE execution_logs SET war_room_round = 1
          WHERE war_room_id IS NOT NULL AND war_room_round IS NULL",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_execution_logs_war_room_round
            ON execution_logs(war_room_id, war_room_round, created_at ASC)
          WHERE war_room_id IS NOT NULL",
        [],
    );
    // v2.3.32 Phase 6 Slice A.2 — unified turn history. Stateful
    // runtimes (claude --resume) and stateless API providers
    // (minimax etc.) both dual-write into this table on every
    // dispatch in a session, so:
    //   - History-replay providers can rebuild the messages array
    //   - Slice B (cross-runtime mid-session switching) sees a
    //     unified log to feed into whichever runtime takes the
    //     next turn
    // turn_index is monotonic per session (max+1 on insert).
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS session_turns (
            session_id  TEXT NOT NULL,
            turn_index  INTEGER NOT NULL,
            role        TEXT NOT NULL,
            text        TEXT NOT NULL,
            runtime     TEXT NOT NULL,
            created_at  TEXT NOT NULL,
            PRIMARY KEY (session_id, turn_index)
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_session_turns_session
            ON session_turns(session_id, turn_index ASC)",
        [],
    );

    // v2.3.27 Phase 6.x — Runtime quota visibility. Stores parsed
    // "rate limit until X" timestamps surfaced from dispatch errors.
    // One row per runtime; UPSERT on new captures. The dispatch
    // pre-flight reads this to short-circuit "try again at <ts>"
    // without burning another quota probe.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS runtime_quotas (
            runtime     TEXT PRIMARY KEY,
            resets_at   TEXT NOT NULL,
            source      TEXT NOT NULL,
            captured_at TEXT NOT NULL
        )",
        [],
    );

    // v2.3.32 Phase 6.x-J — SSH-backed remote runtimes. Each row is a
    // user-registered remote that `ato dispatch <slug> "..."` should
    // route to via `ssh -i <key> -p <port> user@host '<binary> <args>'`
    // instead of spawning a local CLI. Triggered by @iamknownasfesal's
    // X question about laptop ↔ server Claude bridging. One-way only:
    // the laptop initiates; the remote runs the binary; stdout comes
    // back into execution_logs like any other dispatch. The Phase 7+
    // bi-directional mesh (daemons discovering each other) is roadmap
    // but out of scope here.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS remote_runtimes (
            slug          TEXT PRIMARY KEY,
            host          TEXT NOT NULL,
            port          INTEGER NOT NULL DEFAULT 22,
            ssh_user      TEXT,
            key_path      TEXT,
            runtime       TEXT NOT NULL,
            binary_path   TEXT NOT NULL,
            extra_args    TEXT,
            created_at    TEXT NOT NULL
        )",
        [],
    );

    // v2.4.0 Phase 7.0 — Bi-directional mesh: peer registry +
    // pending invites. Each peer has an Ed25519 public key; messages
    // (post_completion) are signed by the sender and verified before
    // the recipient writes them into session_turns / events_log.
    //
    // mesh_invites are short-lived (5 min) single-use codes used for
    // the initial pairing handshake when mDNS doesn't discover the
    // peer (typical for VLAN-isolated setups). After consumption,
    // the row stays around with `consumed=1` for auditability.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS mesh_peers (
            peer_id      TEXT PRIMARY KEY,
            public_key   TEXT NOT NULL,
            name         TEXT NOT NULL,
            paired_at    TEXT NOT NULL,
            last_seen_at TEXT,
            notes        TEXT
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS mesh_invites (
            code         TEXT PRIMARY KEY,
            issued_at    TEXT NOT NULL,
            expires_at   TEXT NOT NULL,
            consumed     INTEGER NOT NULL DEFAULT 0
        )",
        [],
    );
    // session_turns.sender_peer_id distinguishes a turn that landed
    // via the mesh (sender_peer_id matches a mesh_peers row) from a
    // locally-dispatched turn (NULL). The History panel + transcripts
    // render a peer badge when set.
    let _ = conn.execute(
        "ALTER TABLE session_turns ADD COLUMN sender_peer_id TEXT",
        [],
    );

    // 2026-05-16 — session_turns.agent_slug carries the persona/seat
    // slug when a turn was dispatched with `--agent <slug>`. NULL means
    // a generalist turn (no persona overlay). Mirror of the column
    // already on execution_logs; the duplication is intentional so the
    // SessionsList + chat-detail views can render persona badges and
    // role labels without joining across tables per row.
    let _ = conn.execute(
        "ALTER TABLE session_turns ADD COLUMN agent_slug TEXT",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_session_turns_agent_slug ON session_turns(agent_slug)",
        [],
    );

    // 2026-05-17 — SQL views over the audit-trail tables. Common joins
    // (session_turns ↔ execution_logs, per-session cost summary,
    // per-(agent,runtime) rollup) used to be re-implemented at every
    // call site. The views centralize them.
    //
    // SOURCE OF TRUTH: `packages/ato-db-views`. CLI also applies these
    // on `open_readwrite` so power users running CLI-only never see a
    // missing view. Each statement uses `CREATE VIEW IF NOT EXISTS` so
    // re-applies are no-ops.
    for stmt in ato_db_views::ALL_VIEWS {
        let _ = conn.execute(stmt, []);
    }

    // v2.4.1 Phase 7.0 step 2 — mDNS-discovered peers (transient).
    // Separate from mesh_peers (which holds *trusted* peers post-
    // pairing). Discoveries are upserted by peer_id as the daemon's
    // mDNS browser sees them; rows older than ~5 min get pruned so
    // a stale discovery doesn't survive a peer going offline.
    //
    // Discovery DOES NOT imply trust — `ato mesh discovered` shows
    // "what's on the network"; promoting a row to mesh_peers
    // happens via the pairing handshake in step 4.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS mesh_discovered (
            peer_id      TEXT PRIMARY KEY,
            name         TEXT NOT NULL,
            version      TEXT,
            addr         TEXT NOT NULL,
            last_seen_at TEXT NOT NULL
        )",
        [],
    );

    // v2.3.39 Phase 6.x-K — Eval-score ratchet.
    //
    // Inspired by Garry Tan's "AI Agent Complexity Ratchet" (2026-05).
    // Locks a quality floor per target (agent / runtime / global).
    // `ato ratchet check` compares the floor against the current
    // success-rate window and exits non-zero if breached — designed
    // to drop into CI / pre-deploy hooks so a config change that
    // regresses an agent's quality fails the build.
    //
    // Metric for v1 is `success_rate` (0.0–1.0), computed from
    // execution_logs.status. Cloud eval_score can layer on later
    // as a second metric without schema migration: add a `metric`
    // discriminator column and the same table holds both.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS eval_ratchets (
            target_kind          TEXT NOT NULL,
            target_value         TEXT NOT NULL,
            metric               TEXT NOT NULL DEFAULT 'success_rate',
            baseline_value       REAL NOT NULL,
            baseline_window_days INTEGER NOT NULL,
            threshold            REAL NOT NULL DEFAULT 0.05,
            locked_at            TEXT NOT NULL,
            locked_by            TEXT,
            notes                TEXT,
            PRIMARY KEY (target_kind, target_value, metric)
        )",
        [],
    );

    // v2.3.18 Phase 5.3 — partial UNIQUE index enforcing
    // one-ApprovalDecision-per-ApprovalRequest at the storage layer.
    // Codex 5.3 round-1 caught that the CLI's check-then-insert was
    // a race window; concurrent approve/deny would both succeed. The
    // SQL UNIQUE constraint serializes writers without needing a
    // transaction-level lock in app code.
    //
    // Codex round-2 caught that `let _ = conn.execute(...)` would
    // silently swallow the creation failure on DBs that already
    // have duplicates from the pre-fix race. We surface it here
    // so the user (or a future migration) can clean up before
    // relying on the protection.
    if let Err(e) = conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_activity_posts_decision_request
            ON activity_posts(json_extract(payload, '$.request_post_id'))
          WHERE kind = 'approval_decision'",
        [],
    ) {
        eprintln!(
            "WARN: failed to create unique approval-decision index: {} \
             (likely a pre-existing duplicate from a v2.3.17-or-earlier race). \
             Run `sqlite3 ~/.ato/local.db` and inspect duplicate \
             json_extract(payload,'$.request_post_id') values, then retry.",
            e
        );
    }
    // Strategy PR-B (2026-05-21) — conversion telemetry. Every
    // useFeatureFlag() invocation gets aggregated in the renderer and
    // flushed here every 60s (1 row per (session_id, feature, flush)).
    // tier_at_event + trial_cohort are snapshotted at write time, never
    // joined later — a user's tier can change mid-session and we need the
    // pricing question "did this user touch <feature> while on Free"
    // answered correctly forever. session_id is a boot-time UUID, NOT a
    // FK to sessions.id — keeps the behavioral profile join-blocked per
    // 2026-05-21 architecture war-room (CSO seat). Rows are LOCAL ONLY
    // in this PR; cloud-forwarding would need a separate opt-in surface.
    // Surfaces creation failures instead of `let _ =` — matches the
    // activity_posts precedent four blocks up (code-review war-room
    // 2026-05-22, claude #6). For a fresh CREATE TABLE IF NOT EXISTS
    // on a writable DB this is effectively dead code, but if a future
    // migration ever tightens the column shape the WARN avoids silent
    // truncation.
    if let Err(e) = conn.execute(
        "CREATE TABLE IF NOT EXISTS conversion_events (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id      TEXT NOT NULL,
            feature         TEXT NOT NULL,
            tier_at_event   TEXT NOT NULL,
            trial_cohort    TEXT,
            count           INTEGER NOT NULL DEFAULT 1,
            first_seen_at   TEXT NOT NULL,
            last_seen_at    TEXT NOT NULL,
            flushed_at      TEXT NOT NULL
        )",
        [],
    ) {
        eprintln!("WARN: failed to create conversion_events table: {}", e);
    }
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_conversion_events_feature
            ON conversion_events(feature, flushed_at DESC)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_conversion_events_session
            ON conversion_events(session_id, flushed_at DESC)",
        [],
    );

    // v2.9.0 — Grounded mode (PR-1: schema + soft-mode observability).
    //
    // Three additive columns on `agents` that together carry the
    // "every AI follows your rules" contract:
    //   * grounding_mode  — off | soft | strict (default off = no behavior
    //                       change for pre-v2.9 agents; new agents created
    //                       through the wizard land as 'soft').
    //   * mandatory_rules — JSON array of {kind, target, min_count?} rows
    //                       describing must-use-tool / must-read-path /
    //                       must-emit-marker obligations.
    //   * allowed_mode_floor — the laxer-bound dispatch can override toward.
    //                       Defaults to 'off' so existing dispatches keep
    //                       working unchanged; `ato agents serve`-deployed
    //                       agents set this to 'strict' so end users can't
    //                       relax the deployment.
    //
    // Rationale lives in docs/grounding.md (rollout, principles) and
    // /Users/beatriznigri/.claude/plans/witty-crafting-harp.md (the plan
    // that motivated this slice — informed by 4 ATO-driven test rounds).
    // The empirical justification for the `off` default (vs `soft`/`strict`)
    // is the gemini-hallucination control captured at receipts dir
    // /tmp/grounded-mode-receipts/.
    let _ = conn.execute(
        "ALTER TABLE agents ADD COLUMN grounding_mode TEXT NOT NULL DEFAULT 'off'",
        [],
    );
    let _ = conn.execute("ALTER TABLE agents ADD COLUMN mandatory_rules TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE agents ADD COLUMN allowed_mode_floor TEXT NOT NULL DEFAULT 'off'",
        [],
    );

    // v2.9.0 — Grounded mode (PR-1: per-dispatch verdict + override audit).
    //
    // Two additive columns on `execution_logs` that surface the rule
    // outcome on every receipt:
    //   * grounding_verdict   — compliant | violation | advisory |
    //                           not_enforced | NULL (NULL for rows
    //                           written before this column existed).
    //   * grounding_overrides — JSON of the per-dispatch overrides the
    //                           caller passed (mode_override,
    //                           additional_denies, additional_mandatories,
    //                           skip_mandatory with reason). Every override
    //                           appears verbatim so a third party reading
    //                           the receipt later can reconstruct exactly
    //                           which rules applied to this dispatch.
    //
    // The receipt-rendering code (`ato dispatches show`,
    // dispatches_panel.tsx) reads these columns and renders the verdict +
    // override list inline with the existing tool_calls_summary surface
    // shipped in v2.4.5.
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN grounding_verdict TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN grounding_overrides TEXT",
        [],
    );

    // v2.10.0 PR-1 — Methodology Runner foundation.
    //
    // Three additive tables built on top of v2.9's grounded-mode receipts
    // (every execution_logs row IS the atomic event a methodology composes):
    //
    //   * methodologies          — reusable test recipes (variant matrix +
    //                              rubric). One row per methodology slug.
    //   * methodology_runs       — one execution of a recipe with its full
    //                              DUAL COST ACCOUNTING ledger (customer
    //                              tokens/cost AND our provider compute/
    //                              judge/storage). The pricing transparency
    //                              the spec at docs/methodology-runner.md
    //                              promises.
    //   * methodology_run_dispatches — composition table: every
    //                              execution_logs row a methodology run
    //                              composed, with its variant cell coords
    //                              and rubric score.
    //
    // All three are additive — no behavior change to pre-v2.10 callers.
    // The spec + the n=150 empirical proof that motivated the load-
    // bearing schema choices (especially the ~25x grounded-mode token
    // multiplier that forces the cost-estimate UX) lives in
    // docs/methodology-runner.md and the Part 5 build log post.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS methodologies (
            id              TEXT PRIMARY KEY,
            slug            TEXT NOT NULL UNIQUE,
            description     TEXT,
            archetype       TEXT NOT NULL,
            variant_matrix  TEXT NOT NULL,
            rubric          TEXT NOT NULL,
            created_at      TEXT NOT NULL,
            created_by      TEXT
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_methodologies_archetype
            ON methodologies(archetype)",
        [],
    );

    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS methodology_runs (
            id                          TEXT PRIMARY KEY,
            methodology_id              TEXT NOT NULL,
            customer_user_id            TEXT,
            started_at                  TEXT NOT NULL,
            ended_at                    TEXT,
            status                      TEXT NOT NULL,
            total_dispatches_planned    INTEGER NOT NULL,
            total_dispatches_completed  INTEGER NOT NULL DEFAULT 0,

            -- Customer-side cost (their LLM invoice / pool burn)
            customer_cost_usd           REAL NOT NULL DEFAULT 0,
            customer_tokens_in          INTEGER NOT NULL DEFAULT 0,
            customer_tokens_out         INTEGER NOT NULL DEFAULT 0,
            customer_dispatches         INTEGER NOT NULL DEFAULT 0,
            customer_billing_mode       TEXT NOT NULL DEFAULT 'byok',

            -- Provider-side cost (what WE pay)
            provider_llm_cost_usd       REAL NOT NULL DEFAULT 0,
            provider_judge_cost_usd     REAL NOT NULL DEFAULT 0,
            provider_compute_seconds    REAL NOT NULL DEFAULT 0,
            provider_storage_bytes      INTEGER NOT NULL DEFAULT 0,
            provider_bandwidth_bytes    INTEGER NOT NULL DEFAULT 0,
            provider_total_cost_usd     REAL NOT NULL DEFAULT 0,

            -- Computed margin
            margin_usd                  REAL NOT NULL DEFAULT 0,

            -- Result
            verdict_json                TEXT,
            receipt_url                 TEXT
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_methodology_runs_status
            ON methodology_runs(status, started_at DESC)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_methodology_runs_customer
            ON methodology_runs(customer_user_id, started_at DESC)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_methodology_runs_methodology
            ON methodology_runs(methodology_id, started_at DESC)",
        [],
    );

    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS methodology_run_dispatches (
            methodology_run_id  TEXT NOT NULL,
            execution_log_id    TEXT NOT NULL,
            variant_cell        TEXT NOT NULL,
            score               REAL,
            PRIMARY KEY (methodology_run_id, execution_log_id)
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_methodology_run_dispatches_run
            ON methodology_run_dispatches(methodology_run_id)",
        [],
    );

    // v2.11 PR-12.0 — learning-loop foundation.
    //
    // Three additive schema deltas locked by `docs/v2.11-learning-loop.md`.
    // All non-destructive; the diagnose pipeline + variant lineage live
    // in PR-12.1+; this PR ships only the storage shape.
    //
    // 1) parent_run_id on methodology_runs — links a variant A/B run
    //    back to the baseline run it's competing against.
    // 2) agent_variant_lineage — depth tracker for the Q7 overfitting
    //    defense (warns at depth ≥3 within 14 days).
    // 3) production_signals — ingest target for the Langfuse/Helicone
    //    Mode A pipeline. The ingester itself ships in ato-cloud; OSS
    //    just persists what cloud writes so the diagnose pipeline can
    //    read it.
    let _ = conn.execute(
        "ALTER TABLE methodology_runs ADD COLUMN parent_run_id TEXT",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_methodology_runs_parent
            ON methodology_runs(parent_run_id)",
        [],
    );

    // v2.11 PR-12.1 (code-review finding #2, 2026-05-25): track diagnose
    // dispatch cost on the run row so customers asking "what did this
    // methodology cost me, with diagnose included?" get the right
    // answer. Free runs that never trigger diagnose stay at 0.
    let _ = conn.execute(
        "ALTER TABLE methodology_runs ADD COLUMN provider_diagnose_cost_usd REAL NOT NULL DEFAULT 0",
        [],
    );

    // v2.11 PR-12.4 — methodology.agent_slug. When set, the diagnose
    // pipeline reads the real agent definition file (resolving the
    // path via runtime-specific conventions) instead of the synthetic
    // stand-in. The --apply CLI refuses to write a variant on
    // methodologies without an agent_slug binding (code-review
    // finding #5 from PR-12.1).
    let _ = conn.execute(
        "ALTER TABLE methodologies ADD COLUMN agent_slug TEXT",
        [],
    );

    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_variant_lineage (
            variant_slug    TEXT PRIMARY KEY,
            parent_slug     TEXT NOT NULL,
            generation      INTEGER NOT NULL DEFAULT 1,
            created_at      TEXT NOT NULL,
            birthed_by_run  TEXT NOT NULL,
            diagnose_model  TEXT
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_variant_lineage_parent
            ON agent_variant_lineage(parent_slug, generation)",
        [],
    );

    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS production_signals (
            id           TEXT PRIMARY KEY,
            agent_slug   TEXT NOT NULL,
            source       TEXT NOT NULL,
            signal_json  TEXT NOT NULL,
            captured_at  TEXT NOT NULL
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_production_signals_agent
            ON production_signals(agent_slug, captured_at DESC)",
        [],
    );

    // v2.11 PR-11 — workspaces foundation.
    //
    // A workspace is a local-only namespace for organizing agents +
    // methodologies + runs. Sits at the OSS / Team boundary:
    //   * Free tier: 1 implicit "personal" workspace. The primitive
    //     exists so the schema is forward-compatible.
    //   * Team tier (ato-cloud): N workspaces with multi-user
    //     membership, RBAC, and cross-device sync. That logic lives
    //     in ato-cloud; OSS just persists the data structure so it
    //     can be replicated when the user signs in.
    //
    // The `tier_hint` column records what the workspace MIGHT become
    // (`personal` vs `team`) without ATO needing to know the user's
    // actual subscription — useful for the UI to show a "Team" badge
    // when relevant.
    //
    // Note: agents / methodologies / methodology_runs do NOT yet take
    // a workspace_id FK column. That migration is deferred to a later
    // PR once the multi-workspace UX is validated — adding it now
    // would force every existing row into a default workspace before
    // we know how users want them organized.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS workspaces (
            id           TEXT PRIMARY KEY,
            slug         TEXT NOT NULL UNIQUE,
            name         TEXT NOT NULL,
            tier_hint    TEXT NOT NULL DEFAULT 'personal',
            created_at   TEXT NOT NULL,
            archived_at  TEXT
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_workspaces_tier_hint
            ON workspaces(tier_hint, archived_at)",
        [],
    );

    // Members table — populated by ato-cloud when a workspace is
    // shared with collaborators. Free / Personal workspaces never
    // have rows here; Team workspaces do (one row per teammate).
    // role: owner | admin | editor | viewer (no enforcement at the
    // SQLite layer — the cloud is the authority for RBAC).
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS workspace_members (
            workspace_id  TEXT NOT NULL,
            user_id       TEXT NOT NULL,
            role          TEXT NOT NULL DEFAULT 'viewer',
            joined_at     TEXT NOT NULL,
            PRIMARY KEY (workspace_id, user_id)
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_workspace_members_user
            ON workspace_members(user_id)",
        [],
    );

    // Seed the personal workspace if no workspace exists yet — keeps
    // a fresh install with a sensible default that doesn't require
    // the user to run a CLI command before creating their first
    // agent.
    let workspace_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM workspaces", [], |r| r.get(0))
        .unwrap_or(0);
    if workspace_count == 0 {
        let now = chrono::Utc::now().to_rfc3339();
        let _ = conn.execute(
            "INSERT INTO workspaces (id, slug, name, tier_hint, created_at)
             VALUES (?1, 'personal', 'Personal', 'personal', ?2)",
            rusqlite::params![&uuid::Uuid::new_v4().to_string(), &now],
        );
    }

    // v2.14 — Loop Composer (reframed Automations).
    //
    // The Automations tab was a generic React-Flow node editor stored
    // entirely in localStorage. v2.14 promotes loops to a first-class
    // SQLite entity so they're queryable, schedulable, and CLI-driven
    // alongside the desktop UI. Node taxonomy is LLM-aware
    // (dispatch / methodology_run / diagnose / apply / review /
    // war_room / score / input / output) plus the existing
    // service catalog entries (kept under kind='service' for
    // backwards compat with migrated localStorage workflows).
    //
    // `graph` and `variables` hold the canonical Loop blob as JSON;
    // `trigger_config` encodes cron expression / webhook path / etc.
    // `source` records whether the loop came from the UI editor,
    // a skill auto-detection, or the v2.13→v2.14 migration.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS loops (
            id              TEXT PRIMARY KEY,
            slug            TEXT NOT NULL UNIQUE,
            name            TEXT NOT NULL,
            description     TEXT,
            enabled         INTEGER NOT NULL DEFAULT 1,
            graph           TEXT NOT NULL,
            variables       TEXT,
            trigger_kind    TEXT NOT NULL DEFAULT 'manual',
            trigger_config  TEXT,
            source          TEXT NOT NULL DEFAULT 'manual',
            source_ref      TEXT,
            created_at      TEXT NOT NULL,
            updated_at      TEXT NOT NULL
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_loops_enabled_trigger
            ON loops(enabled, trigger_kind)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_loops_source
            ON loops(source, source_ref)",
        [],
    );

    // One row per loop execution. `status` is a state-machine column:
    // pending → running → (success | error | cancelled). `triggered_by`
    // records the caller — manual:<user>, schedule:<schedule_id>,
    // webhook:<path> — so we can audit + filter the runs feed.
    // `variables` is the snapshot of resolved variables AT RUN TIME so
    // a later loop edit doesn't change what an old run reports.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS loop_runs (
            id              TEXT PRIMARY KEY,
            loop_id         TEXT NOT NULL,
            status          TEXT NOT NULL DEFAULT 'pending',
            started_at      TEXT NOT NULL,
            finished_at     TEXT,
            error           TEXT,
            triggered_by    TEXT,
            variables       TEXT,
            FOREIGN KEY (loop_id) REFERENCES loops(id) ON DELETE CASCADE
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_loop_runs_loop_started
            ON loop_runs(loop_id, started_at DESC)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_loop_runs_status
            ON loop_runs(status, started_at DESC)",
        [],
    );

    // Per-step audit of each loop run. `execution_log_id` links to
    // execution_logs for dispatch/review/methodology steps so we can
    // cross-reference token cost + dispatch ID without re-storing it.
    // For pure control-flow steps (decision/parallel/retry) the link
    // is NULL. `input` and `output` capture resolved variables in/out
    // so a step's contribution to subsequent steps is debuggable.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS loop_run_steps (
            id                  TEXT PRIMARY KEY,
            loop_run_id         TEXT NOT NULL,
            node_id             TEXT NOT NULL,
            node_type           TEXT NOT NULL,
            status              TEXT NOT NULL,
            started_at          TEXT,
            finished_at         TEXT,
            input               TEXT,
            output              TEXT,
            error               TEXT,
            execution_log_id    TEXT,
            FOREIGN KEY (loop_run_id) REFERENCES loop_runs(id) ON DELETE CASCADE
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_loop_run_steps_run
            ON loop_run_steps(loop_run_id)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_loop_run_steps_exec
            ON loop_run_steps(execution_log_id)
            WHERE execution_log_id IS NOT NULL",
        [],
    );

    // Recurring schedules. `next_fire_at` is the planner's hint so we
    // can `WHERE next_fire_at <= NOW()` from the cron tick without
    // parsing every cron expression on every tick. `last_fired_at`
    // closes the loop so we never re-fire the same minute on a fast
    // reboot.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS loop_schedules (
            id              TEXT PRIMARY KEY,
            loop_id         TEXT NOT NULL,
            cron_expr       TEXT NOT NULL,
            enabled         INTEGER NOT NULL DEFAULT 1,
            last_fired_at   TEXT,
            next_fire_at    TEXT,
            created_at      TEXT NOT NULL,
            FOREIGN KEY (loop_id) REFERENCES loops(id) ON DELETE CASCADE
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_loop_schedules_next_fire
            ON loop_schedules(enabled, next_fire_at)
            WHERE enabled = 1",
        [],
    );

    // v2.14.3 — master_key_ledger canary column. NULL on pre-2.14.3 rows;
    // backfilled on first call to encryption::master_key() when a
    // valid key is fetched from the keychain. The canary lets rekey
    // assert it has the right "old key" before destructive ops.
    let _ = conn.execute("ALTER TABLE master_key_ledger ADD COLUMN canary_ciphertext TEXT", []);

    // ── v2.16 PR-1 — Missions (proactive coordinator class) ───────────────
    //
    // Mission is a goal-driven coordinator that may spawn Loops (workers)
    // to make progress over time. See docs/v2.16-missions.md for the full
    // design. War-room F16E28F0-2E9A-4260-8A2E-02F0F3CF49E7 — codex 2x +
    // gemini 1x rounds. All three architectural decisions (Q1/Q2/Q3)
    // agreed on, 5 schema refinements from gemini round-3 adopted:
    //   - cleanup_policy (worktree pruning policy)
    //   - check_command in success_criteria JSON (verifiable completion)
    //   - max_loops + token_budget_usd (bounded resource consumption)
    //   - result_metadata JSON (declarative outputs)
    //   - nullable mission_id on related rows (preserves standalone Loop)
    //
    // No-subdelegation rule: missions spawn loops; loops spawn dispatches.
    // Dispatches CANNOT spawn missions. Loops CANNOT spawn sub-loops.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS missions (
            id                  TEXT PRIMARY KEY,
            slug                TEXT NOT NULL UNIQUE,
            name                TEXT NOT NULL,
            goal                TEXT NOT NULL,
            success_criteria    TEXT NOT NULL,
            escalation_policy   TEXT,
            workspace_strategy  TEXT NOT NULL DEFAULT 'single_cwd',
            base_sha            TEXT,
            cleanup_policy      TEXT NOT NULL DEFAULT 'delete_on_success',
            merge_strategy      TEXT NOT NULL DEFAULT 'human_approves_each',
            category            TEXT NOT NULL DEFAULT 'autonomous',
            state               TEXT NOT NULL DEFAULT 'open',
            max_loops           INTEGER,
            token_budget_usd    REAL,
            result_metadata     TEXT,
            narrative_md_path   TEXT NOT NULL,
            created_at          TEXT NOT NULL,
            updated_at          TEXT NOT NULL
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_missions_state_category
            ON missions(state, category)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_missions_updated
            ON missions(updated_at DESC)",
        [],
    );

    // mission_events — append-only event log per mission. The Mission ↔
    // Loop relationship lives in payload.loop_run_id when kind='loop_run_
    // completed', NOT in a separate join table (codex round-1 "B-lite").
    // Allowed kinds: state_changed | category_changed | dispatched |
    //   loop_run_started | loop_run_completed.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS mission_events (
            id              TEXT PRIMARY KEY,
            mission_id      TEXT NOT NULL,
            kind            TEXT NOT NULL,
            payload         TEXT,
            occurred_at     TEXT NOT NULL,
            FOREIGN KEY (mission_id) REFERENCES missions(id) ON DELETE CASCADE
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_mission_events_mission_time
            ON mission_events(mission_id, occurred_at DESC)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_mission_events_kind
            ON mission_events(kind, occurred_at DESC)",
        [],
    );

    // v2.16 PR-3 — per-agent worktree lifecycle. repo_root captures the
    // absolute path to the git repo at mission creation time so the
    // worktree creation code can resolve base_sha against the right repo
    // even when the process cwd has changed. NULL on single_cwd missions.
    let _ = conn.execute("ALTER TABLE missions ADD COLUMN repo_root TEXT", []);

    // v2.16 PR-4 — coordinator tick worker config. JSON shape:
    // {"runtime": "...", "model": null|"...", "require_tools": [...]}
    // NULL means the tick will escalate with reason="no_worker_config".
    let _ = conn.execute("ALTER TABLE missions ADD COLUMN worker_config TEXT", []);

    // v2.15.1 — retry-with-backoff accounting (war_room 08F8629A
    // codex audit verdict: "one execution_logs row per dispatch,
    // plus retry_count and a compact JSON attempt summary column").
    // retry_count: number of retries that happened (0 = first
    // attempt succeeded). attempt_summary: JSON array of
    // AttemptRecord rows from ato-retry-policy. NULL on legacy
    // rows; 0 / "[]" on rows written after this migration.
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN attempt_summary TEXT",
        [],
    );

    // v2.15.2 — subscription-exhaustion audit (war_room 78617E68
    // codex audit verdict: "Do not repurpose execution_logs.attempt_
    // summary into an object. The current code and schema describe
    // it as a JSON array of retry attempts. Add a sibling JSON field
    // for exhaustion audit, or append a clearly typed terminal record
    // to the same array shape."). Sibling field chosen because it
    // keeps attempt_summary's array shape stable for existing
    // consumers. JSON shape: { runtime_attempted, reset_at,
    // policy_chosen, fallback_runtime, raw_message }.
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN exhaustion_audit TEXT",
        [],
    );

    // v2.15.5 — post-retry fallback-chain receipt (war_room CC9DBD0E).
    // fallback_of: the execution_logs.id of the failed dispatch this
    // row replaces. NULL for all non-fallback rows (i.e. the overwhelming
    // majority). The original failed row keeps status=error untouched;
    // the fallback row carries a new id and links back here so the UI
    // can show the chain of attempts.
    let _ = conn.execute(
        "ALTER TABLE execution_logs ADD COLUMN fallback_of TEXT",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_execution_logs_fallback_of \
            ON execution_logs(fallback_of) \
            WHERE fallback_of IS NOT NULL",
        [],
    );

    // v2.15.4 — pause-and-wake scheduler (war_room E063A89E).
    //
    // Codex's amendments to the initial design:
    //   - paused_dispatches is AUTHORITATIVE storage; loop_runs.paused_until
    //     is a cheap mirror for Loop UX queries.
    //   - Pause-and-wake is loops-only in v2.15.4; standalone CLI dispatches
    //     with the policy degrade to stop-and-notify with a clearer error.
    //   - Wake via one-shot OS jobs (launchd plist on macOS) registered
    //     per paused dispatch — no always-on daemon dependency. Cron.rs
    //     already has the per-OS builders to extract.
    //   - Hardcoded pause_count cap of 3 per row as a reliability guard
    //     against infinite re-pause cycles (not a user-facing setting).
    //   - At wake time, the resumer CLAIMS the row transactionally
    //     (status: paused → resuming) and RE-RUNS the quota pre-flight
    //     against runtime_quotas instead of trusting the stale reset_at.
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS paused_dispatches (
            id              TEXT PRIMARY KEY,
            runtime         TEXT NOT NULL,
            reset_at        TEXT NOT NULL,
            loop_run_id     TEXT,
            step_id         TEXT,
            prompt          TEXT NOT NULL,
            model           TEXT,
            agent_slug      TEXT,
            workspace_root  TEXT,
            pause_count     INTEGER NOT NULL DEFAULT 1,
            max_pause_count INTEGER NOT NULL DEFAULT 3,
            status          TEXT NOT NULL DEFAULT 'paused',
            paused_at       TEXT NOT NULL,
            resumed_at      TEXT,
            abandoned_at    TEXT,
            audit_json      TEXT,
            created_at      TEXT NOT NULL
        )",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_paused_dispatches_status_reset
            ON paused_dispatches(status, reset_at)",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_paused_dispatches_loop_run
            ON paused_dispatches(loop_run_id)
            WHERE loop_run_id IS NOT NULL",
        [],
    );

    // Cheap mirror columns on loop_runs so Loop Composer UI can query
    // "paused" status + sort by paused_until without joining
    // paused_dispatches. Authoritative state stays in paused_dispatches.
    let _ = conn.execute(
        "ALTER TABLE loop_runs ADD COLUMN paused_until TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE loop_runs ADD COLUMN paused_dispatch_id TEXT",
        [],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    // v2.7.14 master_key_v2 PR-1 — ledger schema + idempotent backfill.
    // Pins the contract so PR-2 can layer the probe column on without
    // accidentally breaking the backfill semantics.

    fn init_in_memory() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_database(&conn);
        conn
    }

    #[test]
    fn master_key_ledger_backfill_creates_v1_row_on_first_init() {
        let conn = init_in_memory();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM master_key_ledger WHERE version = 'v1'",
                [],
                |r| r.get(0),
            )
            .expect("ledger SELECT");
        assert_eq!(count, 1, "expected exactly one v1 ledger row after first init");

        let (keychain_account, ciphertext_format, identity_probe, source, retired_at): (
            String,
            String,
            Option<String>,
            String,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT keychain_account, ciphertext_format, identity_probe, source, retired_at
                   FROM master_key_ledger WHERE version = 'v1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .expect("v1 row fields");

        assert_eq!(keychain_account, "master_key_v1", "keychain account locked to v1");
        assert_eq!(ciphertext_format, "aes-gcm-v1", "wire format locked to v1");
        assert_eq!(identity_probe, None, "probe stays NULL until PR-2");
        assert_eq!(source, "keychain", "default source is keychain");
        assert_eq!(retired_at, None, "v1 is active, not retired");
    }

    #[test]
    fn master_key_ledger_backfill_is_idempotent_across_multiple_inits() {
        // Simulates the app restarting N times — init_database runs every
        // launch, the backfill must NOT duplicate the v1 row.
        let conn = init_in_memory();
        init_database(&conn);
        init_database(&conn);
        init_database(&conn);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM master_key_ledger", [], |r| r.get(0))
            .expect("ledger count");
        assert_eq!(count, 1, "INSERT OR IGNORE must not duplicate on re-init");
    }

    #[test]
    fn llm_api_keys_has_key_version_column_defaulting_to_v1() {
        let conn = init_in_memory();
        // Insert a fresh row WITHOUT specifying key_version — should
        // default to 'v1' per the ALTER TABLE clause.
        conn.execute(
            "INSERT INTO llm_api_keys (
                id, provider, name, key_preview, encrypted_key, project_id, runtime,
                is_active, last_used, usage_count, created_at, updated_at
            ) VALUES (
                'test-id', 'openai', 'test-key', 'sk-...test',
                'v1:fakecipher', NULL, NULL, 1, NULL, 0, '2026-05-21', '2026-05-21'
            )",
            [],
        )
        .expect("insert key");
        let kv: String = conn
            .query_row(
                "SELECT key_version FROM llm_api_keys WHERE id = 'test-id'",
                [],
                |r| r.get(0),
            )
            .expect("read key_version");
        assert_eq!(
            kv, "v1",
            "rows inserted without key_version should default to v1"
        );
    }

    // v2.9.0 PR-1 — grounding columns: shape, defaults, back-compat.
    //
    // Pins the contract that PR-2 / PR-3 / PR-4 build on. The defaults
    // matter: existing agents and execution_logs rows must keep working
    // unchanged (no NOT NULL on retroactive nullable columns; grounding
    // defaults to 'off' so pre-v2.9 callers see exactly today's behavior).
    #[test]
    fn agents_has_grounding_columns_with_back_compat_defaults() {
        let conn = init_in_memory();

        // Insert an agent the pre-v2.9 way — NO grounding fields specified.
        // The migration must have populated the defaults so the row is valid.
        conn.execute(
            "INSERT INTO agents (
                id, slug, display_name, runtime, created_at
            ) VALUES (
                'g-test-1', 'tester', 'Tester', 'claude', '2026-05-24'
            )",
            [],
        )
        .expect("insert agent without grounding fields");

        let (mode, floor, mandatory): (String, String, Option<String>) = conn
            .query_row(
                "SELECT grounding_mode, allowed_mode_floor, mandatory_rules
                 FROM agents WHERE id = 'g-test-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("read grounding fields");

        assert_eq!(
            mode, "off",
            "pre-v2.9 agents default to grounding_mode='off' so existing dispatch paths see no behavior change"
        );
        assert_eq!(
            floor, "off",
            "allowed_mode_floor defaults to 'off' — dispatch can override to anything until the author tightens"
        );
        assert_eq!(
            mandatory, None,
            "mandatory_rules is nullable — pre-v2.9 agents have no obligations"
        );
    }

    // v2.10.0 PR-1 — Methodology Runner schema.
    //
    // Pins the contract for the three new tables (methodologies +
    // methodology_runs + methodology_run_dispatches). The dual-cost-
    // accounting columns on methodology_runs are the load-bearing
    // schema choice — they're what makes Pro economics auditable per
    // customer per month. Tests below verify the columns exist with
    // back-compat-safe defaults.

    #[test]
    fn methodologies_table_exists_with_required_columns() {
        let conn = init_in_memory();
        // Insert a minimal methodology record; succeeds only if the
        // table + all NOT NULL columns landed.
        conn.execute(
            "INSERT INTO methodologies (
                id, slug, archetype, variant_matrix, rubric, created_at
            ) VALUES (
                'm-1', 'which-model-test', 'which-model',
                '{\"models\":[\"claude\",\"gemini\"]}',
                '{\"kind\":\"regex\"}',
                '2026-05-24T20:00:00Z'
            )",
            [],
        )
        .expect("insert methodology");

        let (slug, archetype): (String, String) = conn
            .query_row(
                "SELECT slug, archetype FROM methodologies WHERE id = 'm-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("read methodology");
        assert_eq!(slug, "which-model-test");
        assert_eq!(archetype, "which-model");
    }

    #[test]
    fn methodology_runs_has_dual_cost_accounting_columns_with_defaults() {
        let conn = init_in_memory();
        // Insert a minimal run row — all the dual-cost-accounting
        // columns must default cleanly so the runner can INSERT a
        // pending row and UPDATE the cost fields as dispatches land.
        conn.execute(
            "INSERT INTO methodology_runs (
                id, methodology_id, started_at, status,
                total_dispatches_planned
            ) VALUES (
                'r-1', 'm-1', '2026-05-24T20:00:00Z', 'queued', 30
            )",
            [],
        )
        .expect("insert methodology_run");

        let row: (
            f64,    // customer_cost_usd
            i64,    // customer_tokens_in
            String, // customer_billing_mode
            f64,    // provider_llm_cost_usd
            f64,    // provider_compute_seconds
            f64,    // margin_usd
        ) = conn
            .query_row(
                "SELECT customer_cost_usd, customer_tokens_in,
                        customer_billing_mode, provider_llm_cost_usd,
                        provider_compute_seconds, margin_usd
                 FROM methodology_runs WHERE id = 'r-1'",
                [],
                |r| {
                    Ok((
                        r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?,
                    ))
                },
            )
            .expect("read methodology_run defaults");

        assert_eq!(row.0, 0.0, "customer_cost_usd defaults to 0");
        assert_eq!(row.1, 0, "customer_tokens_in defaults to 0");
        assert_eq!(
            row.2, "byok",
            "customer_billing_mode defaults to byok — most common case"
        );
        assert_eq!(row.3, 0.0, "provider_llm_cost_usd defaults to 0");
        assert_eq!(row.4, 0.0, "provider_compute_seconds defaults to 0");
        assert_eq!(row.5, 0.0, "margin_usd defaults to 0");
    }

    #[test]
    fn methodology_run_dispatches_composite_pk_enforced() {
        let conn = init_in_memory();

        // Seed prerequisite rows
        conn.execute(
            "INSERT INTO methodologies (id, slug, archetype, variant_matrix, rubric, created_at)
             VALUES ('m-pk', 'pk-test', 'custom', '{}', '{}', '2026-05-24')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO methodology_runs (id, methodology_id, started_at, status, total_dispatches_planned)
             VALUES ('r-pk', 'm-pk', '2026-05-24', 'running', 1)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO execution_logs (id, runtime, status, created_at)
             VALUES ('el-pk', 'claude', 'success', '2026-05-24')",
            [],
        ).unwrap();

        // First insert succeeds
        conn.execute(
            "INSERT INTO methodology_run_dispatches (methodology_run_id, execution_log_id, variant_cell)
             VALUES ('r-pk', 'el-pk', '{\"model\":\"claude\"}')",
            [],
        ).expect("first dispatch link should succeed");

        // Duplicate insert (same composite PK) must fail
        let dup = conn.execute(
            "INSERT INTO methodology_run_dispatches (methodology_run_id, execution_log_id, variant_cell)
             VALUES ('r-pk', 'el-pk', '{\"model\":\"claude\"}')",
            [],
        );
        assert!(
            dup.is_err(),
            "duplicate (run_id, execution_log_id) must violate the composite PK — \
             a dispatch can only contribute to one cell of a single methodology run"
        );
    }

    #[test]
    fn execution_logs_has_grounding_verdict_columns_nullable() {
        let conn = init_in_memory();

        // Insert an execution_log row WITHOUT specifying grounding columns.
        // The columns are nullable on retroactive rows so the existing
        // dispatch-write path keeps working unchanged.
        conn.execute(
            "INSERT INTO execution_logs (
                id, runtime, status, created_at
            ) VALUES (
                'el-test-1', 'claude', 'success', '2026-05-24'
            )",
            [],
        )
        .expect("insert execution_log without grounding fields");

        let (verdict, overrides): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT grounding_verdict, grounding_overrides
                 FROM execution_logs WHERE id = 'el-test-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("read grounding receipt columns");

        assert_eq!(
            verdict, None,
            "rows written before grounding is wired must have NULL verdict (not_enforced is for grounding_mode='off' agents, NULL is for rows from runtimes that haven't been migrated yet)"
        );
        assert_eq!(
            overrides, None,
            "grounding_overrides is nullable — no dispatch-time override means no row"
        );
    }

    // v2.14 Loop Composer schema smoke. Pins the contract: a loop can
    // be inserted, can carry a child loop_run + loop_run_steps, and
    // ON DELETE CASCADE wipes the lineage when the parent loop goes
    // away. Catches the most common slip ("forgot the FK / index").
    #[test]
    fn loops_schema_round_trip_and_cascade() {
        let conn = init_in_memory();
        // Foreign keys are off by default in rusqlite; turn them on so
        // the cascade assertion exercises the actual FK declarations.
        conn.execute_batch("PRAGMA foreign_keys = ON;").expect("enable FK");

        let now = "2026-06-10T00:00:00Z";
        conn.execute(
            "INSERT INTO loops (
                id, slug, name, description, enabled, graph, variables,
                trigger_kind, trigger_config, source, source_ref,
                created_at, updated_at
            ) VALUES (
                'loop-1', 'weekly-security-review', 'Weekly security review',
                'methodology run → diagnose → apply', 1,
                '{\"nodes\":[],\"edges\":[]}', NULL,
                'schedule', '0 9 * * 1', 'manual', NULL,
                ?1, ?1
            )",
            [now],
        )
        .expect("insert loop");

        conn.execute(
            "INSERT INTO loop_runs (
                id, loop_id, status, started_at, triggered_by
            ) VALUES (
                'run-1', 'loop-1', 'success', ?1, 'manual:test'
            )",
            [now],
        )
        .expect("insert loop_run");

        conn.execute(
            "INSERT INTO loop_run_steps (
                id, loop_run_id, node_id, node_type, status,
                input, output
            ) VALUES (
                'step-1', 'run-1', 'n-methodology-run', 'methodology_run',
                'success', '{\"slug\":\"x\"}', '{\"run_id\":\"mr-1\"}'
            )",
            [],
        )
        .expect("insert loop_run_step");

        conn.execute(
            "INSERT INTO loop_schedules (
                id, loop_id, cron_expr, enabled, next_fire_at, created_at
            ) VALUES (
                'sched-1', 'loop-1', '0 9 * * 1', 1, ?1, ?1
            )",
            [now],
        )
        .expect("insert loop_schedule");

        // Read-back roundtrips.
        let (slug, trigger_kind): (String, String) = conn
            .query_row(
                "SELECT slug, trigger_kind FROM loops WHERE id = 'loop-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("read loop");
        assert_eq!(slug, "weekly-security-review");
        assert_eq!(trigger_kind, "schedule");

        let step_status: String = conn
            .query_row(
                "SELECT status FROM loop_run_steps WHERE id = 'step-1'",
                [],
                |r| r.get(0),
            )
            .expect("read step");
        assert_eq!(step_status, "success");

        // Cascade: deleting the parent loop must wipe runs, steps, and
        // schedules in one shot. Otherwise we leak orphan rows.
        conn.execute("DELETE FROM loops WHERE id = 'loop-1'", [])
            .expect("delete loop");

        let runs: i64 = conn
            .query_row("SELECT COUNT(*) FROM loop_runs", [], |r| r.get(0))
            .unwrap();
        let steps: i64 = conn
            .query_row("SELECT COUNT(*) FROM loop_run_steps", [], |r| r.get(0))
            .unwrap();
        let scheds: i64 = conn
            .query_row("SELECT COUNT(*) FROM loop_schedules", [], |r| r.get(0))
            .unwrap();
        assert_eq!(runs, 0, "loop_runs must cascade from loops");
        assert_eq!(steps, 0, "loop_run_steps must cascade from loop_runs");
        assert_eq!(scheds, 0, "loop_schedules must cascade from loops");
    }
}
