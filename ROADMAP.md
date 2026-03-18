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

---

## Upcoming

### v0.4.0 — Monitoring & Analytics
- Live cron execution tracking
- Usage analytics dashboard across all runtimes
- Cost tracking per runtime with burn rate visualization
- Execution replay for failed jobs

### v0.5.0 — Cloud Sync & Collaboration
- Cloud backend for syncing skills, workflows, and cron jobs across machines
- GitHub OAuth login
- Team workspaces with shared skill libraries

### v0.6.0 — Deeper Runtime Integration
- Live context tracking from runtime session logs
- Real MCP tool discovery (connect to running MCP servers)
- Config editor with write support (edit settings from the app)
- Hooks read/write from actual settings files

### v0.7.0 — Marketplace Backend
- Real skill submissions and discovery
- Skill ratings and reviews
- Versioning and update notifications
- Import/export skill packs

### v0.8.0 — Advanced Automation
- Webhook triggers
- Parallel node execution
- Error handling nodes (try/catch/retry per step)
- Variables and data passing between nodes
- Workflow templates

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
