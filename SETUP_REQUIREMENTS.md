# ATO Setup Requirements

This document lists everything that requires manual setup, API keys, or configuration before features are production-ready.

---

## Status Legend

- ✅ **Ready** - Works out of the box
- ⚠️ **Needs Config** - Requires manual setup/keys
- 🚧 **In Progress** - Code exists but not complete
- ❌ **Not Implemented** - Planned but not built yet

---

## Railway Setup (ato-cloud is on Railway)

Since ato-cloud is deployed on Railway, here's what you need to configure:

### Step 1: Add PostgreSQL Database

1. Go to your Railway project dashboard
2. Click **"+ New"** → **"Database"** → **"PostgreSQL"**
3. Railway automatically creates `DATABASE_URL` and links it to your service

### Step 2: Set Environment Variables

In Railway dashboard → Your service → **Variables** tab, add:

```
# Required - Generate with: openssl rand -base64 32
JWT_SECRET=<generate-a-32-char-secret>
JWT_REFRESH_SECRET=<generate-another-32-char-secret>

# GitHub OAuth (see GitHub OAuth Setup section below)
GITHUB_CLIENT_ID=<from-github-oauth-app>
GITHUB_CLIENT_SECRET=<from-github-oauth-app>
GITHUB_CALLBACK_URL=https://<your-railway-domain>/api/auth/github/callback

# Internal API key for service-to-service calls
INTERNAL_API_KEY=<generate-another-secret>
```

### Step 3: Deploy

After setting variables, redeploy. The migrations run automatically via `start.sh`.

### Step 4: Verify

Check your Railway logs for:
```
[ATO] Migration 001 applied
[ATO] Migration 002 applied
[ATO] Migration 003 applied
[ATO] All services running.
```

### Get Your Railway URL

Your API will be at: `https://<your-service>.railway.app`

Use this URL in the desktop app:
```
VITE_CLOUD_API_URL=https://<your-service>.railway.app
```

---

## v0.5.0 — Cloud Sync & Collaboration

### Cloud Backend (ato-cloud)

| Component | Status | What You Need |
|-----------|--------|---------------|
| PostgreSQL Database | ⚠️ | Set up PostgreSQL instance, run migrations |
| Database Migrations | ⚠️ | Run `database/migrations/001_initial.sql` and `002_phase3_cloud_sync.sql` |
| Environment Variables | ⚠️ | See below |

**Required Environment Variables for ato-cloud:**

```bash
# Database
DATABASE_URL=postgresql://user:pass@localhost:5432/ato_cloud

# JWT Authentication
JWT_SECRET=your-secure-secret-key-min-32-chars
JWT_REFRESH_SECRET=another-secure-secret-key

# GitHub OAuth (for login)
GITHUB_CLIENT_ID=your-github-oauth-app-client-id
GITHUB_CLIENT_SECRET=your-github-oauth-app-secret
GITHUB_CALLBACK_URL=http://localhost:3001/api/auth/github/callback

# Service URLs (if not using defaults)
AUTH_SERVICE_URL=http://localhost:3001
TEAMS_SERVICE_URL=http://localhost:3005
SKILLS_SERVICE_URL=http://localhost:3002
NOTIFICATIONS_SERVICE_URL=http://localhost:3006
```

### GitHub OAuth Setup

| Step | Status | Instructions |
|------|--------|--------------|
| Create OAuth App | ⚠️ | Go to GitHub Settings → Developer Settings → OAuth Apps → New |
| Set Callback URL | ⚠️ | Use `http://localhost:3001/api/auth/github/callback` for dev |
| Copy Client ID | ⚠️ | Add to `GITHUB_CLIENT_ID` env var |
| Copy Client Secret | ⚠️ | Add to `GITHUB_CLIENT_SECRET` env var |

**GitHub OAuth App Settings:**
- Application name: `ATO Cloud (Dev)` or your preferred name
- Homepage URL: `http://localhost:5173` (or your frontend URL)
- Authorization callback URL: `http://localhost:3001/api/auth/github/callback`

### Desktop App Cloud Integration

| Feature | Status | Notes |
|---------|--------|-------|
| Cloud Auth UI | ✅ | Built, needs backend running |
| Team Workspaces UI | ✅ | Built, needs backend running |
| Skill Sync UI | ✅ | Built, needs backend running |
| Backend Connection | ⚠️ | Set `VITE_CLOUD_API_URL` env var or uses default `https://api.ato.dev` |

**Desktop App Environment:**
```bash
# In apps/desktop/.env or .env.local
VITE_CLOUD_API_URL=http://localhost:3000  # API Gateway URL
```

---

## v0.5.5 — Notifications & Integrations

### Notifications Service

| Provider | Status | What You Need |
|----------|--------|---------------|
| Slack | 🚧 | Slack webhook URL from your workspace |
| Discord | 🚧 | Discord webhook URL from your server |
| Telegram | 🚧 | Bot token + Chat ID |
| Email (SMTP) | 🚧 | SMTP server credentials |

**Note:** Backend code exists but needs:
1. Database tables for storing configs (not in migrations yet)
2. Desktop UI for managing notification channels (not built yet)
3. Integration with other services to trigger notifications

### Slack Setup

