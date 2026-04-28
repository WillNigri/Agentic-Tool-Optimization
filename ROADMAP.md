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

### v1.0.0 — Production Ready (Released April 2026)
- SDK (`@ato-sdk/js`), web dashboard, cost tracking
- LLM API key management, audit logging, agent monitor
- SSO, rate limiting, Homebrew tap

### v1.1.0 — Projects Dashboard + Multi-Runtime (Released April 2026)
- Projects Dashboard with 7 Claude sections + multi-runtime switcher
- 6 runtimes: Claude Code, Codex/OpenAI Agents SDK, Gemini CLI/ADK, OpenClaw, Hermes
- Ollama provider: auto-detect, model picker, copy endpoint
- CodeMirror 6 editor with conflict detection, auto-backup, audit logging
- Sandbox config + approval policies (editable with write-back)
- File watcher, token chart, backup/restore, i18n (EN/PT/ES)
- 46 tests (35 Rust + 11 frontend), CI/CD, code splitting

### v1.2.0 — Agent Command Center (In Progress)
- Visual workspace canvas: drag nodes, zoom in/out, pan
- Live execution visualization: agent activity pulses nodes, animated edge dots
- Skill palette: drag-to-install from marketplace with suggestions
- Command palette (⌘K): search nodes, skills, actions
- Multi-select batch operations on skill nodes
- Grid + Canvas dual view mode
- Strategy game-inspired UX: semantic zoom, animated transitions

---

## Upcoming

### v1.3.0 — Multiplayer + Teams
- Real-time collaborative workspace (WebSocket via ato-cloud)
- Team cursors on canvas (Figma-style)
- Shared workspace layouts
- Cross-runtime policy enforcement templates

### v1.4.0 — Intelligence Layer
- Proactive suggestions ("Your project is missing X")
- Cost optimization alerts from SDK traces
- Agent performance benchmarking across runtimes

---

## Future Runtime Support

As new AI coding agents emerge:
- Cursor
- Windsurf / Codeium
- Aider
- Continue.dev
- Custom agents via plugin API
