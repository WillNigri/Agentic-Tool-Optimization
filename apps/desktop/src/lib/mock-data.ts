/**
 * Mock data for browser dev mode (when Tauri is not available).
 * This lets you see the full UI without needing the Rust backend.
 */

export const mockContextBreakdown = {
  totalTokens: 67234,
  limit: 200000,
  categories: [
    { name: 'System Prompts', tokens: 28450, color: '#FF4466' },
    { name: 'Skills (4 active)', tokens: 12300, color: '#00FFB2' },
    { name: 'MCP Schemas (3)', tokens: 8200, color: '#3b82f6' },
    { name: 'CLAUDE.md', tokens: 2100, color: '#FFB800' },
    { name: 'Conversation', tokens: 14184, color: '#a78bfa' },
    { name: 'File Reads', tokens: 2000, color: '#8888a0' },
  ],
};

export const mockSkills = [
  { id: '1', name: 'typescript-expert', description: 'TypeScript best practices', filePath: '~/.claude/skills/typescript-expert.md', scope: 'personal' as const, tokenCount: 2340, enabled: true, contentHash: 'abc123' },
  { id: '2', name: 'code-review', description: 'Code review guidelines', filePath: '~/.claude/skills/code-review.md', scope: 'personal' as const, tokenCount: 1890, enabled: true, contentHash: 'def456' },
  { id: '3', name: 'deprecated-skill', description: 'Old unused skill', filePath: '~/.claude/skills/deprecated.md', scope: 'personal' as const, tokenCount: 450, enabled: false, contentHash: 'ghi789' },
  { id: '4', name: 'project-conventions', description: 'Project coding standards', filePath: '.claude/skills/conventions.md', scope: 'project' as const, tokenCount: 3200, enabled: true, contentHash: 'jkl012' },
  { id: '5', name: 'api-guidelines', description: 'REST API design patterns', filePath: '.claude/skills/api.md', scope: 'project' as const, tokenCount: 1200, enabled: true, contentHash: 'mno345' },
];

export const mockUsageSummary = {
  today: { inputTokens: 32450, outputTokens: 12780, costCents: 68 },
  week: { inputTokens: 224300, outputTokens: 88150, costCents: 469 },
  month: { inputTokens: 891000, outputTokens: 354000, costCents: 1867 },
};

export const mockDailyUsage = Array.from({ length: 30 }, (_, i) => {
  const date = new Date();
  date.setDate(date.getDate() - (29 - i));
  const base = 8000 + Math.random() * 20000;
  return {
    date: date.toISOString().split('T')[0],
    inputTokens: Math.round(base),
    outputTokens: Math.round(base * 0.4),
  };
});

export const mockBurnRate = {
  tokensPerHour: 12340,
  costPerHour: 0.19,
  estimatedHoursToLimit: 2.5,
  limit: 200000,
};

export const mockMcpServers = [
  { id: '1', name: 'filesystem', transport: 'stdio', status: 'running' as const, toolCount: 12, command: 'npx @anthropic/mcp-filesystem' },
  { id: '2', name: 'github', transport: 'stdio', status: 'running' as const, toolCount: 8, command: 'npx github-mcp-server' },
  { id: '3', name: 'slack', transport: 'http', status: 'error' as const, toolCount: 0, url: 'https://mcp.slack.com' },
  { id: '4', name: 'postgres', transport: 'stdio', status: 'running' as const, toolCount: 6, command: 'npx @anthropic/mcp-postgres' },
];

export const mockConfigFiles = [
  { path: '~/.claude.json', exists: false, scope: 'Global config' },
  { path: '~/.claude/settings.json', exists: true, scope: 'Global settings' },
  { path: '~/.claude/skills/', exists: true, scope: 'Personal skills' },
  { path: '.claude/settings.json', exists: false, scope: 'Project settings' },
  { path: '.claude/skills/', exists: false, scope: 'Project skills' },
  { path: 'CLAUDE.md', exists: true, scope: 'Project context' },
];
