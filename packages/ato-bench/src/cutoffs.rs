//! Vendor-stated model cutoffs used by the contamination classifier.
//!
//! This registry is factual passthrough, not curation: dates are recorded
//! exactly as vendors state them, with vendor granularity preserved. Models
//! with no vendor-stated cutoff are omitted rather than guessed; lookup then
//! returns `None`, which keeps contamination classification at `Unknown`.
//!
//! Verified omissions as of 2026-07-02: `grok-2-1212`, `grok-2-latest`, all
//! `deepseek-*`, all `glm-*`, all `qwen-*`, and all `MiniMax-*` models. No
//! vendor statement means omit, never guess.

use serde::{Deserialize, Serialize};

/// What kind of cutoff the vendor states. Knowledge cutoff is the latest data
/// the vendor says the model reliably knows; training-data cutoff is the later
/// bound of what could be in training. For contamination we treat both as the
/// conservative bound as-stated; the kind is recorded for transparency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CutoffKind {
    TrainingData,
    Knowledge,
}

impl CutoffKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TrainingData => "training_data",
            Self::Knowledge => "knowledge",
        }
    }
}

/// One vendor-stated cutoff. Every field is auditable: date as the vendor
/// states it (vendor granularity kept — "2025-01" not "2025-01-31"), the
/// vendor source URL, and the date we verified that source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CutoffEntry {
    pub model: &'static str,
    pub cutoff: &'static str,
    pub kind: CutoffKind,
    pub source: &'static str,
    pub verified: &'static str,
}

const VERIFIED: &str = "2026-07-02";
const ANTHROPIC_SOURCE: &str =
    "https://platform.claude.com/docs/en/about-claude/models/overview";
const OPENAI_GPT5_SOURCE: &str = "https://developers.openai.com/api/docs/models/gpt-5";
const OPENAI_GPT41_SOURCE: &str = "https://developers.openai.com/api/docs/models/gpt-4.1";
const OPENAI_GPT41_MINI_SOURCE: &str =
    "https://developers.openai.com/api/docs/models/gpt-4.1-mini";
const OPENAI_GPT41_NANO_SOURCE: &str =
    "https://developers.openai.com/api/docs/models/gpt-4.1-nano";
const OPENAI_GPT4O_SOURCE: &str = "https://developers.openai.com/api/docs/models/gpt-4o";
const OPENAI_GPT4O_MINI_SOURCE: &str =
    "https://developers.openai.com/api/docs/models/gpt-4o-mini";
const OPENAI_O3_SOURCE: &str = "https://developers.openai.com/api/docs/models/o3";
const OPENAI_O3_MINI_SOURCE: &str = "https://developers.openai.com/api/docs/models/o3-mini";
const XAI_GROK3_SOURCE: &str = "https://docs.x.ai/developers/models";
const GOOGLE_GEMINI_15_PRO_SOURCE: &str = "https://web.archive.org/web/20250530060050/https://cloud.google.com/vertex-ai/generative-ai/docs/models/gemini/1-5-pro";
const GOOGLE_GEMINI_15_FLASH_SOURCE: &str = "https://web.archive.org/web/20250523044135/https://cloud.google.com/vertex-ai/generative-ai/docs/models/gemini/1-5-flash";
const GOOGLE_GEMINI_20_FLASH_ARCHIVE_SOURCE: &str =
    "https://web.archive.org/web/20250318093607/https://ai.google.dev/gemini-api/docs/models";
const GOOGLE_GEMINI_20_FLASH_LITE_SOURCE: &str =
    "https://docs.cloud.google.com/vertex-ai/generative-ai/docs/models/gemini/2-0-flash-lite";
const GOOGLE_GEMINI_25_PRO_SOURCE: &str =
    "https://ai.google.dev/gemini-api/docs/models/gemini-2.5-pro";
const GOOGLE_GEMINI_25_FLASH_SOURCE: &str =
    "https://ai.google.dev/gemini-api/docs/models/gemini-2.5-flash";
const GOOGLE_GEMINI_25_FLASH_LITE_SOURCE: &str =
    "https://ai.google.dev/gemini-api/docs/models/gemini-2.5-flash-lite";
const GOOGLE_GEMINI_3_PRO_PREVIEW_SOURCE: &str =
    "https://ai.google.dev/gemini-api/docs/models/gemini-3-pro-preview";
