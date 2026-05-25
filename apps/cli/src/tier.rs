// v2.11 PR-12.05 — CLI-side tier gating.
//
// The open-core principle (per docs/v2.11-learning-loop.md and the
// 2026-05-25 tier conversation): *we let customers run primitives for
// free, we charge for the codified automation on top of those
// primitives.* Setting up your own methodology + dispatching it via
// the CLI is free. The buttons that automate the loop for you —
// scheduled runs, diagnose, cross-device sync — are Pro.
//
// This module is the CLI's tier-check seam. It mirrors
// apps/desktop/src/lib/tier.ts so the same feature flags resolve the
// same way whether the customer is in the desktop app or the terminal.
//
// Resolution chain (matches the desktop hook's behavior):
//   1. $ATO_TIER override — testing / offline escape hatch.
//   2. ~/.ato/auth.json cached tier (last successful /auth/me).
//   3. Network probe of /api/auth/me on cold cache or expired TTL.
//   4. Fall through to "free" if nothing resolves.
//
// Failure mode: every helper here is no-network-friendly. If the
// cloud is unreachable + cache is stale + no env override, the CLI
// degrades to "free" (the safe default — locks the customer out of
// Pro features rather than letting them slip through).

use std::path::PathBuf;
use std::time::Duration;

const CACHE_TTL_SECS: u64 = 60 * 60 * 24; // 24h — same as desktop hook

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Free,
    Pro,
    Team,
    Enterprise,
}

impl Tier {
    pub fn rank(self) -> u8 {
        match self {
            Tier::Free => 0,
            Tier::Pro => 1,
            Tier::Team => 2,
            Tier::Enterprise => 3,
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "pro" => Tier::Pro,
            "team" | "platform" => Tier::Team,
            "enterprise" => Tier::Enterprise,
            _ => Tier::Free,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Tier::Free => "Free",
            Tier::Pro => "Pro",
            Tier::Team => "Team",
            Tier::Enterprise => "Enterprise",
        }
    }
}

fn auth_file_path() -> PathBuf {
    crate::db::home_dir().join(".ato").join("auth.json")
}

