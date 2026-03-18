# ATO Roadmap

## Completed (v0.3.0)

- [x] Multi-LLM platform: Claude Code, Codex, OpenClaw, Hermes
- [x] Two-way communication (send prompts + get status) for all runtimes
- [x] Skills Manager with per-runtime tabs and recursive scanning
- [x] Skills Marketplace (browse, install, publish, share, auto-improve)
- [x] AI-powered skill creation with in-app approval dialog
- [x] Automation Builder with auto-detection from skill Step/Phase headers
- [x] Cron Monitor with calendar view and click-to-inspect
- [x] Per-runtime Context Visualizer (skills shown as on-demand)
- [x] Setup Wizard for first-time runtime configuration
- [x] Prompt Bar with runtime selector
- [x] Subagents Manager with runtime selection
- [x] MCP server with 8 tools including runtime status
- [x] gstack compatibility
- [x] No mock data in production
- [x] GitHub Actions CI for macOS, Windows, Linux
- [x] i18n: English, Portuguese, Spanish

---

## v0.4.0 — Real-Time Monitoring (Pro)

**Goal**: Ship the paid monitoring features that justify a Pro subscription.

- [ ] Live cron execution tracking (websocket status updates)
- [ ] Push notifications for silent failures (Slack, email, desktop)
- [ ] Usage analytics dashboard across all runtimes
- [ ] Cost tracking per runtime with burn rate alerts
- [ ] Execution replay (re-run failed jobs with same inputs)
- [ ] SLA tracking (uptime percentage per cron job)
- [ ] Alert escalation policies (if no ack in X minutes, notify Y)

---

## v0.5.0 — Cloud Sync & Teams

**Goal**: Enable multi-device and team usage.

- [ ] Cloud backend (Railway/Vercel) for sync
- [ ] GitHub OAuth login
- [ ] Sync skills, workflows, cron jobs across machines
- [ ] Team workspaces with role-based access
- [ ] Shared skill libraries within teams
- [ ] Activity feed (who changed what, when)
- [ ] Audit log for compliance

---

## v0.6.0 — Deeper Runtime Integration

**Goal**: Read real runtime state instead of estimating.

- [ ] Parse Claude Code session transcripts for live context tracking
- [ ] Codex session log parsing
- [ ] OpenClaw remote log aggregation via SSH
- [ ] Hermes memory/session state reading
- [ ] Real MCP tool discovery (connect to running MCP servers)
- [ ] Hooks read/write from actual settings.json (not just UI state)
- [ ] Config editor with write support (edit settings.json from the app)

---

## v0.7.0 — Marketplace & Community

**Goal**: Make the marketplace real (not just mock catalog).

- [ ] Marketplace backend with real skill submissions
- [ ] Skill ratings and reviews from users
- [ ] Verified publisher badges
- [ ] Skill versioning and update notifications
- [ ] Import/export skill packs (.ato-pack format)
- [ ] Skill templates for common patterns
- [ ] Community-contributed automation flows

---

## v0.8.0 — Advanced Automation

**Goal**: Make the automation builder production-grade.

- [ ] Webhook triggers (receive HTTP POST to start workflows)
- [ ] Parallel node execution (fan-out/fan-in)
- [ ] Error handling nodes (try/catch/retry per step)
- [ ] Variables and data passing between nodes
- [ ] Workflow templates from marketplace
- [ ] Workflow versioning and rollback
- [ ] Schedule workflows via cron (link automation + cron)
- [ ] Conditional triggers (run only if file changed, PR opened, etc.)

---

## v1.0.0 — Production Ready

**Goal**: Stable release for daily production use.

- [ ] Apple code signing & notarization (no "app is damaged" warning)
- [ ] Windows code signing (EV certificate)
- [ ] Auto-updater (Tauri built-in updater with GitHub releases)
- [ ] Crash reporting and telemetry (opt-in)
- [ ] Comprehensive test suite (unit + integration)
- [ ] Performance profiling (startup time, memory usage)
- [ ] Documentation site (docs.ato.dev or similar)
- [ ] Plugin API for third-party extensions

---

## Platform Priorities

### Code Signing (High Priority)

**Apple** ($99/year):
1. Enroll in Apple Developer Program
2. Create Developer ID Application certificate
3. Export as .p12, add to GitHub Secrets
4. Create app-specific password for notarization
5. Add `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` to repo secrets

**Windows** (~$200-400/year):
1. Get EV code signing certificate from DigiCert/Sectigo
2. Add to GitHub Secrets
3. Eliminates SmartScreen warning

### New Runtime Support

As new AI coding agents emerge, ATO should support them:
- [ ] Cursor (if CLI becomes available)
- [ ] Windsurf/Codeium
- [ ] Aider
- [ ] Continue.dev
- [ ] Custom/self-hosted agents via plugin API

---

## Business Milestones

| Milestone | Target |
|-----------|--------|
| GitHub stars | 1,000 |
| Desktop downloads | 5,000 |
| MCP server installs | 2,000 |
| Pro subscribers | 100 |
| Team workspaces | 20 |
| Marketplace skills | 50 community-submitted |
