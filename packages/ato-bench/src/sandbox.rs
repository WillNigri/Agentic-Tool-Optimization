// Sandboxed execution of model-generated code.
//
// Running code an LLM wrote is running untrusted code. The sandbox is therefore
// a first-class trust feature, not an implementation detail — the scorecard
// states which backend ran and what it isolated. Backends, in preference order:
//
//   1. Docker        — network=none, read-only bind mount, mem/cpu/pids limits,
//                      tmpfs scratch. The preferred, strongest isolation.
//   2. Seatbelt      — macOS `sandbox-exec`: profile denies network + confines
//                      writes to the work dir; CPU/file limits via `ulimit`;
//                      wall-clock kill. Used when Docker is absent on macOS.
//   3. Unconfined    — temp-dir + resource/wall limits ONLY. Network is NOT
//                      blocked. Refused unless the caller explicitly opts in;
//                      never selected silently.
//
// Security invariant shared by every backend: the model's code is written to a
// FILE and the test input is piped to STDIN. Neither is ever interpolated into
// a shell command line, so there is no argv/shell-injection surface from model
// output. Only paths and numeric limits we generate reach any shell string, and
// those are single-quoted.

use crate::problem::Language;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_STDOUT_CAP: usize = 256 * 1024;
const STDERR_CAP: usize = 16 * 1024;

/// What a backend isolated, recorded on the scorecard. Truthful by
/// construction: `network_isolated=false` on the unconfined backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SandboxReport {
    pub backend: String,
    pub network_isolated: bool,
    pub filesystem_isolated: bool,
    pub resource_limited: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Resource ceilings for one execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecLimits {
    /// Hard wall-clock limit; the process is killed past this.
    pub wall_ms: u64,
    /// CPU-seconds cap (`ulimit -t` on native backends, `--cpus`-adjacent on
    /// Docker via the wall limit).
    pub cpu_seconds: u64,
    /// Memory cap in MB. Enforced on Docker; best-effort/None on native.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mem_mb: Option<u64>,
    /// Max stdout bytes captured before truncation.
    pub stdout_cap: usize,
}

impl Default for ExecLimits {
    fn default() -> Self {
        Self {
            wall_ms: 10_000,
            cpu_seconds: 10,
            mem_mb: Some(512),
            stdout_cap: DEFAULT_STDOUT_CAP,
        }
    }
}

/// The result of one execution.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecOutcome {
    pub stdout: String,
    pub stderr: String,
    /// `None` if killed by signal / timeout.
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// True if stdout hit the cap and was truncated.
    pub stdout_truncated: bool,
}

#[derive(Debug)]
pub enum SandboxError {
    /// No usable backend for this host + options.
    Unavailable(String),
    /// Failed to prepare the work dir / write the program.
    Prepare(String),
    /// Failed to spawn the runner process.
    Spawn(String),
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SandboxError::Unavailable(m) => write!(f, "no sandbox available: {m}"),
            SandboxError::Prepare(m) => write!(f, "sandbox prepare failed: {m}"),
            SandboxError::Spawn(m) => write!(f, "sandbox spawn failed: {m}"),
        }
    }
}

impl std::error::Error for SandboxError {}

/// A backend that can run a program with piped stdin under isolation.
pub trait Sandbox: Send + Sync {
    fn report(&self) -> SandboxReport;
    /// Run `program` source (in `language`) with `stdin` piped in, under
    /// `limits`. Writes the program to an isolated temp dir internally.
    fn run(
        &self,
        program: &str,
        stdin: &str,
        language: Language,
        limits: &ExecLimits,
    ) -> Result<ExecOutcome, SandboxError>;
}

/// Options for backend selection.
#[derive(Debug, Clone)]
pub struct SandboxOptions {
    pub docker_image: String,
    /// Permit the unconfined backend (network NOT isolated). Off by default.
    pub allow_unconfined: bool,
}

impl Default for SandboxOptions {
    fn default() -> Self {
        Self {
            docker_image: "python:3.12-slim".to_string(),
            allow_unconfined: false,
        }
    }
}

