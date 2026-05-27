// Per-CLI source registry. Adding a new runtime here = adding it to
// the universal observability tier; the parser lives in its own
// `parser_<runtime>.rs` module and dispatches off the SourceKind
// discriminant in `worker.rs`.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceKind {
    ClaudeCode,
    Codex,
    Gemini,
}

impl SourceKind {
    pub fn id(&self) -> &'static str {
        match self {
            SourceKind::ClaudeCode => "claude_code",
            SourceKind::Codex => "codex",
            SourceKind::Gemini => "gemini",
        }
    }
    /// Reuses ATO's existing runtime taxonomy so the History panel
    /// renders observed runs alongside ATO's own dispatches without
    /// needing a separate icon registry.
    pub fn runtime(&self) -> &'static str {
        match self {
            SourceKind::ClaudeCode => "claude",
            SourceKind::Codex => "codex",
            SourceKind::Gemini => "gemini",
        }
    }
    /// Conservative honest default — we can't introspect the upstream
    /// process's env vars from a passive observer, so we assume the
    /// CLI's primary billing path (subscription) per CLI. The
    /// active-dispatch path's `auth_mode` column is the real signal
    /// for ATO's own dispatches; this default labels the *other CLI's*
    /// runs with their most-common credential path.
    pub fn default_billing_surface(&self) -> &'static str {
        match self {
            SourceKind::ClaudeCode => "claude_code_subscription",
            SourceKind::Codex => "codex_cli_subscription",
            SourceKind::Gemini => "gemini_cli_subscription",
        }
    }
    /// CLI-friendly name accepted on `--runtime claude,codex,gemini`.
    pub fn from_cli_token(s: &str) -> Option<SourceKind> {
        match s.trim().to_lowercase().as_str() {
            "claude" | "claude_code" | "claude-code" => Some(SourceKind::ClaudeCode),
            "codex" => Some(SourceKind::Codex),
            "gemini" => Some(SourceKind::Gemini),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Source {
    pub kind: SourceKind,
    pub root: PathBuf,
}

/// Probe known runtime session directories under $HOME. Returns only
/// the ones that actually exist on disk; a missing dir is treated as
/// "user hasn't installed that CLI yet" — silent skip, not an error.
pub fn discover_sources(home: &Path) -> Vec<Source> {
    let mut out = Vec::new();
    let claude_dir = home.join(".claude").join("projects");
    if claude_dir.exists() {
        out.push(Source { kind: SourceKind::ClaudeCode, root: claude_dir });
    }
    let codex_dir = home.join(".codex").join("sessions");
    if codex_dir.exists() {
        out.push(Source { kind: SourceKind::Codex, root: codex_dir });
    }
    // v2.13 — Gemini CLI session paths. Two layouts in the wild:
    //   1. ~/.gemini/sessions/<session-id>/log.jsonl (newer builds)
    //   2. ~/.gemini/tmp/<session-id>/logs.json (older Code Assist)
    // We watch ~/.gemini recursively so either lands. Filter by file
    // shape in `is_session_file`, not by directory.
    let gemini_dir = home.join(".gemini");
    if gemini_dir.exists() {
        out.push(Source { kind: SourceKind::Gemini, root: gemini_dir });
    }
    out
}

// File-name patterns below are brittle: they encode the current
// (2026-05-26) naming convention of three third-party CLIs we don't
// control. If any of Claude Code, Codex, or Gemini changes its
// session-log naming, the watcher will silently skip new files for
// that runtime — per review MEDIUM-7 (gemini reviewer).
//
// Detection strategy: re-verify each release of the upstream CLIs
// (smoke-test via `ato observe start` on a fresh session). When a
// drift is detected, add the new pattern alongside the old (don't
// replace) so installs that haven't upgraded keep working.
pub fn is_session_file(kind: SourceKind, path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    match kind {
        SourceKind::ClaudeCode => name.ends_with(".jsonl"),
        SourceKind::Codex => name.ends_with(".jsonl") && name.starts_with("rollout-"),
        // Gemini CLI writes session events to logs.json (older) or
        // log.jsonl (newer). Accept both; the parser deals with
        // line-delimited and bracketed-array variants.
        SourceKind::Gemini => {
            name == "logs.json" || name == "log.jsonl" || name == "history.jsonl"
        }
    }
}

pub fn enumerate_existing(src: &Source) -> Vec<PathBuf> {
    let mut out = Vec::new();
    visit_jsonls(&src.root, src.kind, &mut out);
    out
}

fn visit_jsonls(dir: &Path, kind: SourceKind, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            visit_jsonls(&path, kind, out);
        } else if ft.is_file() && is_session_file(kind, &path) {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_code_matches_jsonl() {
        let p = PathBuf::from("/home/me/.claude/projects/x/abcd.jsonl");
        assert!(is_session_file(SourceKind::ClaudeCode, &p));
        let p = PathBuf::from("/home/me/.claude/projects/x/index.json");
        assert!(!is_session_file(SourceKind::ClaudeCode, &p));
    }

    #[test]
    fn codex_matches_rollout_only() {
        let p = PathBuf::from("/home/me/.codex/sessions/2026/05/06/rollout-x.jsonl");
        assert!(is_session_file(SourceKind::Codex, &p));
        let p = PathBuf::from("/home/me/.codex/sessions/2026/05/06/notes.jsonl");
        assert!(!is_session_file(SourceKind::Codex, &p));
    }

    #[test]
    fn gemini_matches_logs_variants() {
        assert!(is_session_file(
            SourceKind::Gemini,
            &PathBuf::from("/home/me/.gemini/tmp/abc/logs.json")
        ));
        assert!(is_session_file(
            SourceKind::Gemini,
            &PathBuf::from("/home/me/.gemini/sessions/abc/log.jsonl")
        ));
        assert!(!is_session_file(
            SourceKind::Gemini,
            &PathBuf::from("/home/me/.gemini/settings.json")
        ));
    }

    #[test]
    fn cli_token_parsing() {
        assert_eq!(SourceKind::from_cli_token("claude"), Some(SourceKind::ClaudeCode));
        assert_eq!(SourceKind::from_cli_token("CLAUDE-CODE"), Some(SourceKind::ClaudeCode));
        assert_eq!(SourceKind::from_cli_token("codex"), Some(SourceKind::Codex));
        assert_eq!(SourceKind::from_cli_token("gemini"), Some(SourceKind::Gemini));
        assert_eq!(SourceKind::from_cli_token("aider"), None);
    }
}
