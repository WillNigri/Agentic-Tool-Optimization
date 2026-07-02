// End-to-end grader tests against the SYNTHETIC MIT-clean fixture.
//
// These deliberately use hand-authored toy problems, never real LiveCodeBench
// corpus — the real corpus is fetched-and-pinned at runtime in a later slice
// and is never committed to this MIT repo (license: the LCB dataset is the
// under-specified generic `cc` tag over scraped LeetCode/Codeforces IP).
//
// The parse test runs everywhere. The execution tests are macOS-gated because
// they need a real sandbox: on this dev host that is `sandbox-exec` (Docker is
// preferred but absent locally). They actually run Python — the "real numbers"
// the dogfood workflow asks for.

use ato_bench::{
    grade_problem, select_sandbox, ExecLimits, FailureKind, GraderConfig, Problem, RunContext,
    Sampling, SandboxOptions,
};

fn load_fixtures() -> Vec<Problem> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/synthetic_problems.json"
    );
    let raw = std::fs::read_to_string(path).expect("read fixture");
    serde_json::from_str(&raw).expect("parse fixture")
}

fn ctx() -> RunContext {
    RunContext {
        model: "test-model".into(),
        provider: "test".into(),
        model_revision: None,
        model_cutoff: None,
        sampling: Sampling::default(),
    }
}

fn fenced(code: &str) -> String {
    format!("Here is my solution:\n```python\n{code}\n```\n")
}

fn problem(id: &str) -> Problem {
    load_fixtures()
        .into_iter()
        .find(|p| p.id == id)
        .unwrap_or_else(|| panic!("fixture {id} not found"))
}

#[test]
fn fixtures_parse_and_are_synthetic() {
    let probs = load_fixtures();
    assert_eq!(probs.len(), 3);
    for p in &probs {
        assert_eq!(p.source, "synthetic");
        assert!(
            p.release_date.is_none(),
            "fixtures carry no real release date"
        );
        assert!(!p.tests.is_empty());
    }
}

// ------------------------------ execution tests ---------------------------

#[cfg(target_os = "macos")]
mod exec {
    use super::*;
    use ato_bench::Language;

    fn sandbox() -> Box<dyn ato_bench::Sandbox> {
        select_sandbox(&SandboxOptions::default()).expect("a sandbox on macOS (seatbelt)")
    }

    #[test]
    fn correct_solution_passes_all_tests() {
        let sb = sandbox();
        let p = problem("synthetic/sum-two");
        let resp = fenced("a, b = map(int, input().split())\nprint(a + b)");
        let r = grade_problem(&p, &resp, &ctx(), &GraderConfig::default(), sb.as_ref());
        assert!(r.pass, "receipt: {r:?}");
        assert_eq!(r.tests_passed, r.tests_total);
        assert!(r.failure_kind.is_none());
        assert!(r.sandbox.network_isolated);
    }

    #[test]
    fn wrong_answer_is_detected() {
        let sb = sandbox();
        let p = problem("synthetic/sum-two");
        // subtracts instead of adds
        let resp = fenced("a, b = map(int, input().split())\nprint(a - b)");
        let r = grade_problem(&p, &resp, &ctx(), &GraderConfig::default(), sb.as_ref());
        assert!(!r.pass);
        assert_eq!(r.failure_kind, Some(FailureKind::WrongAnswer));
    }

    #[test]
    fn syntax_error_is_compile_failure() {
        let sb = sandbox();
        let p = problem("synthetic/reverse-string");
        let resp = fenced("def bad(:\n  print('nope')");
        let r = grade_problem(&p, &resp, &ctx(), &GraderConfig::default(), sb.as_ref());
        assert!(!r.pass);
        assert_eq!(r.failure_kind, Some(FailureKind::Compile));
    }

    #[test]
    fn empty_response_is_extraction_failure() {
        let sb = sandbox();
        let p = problem("synthetic/sum-two");
        let r = grade_problem(&p, "   ", &ctx(), &GraderConfig::default(), sb.as_ref());
        assert!(!r.pass);
        assert_eq!(r.failure_kind, Some(FailureKind::Extraction));
    }