/// Pick the strongest available backend, honoring the plan's "Docker preferred,
/// refuse/warn if unavailable". Returns an error (never a silent downgrade to
/// unconfined) unless the caller explicitly opted in.
pub fn select_sandbox(opts: &SandboxOptions) -> Result<Box<dyn Sandbox>, SandboxError> {
    if docker_available() {
        return Ok(Box::new(DockerSandbox {
            image: opts.docker_image.clone(),
        }));
    }
    if cfg!(target_os = "macos") && seatbelt_available() {
        return Ok(Box::new(SeatbeltSandbox));
    }
    if opts.allow_unconfined {
        return Ok(Box::new(UnconfinedSandbox));
    }
    Err(SandboxError::Unavailable(format!(
        "Docker not found and no OS sandbox available on {}. Install Docker (preferred) \
         for isolated execution, or pass --allow-unsandboxed to run code WITHOUT network \
         isolation (unsafe: only for code you trust).",
        std::env::consts::OS
    )))
}

fn docker_available() -> bool {
    Command::new("docker")
        .arg("info")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn seatbelt_available() -> bool {
    std::path::Path::new("/usr/bin/sandbox-exec").exists()
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Prepare an isolated temp dir with the program written to it. Returns the
/// TempDir (kept alive by the caller for cleanup), the CANONICAL work-dir path,
/// and the program path under it.
///
/// The canonicalization matters for the seatbelt profile: `tempfile` hands back
/// `/var/folders/...`, but macOS resolves `/var` → `/private/var` before
/// matching SBPL `subpath` rules. Without canonicalizing, the work-dir
/// write-allow rule never matches the real path and every scratch-file write
/// fails closed — silently mis-scoring solutions that write to disk.
fn prepare_workdir(
    program: &str,
    language: Language,
) -> Result<(tempfile::TempDir, std::path::PathBuf, std::path::PathBuf), SandboxError> {
    let dir = tempfile::Builder::new()
        .prefix("ato-bench-")
        .tempdir()
        .map_err(|e| SandboxError::Prepare(e.to_string()))?;
    let workdir = std::fs::canonicalize(dir.path()).unwrap_or_else(|_| dir.path().to_path_buf());
    let path = workdir.join(format!("program.{}", language.ext()));
    std::fs::write(&path, program.as_bytes()).map_err(|e| SandboxError::Prepare(e.to_string()))?;
    Ok((dir, workdir, path))
}

/// Build the `ulimit` prologue for native (seatbelt / unconfined) runs.
///
/// Portability notes verified on macOS (Darwin) + Linux:
///   • `-t` (CPU sec) and `-f` (file size) work on both.
///   • `-v`/`-d` (address space / data) CANNOT be modified via `ulimit` on
///     macOS ("Invalid argument"); they DO work on Linux. Emitted guarded so
///     it applies where supported and no-ops (silently) where not.
///   • `-u` (processes) is a per-user GLOBAL cap on macOS, so a fixed low value
///     blocks the interpreter from even starting on a busy machine. We instead
///     cap at (current user process count + headroom): the interpreter always
///     starts, but a fork bomb is bounded to `headroom` extra processes (on top
///     of the wall-clock process-group reap).
/// All lines are `2>/dev/null` so an unsupported limit never pollutes the
/// child's stderr (which the grader parses for SyntaxError).
fn ulimit_prologue(limits: &ExecLimits) -> String {
    let mut s = format!(
        "ulimit -t {cpu} 2>/dev/null; ulimit -f 262144 2>/dev/null; ",
        cpu = limits.cpu_seconds
    );
    // Dynamic process cap: current count + 128 headroom.
    s.push_str(
        "__n=$(ps -U \"$(id -ru)\" 2>/dev/null | wc -l | tr -d ' '); \
         [ -z \"$__n\" ] && __n=256; ulimit -u $((__n + 128)) 2>/dev/null; ",
    );
    if let Some(mb) = limits.mem_mb {
        // Address-space cap in KB — effective on Linux, no-op on macOS.
        s.push_str(&format!("ulimit -v {kb} 2>/dev/null; ", kb = mb * 1024));
    }
    s
}

/// Single-quote a string for safe embedding in a `sh -c` script. Only paths and
/// numbers we generate are ever passed here — never model output.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Spawn `cmd` with `stdin` piped, capture stdout/stderr on reader threads to
/// avoid pipe-buffer deadlock, and enforce a wall-clock deadline. `on_timeout`
/// runs just before the child is killed (e.g. to `docker kill` a container).
fn exec_capture(
    mut cmd: Command,
    stdin: &str,
    limits: &ExecLimits,
    on_timeout: impl Fn(),
) -> Result<ExecOutcome, SandboxError> {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Run in a fresh process group so a timeout can kill the whole tree
    // (sh -> sandbox-exec -> python), not just the process we hold.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| SandboxError::Spawn(e.to_string()))?;

    let mut stdin_pipe = child.stdin.take().expect("stdin piped");
    let stdout_pipe = child.stdout.take().expect("stdout piped");
    let stderr_pipe = child.stderr.take().expect("stderr piped");

    // Writer thread: feed stdin then close (drop) so the child sees EOF.
    let input = stdin.as_bytes().to_vec();
    let writer = thread::spawn(move || {
        let _ = stdin_pipe.write_all(&input);
        // stdin_pipe dropped here -> EOF for the child.
    });

    let out_cap = limits.stdout_cap;
    let out_reader = thread::spawn(move || read_capped(stdout_pipe, out_cap));
    let err_reader = thread::spawn(move || read_capped(stderr_pipe, STDERR_CAP));

    let deadline = Instant::now() + Duration::from_millis(limits.wall_ms);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    on_timeout();
                    kill_group(child.id());
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break None;
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(_) => break None,
        }
    };

    let _ = writer.join();
    let (stdout, stdout_truncated) = out_reader.join().unwrap_or_default();
    let (stderr, _) = err_reader.join().unwrap_or_default();

    Ok(ExecOutcome {
        stdout,
        stderr,
        exit_code: status.and_then(|s| s.code()),
        timed_out,
        stdout_truncated,
    })
}

