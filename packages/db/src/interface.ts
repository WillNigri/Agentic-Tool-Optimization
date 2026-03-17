export interface UpsertSkillInput {
  name: string;
  description?: string;
  filePath: string;
  source: 'personal' | 'project';
  content?: string;
  tokenCount: number;
  enabled: boolean;
  contentHash?: string;
}

export interface InsertUsageInput {
  model: string;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens?: number;
  cacheWriteTokens?: number;
  cost: number;
  requestType?: string;
  sessionId?: string;
}

export interface DailyUsageRow {
  date: string;
  inputTokens: number;
  outputTokens: number;
  cost: number;
}

export interface SkillRow {
  id: string;
  name: string;
  description: string | null;
  filePath: string;
  source: string;
  content: string | null;
  tokenCount: number;
  enabled: boolean;
  contentHash: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface McpServerRow {
  id: string;
  name: string;
  transport: string;
  command: string | null;
  args: string;
  url: string | null;
  toolCount: number;
  status: string;
  lastError: string | null;
  lastSeenAt: string | null;
  configSource: string | null;
}

export interface UsageSummaryRow {
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCost: number;
  recordCount: number;
}

export interface BurnRateRow {
  tokensPerHour: number;
  costPerHour: number;
}

export interface DatabaseAdapter {
  initialize(): Promise<void>;
  close(): Promise<void>;

  // Skills
  listSkills(userId: string): Promise<SkillRow[]>;
  upsertSkill(userId: string, skill: UpsertSkillInput): Promise<SkillRow>;
  toggleSkill(userId: string, skillId: string, enabled: boolean): Promise<void>;
  deleteSkill(userId: string, skillId: string): Promise<void>;

  // Usage
  insertUsage(userId: string, record: InsertUsageInput): Promise<void>;
  getUsageSummary(userId: string, since: Date): Promise<UsageSummaryRow>;
  getDailyUsage(userId: string, days: number): Promise<DailyUsageRow[]>;
  getBurnRate(userId: string): Promise<BurnRateRow>;

  // MCP Servers
  listMcpServers(userId: string): Promise<McpServerRow[]>;
  upsertMcpServer(userId: string, server: Partial<McpServerRow> & { name: string; transport: string }): Promise<McpServerRow>;
  deleteMcpServer(userId: string, serverId: string): Promise<void>;

  // Settings (key-value store)
  getSetting(key: string): Promise<string | null>;
  setSetting(key: string, value: string): Promise<void>;
}
