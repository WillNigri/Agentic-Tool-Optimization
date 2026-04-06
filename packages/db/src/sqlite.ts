import Database, { type Statement } from 'better-sqlite3';
import { randomUUID } from 'node:crypto';
import type {
  DatabaseAdapter,
  UpsertSkillInput,
  InsertUsageInput,
  DailyUsageRow,
  SkillRow,
  McpServerRow,
  UsageSummaryRow,
  BurnRateRow,
} from './interface.js';

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

interface SqliteAdapterOptions {
  /** Busy timeout in milliseconds (default: 5000) */
  busyTimeout?: number;
  /** Cache size in KB (default: 64000 = 64MB, use negative for KB) */
  cacheSize?: number;
  /** Synchronous mode: 'OFF' | 'NORMAL' | 'FULL' (default: 'NORMAL') */
  synchronous?: 'OFF' | 'NORMAL' | 'FULL';
}

const DEFAULT_OPTIONS: Required<SqliteAdapterOptions> = {
  busyTimeout: 5000,
  cacheSize: -64000, // 64MB
  synchronous: 'NORMAL',
};

// ---------------------------------------------------------------------------
// Prepared Statement Cache
// ---------------------------------------------------------------------------

interface PreparedStatements {
  listSkills: Statement;
  upsertSkill: Statement;
  getSkillByUserPath: Statement;
  toggleSkill: Statement;
  deleteSkill: Statement;
  insertUsage: Statement;
  getUsageSummary: Statement;
  getDailyUsage: Statement;
  getBurnRate: Statement;
  listMcpServers: Statement;
  upsertMcpServer: Statement;
  getMcpServerByUserName: Statement;
  deleteMcpServer: Statement;
  getSetting: Statement;
  setSetting: Statement;
}

// ---------------------------------------------------------------------------
// SQLite Adapter Implementation
// ---------------------------------------------------------------------------

export class SqliteAdapter implements DatabaseAdapter {
  private db: Database.Database | null = null;
  private readonly dbPath: string;
  private readonly options: Required<SqliteAdapterOptions>;
  private statements: PreparedStatements | null = null;

  constructor(dbPath: string, options?: SqliteAdapterOptions) {
    this.dbPath = dbPath;
    this.options = { ...DEFAULT_OPTIONS, ...options };
  }