1. Go to your Slack workspace → Apps → Manage → Build
2. Create new app → From scratch
3. Add "Incoming Webhooks" feature
4. Activate and create webhook for a channel
5. Copy the webhook URL (looks like `https://hooks.slack.com/services/T.../B.../xxx`)

### Discord Setup

1. Open Discord server settings → Integrations → Webhooks
2. Create new webhook
3. Copy webhook URL (looks like `https://discord.com/api/webhooks/123/abc...`)

### Telegram Setup

1. Message @BotFather on Telegram
2. Send `/newbot` and follow instructions
3. Copy the bot token
4. Add bot to your channel/group
5. Get chat ID:
   - For groups: add bot, send message, check `https://api.telegram.org/bot<TOKEN>/getUpdates`
   - For channels: use `@channelusername` or numeric ID

### Email (SMTP) Setup

You need SMTP credentials. Options:
- **Gmail**: Enable "App Passwords" in Google Account settings
- **SendGrid**: Create account, get API key, use `smtp.sendgrid.net:587`
- **Mailgun**: Create account, verify domain, get SMTP credentials
- **Self-hosted**: Use your own SMTP server

---

## v0.3.0 - v0.4.0 — Desktop Features

### Features That Work Locally (No Setup Needed)

| Feature | Status | Notes |
|---------|--------|-------|
| Skills Manager | ✅ | Scans local filesystem |
| Context Visualizer | ✅ | Uses local data |
| Subagents Manager | ✅ | Local config |
| Hooks Manager | ✅ | Local config |
| Automation Builder | ✅ | Local workflows |
| Cron Monitor | ✅ | Local scheduling |
| Log Viewer | ✅ | Reads local log files |
| Health Dashboard | ✅ | Polls local runtimes |
| MCP Dashboard | ✅ | Shows local MCP config |

### Runtime Detection

| Runtime | Status | Requirements |
|---------|--------|--------------|
| Claude Code | ✅ | Must be installed (`claude` CLI in PATH) |
| Codex | ✅ | Must be installed |
| OpenClaw | ⚠️ | Needs SSH config (host, key path) |
| Hermes | ✅ | Must be installed |

---

## Database Setup (ato-cloud)

### Quick Start

```bash
# 1. Install PostgreSQL (macOS)
brew install postgresql@15
brew services start postgresql@15

# 2. Create database
createdb ato_cloud

# 3. Run migrations
cd ato-cloud
psql ato_cloud < database/migrations/001_initial.sql
psql ato_cloud < database/migrations/002_phase3_cloud_sync.sql

# 4. Set environment variables
export DATABASE_URL="postgresql://localhost/ato_cloud"
export JWT_SECRET="your-secret-key-here-make-it-long"
export JWT_REFRESH_SECRET="another-secret-key-here"

# 5. Start services
./start.sh
```

### Production Database

For production, use a managed PostgreSQL service:
- **Supabase** (free tier available)
- **Railway** (easy deployment)
- **Neon** (serverless Postgres)
- **AWS RDS** / **Google Cloud SQL** / **Azure Database**

---

## What's NOT Production Ready

### Security Concerns

| Issue | Location | Fix Needed |
|-------|----------|------------|
| JWT secrets hardcoded for dev | ato-cloud services | Use proper secret management |
| No rate limiting | All API endpoints | Add rate limiting middleware |
| No input sanitization | Some endpoints | Add proper validation |
| CORS wide open | API Gateway | Restrict to known origins |
| No HTTPS | Local dev | Use reverse proxy with TLS |

### Missing Features

| Feature | Status | Notes |
|---------|--------|-------|
| Notification UI in desktop | ❌ | Need to build settings panel |
| Notification DB tables | ❌ | Need migration for `notification_configs` table |
| Real skill sync | 🚧 | UI exists, need to connect to skills store |
| Team activity logs UI | ❌ | Backend exists, no UI |
| Password reset flow | ❌ | Not implemented |
| Email verification | ❌ | Not implemented |

### Infrastructure

| Component | Status | Notes |
|-----------|--------|-------|
| Docker Compose | ❌ | Need to create for easy deployment |
| CI/CD for backend | ❌ | Only desktop has GitHub Actions |
| Monitoring/Logging | ❌ | No centralized logging |
| Backups | ❌ | No automated DB backups |

---

## Quick Checklist for Local Development

```
[ ] PostgreSQL installed and running
[ ] Database created and migrations run
[ ] Environment variables set
[ ] GitHub OAuth app created (optional, for GitHub login)
[ ] ato-cloud services started (./start.sh)
[ ] Desktop app pointed to local API (VITE_CLOUD_API_URL)
```

---

## Quick Checklist for Production

```
[ ] Managed PostgreSQL with backups
[ ] Proper JWT secrets in secret manager
[ ] GitHub OAuth app for production domain
[ ] HTTPS/TLS configured
[ ] Rate limiting enabled
[ ] CORS restricted to your domain
[ ] Notification providers configured
[ ] Monitoring and alerting set up
[ ] CI/CD pipeline for backend
```

---

## Files to Review

- `ato-cloud/.env.example` - Create from this template
- `ato-cloud/start.sh` - Service startup script
- `ato-cloud/database/migrations/` - SQL migrations
- `apps/desktop/.env.example` - Desktop env template (create if needed)

---

*Last updated: Phase 3.5 (Notifications structure)*
