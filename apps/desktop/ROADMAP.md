# ATO Full Platform Roadmap

## Vision

ATO becomes the **single control panel** for all AI coding agents and LLMs. Users configure and monitor here, but run their agents however they want (terminal, IDE, Slack, etc.).

```
┌─────────────────────────────────────────────────────────────────────┐
│                         ATO PLATFORM                                 │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  CONFIGURE          MONITOR             ALERT                       │
│  ┌─────────┐       ┌─────────┐        ┌─────────┐                  │
│  │ Skills  │       │ Health  │        │ Slack   │                  │
│  │ Projects│       │ Logs    │        │Telegram │                  │
│  │ Perms   │       │ Metrics │        │ Email   │                  │
│  │ Secrets │       │ Uptime  │        │ Webhook │                  │
│  └─────────┘       └─────────┘        └─────────┘                  │
│       │                 │                  │                        │
│       └────────────┬────┴──────────────────┘                        │
│                    │                                                 │
│              ┌─────▼─────┐                                          │
│              │  SQLite   │  (local)                                 │
│              │  + Cloud  │  (pro - sync)                            │
│              └───────────┘                                          │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        │                     │                     │
        ▼                     ▼                     ▼
   ┌─────────┐          ┌─────────┐          ┌─────────┐
   │ Claude  │          │  Codex  │          │ OpenClaw│
   │  Code   │          │   CLI   │          │  (SSH)  │
   └─────────┘          └─────────┘          └─────────┘
   (terminal)           (terminal)           (remote)
```

---

## Current State (v0.9.x)

### Configuration ✅
- [x] Config file editing (SKILL.md, settings.json, CLAUDE.md, etc.)
- [x] Skills management (view, create, enable/disable, clone)
- [x] Project management (discover, rename, filter)
- [x] Permission matrix
- [x] Profile snapshots
- [x] Health check / linting
- [x] Context preview
- [x] MCP server management
- [x] Subagents configuration
- [x] Hooks management
- [x] Automation flows
- [x] Cron scheduling

### Authentication ✅
- [x] Local-first (always works offline)
- [x] Cloud login (optional, for pro features)
- [x] Token refresh

### Monitoring (Partial)
- [x] Cron job status
- [x] Basic alerts (in-memory)
- [ ] Real-time polling
- [ ] Execution logs
- [ ] Cascade failure detection

---

## Phase 1: Complete Configuration (v1.0)

### 1.1 Secrets Manager
Store API keys and sensitive config securely.

```
┌─────────────────────────────────────────────────┐
│ SECRETS MANAGER                          🔒     │
├─────────────────────────────────────────────────┤
│ ANTHROPIC_API_KEY     ••••••••••sk-ant   [👁]   │
│ OPENAI_API_KEY        ••••••••••sk-...   [👁]   │
│ GITHUB_TOKEN          ••••••••••ghp_...  [👁]   │
│ OPENCLAW_SSH_KEY      ~/.ssh/openclaw    [📁]   │
├─────────────────────────────────────────────────┤
│ [+ Add Secret]                                  │
└─────────────────────────────────────────────────┘
```

**Implementation:**
- Rust: Use `keyring` crate for OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service)
- Frontend: SecretsManager.tsx component
- Never store plaintext - only keychain references
- Show masked values with reveal option

### 1.2 Environment Variables Manager
Manage .env files per project/runtime.

```
┌─────────────────────────────────────────────────┐
│ ENVIRONMENT VARIABLES                           │
├─────────────────────────────────────────────────┤
│ Project: my-app    Runtime: [Claude ▼]          │
├─────────────────────────────────────────────────┤
│ NODE_ENV           = production                 │
│ DATABASE_URL       = postgres://...             │
│ FEATURE_FLAGS      = dark_mode,beta             │
├─────────────────────────────────────────────────┤
│ [+ Add Variable]   [Import .env]   [Export]     │
└─────────────────────────────────────────────────┘
```

**Implementation:**
- Parse/write .env files per project
- Support runtime-specific overrides
- Import from existing .env files

### 1.3 Model Configuration
Select models per runtime/project.

