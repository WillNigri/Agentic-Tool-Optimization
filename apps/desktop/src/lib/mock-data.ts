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
  { id: '6', name: 'security-policy', description: 'Enterprise security standards', filePath: '/etc/claude/skills/security-policy.md', scope: 'enterprise' as const, tokenCount: 4100, enabled: true, contentHash: 'ent001' },
  { id: '7', name: 'compliance-rules', description: 'Regulatory compliance guidelines', filePath: '/etc/claude/skills/compliance.md', scope: 'enterprise' as const, tokenCount: 2800, enabled: true, contentHash: 'ent002' },
  { id: '1', name: 'typescript-expert', description: 'TypeScript best practices', filePath: '~/.claude/skills/ts-expert/', scope: 'personal' as const, tokenCount: 2340, enabled: true, contentHash: 'abc123' },
  { id: '2', name: 'code-review', description: 'Code review guidelines', filePath: '~/.claude/skills/code-review.md', scope: 'personal' as const, tokenCount: 1890, enabled: true, contentHash: 'def456' },
  { id: '3', name: 'deprecated-skill', description: 'Old unused skill', filePath: '~/.claude/skills/deprecated.md', scope: 'personal' as const, tokenCount: 450, enabled: false, contentHash: 'ghi789' },
  { id: '4', name: 'project-conventions', description: 'Project coding standards', filePath: '.claude/skills/conventions.md', scope: 'project' as const, tokenCount: 3200, enabled: true, contentHash: 'jkl012' },
  { id: '5', name: 'api-guidelines', description: 'REST API design patterns', filePath: '.claude/skills/api.md', scope: 'project' as const, tokenCount: 1200, enabled: true, contentHash: 'mno345' },
  { id: '8', name: 'eslint-autofix', description: 'Auto-fix linting issues on save', filePath: '~/.claude/plugins/eslint-autofix/', scope: 'plugin' as const, tokenCount: 890, enabled: true, contentHash: 'plg001' },
  { id: '9', name: 'docker-helper', description: 'Docker compose and Dockerfile assistance', filePath: '~/.claude/plugins/docker-helper.md', scope: 'plugin' as const, tokenCount: 1560, enabled: false, contentHash: 'plg002' },
];

