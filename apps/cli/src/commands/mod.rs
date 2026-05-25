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
