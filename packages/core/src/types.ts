// ============================================================
// ClaudeScope Core Types
// Shared types used by both desktop app and cloud services.
// No database dependencies — pure TypeScript interfaces.
// ============================================================

// --- User ---
export interface User {
  id: string;
  email: string;
  name: string;
  password_hash: string;
  avatar_url: string | null;
  created_at: string;
  updated_at: string;
}

// Never include password_hash in API responses
export type SafeUser = Omit<User, 'password_hash'>;

export interface CreateUserRequest {
  email: string;
  name: string;
  password: string;
}

export interface LoginRequest {
  email: string;
  password: string;
}

export interface AuthTokens {
  accessToken: string;
  refreshToken: string;
}

export interface JwtPayload {
  userId: string;
  email: string;
  iat?: number;
  exp?: number;
}

// --- Sessions ---
export interface Session {
  id: string;
  user_id: string;
  session_id: string | null;
  started_at: string;
  ended_at: string | null;
  total_input_tokens: number;
  total_output_tokens: number;
  total_cost: number;
  model: string | null;
  metadata: Record<string, unknown>;
}

// --- Usage Records ---
export interface UsageRecord {
  id: string;
  user_id: string;
  session_id: string | null;
  timestamp: string;
  model: string;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  cost: number;
  request_type: string | null;
  metadata: Record<string, unknown>;
}

export interface UsageSummary {
  total_input_tokens: number;
  total_output_tokens: number;
  total_cost: number;
  record_count: number;
  period: string;
}

export interface BurnRate {
  tokens_per_hour: number;
  cost_per_hour: number;
  estimated_time_to_limit_minutes: number | null;
}

export interface DailyUsage {
  date: string;
  input_tokens: number;
  output_tokens: number;
  cost: number;
  request_count: number;
}

// --- Skills ---
export interface Skill {
  id: string;
  user_id: string;
  name: string;
  description: string | null;
  file_path: string;
  source: 'personal' | 'project';
  content: string | null;
  token_count: number;
  enabled: boolean;
  last_scanned_at: string | null;
  content_hash: string | null;
  created_at: string;
  updated_at: string;
}

export interface CreateSkillRequest {
  name: string;
  description?: string;
  content: string;
  source: 'personal' | 'project';
}

export interface UpdateSkillRequest {
  name?: string;
  description?: string;
  content?: string;
  enabled?: boolean;
}

// --- MCP Servers ---
export type McpTransport = 'stdio' | 'http' | 'streamable-http';
export type McpStatus = 'connected' | 'disconnected' | 'error' | 'warning' | 'unknown';

export interface McpServer {
  id: string;
  user_id: string;
  name: string;
  transport: McpTransport;
  command: string | null;
  args: string[];
  env: Record<string, string>;
  url: string | null;
  tool_count: number;
  status: McpStatus;
  last_error: string | null;
  last_seen_at: string | null;
  config_source: 'global' | 'project' | null;
  created_at: string;
  updated_at: string;
}

// --- Context Usage ---
export interface ContextBreakdown {
  system_prompts: number;
  skills: number;
  mcp_schemas: number;
  claude_md: number;
  conversation: number;
  file_reads: number;
  total_used: number;
  total_available: number;
  percentage: number;
}

// --- API Response Wrappers ---
export interface ApiResponse<T> {
  success: boolean;
  data: T;
}

export interface ApiError {
  success: false;
  error: {
    code: string;
    message: string;
    details?: unknown;
  };
}

export interface PaginatedResponse<T> {
  success: boolean;
  data: T[];
  pagination: {
    page: number;
    limit: number;
    total: number;
    totalPages: number;
  };
}

// --- Sync & Settings ---
export interface SyncStatus {
  enabled: boolean;
  lastSyncAt: string | null;
  cloudUrl: string | null;
}

export interface AppSettings {
  syncEnabled: boolean;
  cloudUrl: string;
  language: 'en' | 'pt' | 'es';
  theme: string;
}