```
┌─────────────────────────────────────────────────┐
│ MODEL SETTINGS                                  │
├─────────────────────────────────────────────────┤
│ Claude Code                                     │
│   Model: [claude-sonnet-4-20250514 ▼]           │
│   Max tokens: [8192        ]                    │
│                                                 │
│ Codex CLI                                       │
│   Model: [gpt-4-turbo ▼]                        │
│   Temperature: [0.7]                            │
└─────────────────────────────────────────────────┘
```

---

## Phase 2: Monitoring & Logs (v1.1)

### 2.1 Unified Log Viewer
Aggregate logs from all runtimes.

```
┌─────────────────────────────────────────────────────────────────┐
│ EXECUTION LOGS                    [Claude ▼] [All ▼] [🔍 Filter]│
├─────────────────────────────────────────────────────────────────┤
│ 14:32:05  ✓  Claude   /review-pr #423         2.3s   1,234 tok │
│ 14:31:42  ✓  Claude   "fix the tests"         4.1s   3,892 tok │
│ 14:28:11  ✗  Codex    "deploy to staging"     0.4s   Error     │
│ 14:25:33  ✓  OpenClaw  ssh://prod "status"    1.2s   423 tok   │
├─────────────────────────────────────────────────────────────────┤
│ ▼ 14:28:11  Codex - "deploy to staging"                        │
│   Error: API rate limit exceeded                                │
│   Request ID: req_abc123                                        │
│   Stack: at deploy.ts:42                                        │
│   [Retry] [Copy Error] [View Full]                              │
└─────────────────────────────────────────────────────────────────┘
```

**Implementation:**
- Rust: Watch `~/.ato/agent-logs.jsonl` (already exists)
- Rust: File watcher for real-time updates (notify crate)
- Frontend: LogViewer.tsx with virtual scrolling
- Store in SQLite for search/filter

### 2.2 Health Dashboard
Real-time status of all runtimes.

```
┌─────────────────────────────────────────────────────────────────┐
│ SYSTEM HEALTH                              Last check: 2s ago   │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐        │
│  │  Claude  │  │  Codex   │  │ OpenClaw │  │  Hermes  │        │
│  │    ✓     │  │    ✓     │  │    ⚠     │  │    ✗     │        │
│  │ 99.9% up │  │ 99.2% up │  │ SSH slow │  │ Offline  │        │
│  │  23ms    │  │  45ms    │  │  340ms   │  │    -     │        │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘        │
│                                                                  │
│  RECENT ISSUES                                                   │
│  ⚠ OpenClaw: SSH connection latency > 300ms (14:32)             │
│  ✗ Hermes: Local server not running (14:28)                     │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

**Implementation:**
- Rust: Background thread polling each runtime every 30s
- Store health history in SQLite
- Calculate uptime percentages
- Latency tracking

### 2.3 Metrics & Analytics
Usage stats, costs, performance.

```
┌─────────────────────────────────────────────────────────────────┐
│ USAGE ANALYTICS                    [This Week ▼]                │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Total Tokens: 1,234,567        Est. Cost: $12.34               │
│                                                                  │
│  ████████████████████░░░░░░░░░  Claude    (78%)                 │
│  ████░░░░░░░░░░░░░░░░░░░░░░░░  Codex     (15%)                 │
│  ██░░░░░░░░░░░░░░░░░░░░░░░░░░  OpenClaw  (7%)                  │
│                                                                  │
│  Top Skills:                                                     │
│  1. /review-pr      423 uses    156k tokens                     │
│  2. /fix-tests      312 uses    89k tokens                      │
│  3. /ship           198 uses    67k tokens                      │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Phase 3: Alerts & Notifications (v1.2) - PRO

### 3.1 Alert Rules Engine
Define conditions that trigger alerts.

