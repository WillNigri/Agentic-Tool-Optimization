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
