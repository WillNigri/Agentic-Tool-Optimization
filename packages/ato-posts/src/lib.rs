// ato-posts — shared activity-feed types for ATO desktop + CLI.
//
// Phase 5 introduces the activity feed: a single chronological stream
// where humans, agents, and the system itself post. Recipes can post
// (NotifyHuman action). Users post via `ato posts add` or the GUI.
// Agents post when something happens that a human should see.
//
// Like ato-recipes, this crate is intentionally tiny — pure types +
// validation, no I/O. Storage lives in the desktop's posts.rs;
// formatting lives in the CLI's commands/posts.rs.

use serde::{Deserialize, Serialize};

/// Who's speaking. Drives icon/color/treatment in the GUI and keeps
/// system events visually distinct from human messages.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PostAuthorKind {
    /// The local user typing in the GUI or running `ato posts add`.
    Human,
    /// A named agent (codex-reviewer, triage, etc.) posting on its
    /// own behalf — usually via the recipes engine's DispatchAgent
    /// + NotifyHuman chain.
    Agent,
    /// The platform itself: "regression detected", "replay finished",
    /// engine startup, recipe runs. Distinct from Agent so users can
    /// filter "what did the system tell me" vs "what did my agents tell me".
    System,
}

/// What kind of post this is. Recipes are encouraged to use specific
/// kinds rather than always "message" so the GUI can render lists
/// differently (e.g. approval requests get an "approve / dismiss" UI).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PostKind {
    /// A plain text post. Most common.
    Message,
    /// "X happened" — usually emitted by the recipes engine when an
    /// event fires (regression_detected, dispatch_failed, etc.).
    EventNotice,
    /// The post is asking for a yes/no decision. The GUI shows action
    /// buttons; replies become ApprovalDecision posts.
    ApprovalRequest,
    /// A reply to an ApprovalRequest. payload carries the decision +
    /// the request_post_id this is responding to.
    ApprovalDecision,
}

/// A single row in the activity feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Post {
    pub id: String,
    pub created_at: String, // RFC3339
    pub author_kind: PostAuthorKind,
    /// Optional author identity. For Human, often None ("you"). For
    /// Agent, the agent slug. For System, often "ato" or the recipe
    /// slug that triggered the post.
    pub author_slug: Option<String>,
    pub kind: PostKind,
    pub text: String,
    /// When the post was auto-generated from an AtoEvent, the
    /// originating event's sequence number — lets the GUI link a
    /// notice back to its event_log row.
    pub related_event_seq: Option<i64>,
    /// Optional structured payload (JSON). Approval kinds carry their
    /// decision context here; agent posts may carry the agent's full
    /// response if the user wants to expand "show full response".
    pub payload: Option<serde_json::Value>,
}

/// Input for creating a new post. id + created_at are assigned by the
/// storage layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePostInput {
    pub author_kind: PostAuthorKind,
    pub author_slug: Option<String>,
    #[serde(default = "default_kind")]
    pub kind: PostKind,
    pub text: String,
    #[serde(default)]
    pub related_event_seq: Option<i64>,
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
}

fn default_kind() -> PostKind {
    PostKind::Message
}

/// Maximum bytes allowed in a single post. Longer messages should
/// either be split into multiple posts or attached as a separate
/// artifact. 4096 is generous for a feed entry (Slack messages cap
/// at 40k, X posts at 280 chars, GitHub PR titles at 256).
pub const MAX_TEXT_BYTES: usize = 4096;

/// Validate post text.
///
/// Codex round-1 5.1 caught two real issues:
///   - The previous version allowed embedded newlines and control
///     characters. A post like `"first\nsecond\x1b[31mRED"` breaks
///     CLI list rendering and lets attackers (or agents) inject
///     terminal escape sequences. We now allow `\n` (legitimate
///     multi-line text) but reject other C0 controls (\t, \r,
///     ANSI escape, etc.).
///   - The doc claimed "very long posts are truncated by storage"
///     but neither layer truncated. Now enforced here: messages
///     over MAX_TEXT_BYTES are rejected at write time so storage
///     and consumers can assume a bound.
pub fn validate_text(text: &str) -> Result<(), String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("post text must not be empty".to_string());
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(format!(
            "post text exceeds {} bytes ({} given)",
            MAX_TEXT_BYTES,
            text.len()
        ));
    }
    for c in text.chars() {
        // Allow LF; reject every other control character. The most
        // dangerous case is \x1b (ESC) which starts ANSI sequences
        // that can move the cursor, change colors, or impersonate
        // a prompt in the rendered list.
        if c == '\n' {
            continue;
        }
        if c.is_control() {
            return Err(format!(
                "post text contains a disallowed control character (U+{:04X}); only newlines are permitted",
                c as u32
            ));
        }
    }
    Ok(())
}

/// Validate an author_slug (when present). Same shape as recipe slug:
/// lowercase alphanumerics + hyphens, 1-64 chars. Helps the GUI build
/// consistent "@agent-name" rendering without surprises.
pub fn validate_author_slug(slug: &str) -> Result<(), String> {
    if slug.is_empty() || slug.len() > 64 {
        return Err("author_slug must be 1-64 characters".to_string());
    }
    let bytes = slug.as_bytes();
    if !bytes[0].is_ascii_alphanumeric() {
        return Err("author_slug must start with a letter or digit".to_string());
    }
    for &b in bytes {
        let ok = b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-';
        if !ok {
            return Err(format!(
                "author_slug may only contain lowercase letters, digits, hyphens; got '{}'",
                slug
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_text_rejects_empty() {
        assert!(validate_text("").is_err());
        assert!(validate_text("   ").is_err());
        assert!(validate_text("hi").is_ok());
    }

    #[test]
    fn validate_slug_shape() {
        assert!(validate_author_slug("ato").is_ok());
        assert!(validate_author_slug("codex-reviewer").is_ok());
        assert!(validate_author_slug("").is_err());
        assert!(validate_author_slug("-bad").is_err());
        assert!(validate_author_slug("Has Caps").is_err());
        assert!(validate_author_slug("../escape").is_err());
    }
}