export const mockSkillDetails: Record<string, import('./tauri-api').SkillDetail> = {
  '1': {
    id: '1', name: 'typescript-expert', description: 'TypeScript best practices and coding standards. Use when writing or reviewing TypeScript code to ensure type safety, proper patterns, and consistent naming.',
    filePath: '~/.claude/skills/ts-expert/', scope: 'personal', tokenCount: 2340, enabled: true, contentHash: 'abc123',
    content: '# TypeScript Expert\n\nWhen writing TypeScript code, follow these best practices:\n\n## Type Safety\n- Always use strict mode\n- Prefer `unknown` over `any`\n- Use discriminated unions for complex state\n\n## Naming Conventions\n- Use PascalCase for types and interfaces\n- Use camelCase for variables and functions\n- Prefix interfaces with descriptive names, not `I`\n\n## Error Handling\n- Use Result types for expected failures\n- Reserve exceptions for unexpected errors\n- Always type catch blocks with `unknown`\n\n## Additional Resources\n\n- For complete style guide, see [style-guide.md](style-guide.md)\n- Run lint check: use [scripts/lint-check.sh](scripts/lint-check.sh)\n- Run formatter: use [scripts/format.sh](scripts/format.sh)',
    frontmatter: { name: 'typescript-expert', description: 'TypeScript best practices and coding standards. Use when writing or reviewing TypeScript code to ensure type safety, proper patterns, and consistent naming.', 'allowed-tools': 'Read, Write, Bash', allowedTools: ['Read', 'Write', 'Bash'], model: 'claude-sonnet-4-5', 'argument-hint': '[file-path]' },
    hasScripts: true, hasReferences: true, hasAssets: false,
    scripts: ['lint-check.sh', 'format.sh'], references: ['style-guide.md'], assets: [],
    isDirectory: true,
  },
  '2': {
    id: '2', name: 'code-review', description: 'Perform thorough code reviews checking correctness, security, performance, readability, and test coverage. Invoke when asked to review a PR or code changes.',
    filePath: '~/.claude/skills/code-review.md', scope: 'personal', tokenCount: 1890, enabled: true, contentHash: 'def456',
    content: '# Code Review\n\nWhen reviewing code, check for:\n\n1. **Correctness** — Does the code do what it claims?\n2. **Security** — Any injection vectors, exposed secrets?\n3. **Performance** — Unnecessary allocations, N+1 queries?\n4. **Readability** — Clear naming, good structure?\n5. **Tests** — Adequate coverage for the change?',
    frontmatter: { name: 'code-review', description: 'Perform thorough code reviews checking correctness, security, performance, readability, and test coverage. Invoke when asked to review a PR or code changes.', 'allowed-tools': 'Read, Grep, Glob', allowedTools: ['Read', 'Grep', 'Glob'], 'disable-model-invocation': false, 'user-invocable': true },
    hasScripts: false, hasReferences: false, hasAssets: false,
    scripts: [], references: [], assets: [],
    isDirectory: false,
  },
  '3': {
    id: '3', name: 'deprecated-skill', description: 'Old unused skill',
    filePath: '~/.claude/skills/deprecated.md', scope: 'personal', tokenCount: 450, enabled: false, contentHash: 'ghi789',
    content: '# Deprecated\n\nThis skill is no longer maintained.',
    frontmatter: { name: 'deprecated-skill', description: 'Old unused skill' },
    hasScripts: false, hasReferences: false, hasAssets: false,
    scripts: [], references: [], assets: [],
    isDirectory: false,
  },
  '4': {
    id: '4', name: 'project-conventions', description: 'Project coding standards',
    filePath: '.claude/skills/conventions.md', scope: 'project', tokenCount: 3200, enabled: true, contentHash: 'jkl012',
    content: '# Project Conventions\n\n## File Structure\n- Components in `src/components/`\n- Utilities in `src/lib/`\n- Pages in `src/pages/`\n\n## Styling\n- Use Tailwind CSS utility classes\n- Dark theme with cyan accent (#00FFB2)\n- Monospace font for code elements\n\n## State Management\n- Zustand for global state\n- React Query for server state\n- Local state for component-specific UI',
    frontmatter: { name: 'project-conventions', description: 'Project coding standards', allowedTools: ['Read', 'Write', 'Bash', 'Glob', 'Grep'], model: 'claude-sonnet-4-5' },
    hasScripts: false, hasReferences: true, hasAssets: true,
    scripts: [], references: ['architecture.md', 'api-spec.yaml'], assets: ['diagram.png'],
    isDirectory: false,
  },
  '5': {
    id: '5', name: 'api-guidelines', description: 'REST API design patterns',
    filePath: '.claude/skills/api.md', scope: 'project', tokenCount: 1200, enabled: true, contentHash: 'mno345',
    content: '# API Guidelines\n\n## REST Conventions\n- Use plural nouns for resources\n- HTTP verbs for actions\n- Consistent error response format\n\n## Response Format\n```json\n{ "data": ..., "error": null }\n```',
    frontmatter: { name: 'api-guidelines', description: 'REST API design patterns' },
    hasScripts: false, hasReferences: false, hasAssets: false,
    scripts: [], references: [], assets: [],
    isDirectory: false,
  },
  '6': {
    id: '6', name: 'security-policy', description: 'Enterprise security standards',
    filePath: '/etc/claude/skills/security-policy.md', scope: 'enterprise', tokenCount: 4100, enabled: true, contentHash: 'ent001',
    content: '# Security Policy\n\nAll code must comply with enterprise security standards.\n\n## Requirements\n- No hardcoded secrets\n- Parameterized SQL queries only\n- Input validation on all boundaries\n- HTTPS for all external calls\n- Audit logging for sensitive operations',
    frontmatter: { name: 'security-policy', description: 'Enterprise security standards. All code must be validated against security policy before committing.', 'allowed-tools': 'Read, Grep', allowedTools: ['Read', 'Grep'], 'user-invocable': false, 'disable-model-invocation': false },
    hasScripts: false, hasReferences: true, hasAssets: false,
    scripts: [], references: ['owasp-top10.md', 'compliance-checklist.md'], assets: [],
    isDirectory: false,
  },
  '7': {
    id: '7', name: 'compliance-rules', description: 'Regulatory compliance guidelines',
    filePath: '/etc/claude/skills/compliance.md', scope: 'enterprise', tokenCount: 2800, enabled: true, contentHash: 'ent002',
    content: '# Compliance Rules\n\n## Data Handling\n- PII must be encrypted at rest\n- Log redaction for sensitive fields\n- GDPR data subject rights\n\n## Audit Trail\n- All data mutations logged\n- Retention policy: 7 years',
    frontmatter: { name: 'compliance-rules', description: 'Regulatory compliance guidelines' },
    hasScripts: false, hasReferences: false, hasAssets: false,
    scripts: [], references: [], assets: [],
    isDirectory: false,
  },
  '8': {
    id: '8', name: 'eslint-autofix', description: 'Auto-fix linting issues on save',
    filePath: '~/.claude/plugins/eslint-autofix/', scope: 'plugin', tokenCount: 890, enabled: true, contentHash: 'plg001',
    content: '# ESLint Autofix\n\nAutomatically run ESLint --fix after writing JS/TS files.\n\n## Trigger\nRuns as a PostToolUse hook on Write/Edit tools.\n\n## Config\nUses project .eslintrc if available, falls back to recommended.',
    frontmatter: { name: 'eslint-autofix', description: 'Auto-fix linting issues on save. Runs ESLint --fix after writing JS/TS files.', 'allowed-tools': 'Bash', allowedTools: ['Bash'], 'disable-model-invocation': true, 'user-invocable': true },
    hasScripts: true, hasReferences: false, hasAssets: false,
    scripts: ['run-eslint.sh'], references: [], assets: [],
    isDirectory: true,
  },
  '9': {
    id: '9', name: 'docker-helper', description: 'Docker compose and Dockerfile assistance',
    filePath: '~/.claude/plugins/docker-helper.md', scope: 'plugin', tokenCount: 1560, enabled: false, contentHash: 'plg002',
    content: '# Docker Helper\n\nAssist with Dockerfile and docker-compose.yml authoring.\n\n## Capabilities\n- Multi-stage build optimization\n- Security scanning suggestions\n- Compose service dependency analysis',
    frontmatter: { name: 'docker-helper', description: 'Docker compose and Dockerfile assistance. Helps with multi-stage builds, security scanning, and compose service dependencies.', 'allowed-tools': 'Read, Write, Bash', allowedTools: ['Read', 'Write', 'Bash'], model: 'claude-sonnet-4-5', 'argument-hint': '[dockerfile-path]' },
    hasScripts: false, hasReferences: false, hasAssets: false,
    scripts: [], references: [], assets: [],
    isDirectory: false,
  },
};

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

