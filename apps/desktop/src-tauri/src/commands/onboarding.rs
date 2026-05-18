// commands/onboarding.rs — first-run runtime onboarding checklist.
//
// PR 8 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope:
//   - `get_onboarding_status`  — per-runtime checklist (CLI installed,
//     auth configured, settings files present, project config, at least
//     one skill) used by the Home page's runtime card.
//
// Plus the structs that shape its response (OnboardingAction /
// OnboardingItem / OnboardingStatus) and the `which_sync` helper that
// nothing else calls today. If a future PR finds another caller for
// `which_sync`, promote it to commands/shared.rs.
//
// `home_dir` lives in crate::lib.rs and is reused widely; `project_root`
// lives in commands/mod.rs. Both are pub and reachable from here via
// `crate::home_dir()` and `super::project_root()` respectively.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingAction {
    pub action_type: String,    // "create_file" | "open_editor" | "run_command" | "external_link"
    pub target: String,         // Path or URL
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingItem {
    pub id: String,
    pub label: String,
    pub completed: bool,
    pub action: Option<OnboardingAction>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingStatus {
    pub runtime: String,
    pub items: Vec<OnboardingItem>,
    pub completion_percent: u8,
}

use crate::home_dir;

// ══════════════════════════════════════════════════════════════════════════════
// FEATURE 2: Onboarding Checklist
// ══════════════════════════════════════════════════════════════════════════════

/// Get onboarding status for a specific runtime
#[tauri::command]
pub fn get_onboarding_status(runtime: String) -> Result<OnboardingStatus, String> {
    let home = home_dir();
    let project = super::project_root();
    let mut items = Vec::new();

    match runtime.as_str() {
        "claude" => {
            // Check CLI installed
            let cli_installed = which_sync("claude").is_some();
            items.push(OnboardingItem {
                id: "cli_installed".to_string(),
                label: "Claude Code CLI installed".to_string(),
                completed: cli_installed,
                action: if cli_installed { None } else { Some(OnboardingAction {
                    action_type: "external_link".to_string(),
                    target: "https://docs.anthropic.com/en/docs/claude-code".to_string(),
                }) },
            });

            // Check authenticated
            let claude_json = home.join(".claude.json");
            let has_auth = claude_json.exists() && fs::read_to_string(&claude_json)
                .map(|c| c.contains("oauth") || c.contains("apiKey"))
                .unwrap_or(false);
            items.push(OnboardingItem {
                id: "authenticated".to_string(),
                label: "Authenticated (API key or OAuth)".to_string(),
                completed: has_auth,
                action: if has_auth { None } else { Some(OnboardingAction {
                    action_type: "run_command".to_string(),
                    target: "claude auth".to_string(),
                }) },
            });

            // Check settings.json exists
            let settings = home.join(".claude/settings.json");
            items.push(OnboardingItem {
                id: "settings_created".to_string(),
                label: "Created ~/.claude/settings.json".to_string(),
                completed: settings.exists(),
                action: if settings.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: settings.to_string_lossy().to_string(),
                }) },
            });

            // Check CLAUDE.md exists in project
            let claude_md = project.join("CLAUDE.md");
            items.push(OnboardingItem {
                id: "project_config".to_string(),
                label: "Created CLAUDE.md for project".to_string(),
                completed: claude_md.exists(),
                action: if claude_md.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: claude_md.to_string_lossy().to_string(),
                }) },
            });

            // Check at least one skill
            let skills_dir = home.join(".claude/skills");
            let has_skills = skills_dir.exists() && fs::read_dir(&skills_dir)
                .map(|entries| entries.count() > 0)
                .unwrap_or(false);
            items.push(OnboardingItem {
                id: "has_skill".to_string(),
                label: "Added at least one skill".to_string(),
                completed: has_skills,
                action: if has_skills { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: skills_dir.join("my-skill/SKILL.md").to_string_lossy().to_string(),
                }) },
            });
        }
        "codex" => {
            // Check CLI installed
            let cli_installed = which_sync("codex").is_some();
            items.push(OnboardingItem {
                id: "cli_installed".to_string(),
                label: "Codex CLI installed".to_string(),
                completed: cli_installed,
                action: if cli_installed { None } else { Some(OnboardingAction {
                    action_type: "external_link".to_string(),
                    target: "https://github.com/openai/codex".to_string(),
                }) },
            });

            // Check OPENAI_API_KEY
            let has_api_key = std::env::var("OPENAI_API_KEY").is_ok();
            items.push(OnboardingItem {
                id: "api_key".to_string(),
                label: "OPENAI_API_KEY environment variable set".to_string(),
                completed: has_api_key,
                action: if has_api_key { None } else { Some(OnboardingAction {
                    action_type: "external_link".to_string(),
                    target: "https://platform.openai.com/api-keys".to_string(),
                }) },
            });

            // Check config.toml
            let config = home.join(".codex/config.toml");
            items.push(OnboardingItem {
                id: "config_created".to_string(),
                label: "Created ~/.codex/config.toml".to_string(),
                completed: config.exists(),
                action: if config.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: config.to_string_lossy().to_string(),
                }) },
            });

            // Check AGENTS.md
            let agents_md = project.join("AGENTS.md");
            items.push(OnboardingItem {
                id: "project_config".to_string(),
                label: "Created AGENTS.md for project".to_string(),
                completed: agents_md.exists(),
                action: if agents_md.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: agents_md.to_string_lossy().to_string(),
                }) },
            });

            // Check skills
            let skills_dir = home.join(".agents/skills");
            let has_skills = skills_dir.exists() && fs::read_dir(&skills_dir)
                .map(|entries| entries.count() > 0)
                .unwrap_or(false);
            items.push(OnboardingItem {
                id: "has_skill".to_string(),
                label: "Added at least one skill".to_string(),
                completed: has_skills,
                action: if has_skills { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: skills_dir.join("my-skill/SKILL.md").to_string_lossy().to_string(),
                }) },
            });
        }
        "hermes" => {
            // Check CLI installed
            let cli_installed = which_sync("hermes").is_some();
            items.push(OnboardingItem {
                id: "cli_installed".to_string(),
                label: "Hermes installed".to_string(),
                completed: cli_installed,
                action: if cli_installed { None } else { Some(OnboardingAction {
                    action_type: "external_link".to_string(),
                    target: "https://github.com/hermes-ai/hermes".to_string(),
                }) },
            });

            // Check config.yaml
            let config = home.join(".hermes/config.yaml");
            items.push(OnboardingItem {
                id: "config_created".to_string(),
                label: "Created ~/.hermes/config.yaml".to_string(),
                completed: config.exists(),
                action: if config.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: config.to_string_lossy().to_string(),
                }) },
            });

            // Check SOUL.md
            let soul_md = project.join("SOUL.md");
            items.push(OnboardingItem {
                id: "soul_created".to_string(),
                label: "Created SOUL.md".to_string(),
                completed: soul_md.exists(),
                action: if soul_md.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: soul_md.to_string_lossy().to_string(),
                }) },
            });

            // Check memories directory
            let memories = home.join(".hermes/memories");
            items.push(OnboardingItem {
                id: "memories_setup".to_string(),
                label: "Set up memories/ directory".to_string(),
                completed: memories.exists(),
                action: if memories.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: memories.join("MEMORY.md").to_string_lossy().to_string(),
                }) },
            });
        }
        "openclaw" => {
            // Check gateway config
            let config = home.join(".openclaw/openclaw.json");
            let config_valid = config.exists() && fs::read_to_string(&config)
                .map(|c| c.contains("gateway"))
                .unwrap_or(false);
            items.push(OnboardingItem {
                id: "gateway_configured".to_string(),
                label: "OpenClaw gateway configured".to_string(),
                completed: config_valid,
                action: if config_valid { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: config.to_string_lossy().to_string(),
                }) },
            });

            // Check SOUL.md
            let soul_md = project.join("SOUL.md");
            items.push(OnboardingItem {
                id: "soul_created".to_string(),
                label: "Created workspace SOUL.md".to_string(),
                completed: soul_md.exists(),
                action: if soul_md.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: soul_md.to_string_lossy().to_string(),
                }) },
            });

            // Check TOOLS.md
            let tools_md = project.join("TOOLS.md");
            items.push(OnboardingItem {
                id: "tools_created".to_string(),
                label: "Added TOOLS.md".to_string(),
                completed: tools_md.exists(),
                action: if tools_md.exists() { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: tools_md.to_string_lossy().to_string(),
                }) },
            });

            // Check skills
            let skills_dir = home.join(".openclaw/skills");
            let has_skills = skills_dir.exists() && fs::read_dir(&skills_dir)
                .map(|entries| entries.count() > 0)
                .unwrap_or(false);
            items.push(OnboardingItem {
                id: "has_skill".to_string(),
                label: "Added at least one skill".to_string(),
                completed: has_skills,
                action: if has_skills { None } else { Some(OnboardingAction {
                    action_type: "create_file".to_string(),
                    target: skills_dir.join("my-skill/SKILL.md").to_string_lossy().to_string(),
                }) },
            });
        }
        _ => {}
    }

    let completed_count = items.iter().filter(|i| i.completed).count();
    let total = items.len();
    let completion_percent = if total > 0 {
        ((completed_count as f32 / total as f32) * 100.0) as u8
    } else {
        0
    };

    Ok(OnboardingStatus {
        runtime,
        items,
        completion_percent,
    })
}

/// Helper to check if a command exists in PATH
pub fn which_sync(cmd: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .filter_map(|dir| {
                let full_path = dir.join(cmd);
                if full_path.is_file() {
                    Some(full_path)
                } else {
                    None
                }
            })
            .next()
    })
}
