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
}