```
┌─────────────────────────────────────────────────────────────────┐
│ ALERT RULES                                    [+ New Rule]     │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ 🔴 Runtime Down                              [Enabled]   │   │
│  │ IF runtime.status = "offline" FOR 2 minutes             │   │
│  │ THEN notify: Slack #alerts, Email                       │   │
│  │ Severity: Critical                                       │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ 🟡 Cron Silent Failure                       [Enabled]   │   │
│  │ IF cron.execution.output = empty AND status = "success" │   │
│  │ THEN notify: Telegram, In-App                           │   │
│  │ Severity: Warning                                        │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ 🟡 High Token Usage                          [Enabled]   │   │
│  │ IF daily.tokens > 100000                                │   │
│  │ THEN notify: Email                                       │   │
│  │ Severity: Info                                           │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

**Alert Types:**
- Runtime offline
- Cron job failed
- Silent failure (ran but no output)
- Error rate spike
- Token usage threshold
- Latency threshold
- API key expiring
- Cascade failure (dependency failed)

### 3.2 Notification Channels

```
┌─────────────────────────────────────────────────────────────────┐
│ NOTIFICATION CHANNELS                                           │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ✓ In-App Notifications                         [Configure]     │
│    Show alerts in notification center                           │
│                                                                  │
│  ✓ Slack                                        [Connected]     │
│    Workspace: acme-corp  Channel: #agent-alerts                │
│                                                                  │
│  ○ Telegram                                     [Connect]       │
│    Send alerts to Telegram bot                                  │
│                                                                  │
│  ○ Email                                        [Configure]     │
│    Send to: alerts@company.com                                  │
│                                                                  │
│  ○ Webhook                                      [Configure]     │
│    POST alerts to custom endpoint                               │
│                                                                  │
│  ○ PagerDuty                                    [Connect]       │
│    Integration for on-call escalation                           │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

**Implementation:**
- Slack: OAuth app, webhook integration
- Telegram: Bot token, chat ID
- Email: SMTP or SendGrid/Resend API
- Webhook: Generic HTTP POST
- In-app: Notification center with badge count

### 3.3 Cascade Failure Detection
Map dependencies and detect chain failures.