fn cloud_base() -> String {
    std::env::var("ATO_CLOUD_URL")
        .unwrap_or_else(|_| "https://api.agentictool.ai".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn read_cached_tier_and_token() -> Option<(Tier, String, u64)> {
    let content = std::fs::read_to_string(auth_file_path()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    let token = v.get("token")?.as_str()?.to_string();
    let tier_str = v.get("tier").and_then(|x| x.as_str()).unwrap_or("free");
    let cached_at = v
        .get("tier_cached_at")
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    Some((Tier::parse(tier_str), token, cached_at))
}

fn write_cached_tier(token: &str, tier: Tier) {
    let path = auth_file_path();
    let mut data: serde_json::Map<String, serde_json::Value> =
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
    data.insert("token".to_string(), serde_json::Value::from(token));
    data.insert(
        "tier".to_string(),
        serde_json::Value::from(match tier {
            Tier::Free => "free",
            Tier::Pro => "pro",
            Tier::Team => "team",
            Tier::Enterprise => "enterprise",
        }),
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    data.insert("tier_cached_at".to_string(), serde_json::Value::from(now));
    let _ = std::fs::write(
        &path,
        serde_json::to_string_pretty(&serde_json::Value::Object(data)).unwrap_or_default(),
    );
}

/// Resolve the customer's current tier with the cache-first chain
/// described at the top of the module. Never fails — falls through
/// to Free if nothing resolves. Use this on read paths where you
/// just want to know "are we Pro+?"
pub fn current_tier() -> Tier {
    if let Ok(s) = std::env::var("ATO_TIER") {
        return Tier::parse(&s);
    }
    let Some((cached, token, cached_at)) = read_cached_tier_and_token() else {
        return Tier::Free;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cache_age = now.saturating_sub(cached_at);
    if cache_age < CACHE_TTL_SECS {
        return cached;
    }
    // Cache expired — try to refresh from cloud. If network fails,
    // fall back to the cached value (it's still better than
    // pessimistically downgrading every Pro user when offline).
    if let Some(fresh) = probe_tier_from_cloud(&token) {
        write_cached_tier(&token, fresh);
        fresh
    } else {
        cached
    }
}

fn probe_tier_from_cloud(token: &str) -> Option<Tier> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;
    let resp = client
        .get(format!("{}/api/auth/me", cloud_base()))
        .bearer_auth(token)
        .send()
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().ok()?;
    let tier_str = body
        .pointer("/data/user/subscription_tier")
        .and_then(|v| v.as_str())
        .unwrap_or("free");
    Some(Tier::parse(tier_str))
}

/// Feature catalog. Stays in lockstep with apps/desktop/src/lib/tier.ts
/// FEATURE_MIN_TIER and apps/cli/src/commands/pro.rs FEATURES. The
/// `id` is the canonical key both surfaces use.
pub struct FeatureGate {
    pub id: &'static str,
    pub label: &'static str,
    pub min_tier: Tier,
    /// Short one-liner shown in the "upgrade" message.
    pub roi: &'static str,
}

pub const FEATURES: &[FeatureGate] = &[
    // Pro features added in v2.10/v2.11 that gate the CLI surface:
    FeatureGate {
        id: "methodology.schedule",
        label: "Scheduled methodology runs",
        min_tier: Tier::Pro,
        roi: "Re-run any methodology automatically on cron — the regression-watch archetype's 'diff this week vs last week' loop only closes with automation we provide.",
    },
    FeatureGate {
        id: "methodology.diagnose",
        label: "Methodology diagnose (learning loop)",
        min_tier: Tier::Pro,
        roi: "Read failing methodology cells + propose a structured change to the agent definition + A/B test the change. The codified self-improvement loop you'd otherwise script by hand.",
    },
];

pub fn lookup_feature(id: &str) -> Option<&'static FeatureGate> {
    FEATURES.iter().find(|f| f.id == id)
}

/// Check whether the current tier meets a feature's minimum. Used by
/// CLI command handlers that need to short-circuit before doing work.
pub fn is_allowed(feature_id: &str) -> bool {
    let Some(gate) = lookup_feature(feature_id) else {
        // Unknown feature → assume free (open by default). Catches the
        // case where a new CLI subcommand forgot to register itself in
        // the FEATURES table.
        return true;
    };
    current_tier().rank() >= gate.min_tier.rank()
}

/// CLI helper: if the feature isn't allowed at the current tier,
/// print a structured upgrade prompt to stderr + return Err. The
/// caller's `?` propagates the bail upward.
pub fn require_feature(feature_id: &str) -> anyhow::Result<()> {
    if is_allowed(feature_id) {
        return Ok(());
    }
    let gate = lookup_feature(feature_id)
        .ok_or_else(|| anyhow::anyhow!("unknown feature id '{}' (this is a bug — register it in apps/cli/src/tier.rs FEATURES)", feature_id))?;
    let current = current_tier();
    anyhow::bail!(
        "{label} is a {required_tier} feature; your current tier is {current_tier}.\n\
         \n\
         {roi}\n\
         \n\
         You can still build this yourself with the free primitives — `ato dispatch`, `ato review`, your own bash loop. You lose the codified safety net (holdouts, Welch t, lineage warnings, auto-revert).\n\
         \n\
         Upgrade: `ato pro enable`  (or set ATO_TIER=pro for local testing).",
        label = gate.label,
        required_tier = gate.min_tier.label(),
        current_tier = current.label(),
        roi = gate.roi,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_rank_is_monotonic() {
        assert!(Tier::Free.rank() < Tier::Pro.rank());
        assert!(Tier::Pro.rank() < Tier::Team.rank());
        assert!(Tier::Team.rank() < Tier::Enterprise.rank());
    }

    #[test]
    fn tier_parse_round_trips_known_values() {
        assert_eq!(Tier::parse("free"), Tier::Free);
        assert_eq!(Tier::parse("pro"), Tier::Pro);
        assert_eq!(Tier::parse("team"), Tier::Team);
        assert_eq!(Tier::parse("platform"), Tier::Team); // alias from desktop
        assert_eq!(Tier::parse("enterprise"), Tier::Enterprise);
    }

    #[test]
    fn tier_parse_unknown_falls_to_free() {
        assert_eq!(Tier::parse("garbage"), Tier::Free);
        assert_eq!(Tier::parse(""), Tier::Free);
    }

    #[test]
    fn feature_lookup_returns_known_features() {
        assert!(lookup_feature("methodology.diagnose").is_some());
        assert!(lookup_feature("methodology.schedule").is_some());
        assert!(lookup_feature("nonexistent.feature").is_none());
    }

    // Env-var tests below are intentionally consolidated into ONE test
    // because cargo runs unit tests in parallel by default + std::env
    // is process-global state. Splitting these across tests caused
    // race conditions where one test's `remove_var` cleared another
    // test's `set_var`. Single sequential test owns the ATO_TIER
    // lifecycle for the whole suite.
    #[test]
    fn ato_tier_env_var_overrides_and_require_feature_paths() {
        // Set to pro → diagnose passes
        std::env::set_var("ATO_TIER", "pro");
        assert_eq!(current_tier(), Tier::Pro);
        assert!(require_feature("methodology.diagnose").is_ok());
        assert!(require_feature("methodology.schedule").is_ok());

        // Bump to enterprise → still passes (higher rank than required)
        std::env::set_var("ATO_TIER", "enterprise");
        assert_eq!(current_tier(), Tier::Enterprise);
        assert!(require_feature("methodology.diagnose").is_ok());

        // Drop to free → diagnose denied with the "Pro feature" upgrade message
        std::env::set_var("ATO_TIER", "free");
        let err = require_feature("methodology.diagnose")
            .err()
            .expect("free tier must be denied");
        let msg = err.to_string();
        assert!(msg.contains("Pro feature"), "expected 'Pro feature' in msg; got: {}", msg);
        assert!(msg.contains("Upgrade"), "expected 'Upgrade' in msg; got: {}", msg);

        // Unknown features default-open (see is_allowed comment)
        std::env::set_var("ATO_TIER", "free");
        assert!(require_feature("brand.new.unknown.feature").is_ok());

        // Clean up
        std::env::remove_var("ATO_TIER");
    }
}