const GOOGLE_GEMINI_3_PRO_SOURCE: &str =
    "https://storage.googleapis.com/deepmind-media/Model-Cards/Gemini-3-Pro-Model-Card.pdf";
const GOOGLE_GEMINI_3_FLASH_PREVIEW_SOURCE: &str =
    "https://ai.google.dev/gemini-api/docs/models/gemini-3-flash-preview";
const GOOGLE_GEMINI_3_FLASH_SOURCE: &str =
    "https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/gemini/3-flash";
const GOOGLE_GEMINI_31_PRO_PREVIEW_SOURCE: &str =
    "https://ai.google.dev/gemini-api/docs/models/gemini-3.1-pro-preview";
const GOOGLE_GEMINI_31_PRO_SOURCE: &str =
    "https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/gemini/3-1-pro";
const GOOGLE_GEMINI_35_FLASH_SOURCE: &str =
    "https://ai.google.dev/gemini-api/docs/models/gemini-3.5-flash";

static CUTOFFS: &[CutoffEntry] = &[
    CutoffEntry {
        model: "claude-opus-4-8",
        cutoff: "2026-01",
        kind: CutoffKind::TrainingData,
        source: ANTHROPIC_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "claude-opus-4-7",
        cutoff: "2026-01",
        kind: CutoffKind::TrainingData,
        source: ANTHROPIC_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "claude-opus-4-6",
        cutoff: "2025-08",
        kind: CutoffKind::TrainingData,
        source: ANTHROPIC_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "claude-sonnet-4-6",
        cutoff: "2026-01",
        kind: CutoffKind::TrainingData,
        source: ANTHROPIC_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "claude-sonnet-4-5",
        cutoff: "2025-07",
        kind: CutoffKind::TrainingData,
        source: ANTHROPIC_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "claude-haiku-4-5",
        cutoff: "2025-07",
        kind: CutoffKind::TrainingData,
        source: ANTHROPIC_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "claude-haiku-4-5-20251001",
        cutoff: "2025-07",
        kind: CutoffKind::TrainingData,
        source: ANTHROPIC_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gpt-5",
        cutoff: "2024-09-30",
        kind: CutoffKind::Knowledge,
        source: OPENAI_GPT5_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gpt-5-2025",
        cutoff: "2024-09-30",
        kind: CutoffKind::Knowledge,
        source: OPENAI_GPT5_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gpt-4.1",
        cutoff: "2024-06-01",
        kind: CutoffKind::Knowledge,
        source: OPENAI_GPT41_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gpt-4.1-mini",
        cutoff: "2024-06-01",
        kind: CutoffKind::Knowledge,
        source: OPENAI_GPT41_MINI_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gpt-4.1-nano",
        cutoff: "2024-06-01",
        kind: CutoffKind::Knowledge,
        source: OPENAI_GPT41_NANO_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gpt-4o",
        cutoff: "2023-10-01",
        kind: CutoffKind::Knowledge,
        source: OPENAI_GPT4O_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gpt-4o-mini",
        cutoff: "2023-10-01",
        kind: CutoffKind::Knowledge,
        source: OPENAI_GPT4O_MINI_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "o3",
        cutoff: "2024-06-01",
        kind: CutoffKind::Knowledge,
        source: OPENAI_O3_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "o3-mini",
        cutoff: "2023-10-01",
        kind: CutoffKind::Knowledge,
        source: OPENAI_O3_MINI_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "grok-3",
        cutoff: "2024-11",
        kind: CutoffKind::Knowledge,
        source: XAI_GROK3_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gemini-1.5-pro",
        cutoff: "2024-05",
        kind: CutoffKind::Knowledge,
        source: GOOGLE_GEMINI_15_PRO_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gemini-1.5-flash",
        cutoff: "2024-05",
        kind: CutoffKind::Knowledge,
        source: GOOGLE_GEMINI_15_FLASH_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gemini-2.0-flash",
        // Google's live cloud page states "June 2024" while Google's own
        // archived ai.google.dev models page stated "August 2024"; we take the
        // later date as the conservative contamination bound.
        cutoff: "2024-08",
        kind: CutoffKind::Knowledge,
        source: GOOGLE_GEMINI_20_FLASH_ARCHIVE_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gemini-2.0-flash-lite",
        cutoff: "2024-06",
        kind: CutoffKind::Knowledge,
        source: GOOGLE_GEMINI_20_FLASH_LITE_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gemini-2.0-flash-exp",
        cutoff: "2024-08",
        kind: CutoffKind::Knowledge,
        source: GOOGLE_GEMINI_20_FLASH_ARCHIVE_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gemini-2.5-pro",
        cutoff: "2025-01",
        kind: CutoffKind::Knowledge,
        source: GOOGLE_GEMINI_25_PRO_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gemini-2.5-flash",
        cutoff: "2025-01",
        kind: CutoffKind::Knowledge,
        source: GOOGLE_GEMINI_25_FLASH_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gemini-2.5-flash-lite",
        cutoff: "2025-01",
        kind: CutoffKind::Knowledge,
        source: GOOGLE_GEMINI_25_FLASH_LITE_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gemini-3-pro-preview",
        cutoff: "2025-01",
        kind: CutoffKind::Knowledge,
        source: GOOGLE_GEMINI_3_PRO_PREVIEW_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gemini-3-pro",
        cutoff: "2025-01",
        kind: CutoffKind::Knowledge,
        source: GOOGLE_GEMINI_3_PRO_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gemini-3-flash-preview",
        cutoff: "2025-01",
        kind: CutoffKind::Knowledge,
        source: GOOGLE_GEMINI_3_FLASH_PREVIEW_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gemini-3-flash",
        cutoff: "2025-01",
        kind: CutoffKind::Knowledge,
        source: GOOGLE_GEMINI_3_FLASH_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gemini-3.1-pro-preview",
        cutoff: "2025-01",
        kind: CutoffKind::Knowledge,
        source: GOOGLE_GEMINI_31_PRO_PREVIEW_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gemini-3.1-pro",
        cutoff: "2025-01",
        kind: CutoffKind::Knowledge,
        source: GOOGLE_GEMINI_31_PRO_SOURCE,
        verified: VERIFIED,
    },
    CutoffEntry {
        model: "gemini-3.5-flash",
        cutoff: "2025-01",
        kind: CutoffKind::Knowledge,
        source: GOOGLE_GEMINI_35_FLASH_SOURCE,
        verified: VERIFIED,
    },
];