/// Kill an entire process group by its leader PID (the child we spawned with
/// `process_group(0)`, so `pid == pgid`). Uses `kill(1)` to avoid a `libc`
/// dependency. No-op on non-unix.
#[cfg(unix)]
fn kill_group(pid: u32) {
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg(format!("-{pid}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(unix))]
fn kill_group(_pid: u32) {}

/// Read a pipe up to `cap` bytes; drain and discard the rest so the writer
/// never blocks. Returns the captured (lossy-UTF8) text and whether it was
/// truncated.
fn read_capped<R: Read>(mut r: R, cap: usize) -> (String, bool) {
    let mut buf = Vec::with_capacity(cap.min(8192));
    let mut chunk = [0u8; 8192];
    let mut truncated = false;
    loop {
        match r.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() < cap {
                    let take = n.min(cap - buf.len());
                    buf.extend_from_slice(&chunk[..take]);
                    if take < n {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
            Err(_) => break,
        }
    }
    (String::from_utf8_lossy(&buf).into_owned(), truncated)
}

// ---------------------------------------------------------------------------
// Seatbelt (macOS)
// ---------------------------------------------------------------------------

/// macOS `sandbox-exec` backend. Profile allows normal operation but denies all
/// network and confines file writes to the work dir. CPU/file-size limits via
/// `ulimit`; wall-clock via the shared killer.
pub struct SeatbeltSandbox;

impl SeatbeltSandbox {
    fn profile(workdir: &str, home: Option<&str>) -> String {
        // Last-match-wins SBPL: start permissive, then deny network entirely,
        // deny writes everywhere except the work dir and /dev, and deny reads of
        // the invoking user's home (SSH/cloud creds) so a hostile solution can't
        // slurp secrets. Network is already denied, but this closes the "read a
        // secret, smuggle it out via captured stdout into a stored receipt" path.
        let mut p = format!(
            "(version 1)\n\
             (allow default)\n\
             (deny network*)\n\
             (deny file-write* (subpath \"/\"))\n\
             (allow file-write* (subpath \"{workdir}\"))\n\
             (allow file-write* (subpath \"/dev\"))\n"
        );
        // Confine reads of home, but keep the work dir readable (it lives outside
        // home). Skip if home somehow contains the work dir.
        if let Some(h) = home {
            if !workdir.starts_with(h) {
                p.push_str(&format!("(deny file-read* (subpath \"{h}\"))\n"));
            }
        }
        p
    }
}

impl Sandbox for SeatbeltSandbox {
    fn report(&self) -> SandboxReport {
        SandboxReport {
            backend: "seatbelt".into(),
            network_isolated: true,
            filesystem_isolated: true,
            resource_limited: true,
            note: Some(
                "macOS sandbox-exec: network denied, writes confined to work dir, \
                 home reads denied. No HARD memory cap on native (macOS ulimit -v is \
                 unsupported) — bounded by wall-clock + soft process cap; use Docker \
                 for a hard memory/pids ceiling."
                    .into(),
            ),
        }
    }

    fn run(
        &self,
        program: &str,
        stdin: &str,
        language: Language,
        limits: &ExecLimits,
    ) -> Result<ExecOutcome, SandboxError> {
        let (dir, workdir_path, path) = prepare_workdir(program, language)?;
        let workdir = workdir_path.to_string_lossy().to_string();
        // Real home of the invoking user, captured BEFORE we override the child's
        // HOME env below. Canonicalized for parity with the work-dir rule, so a
        // symlinked home can't drift past the deny-read.
        let home = std::env::var("HOME").ok().map(|h| {
            std::fs::canonicalize(&h)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or(h)
        });
        let profile_path = workdir_path.join("sandbox.sb");
        std::fs::write(&profile_path, Self::profile(&workdir, home.as_deref()))
            .map_err(|e| SandboxError::Prepare(e.to_string()))?;

        let argv = language.run_argv(&path.to_string_lossy());
        // sh -c: set resource limits, then exec sandbox-exec with the profile.
        let quoted_argv = argv
            .iter()
            .map(|a| sh_quote(a))
            .collect::<Vec<_>>()
            .join(" ");
        let script = format!(
            "{prologue}exec sandbox-exec -f {profile} {argv}",
            prologue = ulimit_prologue(limits),
            profile = sh_quote(&profile_path.to_string_lossy()),
            argv = quoted_argv,
        );

        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg(script);
        harden_env(&mut cmd, &workdir);

        let outcome = exec_capture(cmd, stdin, limits, || {})?;
        drop(dir); // keep dir alive until here
                   // If `sandbox-exec` itself failed to initialize (e.g. a nested-sandbox
                   // denial: "sandbox_apply: Operation not permitted"), the model's code
                   // never ran. That is an INFRA failure, not the code failing — surface it
                   // as an error so the grader records FailureKind::Sandbox rather than
                   // blaming the model with Runtime and poisoning the pass rate.
        if !outcome.timed_out
            && outcome.exit_code != Some(0)
            && seatbelt_bootstrap_failed(&outcome.stderr)
        {
            return Err(SandboxError::Spawn(format!(
                "sandbox-exec failed to initialize (code never ran): {}",
                outcome.stderr.trim()
            )));
        }
        Ok(outcome)
    }
}

/// Detect `sandbox-exec`'s own bootstrap errors — markers that mean the sandbox
/// failed to apply, not that the program failed. These strings do not appear in
/// normal Python tracebacks.
fn seatbelt_bootstrap_failed(stderr: &str) -> bool {
    stderr.contains("sandbox_apply")
        || stderr.contains("sandbox-exec:")
        || stderr.contains("failed to initialize sandbox")
}

// ---------------------------------------------------------------------------
// Docker
// ---------------------------------------------------------------------------

/// Docker backend — the preferred, strongest isolation. NOTE: implemented and
/// reviewable by inspection, but not exercised on the macOS dev host (no Docker
/// installed); a Linux/CI integration slice will exercise it end to end.
pub struct DockerSandbox {
    pub image: String,
}

impl Sandbox for DockerSandbox {
    fn report(&self) -> SandboxReport {
        SandboxReport {
            backend: "docker".into(),
            network_isolated: true,
            filesystem_isolated: true,
            resource_limited: true,
            note: Some(format!(
                "docker image {}, --network none, read-only mount",
                self.image
            )),
        }
    }

    fn run(
        &self,
        program: &str,
        stdin: &str,
        language: Language,
        limits: &ExecLimits,
    ) -> Result<ExecOutcome, SandboxError> {
        let (dir, workdir_path, _path) = prepare_workdir(program, language)?;
        let workdir = workdir_path.to_string_lossy().to_string();
        let container_prog = format!("/work/program.{}", language.ext());
        // Unique container name so the timeout hook can kill it deterministically.
        let name = format!(
            "ato-bench-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );

        let mem = limits.mem_mb.unwrap_or(512);
        let mut cmd = Command::new("docker");
        cmd.arg("run")
            .arg("--rm")
            // PID 1 reaper so a fork bomb's zombies are collected in-container.
            .arg("--init")
            .args(["--name", &name])
            .args(["--network", "none"])
            .args(["--memory", &format!("{mem}m")])
            .args(["--cpus", "1"])
            .args(["--pids-limit", "128"])
            .arg("--read-only")
            // noexec scratch: writable for temp files, but no dropping+running a
            // binary. Explicit rather than relying on the daemon's tmpfs default.
            .args(["--tmpfs", "/tmp:size=64m,noexec"])
            // CWD on the writable tmpfs (the /work mount is read-only) so relative
            // scratch writes by the solution succeed, consistent with native.
            .args(["--workdir", "/tmp"])
            .args(["-v", &format!("{workdir}:/work:ro")])
            .arg("-i")
            .arg(&self.image);
        // language argv, but pointing at the in-container path.
        let mut argv = language.run_argv(&container_prog);
        // run_argv includes the host path as the last element; replace it.
        if let Some(last) = argv.last_mut() {
            *last = container_prog.clone();
        }
        cmd.args(&argv);

        let kill_name = name.clone();
        let outcome = exec_capture(cmd, stdin, limits, move || {
            let _ = Command::new("docker")
                .args(["kill", &kill_name])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        })?;
        drop(dir);
        Ok(outcome)
    }
}

// ---------------------------------------------------------------------------
// Unconfined (last resort — NOT network isolated)
// ---------------------------------------------------------------------------

/// Temp-dir + resource/wall limits only. Network is NOT blocked. Selected only
/// when the caller explicitly opts in; the report tells the truth.
pub struct UnconfinedSandbox;

impl Sandbox for UnconfinedSandbox {
    fn report(&self) -> SandboxReport {
        SandboxReport {
            backend: "unconfined".into(),
            network_isolated: false,
            filesystem_isolated: false,
            resource_limited: true,
            note: Some(
                "NO network isolation and NO filesystem confinement; \
                 resource/time limits only. Results ran untrusted code unsandboxed."
                    .into(),
            ),
        }
    }

    fn run(
        &self,
        program: &str,
        stdin: &str,
        language: Language,
        limits: &ExecLimits,
    ) -> Result<ExecOutcome, SandboxError> {
        let (dir, workdir_path, path) = prepare_workdir(program, language)?;
        let workdir = workdir_path.to_string_lossy().to_string();
        let argv = language.run_argv(&path.to_string_lossy());
        let quoted_argv = argv
            .iter()
            .map(|a| sh_quote(a))
            .collect::<Vec<_>>()
            .join(" ");
        let script = format!(
            "{prologue}exec {argv}",
            prologue = ulimit_prologue(limits),
            argv = quoted_argv,
        );
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg(script);
        harden_env(&mut cmd, &workdir);
        let outcome = exec_capture(cmd, stdin, limits, || {})?;
        drop(dir);
        Ok(outcome)
    }
}

/// Scope environment + CWD to the work dir. `python3 -I` already ignores
/// PYTHON* env and user site; setting HOME/TMPDIR and the working directory
/// keeps any relative-path or OS-level temp writes inside the confined dir
/// (which is exactly the dir the seatbelt profile allows writing to).
fn harden_env(cmd: &mut Command, workdir: &str) {
    cmd.current_dir(workdir)
        .env("HOME", workdir)
        .env("TMPDIR", workdir)
        .env("PYTHONDONTWRITEBYTECODE", "1");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sh_quote_escapes_single_quotes() {
        assert_eq!(sh_quote("abc"), "'abc'");
        assert_eq!(sh_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn read_capped_truncates_past_cap() {
        let data = b"hello world".to_vec();
        let (s, trunc) = read_capped(&data[..], 5);
        assert_eq!(s, "hello");
        assert!(trunc);
        let (s2, trunc2) = read_capped(&b"hi".to_vec()[..], 100);
        assert_eq!(s2, "hi");
        assert!(!trunc2);
    }

    #[test]
    fn unconfined_report_is_honest() {
        let r = UnconfinedSandbox.report();
        assert!(!r.network_isolated);
        assert!(!r.filesystem_isolated);
    }

    #[test]
    fn select_refuses_unconfined_without_optin_when_no_sandbox() {
        // On a host with neither docker nor seatbelt, selection must error
        // rather than silently downgrade. We can only assert the opt-in gate
        // shape here; on macOS seatbelt is present so this returns Ok.
        let opts = SandboxOptions {
            allow_unconfined: false,
            ..Default::default()
        };
        let res = select_sandbox(&opts);
        if cfg!(target_os = "macos") {
            assert!(res.is_ok(), "seatbelt should be selected on macOS");
        }
    }
}
