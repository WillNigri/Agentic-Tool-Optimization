// Binomial pass-rate statistics for the benchmark harness.
//
// A code-exec benchmark scores each of N distinct tasks as pass/fail
// (attempts=1). The right summary is NOT a point estimate — it's a binomial
// proportion with a confidence interval. We use the **Wilson score interval**,
// which stays inside [0,1] and behaves well at the extremes (0/30, 30/30)
// where the naive normal ("Wald") interval breaks.
//
// This is deliberately different from the methodology runner's
// `reps_per_cell=30`-reps-of-one-prompt model: there the unit is a repeated
// subjective score; here the unit is a distinct verifiable task. Two scorecards
// are only comparable when their task set + harness hash match — the CI tells
// you whether an apparent gap is real.

use serde::{Deserialize, Serialize};

/// z for common two-sided confidence levels. 95% is the default.
pub const Z_95: f64 = 1.959_963_984_540_054;
pub const Z_90: f64 = 1.644_853_626_951_472;
pub const Z_99: f64 = 2.575_829_303_548_901;

/// A binomial pass-rate with a Wilson score confidence interval.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WilsonInterval {
    /// Observed pass rate, `passes / n` (0.0 when `n == 0`).
    pub point: f64,
    /// Lower bound of the Wilson interval, clamped to [0,1].
    pub low: f64,
    /// Upper bound of the Wilson interval, clamped to [0,1].
    pub high: f64,
    /// Number of tasks that passed.
    pub passes: u64,
    /// Total number of tasks scored.
    pub n: u64,
    /// z value used (e.g. 1.96 for 95%).
    pub z: f64,
}

impl WilsonInterval {
    /// Half-width of the interval — a quick "± this much" for display.
    pub fn margin(&self) -> f64 {
        (self.high - self.low) / 2.0
    }

    /// Do two intervals overlap? Used to define reproducibility: a re-run
    /// reproduces if the harness hash matches AND the intervals overlap.
    pub fn overlaps(&self, other: &WilsonInterval) -> bool {
        self.low <= other.high && other.low <= self.high
    }
}

/// Compute the Wilson score interval for `passes` successes out of `n` trials.
///
/// `z` is the standard-normal quantile for the desired confidence (use the
/// `Z_95` / `Z_90` / `Z_99` constants). `n == 0` yields a degenerate `[0,1]`
/// interval with `point = 0` rather than a division-by-zero.
pub fn wilson_interval(passes: u64, n: u64, z: f64) -> WilsonInterval {
    if n == 0 {
        return WilsonInterval {
            point: 0.0,
            low: 0.0,
            high: 1.0,
            passes,
            n,
            z,
        };
    }

    let n_f = n as f64;
    let p_hat = passes as f64 / n_f;
    let z2 = z * z;

    let denom = 1.0 + z2 / n_f;
    let center = (p_hat + z2 / (2.0 * n_f)) / denom;
    let spread = (z / denom) * ((p_hat * (1.0 - p_hat) / n_f) + (z2 / (4.0 * n_f * n_f))).sqrt();

    WilsonInterval {
        point: p_hat,
        low: (center - spread).clamp(0.0, 1.0),
        high: (center + spread).clamp(0.0, 1.0),
        passes,
        n,
        z,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-5, "expected {b}, got {a}");
    }

    #[test]
    fn point_estimate_is_ratio() {
        let w = wilson_interval(18, 30, Z_95);
        approx(w.point, 0.6);
    }

    #[test]
    fn interval_stays_in_unit_range_at_extremes() {
        // All pass: point 1.0, but the interval must not exceed 1.0 or the
        // lower bound reflect the finite sample.
        let all = wilson_interval(30, 30, Z_95);
        approx(all.point, 1.0);
        // Wilson's upper bound at k=n approaches (but in f64 lands a hair below)
        // 1.0; the clamp guarantees it never exceeds it.
        assert!((all.high - 1.0).abs() < 1e-6, "high was {}", all.high);
        assert!(all.high <= 1.0);
        assert!(all.low > 0.8 && all.low < 1.0, "low was {}", all.low);

        // None pass: point 0.0, lower bound 0.0, upper bound reflects sample.
        let none = wilson_interval(0, 30, Z_95);
        approx(none.point, 0.0);
        assert_eq!(none.low, 0.0);
        assert!(none.high > 0.0 && none.high < 0.2, "high was {}", none.high);
    }

    #[test]
    fn matches_known_wilson_values() {
        // Reference (canonical Wilson, z=1.959964): 18/30 → [0.42320, 0.75409].
        // Cross-checks against the shrink-toward-0.5 behavior vs the Wald
        // interval [0.425, 0.775].
        let w = wilson_interval(18, 30, Z_95);
        approx(w.low, 0.423_204);
        approx(w.high, 0.754_094);
    }

    #[test]
    fn zero_trials_is_degenerate_not_nan() {
        let w = wilson_interval(0, 0, Z_95);
        assert!(!w.point.is_nan());
        assert_eq!(w.low, 0.0);
        assert_eq!(w.high, 1.0);
    }

    #[test]
    fn overlap_detects_reproducible_reruns() {
        let run1 = wilson_interval(20, 30, Z_95);
        let run2 = wilson_interval(19, 30, Z_95); // one task flipped
        assert!(run1.overlaps(&run2));

        let low = wilson_interval(3, 30, Z_95);
        let high = wilson_interval(27, 30, Z_95);
        assert!(!low.overlaps(&high));
    }

    #[test]
    fn tighter_confidence_gives_narrower_interval() {
        let w90 = wilson_interval(18, 30, Z_90);
        let w99 = wilson_interval(18, 30, Z_99);
        assert!(w90.margin() < w99.margin());
    }
}
