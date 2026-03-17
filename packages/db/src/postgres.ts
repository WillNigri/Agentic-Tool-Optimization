import pg from 'pg';
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

const { Pool } = pg;

export class PostgresAdapter implements DatabaseAdapter {
  private pool: pg.Pool;

  constructor(connectionStringOrConfig: string | pg.PoolConfig) {
    if (typeof connectionStringOrConfig === 'string') {
      this.pool = new Pool({ connectionString: connectionStringOrConfig });
    } else {
      this.pool = new Pool(connectionStringOrConfig);
    }
  }

  async initialize(): Promise<void> {
    // Assumes tables already exist (migrations run separately).
    // Verify connectivity.
    const client = await this.pool.connect();
    client.release();
  }

  async close(): Promise<void> {
    await this.pool.end();
  }

  // ── Skills ──────────────────────────────────────────────────────────

  async listSkills(userId: string): Promise<SkillRow[]> {
    const { rows } = await this.pool.query(
      'SELECT * FROM skills WHERE user_id = $1 ORDER BY name',
      [userId],
    );
    return rows.map(this.mapSkillRow);
  }

  async upsertSkill(userId: string, skill: UpsertSkillInput): Promise<SkillRow> {
    const now = new Date().toISOString();
    const id = randomUUID();

    const { rows } = await this.pool.query(
      `INSERT INTO skills (id, user_id, name, description, file_path, source, content, token_count, enabled, content_hash, created_at, updated_at)
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
       ON CONFLICT (user_id, file_path) DO UPDATE SET
         name = EXCLUDED.name,
         description = EXCLUDED.description,
         source = EXCLUDED.source,
         content = EXCLUDED.content,
         token_count = EXCLUDED.token_count,
         enabled = EXCLUDED.enabled,
         content_hash = EXCLUDED.content_hash,
         updated_at = EXCLUDED.updated_at
       RETURNING *`,
      [
        id,
        userId,
        skill.name,
        skill.description ?? null,
        skill.filePath,
        skill.source,
        skill.content ?? null,
        skill.tokenCount,
        skill.enabled,
        skill.contentHash ?? null,
        now,
        now,
      ],
    );

    return this.mapSkillRow(rows[0]);
  }

  async toggleSkill(userId: string, skillId: string, enabled: boolean): Promise<void> {
    await this.pool.query(
      'UPDATE skills SET enabled = $1, updated_at = $2 WHERE id = $3 AND user_id = $4',
      [enabled, new Date().toISOString(), skillId, userId],
    );
  }

  async deleteSkill(userId: string, skillId: string): Promise<void> {
    await this.pool.query(
      'DELETE FROM skills WHERE id = $1 AND user_id = $2',
      [skillId, userId],
    );
  }

  // ── Usage ───────────────────────────────────────────────────────────

  async insertUsage(userId: string, record: InsertUsageInput): Promise<void> {
    await this.pool.query(
      `INSERT INTO usage_records (id, user_id, session_id, timestamp, model, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, cost, request_type)
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)`,
      [
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
      ],
    );
  }

  async getUsageSummary(userId: string, since: Date): Promise<UsageSummaryRow> {
    const { rows } = await this.pool.query(
      `SELECT
         COALESCE(SUM(input_tokens), 0)::bigint AS total_input_tokens,
         COALESCE(SUM(output_tokens), 0)::bigint AS total_output_tokens,
         COALESCE(SUM(cost), 0)::double precision AS total_cost,
         COUNT(*)::bigint AS record_count
       FROM usage_records
       WHERE user_id = $1 AND timestamp >= $2`,
      [userId, since.toISOString()],
    );

    const row = rows[0];
    return {
      totalInputTokens: Number(row.total_input_tokens),
      totalOutputTokens: Number(row.total_output_tokens),
      totalCost: Number(row.total_cost),
      recordCount: Number(row.record_count),
    };
  }