```
┌─────────────────────────────────────────────────────────────────┐
│ DEPENDENCY MAP                                                  │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌─────────┐                                                    │
│  │ Claude  │◄────┐                                              │
│  │  Code   │     │                                              │
│  └────┬────┘     │                                              │
│       │          │                                              │
│       ▼          │                                              │
│  ┌─────────┐     │     ┌─────────┐                             │
│  │ /review │─────┼────►│  Slack  │                             │
│  │   -pr   │     │     │  Bot    │                             │
│  └────┬────┘     │     └─────────┘                             │
│       │          │                                              │
│       ▼          │                                              │
│  ┌─────────┐     │                                              │
│  │ /ship   │─────┘                                              │
│  │  cron   │                                                    │
│  └─────────┘                                                    │
│                                                                  │
│  ⚠ If Claude Code goes down:                                    │
│    - /review-pr will fail                                       │
│    - /ship cron will fail (depends on /review-pr)               │
│    - Slack notifications will stop                              │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Phase 4: Pro Features & Cloud (v1.3)

### 4.1 User Accounts (Free)
- Sign up with email
- Profile settings
- Stored locally until cloud sync enabled

### 4.2 Cloud Sync (Pro)
- Sync configurations across machines
- Backup profile snapshots
- Team workspaces

### 4.3 Team Features (Pro)
- Shared skill library
- Shared alert rules
- Audit log (who changed what)
- Role-based access (admin, member, viewer)

### 4.4 Pro Dashboard
- Historical analytics (30/90 days)
- Cost tracking per project
- Performance benchmarks
- Export reports

---

## Database Schema Updates

### New Tables

```sql
-- Secrets (reference only, actual values in OS keychain)
CREATE TABLE secrets (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  key_type TEXT NOT NULL,  -- "api_key", "ssh_key", "token"
  runtime TEXT,            -- NULL = global
  project_id TEXT,         -- NULL = global
  keychain_ref TEXT NOT NULL,  -- OS keychain identifier
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- Environment variables
CREATE TABLE env_vars (
  id TEXT PRIMARY KEY,
  project_id TEXT,
  runtime TEXT,
  key TEXT NOT NULL,
  value TEXT NOT NULL,     -- encrypted at rest
  created_at TEXT NOT NULL
);

-- Execution logs
CREATE TABLE execution_logs (
  id TEXT PRIMARY KEY,
  runtime TEXT NOT NULL,
  prompt TEXT,
  response TEXT,
  tokens_in INTEGER,
  tokens_out INTEGER,
  duration_ms INTEGER,
  status TEXT NOT NULL,    -- "success", "error", "timeout"
  error_message TEXT,
  created_at TEXT NOT NULL
);

-- Health checks
CREATE TABLE health_checks (
  id TEXT PRIMARY KEY,
  runtime TEXT NOT NULL,
  status TEXT NOT NULL,    -- "healthy", "degraded", "offline"
  latency_ms INTEGER,
  error_message TEXT,
  checked_at TEXT NOT NULL
);

-- Alert rules
CREATE TABLE alert_rules (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  condition_type TEXT NOT NULL,  -- "runtime_down", "cron_failed", etc.
  condition_config JSON NOT NULL,
  severity TEXT NOT NULL,  -- "critical", "warning", "info"
  channels JSON NOT NULL,  -- ["slack", "email"]
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL
);

-- Alert history
CREATE TABLE alert_history (
  id TEXT PRIMARY KEY,
  rule_id TEXT NOT NULL,
  triggered_at TEXT NOT NULL,
  resolved_at TEXT,
  acknowledged_at TEXT,
  acknowledged_by TEXT,
  details JSON
);

-- Notification channels
CREATE TABLE notification_channels (
  id TEXT PRIMARY KEY,
  type TEXT NOT NULL,      -- "slack", "telegram", "email", "webhook"
  name TEXT NOT NULL,
  config JSON NOT NULL,    -- encrypted credentials
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL
);
```

---

## Implementation Priority

### v1.0 (Complete Configuration)
1. Secrets Manager (keychain integration)
2. Environment Variables Manager
3. Model Configuration UI
4. File watcher for external changes

### v1.1 (Monitoring & Logs)
1. Unified Log Viewer
2. Health Dashboard with polling
3. Metrics aggregation
4. SQLite storage for history

### v1.2 (Alerts - Pro)
1. Alert rules engine
2. In-app notification center
3. Slack integration
4. Telegram integration
5. Email notifications
6. Webhook support
7. Cascade failure detection

### v1.3 (Cloud - Pro)
1. User accounts backend
2. Cloud sync API
3. Team workspaces
4. Billing integration

---

## File Structure (New)

```
apps/desktop/src/
├── components/
│   ├── SecretsManager/
│   │   ├── SecretsManager.tsx
│   │   ├── SecretForm.tsx
│   │   └── SecretList.tsx
│   ├── EnvManager/
│   │   ├── EnvManager.tsx
│   │   ├── EnvEditor.tsx
│   │   └── EnvImport.tsx
│   ├── LogViewer/
│   │   ├── LogViewer.tsx
│   │   ├── LogEntry.tsx
│   │   ├── LogFilters.tsx
│   │   └── LogDetail.tsx
│   ├── HealthDashboard/
│   │   ├── HealthDashboard.tsx
│   │   ├── RuntimeCard.tsx
│   │   ├── UptimeChart.tsx
│   │   └── LatencyGraph.tsx
│   ├── AlertCenter/
│   │   ├── AlertCenter.tsx
│   │   ├── AlertRules.tsx
│   │   ├── RuleEditor.tsx
│   │   ├── NotificationChannels.tsx
│   │   └── AlertHistory.tsx
│   └── NotificationBell/
│       ├── NotificationBell.tsx
│       └── NotificationPanel.tsx
├── stores/
│   ├── useSecretsStore.ts
│   ├── useEnvStore.ts
│   ├── useLogsStore.ts
│   ├── useHealthStore.ts
│   └── useAlertsStore.ts
└── lib/
    ├── keychain.ts
    ├── log-parser.ts
    ├── health-check.ts
    └── notification-sender.ts

apps/desktop/src-tauri/src/
├── keychain.rs      -- OS keychain integration
├── log_watcher.rs   -- File watcher for logs
├── health.rs        -- Runtime health checks
├── alerts.rs        -- Alert rule evaluation
└── notifications.rs -- Send to Slack/Telegram/etc.
```

---

---

## Open Source vs Closed Source Split

### Open Source (This Repo - MIT License)

**All configuration features are FREE:**
- Config file editing (all runtimes)
- Skills management
- Project management
- Permission matrix
- Profile snapshots (local only)
- Health check / linting
- Context preview
- MCP server management
- Subagents configuration
- Hooks management
- Automation flows builder
- Cron scheduling (local)
- Secrets Manager (OS keychain)
- Environment Variables Manager
- Model Configuration
- Log Viewer (local logs)
- Basic Health Dashboard

**Rationale:** Configuration should be free. Users should be able to set up their agents without paying.

### Closed Source (Separate Repo - Paid)

**Pro features require account + subscription:**

| Feature | Why Pro? |
|---------|----------|
| Real-time monitoring | Server-side polling, infrastructure cost |
| Alert rules engine | Complex evaluation, background processing |
| Slack integration | OAuth app maintenance, API costs |
| Telegram integration | Bot hosting, API costs |
| Email notifications | Email service costs (SendGrid/Resend) |
| Webhook notifications | Reliability, retry logic |
| Cascade failure detection | Complex graph analysis |
| Cloud sync | Server storage, bandwidth |
| Team workspaces | Multi-user infra, permissions |
| Historical analytics (30+ days) | Cloud database storage |
| Export reports | PDF generation, formatting |
| Priority support | Human time |

**Pro Architecture:**
```
┌─────────────────────┐     ┌─────────────────────┐
│   ATO Desktop       │     │   ATO Cloud (Pro)   │
│   (Open Source)     │────►│   (Closed Source)   │
│                     │     │                     │
│ - Local config      │     │ - User accounts     │
│ - Local logs        │     │ - Sync API          │
│ - Local health      │     │ - Alert service     │
│ - Local alerts      │     │ - Notification hub  │
│                     │     │ - Team management   │
│                     │     │ - Analytics DB      │
└─────────────────────┘     └─────────────────────┘
```

---

## Implementation Strategy

### Open Source Repo Structure
```
apps/desktop/
├── src/
│   ├── components/
│   │   ├── SecretsManager/      # FREE
│   │   ├── EnvManager/          # FREE
│   │   ├── LogViewer/           # FREE (local logs)
│   │   ├── HealthDashboard/     # FREE (local checks)
│   │   └── AlertCenter/         # FREE (local only)
│   └── lib/
│       ├── keychain.ts          # FREE
│       └── log-parser.ts        # FREE
```

### Closed Source Repo Structure
```
ato-cloud/
├── api/                         # Backend API
│   ├── auth/                    # User auth
│   ├── sync/                    # Config sync
│   ├── alerts/                  # Alert service
│   └── notifications/           # Slack/Telegram/Email
├── workers/                     # Background jobs
│   ├── health-poller/           # Runtime monitoring
│   ├── alert-evaluator/         # Rule evaluation
│   └── notification-sender/     # Dispatch notifications
├── dashboard/                   # Pro web dashboard
└── desktop-plugin/              # Pro features for desktop
    ├── src/
    │   ├── ProFeatures.tsx      # Pro UI components
    │   ├── CloudSync.tsx
    │   ├── AlertRulesEditor.tsx
    │   └── NotificationChannels.tsx
    └── package.json
```

### How Pro Integrates with Open Source

```typescript
// In open source desktop app
import { useAuthStore } from "@/hooks/useAuth";

function AlertCenter() {
  const { isCloudUser } = useAuthStore();

  if (!isCloudUser) {
    return (
      <div>
        <h2>Local Alerts</h2>
        <LocalAlertsList />
        <ProUpgradePrompt feature="real-time-alerts" />
      </div>
    );
  }

  // Pro user - load from cloud
  return <CloudAlertCenter />;
}

// ProUpgradePrompt shows:
// "Upgrade to Pro for real-time alerts, Slack/Telegram notifications,
//  and cascade failure detection"
```

---

## Summary

| Phase | Features | License | Repo |
|-------|----------|---------|------|
| v1.0 | Secrets, Env Vars, Models, Local Logs | MIT | Open |
| v1.1 | Health Dashboard (local), Basic Metrics | MIT | Open |
| v1.2 | Local Alerts (in-app only) | MIT | Open |
| v1.2-Pro | Real-time monitoring, Slack/Telegram, Email, Cascade | Paid | Closed |
| v1.3-Pro | Cloud sync, Teams, Historical analytics | Paid | Closed |

**Principle:**
- **Configure for free** - All setup/editing is open source
- **Monitor for free** - Local logs and basic health checks are open source
- **Alert at scale = Pro** - Real-time monitoring, external notifications, team features need infrastructure

**End Goal:** Users open ATO once to configure everything, then use their agents however they want. Free users see local logs and basic health. Pro users get real-time monitoring, alerts to Slack/Telegram/Email, and cloud sync.
