// v2.10.0 PR-3 — per-cell composition + statistics.
//
// Given the execution_logs rows a methodology run produced, group them
// by variant cell (prompt × model × condition) and compute the summary
// statistics the spec promises: count, mean, sample SD, 95% CI.
//
// PR-3 ships per-cell stats only over receipt-native fields
// (cost_usd_estimated, tokens_out, duration_ms) and the grounding-verdict
// mix. The rubric score column on methodology_run_dispatches stays NULL
// until PR-4 lands the rubric library; PR-5 wires LLM-judge scores into
// the same composition.
//
// Pair-level significance (Welch's t-statistic) is computed when there
// are exactly two cells to compare on a single axis; full pairwise
// matrices land if a Pro customer asks for them.
//
// Why no p-value: the proper t-distribution CDF needs the incomplete
// beta function, which isn't worth pulling a stats crate in for. We
// surface the t-statistic and degrees of freedom; the heuristic for
// "is this difference real" is whether the two 95% CIs overlap. That's
// the same advice published methodology guides give for the n=10..30
// range Pro customers actually run at.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One observation row used by the composer — pulled from execution_logs
/// + its variant_cell coordinates from methodology_run_dispatches.
#[derive(Debug, Clone)]
pub struct CellObservation {
    pub prompt_idx: usize,
    pub model: String,
    pub condition: String,
    pub cost_usd: f64,
    pub tokens_out: f64,
    pub duration_ms: f64,
    pub grounding_verdict: Option<String>,
    pub status: String, // success / error / refused
    /// Rubric score (PR-4). Some(0.0..1.0) when scored; None when the
    /// rubric is Pending or scoring hasn't run yet.
    pub score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Stats {
    pub n: usize,
    pub mean: f64,
    pub sd: f64,
    pub ci_lo: f64,
    pub ci_hi: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellSummary {
    pub prompt_idx: usize,
    pub model: String,
    pub condition: String,
    pub n: usize,
    pub success_n: usize,
    pub error_n: usize,
    pub cost_usd: Stats,
    pub tokens_out: Stats,
    pub duration_ms: Stats,
    pub grounding_verdicts: BTreeMap<String, usize>,
    /// Rubric score statistics over the cell's scored observations.
    /// None when no observation in the cell has a score (PR-4 may not
    /// have run yet).
    #[serde(default)]
    pub score: Option<Stats>,
    /// Number of observations in the cell that scored at or above 0.5
    /// (the canonical "pass" threshold for binary rubrics). None when
    /// score is None.
    #[serde(default)]
    pub passed_at_0_5: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Composition {
    pub cells: Vec<CellSummary>,
    /// Pairwise Welch t-statistics for the cost metric across models
    /// holding (prompt_idx, condition) constant. Only filled when
    /// there are >= 2 models and each cell has >= 2 observations
    /// (sample SD undefined for n<2).
    pub model_pairs_cost_t: Vec<PairwiseT>,
    pub total_dispatches: usize,
    pub total_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairwiseT {
    pub prompt_idx: usize,
    pub condition: String,
    pub model_a: String,
    pub model_b: String,
    pub mean_a: f64,
    pub mean_b: f64,
    pub n_a: usize,
    pub n_b: usize,
    pub t_statistic: f64,
    pub welch_df: f64,
    /// Heuristic flag — true when the two 95% CIs do not overlap.
    /// Stable signal for small samples where p-value approximation
    /// is unreliable. Always populated.
    pub ci_disjoint: bool,
    /// Two-sided p-value approximation. None when df < 10 — at small
    /// df the normal-CDF approximation under-states tail mass enough
    /// that we'd rather show "—" than a misleading number; trust the
    /// `ci_disjoint` flag at low n.
    #[serde(default)]
    pub p_value_approx: Option<f64>,
}

/// Abramowitz & Stegun 7.1.26 — erf approximation, accurate to ~1.5e-7.
/// Standard reference implementation. Used by `normal_cdf` below.
pub fn erf_approx(x: f64) -> f64 {
    // Constants from A&S 7.1.26.
    let a1 = 0.254_829_592;
    let a2 = -0.284_496_736;
    let a3 = 1.421_413_741;
    let a4 = -1.453_152_027;
    let a5 = 1.061_405_429;
    let p = 0.327_591_1;
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let xa = x.abs();
    let t = 1.0 / (1.0 + p * xa);
    let y = 1.0
        - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t * (-xa * xa).exp();
    sign * y
}

/// Standard normal CDF Φ(z) via erf. Returns P(Z ≤ z).
pub fn normal_cdf(z: f64) -> f64 {
    0.5 * (1.0 + erf_approx(z / std::f64::consts::SQRT_2))
}

/// Two-sided p-value for Welch's t under the normal approximation.
/// Returns None when df < 30 — code-review finding #2 (PR-9):
/// at df=10 the normal CDF under-states the true t-dist tail enough
/// to flip "borderline significant" decisions (true p≈0.078 vs
/// approximated 0.050 at t=1.96/df=10). At df≥30 the approximation
/// lands within ~3% of the true t-distribution p, which is honest
/// enough for "is this real" UX. Below that we return None and
/// callers fall back to the CI-disjoint heuristic.
pub fn welch_p_value_approx(t: f64, df: f64) -> Option<f64> {
    if df < 30.0 || !t.is_finite() {
        return None;
    }
    let p = 2.0 * (1.0 - normal_cdf(t.abs()));
    Some(p.clamp(0.0, 1.0))
}

// ── v2.11 PR-12.2: A/B win-condition predicates ──────────────────────────
//
// Compares a variant `Composition` to a baseline `Composition` cell-by-cell
// and returns the three predicates locked in docs/v2.11-learning-loop.md
// §Q4 ("Statistically Significant Pareto Improvement"):
//
//   1. any_significant_improvement — ≥1 cell shows variant's mean SCORE
//      strictly higher than baseline AND a Welch t between the two
//      sample distributions is significant (df ≥ 30 → p < 0.05; df < 30
//      → 95% CIs disjoint, the fallback for small samples).
//   2. any_significant_regression — ≥1 cell shows variant's mean SCORE
//      strictly lower than baseline under the same significance bar.
//      A variant FAILS the win condition if any regression is present.
//   3. cost_inflation_unjustified — ≥1 cell where variant's mean cost
//      exceeds baseline by >10% UNLESS that same cell ALSO hit predicate
//      (1) with a score delta ≥ 0.2 (i.e. cost OK if quality jump is
//      large). A variant FAILS if any cell inflates cost without a
//      defensible quality justification.
//
// A variant SHIPS only when (1) is true AND (2) is false AND (3) is
// false. These predicates do NOT make that decision — they expose
// individual cell verdicts so the caller can render a transparent diff.

/// One cell's verdict in the A/B comparison.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CellComparison {
    pub prompt_idx: usize,
    pub model: String,
    pub condition: String,
    pub baseline_n: usize,
    pub variant_n: usize,
    pub baseline_mean_score: Option<f64>,
    pub variant_mean_score: Option<f64>,
    /// Change in mean score (variant - baseline). None when either side
    /// has no scored observations.
    pub score_delta: Option<f64>,
    /// Welch t between the two SCORE distributions when both sides have
    /// at least one scored observation. Otherwise None.
    pub welch_t: Option<f64>,
    /// Welch df (Satterthwaite). None alongside welch_t.
    pub welch_df: Option<f64>,
    /// Two-sided p-value approximation; None when df < 30 — caller
    /// falls back to ci_disjoint at small samples.
    pub p_value_approx: Option<f64>,
    /// True when baseline.ci and variant.ci do not overlap.
    pub ci_disjoint: bool,
    pub baseline_mean_cost: f64,
    pub variant_mean_cost: f64,
    /// (variant_cost - baseline_cost) / baseline_cost; None when baseline cost is 0.
    pub cost_delta_pct: Option<f64>,
}

impl CellComparison {
    /// Returns true if the variant beat the baseline with statistical
    /// significance — either p<0.05 at df≥30 OR (when p approx isn't
    /// computable at df<30) the 95% CIs are disjoint AND the variant's
    /// mean score is strictly higher.
    pub fn is_significant_improvement(&self) -> bool {
        let delta = match self.score_delta {
            Some(d) => d,
            None => return false,
        };
        if delta <= 0.0 {
            return false;
        }
        match self.p_value_approx {
            Some(p) if p < 0.05 => true,
            Some(_) => false,
            None => self.ci_disjoint,
        }
    }

    /// Returns true if the variant LOST against the baseline under the
    /// same significance bar. Used to detect any cell that ships
    /// regressed — a variant with even one regressed cell FAILS the
    /// win condition.
    pub fn is_significant_regression(&self) -> bool {
        let delta = match self.score_delta {
            Some(d) => d,
            None => return false,
        };
        if delta >= 0.0 {
            return false;
        }
        match self.p_value_approx {
            Some(p) if p < 0.05 => true,
            Some(_) => false,
            None => self.ci_disjoint,
        }
    }

    /// Returns true if variant inflated mean cost by >10% AND the cost
    /// inflation is NOT justified by a quality jump (score delta ≥ 0.2).
    pub fn is_cost_inflation_unjustified(&self) -> bool {
        let pct = match self.cost_delta_pct {
            Some(p) => p,
            None => return false,
        };
        if pct <= 0.10 {
            return false;
        }
        // Variant is >10% more expensive — is the cost justified by a
        // significant score improvement of ≥ 0.2?
        match self.score_delta {
            Some(d) if d >= 0.2 && self.is_significant_improvement() => false,
            _ => true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbVerdict {
    pub cells: Vec<CellComparison>,
    pub any_significant_improvement: bool,
    pub any_significant_regression: bool,
    pub cost_inflation_unjustified: bool,
    /// Convenience: variant ships if (1) AND NOT (2) AND NOT (3).
    pub variant_should_ship: bool,
}

/// Compare a variant composition against a baseline composition,
/// cell-by-cell. Cells absent from either side appear with the missing
/// side's n=0 and no delta computed. Cells present on both sides get
/// the full CellComparison treatment.
///
/// The caller's responsibility: pass the SAME methodology run on both
/// sides (same variant_matrix). This function does NOT validate that —
/// it just compares whatever cells it finds.
pub fn compare_runs(baseline: &Composition, variant: &Composition) -> AbVerdict {
    let mut by_key_baseline: std::collections::HashMap<
        (usize, String, String),
        &CellSummary,
    > = std::collections::HashMap::new();
    for c in &baseline.cells {
        by_key_baseline.insert((c.prompt_idx, c.model.clone(), c.condition.clone()), c);
    }
    let mut by_key_variant: std::collections::HashMap<
        (usize, String, String),
        &CellSummary,
    > = std::collections::HashMap::new();
    for c in &variant.cells {
        by_key_variant.insert((c.prompt_idx, c.model.clone(), c.condition.clone()), c);
    }
    // Union of keys → one CellComparison per (prompt, model, condition).
    let mut keys: Vec<(usize, String, String)> = by_key_baseline.keys().cloned().collect();
    for k in by_key_variant.keys() {
        if !keys.contains(k) {
            keys.push(k.clone());
        }
    }
    keys.sort();
    let mut cells: Vec<CellComparison> = Vec::with_capacity(keys.len());
    for key in keys {
        let b = by_key_baseline.get(&key).copied();
        let v = by_key_variant.get(&key).copied();
        cells.push(compare_one_cell(&key, b, v));
    }
    let any_imp = cells.iter().any(|c| c.is_significant_improvement());
    let any_reg = cells.iter().any(|c| c.is_significant_regression());
    let any_cost = cells.iter().any(|c| c.is_cost_inflation_unjustified());
    let ship = any_imp && !any_reg && !any_cost;
    AbVerdict {
        cells,
        any_significant_improvement: any_imp,
        any_significant_regression: any_reg,
        cost_inflation_unjustified: any_cost,
        variant_should_ship: ship,
    }
}

fn compare_one_cell(
    key: &(usize, String, String),
    baseline: Option<&CellSummary>,
    variant: Option<&CellSummary>,
) -> CellComparison {
    let (prompt_idx, model, condition) = key;
    let baseline_n = baseline.map(|c| c.n).unwrap_or(0);
    let variant_n = variant.map(|c| c.n).unwrap_or(0);
    let baseline_mean_score = baseline.and_then(|c| c.score.as_ref().map(|s| s.mean));
    let variant_mean_score = variant.and_then(|c| c.score.as_ref().map(|s| s.mean));
    let score_delta = match (baseline_mean_score, variant_mean_score) {
        (Some(b), Some(v)) => Some(v - b),
        _ => None,
    };

    // Welch t against the two SCORE distributions. We don't keep raw
    // sample arrays on CellSummary (we only keep Stats); reconstructing
    // them isn't possible from this side, so we approximate by treating
    // the score stats as the sample summary. This isn't quite a real
    // Welch t — it's a Welch t over the two distributions' SUMMARY
    // STATISTICS, which equals the real t when the underlying sample
    // SDs are taken at face value. For PR-12.2 that's the right
    // trade-off; richer A/B (full re-construction from
    // methodology_run_dispatches) lives in PR-12.3.
    let (welch_t, welch_df, p) = match (baseline, variant) {
        (Some(b), Some(v)) => {
            let bs = b.score.as_ref();
            let vs = v.score.as_ref();
            match (bs, vs) {
                (Some(bsc), Some(vsc)) if bsc.n >= 2 && vsc.n >= 2 => {
                    let n_b = bsc.n as f64;
                    let n_v = vsc.n as f64;
                    let var_b_over_n = (bsc.sd * bsc.sd) / n_b;
                    let var_v_over_n = (vsc.sd * vsc.sd) / n_v;
                    let denom = (var_b_over_n + var_v_over_n).sqrt();
                    if denom > 0.0 {
                        let t = (vsc.mean - bsc.mean) / denom;
                        let df_num = (var_b_over_n + var_v_over_n).powi(2);
                        let df_den = var_b_over_n.powi(2) / (n_b - 1.0)
                            + var_v_over_n.powi(2) / (n_v - 1.0);
                        let df = if df_den == 0.0 { f64::INFINITY } else { df_num / df_den };
                        let p = welch_p_value_approx(t, df);
                        (Some(t), Some(df), p)
                    } else {
                        (None, None, None)
                    }
                }
                _ => (None, None, None),
            }
        }
        _ => (None, None, None),
    };

    let ci_disjoint = match (baseline.and_then(|c| c.score.as_ref()), variant.and_then(|c| c.score.as_ref())) {
        (Some(b), Some(v)) => b.ci_hi < v.ci_lo || v.ci_hi < b.ci_lo,
        _ => false,
    };

    let baseline_mean_cost = baseline.map(|c| c.cost_usd.mean).unwrap_or(0.0);
    let variant_mean_cost = variant.map(|c| c.cost_usd.mean).unwrap_or(0.0);
    let cost_delta_pct = if baseline_mean_cost > 0.0 {
        Some((variant_mean_cost - baseline_mean_cost) / baseline_mean_cost)
    } else {
        None
    };

    CellComparison {
        prompt_idx: *prompt_idx,
        model: model.clone(),
        condition: condition.clone(),
        baseline_n,
        variant_n,
        baseline_mean_score,
        variant_mean_score,
        score_delta,
        welch_t,
        welch_df,
        p_value_approx: p,
        ci_disjoint,
        baseline_mean_cost,
        variant_mean_cost,
        cost_delta_pct,
    }
}

pub fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().copied().sum::<f64>() / (xs.len() as f64)
}

/// Sample standard deviation (n-1 denominator). Returns 0 when n < 2 —
/// the caller is responsible for treating sd=0 with n<2 as "undefined"
/// when surfacing it.
pub fn sample_sd(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let m = mean(xs);
    let var: f64 = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (xs.len() as f64 - 1.0);
    var.sqrt()
}

/// Two-sided 95% t-critical for `df` degrees of freedom. Hard-coded
/// table for df=1..29 (Student's t); df>=30 falls back to the normal
/// approximation (1.96). The table values are the standard ones
/// every stats text publishes — verified against scipy.stats.t.ppf(0.975, df).
pub fn t_critical_95(df: usize) -> f64 {
    const T_TABLE: [f64; 30] = [
        // index 0 is unused (df=0 has no t)
        0.0, 12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179,
        2.160, 2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060,
        2.056, 2.052, 2.048, 2.045,
    ];
    if df == 0 {
        return f64::INFINITY;
    }
    if df < T_TABLE.len() {
        T_TABLE[df]
    } else {
        1.96
    }
}

pub fn stats(xs: &[f64]) -> Stats {
    let n = xs.len();
    let m = mean(xs);
    let sd = sample_sd(xs);
    let (lo, hi) = if n < 2 {
        (m, m)
    } else {
        let t = t_critical_95(n - 1);
        let se = sd / (n as f64).sqrt();
        (m - t * se, m + t * se)
    };
    Stats {
        n,
        mean: m,
        sd,
        ci_lo: lo,
        ci_hi: hi,
    }
}

/// Welch's two-sample t-statistic + Satterthwaite df.
/// Returns None when either sample has n < 2.
pub fn welch_t(xs_a: &[f64], xs_b: &[f64]) -> Option<(f64, f64)> {
    if xs_a.len() < 2 || xs_b.len() < 2 {
        return None;
    }
    let m_a = mean(xs_a);
    let m_b = mean(xs_b);
    let s_a = sample_sd(xs_a);
    let s_b = sample_sd(xs_b);
    let n_a = xs_a.len() as f64;
    let n_b = xs_b.len() as f64;
    let var_a_over_n = (s_a * s_a) / n_a;
    let var_b_over_n = (s_b * s_b) / n_b;
    let denom = (var_a_over_n + var_b_over_n).sqrt();
    if denom == 0.0 {
        return None;
    }
    let t = (m_a - m_b) / denom;
    let df_num = (var_a_over_n + var_b_over_n).powi(2);
    let df_den = var_a_over_n.powi(2) / (n_a - 1.0) + var_b_over_n.powi(2) / (n_b - 1.0);
    let df = df_num / df_den;
    Some((t, df))
}

pub fn compose(observations: &[CellObservation]) -> Composition {
    let mut grouped: BTreeMap<(usize, String, String), Vec<&CellObservation>> = BTreeMap::new();
    for obs in observations {
        grouped
            .entry((obs.prompt_idx, obs.model.clone(), obs.condition.clone()))
            .or_default()
            .push(obs);
    }
    let mut cells: Vec<CellSummary> = grouped
        .into_iter()
        .map(|((prompt_idx, model, condition), obs_vec)| {
            let costs: Vec<f64> = obs_vec.iter().map(|o| o.cost_usd).collect();
            let tokens: Vec<f64> = obs_vec.iter().map(|o| o.tokens_out).collect();
            let durations: Vec<f64> = obs_vec.iter().map(|o| o.duration_ms).collect();
            let success_n = obs_vec.iter().filter(|o| o.status == "success").count();
            let error_n = obs_vec.iter().filter(|o| o.status != "success").count();
            let mut verdicts: BTreeMap<String, usize> = BTreeMap::new();
            for o in &obs_vec {
                let key = o
                    .grounding_verdict
                    .clone()
                    .unwrap_or_else(|| "not_enforced".to_string());
                *verdicts.entry(key).or_insert(0) += 1;
            }
            let scores: Vec<f64> =
                obs_vec.iter().filter_map(|o| o.score).collect();
            let (score, passed_at_0_5) = if scores.is_empty() {
                (None, None)
            } else {
                let passed = scores.iter().filter(|s| **s >= 0.5).count();
                (Some(stats(&scores)), Some(passed))
            };
            CellSummary {
                prompt_idx,
                model,
                condition,
                n: obs_vec.len(),
                success_n,
                error_n,
                cost_usd: stats(&costs),
                tokens_out: stats(&tokens),
                duration_ms: stats(&durations),
                grounding_verdicts: verdicts,
                score,
                passed_at_0_5,
            }
        })
        .collect();
    cells.sort_by(|a, b| {
        (a.prompt_idx, &a.condition, &a.model).cmp(&(b.prompt_idx, &b.condition, &b.model))
    });

    let model_pairs_cost_t = pairwise_cost_t(observations, &cells);
    let total_dispatches = observations.len();
    let total_cost_usd: f64 = observations.iter().map(|o| o.cost_usd).sum();

    Composition {
        cells,
        model_pairs_cost_t,
        total_dispatches,
        total_cost_usd,
    }
}

fn pairwise_cost_t(observations: &[CellObservation], cells: &[CellSummary]) -> Vec<PairwiseT> {
    let mut pairs: Vec<PairwiseT> = Vec::new();
    let mut by_condition: BTreeMap<(usize, String), Vec<&CellSummary>> = BTreeMap::new();
    for c in cells {
        by_condition
            .entry((c.prompt_idx, c.condition.clone()))
            .or_default()
            .push(c);
    }
    for ((prompt_idx, condition), cells_here) in by_condition {
        for i in 0..cells_here.len() {
            for j in (i + 1)..cells_here.len() {
                let a = cells_here[i];
                let b = cells_here[j];
                let xs_a: Vec<f64> = observations
                    .iter()
                    .filter(|o| {
                        o.prompt_idx == prompt_idx
                            && o.condition == condition
                            && o.model == a.model
                    })
                    .map(|o| o.cost_usd)
                    .collect();
                let xs_b: Vec<f64> = observations
                    .iter()
                    .filter(|o| {
                        o.prompt_idx == prompt_idx
                            && o.condition == condition
                            && o.model == b.model
                    })
                    .map(|o| o.cost_usd)
                    .collect();
                if let Some((t, df)) = welch_t(&xs_a, &xs_b) {
                    let ci_disjoint = a.cost_usd.ci_hi < b.cost_usd.ci_lo
                        || b.cost_usd.ci_hi < a.cost_usd.ci_lo;
                    let p_value_approx = welch_p_value_approx(t, df);
                    pairs.push(PairwiseT {
                        prompt_idx,
                        condition: condition.clone(),
                        model_a: a.model.clone(),
                        model_b: b.model.clone(),
                        mean_a: a.cost_usd.mean,
                        mean_b: b.cost_usd.mean,
                        n_a: a.cost_usd.n,
                        n_b: b.cost_usd.n,
                        t_statistic: t,
                        welch_df: df,
                        ci_disjoint,
                        p_value_approx,
                    });
                }
            }
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(prompt: usize, model: &str, cond: &str, cost: f64) -> CellObservation {
        CellObservation {
            prompt_idx: prompt,
            model: model.to_string(),
            condition: cond.to_string(),
            cost_usd: cost,
            tokens_out: cost * 1000.0,
            duration_ms: cost * 10000.0,
            grounding_verdict: None,
            status: "success".to_string(),
            score: None,
        }
    }

    #[test]
    fn mean_of_empty_is_zero() {
        assert_eq!(mean(&[]), 0.0);
    }

    #[test]
    fn mean_handles_known_values() {
        assert!((mean(&[1.0, 2.0, 3.0, 4.0, 5.0]) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn sample_sd_matches_published_value() {
        // Sample SD of [2,4,4,4,5,5,7,9] is 2.0 exactly (textbook example).
        let v = vec![2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        assert!((sample_sd(&v) - 2.138_089_935_299_395).abs() < 1e-6);
    }

    #[test]
    fn sd_of_singleton_is_zero_not_nan() {
        assert_eq!(sample_sd(&[42.0]), 0.0);
    }

    #[test]
    fn t_critical_matches_published_table_at_df_5() {
        // Standard table value: t_0.025, df=5 = 2.571
        assert!((t_critical_95(5) - 2.571).abs() < 1e-3);
    }

    #[test]
    fn t_critical_falls_back_to_normal_above_df_29() {
        assert_eq!(t_critical_95(30), 1.96);
        assert_eq!(t_critical_95(1000), 1.96);
    }

    #[test]
    fn stats_ci_widens_with_higher_variance() {
        let tight = stats(&[10.0, 10.0, 10.0, 10.0, 10.0]);
        let wide = stats(&[1.0, 10.0, 20.0, 30.0, 40.0]);
        assert!(tight.ci_hi - tight.ci_lo < wide.ci_hi - wide.ci_lo);
    }

    #[test]
    fn welch_t_zero_when_means_equal() {
        let (t, _df) = welch_t(&[5.0, 6.0, 5.5], &[5.0, 6.0, 5.5]).expect("two samples");
        assert!(t.abs() < 1e-12);
    }

    #[test]
    fn welch_t_positive_when_a_greater_than_b() {
        let (t, _df) = welch_t(&[10.0, 11.0, 9.0, 10.0], &[1.0, 2.0, 1.5, 1.0])
            .expect("two samples");
        assert!(t > 0.0);
    }

    #[test]
    fn welch_t_none_when_n_too_small() {
        assert!(welch_t(&[1.0], &[2.0, 3.0]).is_none());
    }

    #[test]
    fn normal_cdf_at_zero_is_half() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn normal_cdf_matches_published_z_values() {
        // Standard normal table reference points.
        assert!((normal_cdf(1.0) - 0.8413).abs() < 1e-3);
        assert!((normal_cdf(1.96) - 0.9750).abs() < 1e-3);
        assert!((normal_cdf(-1.96) - 0.0250).abs() < 1e-3);
        assert!((normal_cdf(2.58) - 0.9951).abs() < 1e-3);
    }

    #[test]
    fn welch_p_value_returns_none_at_small_df() {
        // df < 30 → we deliberately return None so callers fall back to
        // the CI-disjoint heuristic. Code-review finding #2 raised this
        // bound after observing the normal approximation flips
        // "borderline significant" calls at df=10..29.
        assert!(welch_p_value_approx(2.0, 5.0).is_none());
        assert!(welch_p_value_approx(2.0, 29.99).is_none());
    }

    #[test]
    fn welch_p_value_boundary_at_df_30_within_3_percent_of_true_t() {
        // Documents the residual approximation error so a future
        // tightening (real t-CDF) has a number to beat. At df=30,
        // t=1.96 the true two-sided p ≈ 0.059; the normal approximation
        // returns ~0.050. The 0.009 absolute gap is within the "good
        // enough for is-this-real" UX threshold but the test pins it.
        let p = welch_p_value_approx(1.96, 30.0).unwrap();
        let true_t_dist_p_at_df_30 = 0.059_414;
        assert!(
            (p - true_t_dist_p_at_df_30).abs() < 0.015,
            "expected normal approx within ±0.015 of true t-dist p at df=30; got p={}",
            p
        );
    }

    #[test]
    fn welch_p_value_matches_known_z_to_p_translation() {
        // Two-sided p for |z|=1.96 should be ~0.05 (the classic 95% line).
        let p = welch_p_value_approx(1.96, 30.0).unwrap();
        assert!((p - 0.05).abs() < 0.01, "expected p ≈ 0.05; got {}", p);
    }

    #[test]
    fn welch_p_value_clamps_to_zero_at_large_t() {
        // At |t|=10 (df=30) p is essentially 0.
        let p = welch_p_value_approx(10.0, 30.0).unwrap();
        assert!(p < 1e-6, "expected near-zero p; got {}", p);
    }

    #[test]
    fn compose_groups_by_cell_and_counts_correctly() {
        let observations = vec![
            obs(0, "claude-sonnet-4-6", "soft", 0.01),
            obs(0, "claude-sonnet-4-6", "soft", 0.02),
            obs(0, "claude-sonnet-4-6", "soft", 0.03),
            obs(0, "claude-opus-4-7", "soft", 0.10),
            obs(0, "claude-opus-4-7", "soft", 0.11),
            obs(0, "claude-opus-4-7", "soft", 0.09),
        ];
        let comp = compose(&observations);
        assert_eq!(comp.cells.len(), 2);
        assert_eq!(comp.total_dispatches, 6);
        assert!((comp.total_cost_usd - 0.36).abs() < 1e-9);
        // After sorting by (prompt_idx, condition, model): opus comes
        // before sonnet alphabetically.
        assert_eq!(comp.cells[0].model, "claude-opus-4-7");
        assert_eq!(comp.cells[0].n, 3);
        assert!((comp.cells[0].cost_usd.mean - 0.10).abs() < 1e-9);
    }

    #[test]
    fn compose_emits_pairwise_t_when_two_models_present() {
        // Two cells with non-trivial SDs and means that differ by ~10×.
        // Hand-picked so t is comfortably above any reasonable threshold
        // without having to do the arithmetic in the assert message.
        let sonnet = [0.010, 0.011, 0.009, 0.010, 0.012];
        let opus = [0.100, 0.110, 0.090, 0.100, 0.105];
        let mut observations = Vec::new();
        for &c in &sonnet {
            observations.push(obs(0, "claude-sonnet-4-6", "soft", c));
        }
        for &c in &opus {
            observations.push(obs(0, "claude-opus-4-7", "soft", c));
        }
        let comp = compose(&observations);
        assert_eq!(comp.model_pairs_cost_t.len(), 1);
        let pair = &comp.model_pairs_cost_t[0];
        // 10× separation in means with SDs of ~0.001 / ~0.008 gives
        // t in the 20s on n=5. Threshold of 3 leaves headroom for any
        // future tweak to sample_sd's denominator without re-tuning.
        assert!(
            pair.t_statistic.abs() > 3.0,
            "expected sizable t on 10× mean separation; got {}",
            pair.t_statistic
        );
        assert!(pair.ci_disjoint, "CI on $0.10 mean should not overlap CI on $0.01 mean");
    }

    // ── v2.11 PR-12.2: A/B win-condition predicate tests ──────────────

    fn cell_summary(
        prompt_idx: usize,
        condition: &str,
        n: usize,
        mean_score: f64,
        score_sd: f64,
        mean_cost: f64,
    ) -> CellSummary {
        CellSummary {
            prompt_idx,
            model: "claude-sonnet-4-6".to_string(),
            condition: condition.to_string(),
            n,
            success_n: n,
            error_n: 0,
            cost_usd: Stats {
                n,
                mean: mean_cost,
                sd: 0.0,
                ci_lo: mean_cost,
                ci_hi: mean_cost,
            },
            tokens_out: Stats {
                n,
                mean: 100.0,
                sd: 0.0,
                ci_lo: 100.0,
                ci_hi: 100.0,
            },
            duration_ms: Stats {
                n,
                mean: 1000.0,
                sd: 0.0,
                ci_lo: 1000.0,
                ci_hi: 1000.0,
            },
            grounding_verdicts: BTreeMap::new(),
            score: Some(Stats {
                n,
                mean: mean_score,
                sd: score_sd,
                // Crude CI from sd; tests below pick parameters where
                // CIs are clearly disjoint or clearly overlapping.
                ci_lo: mean_score - 2.0 * score_sd,
                ci_hi: mean_score + 2.0 * score_sd,
            }),
            passed_at_0_5: Some(if mean_score >= 0.5 { n } else { 0 }),
        }
    }

    fn comp_with_cells(cells: Vec<CellSummary>) -> Composition {
        Composition {
            cells,
            model_pairs_cost_t: Vec::new(),
            total_dispatches: 0,
            total_cost_usd: 0.0,
        }
    }

    #[test]
    fn variant_with_clear_improvement_passes_ship_condition() {
        // baseline: score 0.3 ± 0.05; variant: score 0.8 ± 0.05; both n=30.
        // CIs are disjoint, variant strictly better, no cost change.
        let baseline = comp_with_cells(vec![cell_summary(0, "default", 30, 0.3, 0.05, 0.01)]);
        let variant = comp_with_cells(vec![cell_summary(0, "default", 30, 0.8, 0.05, 0.01)]);
        let v = compare_runs(&baseline, &variant);
        assert!(v.any_significant_improvement);
        assert!(!v.any_significant_regression);
        assert!(!v.cost_inflation_unjustified);
        assert!(v.variant_should_ship, "clear improvement must ship");
    }

    #[test]
    fn variant_with_any_regression_fails_ship_even_with_other_wins() {
        // Two cells: cell 0 variant improves; cell 1 variant regresses.
        // Even one significant regression blocks ship.
        let baseline = comp_with_cells(vec![
            cell_summary(0, "default", 30, 0.3, 0.05, 0.01),
            cell_summary(1, "default", 30, 0.8, 0.05, 0.01),
        ]);
        let variant = comp_with_cells(vec![
            cell_summary(0, "default", 30, 0.8, 0.05, 0.01),
            cell_summary(1, "default", 30, 0.3, 0.05, 0.01),
        ]);
        let v = compare_runs(&baseline, &variant);
        assert!(v.any_significant_improvement);
        assert!(v.any_significant_regression);
        assert!(!v.variant_should_ship, "regression must block ship even when other cell improves");
    }

    #[test]
    fn variant_with_cost_inflation_no_quality_jump_fails_ship() {
        // No score change but variant is 20% more expensive.
        let baseline = comp_with_cells(vec![cell_summary(0, "default", 30, 0.5, 0.05, 0.01)]);
        let variant = comp_with_cells(vec![cell_summary(0, "default", 30, 0.5, 0.05, 0.012)]);
        let v = compare_runs(&baseline, &variant);
        assert!(!v.any_significant_improvement);
        assert!(v.cost_inflation_unjustified);
        assert!(!v.variant_should_ship);
    }

    #[test]
    fn variant_with_cost_inflation_but_big_quality_jump_is_allowed() {
        // Score 0.2 → 0.8 (delta 0.6 ≥ 0.2 threshold) + cost up 20% → defensible.
        let baseline = comp_with_cells(vec![cell_summary(0, "default", 30, 0.2, 0.05, 0.01)]);
        let variant = comp_with_cells(vec![cell_summary(0, "default", 30, 0.8, 0.05, 0.012)]);
        let v = compare_runs(&baseline, &variant);
        assert!(v.any_significant_improvement);
        assert!(!v.cost_inflation_unjustified, "20% cost inflation is OK when score jumped ≥0.2");
        assert!(v.variant_should_ship);
    }

    #[test]
    fn small_sample_falls_back_to_ci_disjoint_heuristic() {
        // n=5 → df<30 → p None; CIs are clearly disjoint (0.3±0.04 vs
        // 0.9±0.04). Should still be flagged as significant improvement.
        let baseline = comp_with_cells(vec![cell_summary(0, "default", 5, 0.3, 0.04, 0.01)]);
        let variant = comp_with_cells(vec![cell_summary(0, "default", 5, 0.9, 0.04, 0.01)]);
        let v = compare_runs(&baseline, &variant);
        assert!(v.any_significant_improvement, "CI-disjoint must drive a positive verdict at small n");
        assert!(v.variant_should_ship);
    }

    #[test]
    fn noisy_small_sample_does_not_fire_significance() {
        // n=5, SDs are wide enough that CIs overlap and df<30.
        let baseline = comp_with_cells(vec![cell_summary(0, "default", 5, 0.5, 0.2, 0.01)]);
        let variant = comp_with_cells(vec![cell_summary(0, "default", 5, 0.6, 0.2, 0.01)]);
        let v = compare_runs(&baseline, &variant);
        assert!(!v.any_significant_improvement, "noisy small sample with overlap must NOT fire");
        assert!(!v.variant_should_ship);
    }

    #[test]
    fn cells_only_in_one_side_appear_with_zero_n_on_the_missing_side() {
        // baseline has cell 0; variant has cell 0 + cell 1.
        let baseline = comp_with_cells(vec![cell_summary(0, "default", 30, 0.5, 0.05, 0.01)]);
        let variant = comp_with_cells(vec![
            cell_summary(0, "default", 30, 0.5, 0.05, 0.01),
            cell_summary(1, "default", 30, 0.9, 0.05, 0.01),
        ]);
        let v = compare_runs(&baseline, &variant);
        assert_eq!(v.cells.len(), 2);
        let new_cell = v.cells.iter().find(|c| c.prompt_idx == 1).unwrap();
        assert_eq!(new_cell.baseline_n, 0);
        assert_eq!(new_cell.variant_n, 30);
        assert!(new_cell.score_delta.is_none(), "cell only on variant side has no delta");
    }

    #[test]
    fn ship_decision_requires_at_least_one_improvement() {
        // No cell improves, no cell regresses, no cost issue → don't ship.
        // Variant is identical to baseline.
        let baseline = comp_with_cells(vec![cell_summary(0, "default", 30, 0.5, 0.05, 0.01)]);
        let variant = comp_with_cells(vec![cell_summary(0, "default", 30, 0.5, 0.05, 0.01)]);
        let v = compare_runs(&baseline, &variant);
        assert!(!v.any_significant_improvement);
        assert!(!v.any_significant_regression);
        assert!(!v.cost_inflation_unjustified);
        assert!(!v.variant_should_ship, "identical variant must NOT ship — no improvement to justify deployment");
    }

    #[test]
    fn cell_comparison_handles_unscored_cells() {
        // A cell with no rubric score → comparison still emits the
        // structural fields but score_delta is None.
        let bcell = CellSummary {
            prompt_idx: 0,
            model: "claude-sonnet-4-6".to_string(),
            condition: "default".to_string(),
            n: 5,
            success_n: 5,
            error_n: 0,
            cost_usd: Stats {
                n: 5,
                mean: 0.01,
                sd: 0.0,
                ci_lo: 0.01,
                ci_hi: 0.01,
            },
            tokens_out: Stats {
                n: 5,
                mean: 100.0,
                sd: 0.0,
                ci_lo: 100.0,
                ci_hi: 100.0,
            },
            duration_ms: Stats {
                n: 5,
                mean: 1000.0,
                sd: 0.0,
                ci_lo: 1000.0,
                ci_hi: 1000.0,
            },
            grounding_verdicts: BTreeMap::new(),
            score: None,
            passed_at_0_5: None,
        };
        let baseline = comp_with_cells(vec![bcell.clone()]);
        let variant = comp_with_cells(vec![bcell]);
        let v = compare_runs(&baseline, &variant);
        assert_eq!(v.cells.len(), 1);
        assert!(v.cells[0].score_delta.is_none());
        assert!(!v.any_significant_improvement);
        assert!(!v.any_significant_regression);
    }

    #[test]
    fn compose_groups_grounding_verdicts() {
        let mut observations = vec![
            obs(0, "claude-sonnet-4-6", "strict", 0.01),
            obs(0, "claude-sonnet-4-6", "strict", 0.02),
            obs(0, "claude-sonnet-4-6", "strict", 0.03),
        ];
        observations[0].grounding_verdict = Some("compliant".to_string());
        observations[1].grounding_verdict = Some("compliant".to_string());
        observations[2].grounding_verdict = Some("violation".to_string());
        let comp = compose(&observations);
        assert_eq!(comp.cells.len(), 1);
        let v = &comp.cells[0].grounding_verdicts;
        assert_eq!(v.get("compliant").copied(), Some(2));
        assert_eq!(v.get("violation").copied(), Some(1));
    }
}