  async getDailyUsage(userId: string, days: number): Promise<DailyUsageRow[]> {
    const { rows } = await this.pool.query(
      `SELECT
         date_trunc('day', timestamp::timestamptz)::date::text AS date,
         SUM(input_tokens) AS input_tokens,
         SUM(output_tokens) AS output_tokens,
         SUM(cost) AS cost
       FROM usage_records
       WHERE user_id = $1 AND timestamp >= (NOW() - ($2 || ' days')::interval)
       GROUP BY date_trunc('day', timestamp::timestamptz)
       ORDER BY date`,
      [userId, days],
    );

    return rows.map((r: Record<string, unknown>) => ({
      date: r.date as string,
      inputTokens: Number(r.input_tokens),
      outputTokens: Number(r.output_tokens),
      cost: Number(r.cost),
    }));
  }

  async getBurnRate(userId: string): Promise<BurnRateRow> {
    const { rows } = await this.pool.query(
      `SELECT
         COALESCE(SUM(input_tokens + output_tokens), 0) AS total_tokens,
         COALESCE(SUM(cost), 0) AS total_cost
       FROM usage_records
       WHERE user_id = $1 AND timestamp >= (NOW() - INTERVAL '1 hour')`,
      [userId],
    );

    const row = rows[0];
    return {
      tokensPerHour: Number(row.total_tokens),
      costPerHour: Number(row.total_cost),
    };
  }

  // ── MCP Servers ─────────────────────────────────────────────────────

  async listMcpServers(userId: string): Promise<McpServerRow[]> {
    const { rows } = await this.pool.query(
      'SELECT * FROM mcp_servers WHERE user_id = $1 ORDER BY name',
      [userId],
    );
    return rows.map(this.mapMcpServerRow);
  }

  async upsertMcpServer(
    userId: string,
    server: Partial<McpServerRow> & { name: string; transport: string },
  ): Promise<McpServerRow> {
    const id = server.id ?? randomUUID();

    const { rows } = await this.pool.query(
      `INSERT INTO mcp_servers (id, user_id, name, transport, command, args, url, tool_count, status, last_error, last_seen_at, config_source)
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
       ON CONFLICT (user_id, name) DO UPDATE SET
         transport = EXCLUDED.transport,
         command = EXCLUDED.command,
         args = EXCLUDED.args,
         url = EXCLUDED.url,
         tool_count = EXCLUDED.tool_count,
         status = EXCLUDED.status,
         last_error = EXCLUDED.last_error,
         last_seen_at = EXCLUDED.last_seen_at,
         config_source = EXCLUDED.config_source
       RETURNING *`,
      [
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
      ],
    );

    return this.mapMcpServerRow(rows[0]);
  }

  async deleteMcpServer(userId: string, serverId: string): Promise<void> {
    await this.pool.query(
      'DELETE FROM mcp_servers WHERE id = $1 AND user_id = $2',
      [serverId, userId],
    );
  }

  // ── Settings ────────────────────────────────────────────────────────

  async getSetting(key: string): Promise<string | null> {
    const { rows } = await this.pool.query(
      'SELECT value FROM settings WHERE key = $1',
      [key],
    );
    return rows.length > 0 ? (rows[0].value as string) : null;
  }

  async setSetting(key: string, value: string): Promise<void> {
    await this.pool.query(
      `INSERT INTO settings (key, value, updated_at)
       VALUES ($1, $2, $3)
       ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = EXCLUDED.updated_at`,
      [key, value, new Date().toISOString()],
    );
  }

  // ── Helpers ─────────────────────────────────────────────────────────

  private mapSkillRow(row: Record<string, unknown>): SkillRow {
    return {
      id: row.id as string,
      name: row.name as string,
      description: (row.description as string) ?? null,
      filePath: row.file_path as string,
      source: row.source as string,
      content: (row.content as string) ?? null,
      tokenCount: Number(row.token_count),
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
      toolCount: Number(row.tool_count),
      status: row.status as string,
      lastError: (row.last_error as string) ?? null,
      lastSeenAt: (row.last_seen_at as string) ?? null,
      configSource: (row.config_source as string) ?? null,
    };
  }
}