// ── Mocks for Batches 1-5 + A-C features ──────────────────────────────────

export const mockProjectBundle = {
  projectPath: "/mock/project",
  projectName: "Mock Project",
  hasClaude: true, hasCodex: false, hasHermes: false, hasOpenclaw: false, hasGemini: false,
  memoryFiles: [
    { label: "CLAUDE.md", path: "/mock/CLAUDE.md", scope: "project" as const, exists: true, sizeBytes: 1024, tokenEstimate: 256, lastModified: Math.floor(Date.now() / 1000) },
  ],
  subagents: [] as { label: string; path: string; scope: string; exists: boolean; sizeBytes: number; tokenEstimate: number; lastModified: number | null }[],
  commands: [] as typeof mockProjectBundle.subagents,
  settingsFiles: [
    { label: "~/.claude/settings.json", path: "/mock/.claude/settings.json", scope: "user" as const, exists: true, sizeBytes: 512, tokenEstimate: 128, lastModified: Math.floor(Date.now() / 1000) },
  ],
  skills: [] as typeof mockSkills,
  hooks: [] as { event: string; matcher: string | null; command: string; scope: string }[],
  permissionsUser: { allow: [] as string[], deny: [] as string[], ask: [] as string[], scope: "user" },
  permissionsProject: { allow: [] as string[], deny: [] as string[], ask: [] as string[], scope: "project" },
  mcpServers: [] as { name: string; kind: string; commandOrUrl: string; scope: string }[],
  codexFiles: [] as typeof mockProjectBundle.subagents,
  codexSkills: [] as typeof mockSkills,
  openclawFiles: [] as typeof mockProjectBundle.subagents,
  openclawSkills: [] as typeof mockSkills,
  hermesFiles: [] as typeof mockProjectBundle.subagents,
  hermesSkills: [] as typeof mockSkills,
  geminiFiles: [] as typeof mockProjectBundle.subagents,
  geminiSkills: [] as typeof mockSkills,
  sandboxConfig: null,
  approvalPolicies: [] as { toolName: string; policy: string; scope: string }[],
};

export const mockParsedConfigFile = {
  path: "/mock/file.md",
  format: "markdown" as const,
  content: { body: "# Mock file\n\nThis is mock content for browser dev mode." },
  raw: "# Mock file\n\nThis is mock content for browser dev mode.",
  contentHash: "mockhash0000",
  lastModified: Math.floor(Date.now() / 1000),
  sizeBytes: 64,
};

export const mockWriteResult = {
  path: "/mock/file.md",
  newHash: "mockhash0001",
  bytesWritten: 64,
  backupPath: null,
  addedLines: 0,
  removedLines: 0,
};

export const mockWritePreview = {
  diff: [] as { kind: string; oldLine: number | null; newLine: number | null; text: string }[],
  addedLines: 0, removedLines: 0,
  currentHash: "mockhash0000", newHash: "mockhash0001",
  validation: null,
};

export const mockValidation = { valid: true, errors: [] as { field: string; message: string; line: number | null }[] };

export const mockBackups = [] as { backupPath: string; originalFilename: string; timestamp: number; sha8: string; sizeBytes: number }[];

export const mockOllamaStatus = { running: false, version: null, endpoint: "http://localhost:11434" };
export const mockOllamaModels = [] as { name: string; size: number; digest: string; modifiedAt: string; parameterSize: string | null; quantization: string | null }[];
export const mockOllamaConfig = { host: null, modelsDir: null, keepAlive: null, flashAttention: null, cudaVisibleDevices: null, numParallel: null };

export const mockProjects = [
  { id: "mock-1", name: "Mock Project", path: "/mock/project", isActive: true, skillCount: 3, lastAccessed: new Date().toISOString(), createdAt: new Date().toISOString(), hasClaude: true, hasCodex: false, hasHermes: false, hasOpenclaw: false, hasGemini: false },
];
