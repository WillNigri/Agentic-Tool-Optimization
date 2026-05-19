// commands/skills_validate.rs — SKILL.md linter / validator.
//
// PR 27a of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
// First slice of the skills_mcps domain — the validator is the most
// self-contained piece (no DB, no agent dispatch, no cross-domain
// helpers beyond `estimate_tokens` + `project_root`), so it lands
// first as a clean ~200-line module. Skill CRUD (`get_local_skills`,
// `create_skill`, `update_skill`, `delete_skill`, version commands),
// MCP discovery + install, project-skills, and openclaw skills follow
// in subsequent PRs as the cross-cutting helpers (`collect_skills`,
// `McpServerDetails`, `LocalSkill` writes) find their natural homes.
//
// Scope (2 commands + 2 structs + 1 const):
//   - validate_skill          — single SKILL.md / CLAUDE.md / AGENTS.md /
//                               SOUL.md linter; checks YAML frontmatter
//                               (name, description, allowed-tools against
//                               VALID_TOOLS), body presence, token-size
//                               warnings.
//   - validate_all_skills     — scan ~/.{claude,codex,agents,hermes,openclaw}/skills
//                               + project ./{.claude,.agents,skills}/ and run
//                               validate_skill on every SKILL.md found.
//   - ValidationIssue         — { code, severity, message, line, suggestion }
//   - SkillValidation         — { path, skill_name, valid, errors, warnings,
//                                 token_count }
//   - VALID_TOOLS             — closed-list of tool names allowed in
//                               allowed-tools frontmatter.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::home_dir;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    pub code: String,     // "MISSING_FRONTMATTER", "TOKEN_SIZE_WARNING", etc.
    pub severity: String, // "error" | "warning"
    pub message: String,
    pub line: Option<u32>,
    pub suggestion: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillValidation {
    pub path: String,
    pub skill_name: Option<String>,
    pub valid: bool,
    pub errors: Vec<ValidationIssue>,
    pub warnings: Vec<ValidationIssue>,
    pub token_count: u64,
}

pub const VALID_TOOLS: &[&str] = &[
    "Bash",
    "Read",
    "Write",
    "Edit",
    "Glob",
    "Grep",
    "WebFetch",
    "WebSearch",
    "Task",
    "TodoWrite",
    "NotebookEdit",
    "AskUserQuestion",
    "Skill",
    "KillShell",
    "mcp",
    "computer",
    "text_editor",
    "browser",
    "code_execution",
];

