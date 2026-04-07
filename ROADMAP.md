# ATO Roadmap

## Released

### v0.3.0 — Multi-LLM Platform
- Multi-runtime support: Claude Code, Codex, OpenClaw, Hermes
- Two-way communication with all runtimes (send prompts + get status)
- Skills Manager with per-runtime tabs and recursive scanning
- Skills Marketplace (browse, install, publish, share, auto-improve)
- AI-powered skill creation with in-app approval dialog
- Automation Builder with auto-detection from skill content
- Cron Monitor with Google Calendar view and click-to-inspect
- Per-runtime Context Visualizer (skills shown as on-demand)
- Setup Wizard for first-time runtime configuration
- Subagents Manager with runtime selection
- MCP server with 8 tools including runtime status
- GitHub Actions CI for macOS, Windows, Linux
- i18n: English, Portuguese, Spanish

### v0.4.0 — Monitoring & Analytics
- Real-time log viewer with file watcher
- Background health polling for all runtimes
- Usage analytics dashboard with execution metrics
- Latency/uptime charts per runtime
- Cost tracking per runtime with burn rate visualization

### v0.5.0 — Cloud Sync & Collaboration
- Cloud backend (ato-cloud) with PostgreSQL
- GitHub OAuth login
- Team workspaces with shared skill libraries
- Team member management (invite, roles, permissions)
- Team skills sharing and collaboration
- Activity logs for audit trail
- Skill sync across devices

### v0.6.0 — Deeper Runtime Integration
- Live context tracking from runtime session logs (reads Claude session JSONL)
- Real MCP tool discovery (JSON-RPC protocol to running MCP servers)
- Config editor with write support (FileViewer with save functionality)
- Hooks read/write from actual settings files (HooksManager + Tauri commands)

### v0.7.0 — Marketplace Backend
- Marketplace service with PostgreSQL schema
- Skill submissions with versioning (semver)
- Search, filter, and discovery endpoints
- Ratings and reviews with helpfulness voting
- Skill packs (collections) with import/export as JSON
- Update notifications for installed skills

### v0.8.0 — Advanced Automation
- Webhook triggers (inbound) with path/method/secret configuration
- Parallel node execution with group tracking
- Error handling nodes (try-catch, retry with exponential backoff)
- Variables and data passing between nodes (set, get, transform, jq expressions)
- Workflow templates (4 built-in: Webhook to Slack, Parallel Deploy, Error Handling, Data Transform)
- New node types: parallel, try-catch, retry, variable, template
- Enhanced execution state with runId, trigger payload, parallel groups, retry tracking

### v0.5.5 — Notifications & Integrations
- Notifications service with provider abstraction (Tauri backend)
- Slack webhook integration (Block Kit formatting)
- Discord webhook integration (embed support)
- Telegram bot integration (Markdown formatting)
- Email notifications (SMTP - placeholder, requires lettre crate)
- Notification preferences per event type (8 event types)
- Desktop UI for managing notification channels (existing component, now connected to backend)
- SQLite persistence for channel configurations
- Test notification functionality

---

## Upcoming

### v1.0.0 — Production Ready
- Apple code signing & notarization
- Windows code signing
- Auto-updater via GitHub releases
- Documentation site
- Plugin API for third-party extensions

---

## Future Runtime Support

As new AI coding agents emerge:
- Cursor
- Windsurf / Codeium
- Aider
- Continue.dev
- Custom agents via plugin API
