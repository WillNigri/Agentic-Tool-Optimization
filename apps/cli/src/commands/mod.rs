pub mod agents;
pub mod auth;
pub mod bridge;
pub mod evaluators;
pub mod chats;
// v2.8.x Phase A chunk 6 — `ato pro enable` / `ato pro status`
// (war-room 87E6CADF round 3, DevEx AMEND: smooth OSS→Pro upgrade).
pub mod pro;
pub mod compare;
pub mod config_changes;
pub mod conversation_close;
pub mod demo_compare;
pub mod cost;
pub mod cost_recommend;
pub mod war_rooms;
pub mod dispatch;
pub mod dispatches;
pub mod events;
pub mod files_touched;
pub mod kill;
pub mod master_key;
pub mod posts;
pub mod providers;
pub mod ratchet;
pub mod recipes;
pub mod review;
pub mod sessions;
pub mod regressions;
pub mod replay;
pub mod replays;
pub mod runs;
pub mod runtimes;
pub mod setup_path;
pub mod skills;
pub mod traces;
// v2.10.0 PR-2 — methodology CLI surface (`ato evaluations methodology …`).
// Local-first CRUD over the methodology tables shipped in v2.10 PR-1.
// Fan-out runner + composer + rubric library land in PR-3+.
pub mod methodology;
// v2.11 PR-11 — workspaces (local-first namespace primitive). Free tier
// gets a single auto-seeded "Personal" workspace; Team tier (ato-cloud)
// layers multi-user membership + sync on top of the same tables.
pub mod workspaces;
// v2.11 PR-12.5 — production_signals CLI (OSS consumer side). The
// Langfuse/Helicone ingester lives in ato-cloud; this surface accepts
// any structured trace export the customer can pipe in.
pub mod production_signals;
// v2.13 — `ato teams` thin-client. Multi-user shared agents +
// methodologies are a Team-tier feature; persistence + tier gating
// live in ato-cloud. This module is HTTP-only — no local fallback.
pub mod teams;
// v2.13 — `ato observe start/stop/status`. Thin foreground wrapper
// around the shared `ato-passive-observer` crate so headless dev
// boxes + CI runners get the same universal multi-LLM observability
// the desktop auto-starts. See [[ato-live-billing-path]].
pub mod observe;
// v2.14 — `ato loop create/list/show/edit/delete/runs`. CLI parity
// for the Loop Composer (reframed Automations) so loops are
// scriptable from headless boxes + agents over MCP. Both the CLI
// and the desktop Tauri commands write to the same `loops` /
// `loop_runs` / `loop_run_steps` tables in ~/.ato/local.db.
pub mod loops;