  async initialize(): Promise<void> {
    this.db = new Database(this.dbPath);

    // Performance PRAGMAs
    this.db.pragma(`busy_timeout = ${this.options.busyTimeout}`);
    this.db.pragma('journal_mode = WAL');
    this.db.pragma(`synchronous = ${this.options.synchronous}`);
    this.db.pragma(`cache_size = ${this.options.cacheSize}`);
    this.db.pragma('temp_store = MEMORY');
    this.db.pragma('foreign_keys = ON');

    // Create tables and indexes
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS skills (
        id TEXT PRIMARY KEY,
        user_id TEXT NOT NULL,
        name TEXT NOT NULL,
        description TEXT,
        file_path TEXT NOT NULL,
        source TEXT NOT NULL,
        content TEXT,
        token_count INTEGER NOT NULL DEFAULT 0,
        enabled INTEGER NOT NULL DEFAULT 1,
        content_hash TEXT,
        created_at TEXT NOT NULL DEFAULT (datetime('now')),
        updated_at TEXT NOT NULL DEFAULT (datetime('now'))
      );

      CREATE INDEX IF NOT EXISTS idx_skills_user_id ON skills(user_id);
      CREATE UNIQUE INDEX IF NOT EXISTS idx_skills_user_file ON skills(user_id, file_path);
      -- Partial index for enabled skills only (faster queries for active skills)
      CREATE INDEX IF NOT EXISTS idx_skills_user_enabled ON skills(user_id) WHERE enabled = 1;

      CREATE TABLE IF NOT EXISTS usage_records (
        id TEXT PRIMARY KEY,
        user_id TEXT NOT NULL,
        session_id TEXT,
        timestamp TEXT NOT NULL DEFAULT (datetime('now')),
        model TEXT NOT NULL,
        input_tokens INTEGER NOT NULL DEFAULT 0,
        output_tokens INTEGER NOT NULL DEFAULT 0,
        cache_read_tokens INTEGER NOT NULL DEFAULT 0,
        cache_write_tokens INTEGER NOT NULL DEFAULT 0,
        cost REAL NOT NULL DEFAULT 0,
        request_type TEXT
      );

      CREATE INDEX IF NOT EXISTS idx_usage_user_id ON usage_records(user_id);
      CREATE INDEX IF NOT EXISTS idx_usage_timestamp ON usage_records(timestamp);
      CREATE INDEX IF NOT EXISTS idx_usage_user_timestamp ON usage_records(user_id, timestamp);
      CREATE INDEX IF NOT EXISTS idx_usage_session ON usage_records(session_id);
      -- Covering index for daily aggregation queries
      CREATE INDEX IF NOT EXISTS idx_usage_daily_agg ON usage_records(user_id, timestamp, input_tokens, output_tokens, cost);

      CREATE TABLE IF NOT EXISTS mcp_servers (
        id TEXT PRIMARY KEY,
        user_id TEXT NOT NULL,
        name TEXT NOT NULL,
        transport TEXT NOT NULL,
        command TEXT,
        args TEXT NOT NULL DEFAULT '[]',
        url TEXT,
        tool_count INTEGER NOT NULL DEFAULT 0,
        status TEXT NOT NULL DEFAULT 'unknown',
        last_error TEXT,
        last_seen_at TEXT,
        config_source TEXT
      );

      CREATE INDEX IF NOT EXISTS idx_mcp_user_id ON mcp_servers(user_id);
      CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_user_name ON mcp_servers(user_id, name);

      CREATE TABLE IF NOT EXISTS settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL,
        updated_at TEXT NOT NULL DEFAULT (datetime('now'))
      );
    `);

    // Pre-compile frequently used statements
    this.prepareStatements();
  }

  private prepareStatements(): void {
    const db = this.getDb();

    this.statements = {
      listSkills: db.prepare(
        'SELECT * FROM skills WHERE user_id = ? ORDER BY name'
      ),
      upsertSkill: db.prepare(`
        INSERT INTO skills (id, user_id, name, description, file_path, source, content, token_count, enabled, content_hash, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT (user_id, file_path) DO UPDATE SET
          name = excluded.name,
          description = excluded.description,
          source = excluded.source,
          content = excluded.content,
          token_count = excluded.token_count,
          enabled = excluded.enabled,
          content_hash = excluded.content_hash,
          updated_at = excluded.updated_at
      `),
      getSkillByUserPath: db.prepare(
        'SELECT * FROM skills WHERE user_id = ? AND file_path = ?'
      ),
      toggleSkill: db.prepare(
        'UPDATE skills SET enabled = ?, updated_at = ? WHERE id = ? AND user_id = ?'
      ),
      deleteSkill: db.prepare(
        'DELETE FROM skills WHERE id = ? AND user_id = ?'
      ),
      insertUsage: db.prepare(`
        INSERT INTO usage_records (id, user_id, session_id, timestamp, model, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, cost, request_type)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
      `),
      getUsageSummary: db.prepare(`
        SELECT
          COALESCE(SUM(input_tokens), 0) AS total_input_tokens,
          COALESCE(SUM(output_tokens), 0) AS total_output_tokens,
          COALESCE(SUM(cost), 0) AS total_cost,
          COUNT(*) AS record_count
        FROM usage_records
        WHERE user_id = ? AND timestamp >= ?
      `),
      getDailyUsage: db.prepare(`
        SELECT
          date(timestamp) AS date,
          SUM(input_tokens) AS input_tokens,
          SUM(output_tokens) AS output_tokens,
          SUM(cost) AS cost
        FROM usage_records
        WHERE user_id = ? AND timestamp >= date('now', ? || ' days')
        GROUP BY date(timestamp)
        ORDER BY date
      `),
      getBurnRate: db.prepare(`
        SELECT
          COALESCE(SUM(input_tokens + output_tokens), 0) AS total_tokens,
          COALESCE(SUM(cost), 0) AS total_cost
        FROM usage_records
        WHERE user_id = ? AND timestamp >= ?
      `),
      listMcpServers: db.prepare(
        'SELECT * FROM mcp_servers WHERE user_id = ? ORDER BY name'
      ),
      upsertMcpServer: db.prepare(`
        INSERT INTO mcp_servers (id, user_id, name, transport, command, args, url, tool_count, status, last_error, last_seen_at, config_source)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT (user_id, name) DO UPDATE SET
          transport = excluded.transport,
          command = excluded.command,
          args = excluded.args,
          url = excluded.url,
          tool_count = excluded.tool_count,
          status = excluded.status,
          last_error = excluded.last_error,
          last_seen_at = excluded.last_seen_at,
          config_source = excluded.config_source
      `),
      getMcpServerByUserName: db.prepare(
        'SELECT * FROM mcp_servers WHERE user_id = ? AND name = ?'
      ),
      deleteMcpServer: db.prepare(
        'DELETE FROM mcp_servers WHERE id = ? AND user_id = ?'
      ),
      getSetting: db.prepare(
        'SELECT value FROM settings WHERE key = ?'
      ),
      setSetting: db.prepare(`
        INSERT INTO settings (key, value, updated_at)
        VALUES (?, ?, ?)
        ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
      `),
    };
  }

  async close(): Promise<void> {
    this.statements = null;
    this.db?.close();
    this.db = null;
  }

  private getDb(): Database.Database {
    if (!this.db) {
      throw new Error('Database not initialized. Call initialize() first.');
    }
    return this.db;
  }

  private getStatements(): PreparedStatements {
    if (!this.statements) {
      throw new Error('Statements not prepared. Call initialize() first.');
    }
    return this.statements;
  }

  // ── Transaction Support ─────────────────────────────────────────────────

  /**
   * Execute a function within a transaction.
   * Automatically commits on success, rolls back on error.
   */
  async transaction<T>(fn: () => T): Promise<T> {
    const db = this.getDb();
    return db.transaction(fn)();
  }

  // ── Skills ──────────────────────────────────────────────────────────────

  async listSkills(userId: string): Promise<SkillRow[]> {
    const stmt = this.getStatements().listSkills;
    const rows = stmt.all(userId) as Array<Record<string, unknown>>;
    return rows.map(this.mapSkillRow);
  }

  async upsertSkill(userId: string, skill: UpsertSkillInput): Promise<SkillRow> {
    const stmt = this.getStatements();
    const now = new Date().toISOString();
    const id = randomUUID();

    stmt.upsertSkill.run(
      id,
      userId,
      skill.name,
      skill.description ?? null,
      skill.filePath,
      skill.source,
      skill.content ?? null,
      skill.tokenCount,
      skill.enabled ? 1 : 0,
      skill.contentHash ?? null,
      now,
      now,
    );

    const row = stmt.getSkillByUserPath.get(userId, skill.filePath) as Record<string, unknown>;
    return this.mapSkillRow(row);
  }

  async toggleSkill(userId: string, skillId: string, enabled: boolean): Promise<void> {
    const stmt = this.getStatements().toggleSkill;
    stmt.run(enabled ? 1 : 0, new Date().toISOString(), skillId, userId);
  }

  async deleteSkill(userId: string, skillId: string): Promise<void> {
    const stmt = this.getStatements().deleteSkill;
    stmt.run(skillId, userId);
  }

  // ── Usage ───────────────────────────────────────────────────────────────

  async insertUsage(userId: string, record: InsertUsageInput): Promise<void> {
    const stmt = this.getStatements().insertUsage;
    stmt.run(
      randomUUID(),
      userId,
      record.sessionId ?? null,
      new Date().toISOString(),
      record.model,
      record.inputTokens,
      record.outputTokens,
      record.cacheReadTokens ?? 0,
      record.cacheWriteTokens ?? 0,
      record.cost,
      record.requestType ?? null,
    );
  }

  /**
   * Batch insert multiple usage records in a single transaction.
   * Much faster than individual inserts for bulk operations.
   */
  async insertUsageBatch(userId: string, records: InsertUsageInput[]): Promise<void> {
    if (records.length === 0) return;

    const db = this.getDb();
    const stmt = this.getStatements().insertUsage;

    // Wrap in transaction for atomicity and performance
    db.transaction(() => {
      const now = new Date().toISOString();
      for (const record of records) {
        stmt.run(
          randomUUID(),
          userId,
          record.sessionId ?? null,
          now,
          record.model,
          record.inputTokens,
          record.outputTokens,
          record.cacheReadTokens ?? 0,
          record.cacheWriteTokens ?? 0,
          record.cost,
          record.requestType ?? null,
        );
      }
    })();
  }

  async getUsageSummary(userId: string, since: Date): Promise<UsageSummaryRow> {
    const stmt = this.getStatements().getUsageSummary;
    const row = stmt.get(userId, since.toISOString()) as Record<string, number>;

    return {
      totalInputTokens: row.total_input_tokens,
      totalOutputTokens: row.total_output_tokens,
      totalCost: row.total_cost,
      recordCount: row.record_count,
    };
  }

  async getDailyUsage(userId: string, days: number): Promise<DailyUsageRow[]> {
    const stmt = this.getStatements().getDailyUsage;
    const rows = stmt.all(userId, -days) as Array<Record<string, unknown>>;

    return rows.map((r) => ({
      date: r.date as string,
      inputTokens: r.input_tokens as number,
      outputTokens: r.output_tokens as number,
      cost: r.cost as number,
    }));
  }

  async getBurnRate(userId: string): Promise<BurnRateRow> {
    const stmt = this.getStatements().getBurnRate;
    const oneHourAgo = new Date(Date.now() - 3_600_000).toISOString();
    const row = stmt.get(userId, oneHourAgo) as Record<string, number>;

    return {
      tokensPerHour: row.total_tokens,
      costPerHour: row.total_cost,
    };
  }

  // ── MCP Servers ─────────────────────────────────────────────────────────

  async listMcpServers(userId: string): Promise<McpServerRow[]> {
    const stmt = this.getStatements().listMcpServers;
    const rows = stmt.all(userId) as Array<Record<string, unknown>>;
    return rows.map(this.mapMcpServerRow);
  }

  async upsertMcpServer(
    userId: string,
    server: Partial<McpServerRow> & { name: string; transport: string },
  ): Promise<McpServerRow> {
    const stmt = this.getStatements();
    const id = server.id ?? randomUUID();

    stmt.upsertMcpServer.run(
      id,
      userId,
      server.name,
      server.transport,
      server.command ?? null,
      server.args ?? '[]',
      server.url ?? null,
      server.toolCount ?? 0,
      server.status ?? 'unknown',
      server.lastError ?? null,
      server.lastSeenAt ?? null,
      server.configSource ?? null,
    );

    const row = stmt.getMcpServerByUserName.get(userId, server.name) as Record<string, unknown>;
    return this.mapMcpServerRow(row);
  }

  async deleteMcpServer(userId: string, serverId: string): Promise<void> {
    const stmt = this.getStatements().deleteMcpServer;
    stmt.run(serverId, userId);
  }

  // ── Settings ────────────────────────────────────────────────────────────

  async getSetting(key: string): Promise<string | null> {
    const stmt = this.getStatements().getSetting;
    const row = stmt.get(key) as { value: string } | undefined;
    return row?.value ?? null;
  }

  async setSetting(key: string, value: string): Promise<void> {
    const stmt = this.getStatements().setSetting;
    stmt.run(key, value, new Date().toISOString());
  }

  // ── Helpers ─────────────────────────────────────────────────────────────

  private mapSkillRow(row: Record<string, unknown>): SkillRow {
    return {
      id: row.id as string,
      name: row.name as string,
      description: (row.description as string) ?? null,
      filePath: row.file_path as string,
      source: row.source as string,
      content: (row.content as string) ?? null,
      tokenCount: row.token_count as number,
      enabled: Boolean(row.enabled),
      contentHash: (row.content_hash as string) ?? null,
      createdAt: row.created_at as string,
      updatedAt: row.updated_at as string,
    };
  }

  private mapMcpServerRow(row: Record<string, unknown>): McpServerRow {
    return {
      id: row.id as string,
      name: row.name as string,
      transport: row.transport as string,
      command: (row.command as string) ?? null,
      args: row.args as string,
      url: (row.url as string) ?? null,
      toolCount: row.tool_count as number,
      status: row.status as string,
      lastError: (row.last_error as string) ?? null,
      lastSeenAt: (row.last_seen_at as string) ?? null,
      configSource: (row.config_source as string) ?? null,
    };
  }
}