    #[test]
    fn infinite_loop_hits_wall_clock_timeout() {
        let sb = sandbox();
        let p = problem("synthetic/sum-two");
        let resp = fenced("while True:\n  pass");
        let cfg = GraderConfig {
            limits: ExecLimits {
                wall_ms: 1_200,
                cpu_seconds: 10,
                ..ExecLimits::default()
            },
            ..GraderConfig::default()
        };
        let start = std::time::Instant::now();
        let r = grade_problem(&p, &resp, &ctx(), &cfg, sb.as_ref());
        let elapsed = start.elapsed();
        assert!(!r.pass);
        assert_eq!(r.failure_kind, Some(FailureKind::Timeout));
        // Killed near the deadline, not left running for the CPU limit.
        assert!(elapsed.as_millis() < 4_000, "took {elapsed:?}");
    }

    #[test]
    fn sandbox_blocks_network_access() {
        let sb = sandbox();
        let p = problem("synthetic/sum-two");
        // Tries to phone home before solving — must be denied by the sandbox.
        let resp = fenced(
            "import socket\nsocket.create_connection(('1.1.1.1', 53), 2)\nprint('SHOULD NOT REACH')",
        );
        let r = grade_problem(&p, &resp, &ctx(), &GraderConfig::default(), sb.as_ref());
        assert!(!r.pass, "network egress must not succeed");
        assert_eq!(r.failure_kind, Some(FailureKind::Runtime));
    }

    #[test]
    fn full_run_produces_reproducible_scorecard() {
        use ato_bench::{wilson_interval, Z_95};
        let sb = sandbox();
        let probs = load_fixtures();
        // A mix: solve two correctly, botch one, to exercise the pass-rate math.
        let solutions = [
            (
                "synthetic/sum-two",
                "a,b=map(int,input().split())\nprint(a+b)",
                true,
            ),
            ("synthetic/reverse-string", "print(input()[::-1])", true),
            (
                "synthetic/max-of-list",
                "input()\nprint(min(map(int,input().split())))",
                false,
            ),
        ];
        let mut passes = 0u64;
        for p in &probs {
            let (_, code, _) = solutions.iter().find(|(id, _, _)| *id == p.id).unwrap();
            let r = grade_problem(
                p,
                &fenced(code),
                &ctx(),
                &GraderConfig::default(),
                sb.as_ref(),
            );
            if r.pass {
                passes += 1;
            }
        }
        assert_eq!(passes, 2, "two correct, one wrong");
        let w = wilson_interval(passes, probs.len() as u64, Z_95);
        assert!((w.point - 2.0 / 3.0).abs() < 1e-9);
        assert!(w.low > 0.0 && w.high <= 1.0);
    }

    // --- security regressions from the cso review ---

    #[test]
    fn scratch_file_write_in_workdir_succeeds() {
        // A solution that writes a relative scratch file must be ALLOWED. This
        // is the realpath + CWD-confinement regression: before the fix, the
        // seatbelt allow-write rule (built from /var/folders/...) never matched
        // the real /private/var/folders/... path, so every disk write failed
        // closed and was mis-scored as a Runtime error.
        let sb = sandbox();
        let program = "open('scratch.txt','w').write('hi')\nprint(open('scratch.txt').read())";
        let out = sb
            .run(program, "", Language::Python, &ExecLimits::default())
            .expect("run");
        assert_eq!(out.exit_code, Some(0), "stderr: {}", out.stderr);
        assert_eq!(out.stdout.trim(), "hi");
    }

    #[test]
    fn reading_user_home_is_denied() {
        // The invoking user's home holds SSH/cloud creds. A hostile solution
        // must not be able to read it (network is already blocked, but this
        // closes the read-secret-then-smuggle-via-stdout path).
        let sb = sandbox();
        let home = std::env::var("HOME").unwrap();
        let program = format!("import os\nprint(os.listdir({home:?}))");
        let out = sb
            .run(&program, "", Language::Python, &ExecLimits::default())
            .expect("run");
        assert_ne!(
            out.exit_code,
            Some(0),
            "home read must be denied; stdout={}",
            out.stdout
        );
    }
}