/// Validate a single skill file
#[tauri::command]
pub fn validate_skill(path: String) -> Result<SkillValidation, String> {
    let path_buf = PathBuf::from(&path);

    if !path_buf.exists() {
        return Ok(SkillValidation {
            path: path.clone(),
            skill_name: None,
            valid: false,
            errors: vec![ValidationIssue {
                code: "FILE_NOT_FOUND".to_string(),
                severity: "error".to_string(),
                message: "File does not exist".to_string(),
                line: None,
                suggestion: Some("Create the file or check the path".to_string()),
            }],
            warnings: vec![],
            token_count: 0,
        });
    }

    let content =
        fs::read_to_string(&path_buf).map_err(|e| format!("Failed to read file: {}", e))?;

    let token_count = super::estimate_tokens(content.len() as u64);
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut skill_name: Option<String> = None;

    // Check if it's a SKILL.md or similar markdown file
    let is_skill_file = path.ends_with("SKILL.md")
        || path.ends_with("CLAUDE.md")
        || path.ends_with("AGENTS.md")
        || path.ends_with("SOUL.md");

    if is_skill_file {
        // Check for YAML frontmatter
        if content.starts_with("---") {
            let parts: Vec<&str> = content.splitn(3, "---").collect();
            if parts.len() >= 3 {
                let frontmatter = parts[1].trim();

                // Try to parse YAML
                match serde_yaml::from_str::<serde_json::Value>(frontmatter) {
                    Ok(yaml) => {
                        // Check for name field
                        if let Some(name) = yaml.get("name").and_then(|n| n.as_str()) {
                            skill_name = Some(name.to_string());
                        } else {
                            warnings.push(ValidationIssue {
                                code: "MISSING_NAME".to_string(),
                                severity: "warning".to_string(),
                                message: "Skill has no 'name' field in frontmatter".to_string(),
                                line: Some(2),
                                suggestion: Some(
                                    "Add 'name: my-skill' to frontmatter".to_string(),
                                ),
                            });
                        }

                        // Check for description field
                        if yaml.get("description").is_none() {
                            warnings.push(ValidationIssue {
                                code: "MISSING_DESCRIPTION".to_string(),
                                severity: "warning".to_string(),
                                message:
                                    "Skill has no description — agents may not understand when to use it"
                                        .to_string(),
                                line: Some(2),
                                suggestion: Some(
                                    "Add 'description: What this skill does' to frontmatter"
                                        .to_string(),
                                ),
                            });
                        }

                        // Validate allowed-tools
                        if let Some(tools) = yaml.get("allowed-tools").and_then(|t| t.as_array()) {
                            for tool in tools {
                                if let Some(tool_str) = tool.as_str() {
                                    // Extract tool name (before any parentheses for patterns)
                                    let tool_name =
                                        tool_str.split('(').next().unwrap_or(tool_str);
                                    if !VALID_TOOLS.contains(&tool_name) {
                                        errors.push(ValidationIssue {
                                            code: "INVALID_TOOL".to_string(),
                                            severity: "error".to_string(),
                                            message: format!(
                                                "Unknown tool '{}' in allowed-tools",
                                                tool_name
                                            ),
                                            line: None,
                                            suggestion: Some(format!(
                                                "Valid tools: {}",
                                                VALID_TOOLS.join(", ")
                                            )),
                                        });
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        errors.push(ValidationIssue {
                            code: "INVALID_FRONTMATTER".to_string(),
                            severity: "error".to_string(),
                            message: format!("Frontmatter YAML parse error: {}", e),
                            line: Some(2),
                            suggestion: Some("Check YAML syntax in frontmatter".to_string()),
                        });
                    }
                }

                // Check for empty content body
                let body = parts[2].trim();
                if body.is_empty() {
                    warnings.push(ValidationIssue {
                        code: "EMPTY_CONTENT".to_string(),
                        severity: "warning".to_string(),
                        message: "Skill has frontmatter but no content body".to_string(),
                        line: None,
                        suggestion: Some(
                            "Add instructions after the frontmatter".to_string(),
                        ),
                    });
                }
            } else {
                errors.push(ValidationIssue {
                    code: "INCOMPLETE_FRONTMATTER".to_string(),
                    severity: "error".to_string(),
                    message: "Frontmatter not properly closed with '---'".to_string(),
                    line: Some(1),
                    suggestion: Some("Add closing '---' after frontmatter".to_string()),
                });
            }
        } else if path.ends_with("SKILL.md") {
            errors.push(ValidationIssue {
                code: "MISSING_FRONTMATTER".to_string(),
                severity: "error".to_string(),
                message: "SKILL.md missing YAML frontmatter".to_string(),
                line: Some(1),
                suggestion: Some(
                    "Add frontmatter starting with '---' at the top".to_string(),
                ),
            });
        }
    }

    // Token size warnings
    if token_count > 15000 {
        errors.push(ValidationIssue {
            code: "TOKEN_SIZE_ERROR".to_string(),
            severity: "error".to_string(),
            message: format!(
                "Skill is ~{} tokens — too large, will consume significant context",
                token_count
            ),
            line: None,
            suggestion: Some("Split into smaller, focused skills".to_string()),
        });
    } else if token_count > 8000 {
        warnings.push(ValidationIssue {
            code: "TOKEN_SIZE_WARNING".to_string(),
            severity: "warning".to_string(),
            message: format!(
                "Skill is ~{} tokens — consider splitting for better context efficiency",
                token_count
            ),
            line: None,
            suggestion: Some(
                "Large skills reduce available context for conversation".to_string(),
            ),
        });
    }

    let valid = errors.is_empty();

    Ok(SkillValidation {
        path,
        skill_name,
        valid,
        errors,
        warnings,
        token_count,
    })
}

/// Validate all skill files across all runtimes
#[tauri::command]
pub fn validate_all_skills() -> Result<Vec<SkillValidation>, String> {
    let home = home_dir();
    let mut validations = Vec::new();

    // Skill directories to scan
    let skill_dirs = vec![
        home.join(".claude/skills"),
        home.join(".codex/skills"),
        home.join(".agents/skills"),
        home.join(".hermes/skills"),
        home.join(".openclaw/skills"),
    ];

    for dir in skill_dirs {
        if dir.exists() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let skill_md = entry.path().join("SKILL.md");
                    if skill_md.exists() {
                        if let Ok(validation) =
                            validate_skill(skill_md.to_string_lossy().to_string())
                        {
                            validations.push(validation);
                        }
                    }
                }
            }
        }
    }

    // Also check project skills
    let project = super::project_root();
    let project_skill_dirs = vec![
        project.join(".claude/skills"),
        project.join(".agents/skills"),
        project.join("skills"),
    ];

    for dir in project_skill_dirs {
        if dir.exists() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let skill_md = entry.path().join("SKILL.md");
                    if skill_md.exists() {
                        if let Ok(validation) =
                            validate_skill(skill_md.to_string_lossy().to_string())
                        {
                            validations.push(validation);
                        }
                    }
                }
            }
        }
    }

    Ok(validations)
}