/// Exact-match lookup. NO fuzzy/prefix matching — a wrong-family match would
/// mis-classify contamination, which is an integrity bug. Dated snapshots get
/// their own explicit rows (e.g. claude-haiku-4-5-20251001).
pub fn cutoff_for_model(model: &str) -> Option<&'static CutoffEntry> {
    CUTOFFS.iter().find(|entry| entry.model == model)
}

/// The whole table, for the transparency listing (`ato bench cutoffs`).
pub fn all_cutoffs() -> &'static [CutoffEntry] {
    CUTOFFS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{classify_contamination, ContaminationFlag};
    use std::collections::HashSet;

    #[test]
    fn lookup_is_exact_only() {
        assert_eq!(
            cutoff_for_model("gemini-2.5-pro").map(|entry| entry.cutoff),
            Some("2025-01")
        );
        assert!(cutoff_for_model("gemini-2.5-pro-exp").is_none());
        assert!(cutoff_for_model("claude-sonnet").is_none());
    }

    #[test]
    fn no_duplicate_models_in_table() {
        let mut seen = HashSet::new();
        for entry in all_cutoffs() {
            assert!(
                seen.insert(entry.model),
                "duplicate model in cutoff table: {}",
                entry.model
            );
        }
    }

    #[test]
    fn every_cutoff_date_parses() {
        for entry in all_cutoffs() {
            assert_eq!(
                classify_contamination(Some("2030-01-01"), Some(entry.cutoff)),
                ContaminationFlag::Clean,
                "cutoff should parse and classify clean for model {}",
                entry.model
            );
        }
    }

    #[test]
    fn every_source_is_https_vendor_url() {
        for entry in all_cutoffs() {
            assert!(!entry.source.is_empty(), "source must be non-empty");
            assert!(
                entry.source.starts_with("https://"),
                "source must be https for model {}",
                entry.model
            );
        }
    }

    #[test]
    fn snapshot_ids_present() {
        let base = cutoff_for_model("claude-haiku-4-5");
        let snapshot = cutoff_for_model("claude-haiku-4-5-20251001");
        assert_eq!(snapshot.is_some(), base.is_some());
        assert_eq!(snapshot.map(|entry| entry.cutoff), base.map(|entry| entry.cutoff));
    }
}
