// v2.4.0 Phase 7.0 — ATO daemon for bi-directional LAN mesh.
//
// This is step 1 of the Phase 7.0 plan (see PHASE-7-PLAN.md):
//   1. ✓ Daemon binary scaffold + Ed25519 keypair + pidfile + CLI
//      subcommand. This commit.
//   2. ☐ mDNS broadcast / discovery (`_ato._tcp.local`).
//   3. ☐ WS+JSON-RPC server with `post_completion` method.
//   4. ☐ Invite-code pairing handshake.
//   5. ☐ GUI Mesh tab in Settings → Runtimes.
//
// What this slice ships:
//   - `ato daemon start` — runs a foreground tokio runtime that
//     binds a TCP listener on 127.0.0.1:7755 (will become 0.0.0.0 in
//     step 2 once mDNS is live; localhost for now is the safe default).
//   - `ato daemon stop` — sends SIGTERM to the pidfile-recorded PID.
//   - `ato daemon status` — reports running / not-running + the peer
//     id derived from the keypair.
//   - Ed25519 keypair generated on first start at `~/.ato/daemon/keys/`.
//   - pidfile at `~/.ato/daemon/daemon.pid` so subsequent invocations
//     can find the running instance.
//
// What this slice does NOT do (deferred to later steps):
//   - No mDNS broadcast — peers can't discover each other yet.
//   - No protocol over the TCP listener; the bind exists to validate
//     the lifecycle + reserve the port. Connections are accepted and
//     immediately closed with a stub log line.
//   - No pairing / signing / message handling.
//
// Why this slice on its own: it lets us validate the daemon's
// install / launchctl / pidfile mechanics independent of the network
// protocol, which is the source of most "Phase 7 doesn't run cleanly"
// bugs we'd otherwise find later.

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub mod mdns;
pub mod protocol;

/// Default TCP port the daemon binds to. Localhost-only in step 1;
/// will become the LAN-advertised port once mDNS lands in step 2.
pub const DEFAULT_DAEMON_PORT: u16 = 7755;

/// Returns `~/.ato/daemon/`, creating it if missing.
fn daemon_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| anyhow!("neither HOME nor USERPROFILE set"))?;
    let dir = PathBuf::from(home).join(".ato").join("daemon");
    fs::create_dir_all(&dir).with_context(|| format!("mkdir {}", dir.display()))?;
    Ok(dir)
}

fn keys_dir() -> Result<PathBuf> {
    let d = daemon_dir()?.join("keys");
    fs::create_dir_all(&d).with_context(|| format!("mkdir {}", d.display()))?;
    Ok(d)
}

fn pidfile_path() -> Result<PathBuf> {
    Ok(daemon_dir()?.join("daemon.pid"))
}

/// Load or create the daemon's Ed25519 keypair. The private key is
/// written at 0600 on Unix; on Windows we rely on the default per-user
/// ACL. The public key is base64-encoded so we can drop it into a
/// CLI flag / URL without binary handling.
fn load_or_generate_keys() -> Result<(SigningKey, VerifyingKey)> {
    let dir = keys_dir()?;
    let priv_path = dir.join("private.bin");
    let pub_path = dir.join("public.bin");
    if priv_path.exists() && pub_path.exists() {
        let priv_bytes = fs::read(&priv_path).with_context(|| format!("read {}", priv_path.display()))?;
        if priv_bytes.len() != 32 {
            anyhow::bail!(
                "{}: expected 32 bytes, got {}",
                priv_path.display(),
                priv_bytes.len()
            );
        }
        let mut priv_arr = [0u8; 32];
        priv_arr.copy_from_slice(&priv_bytes);
        let signing = SigningKey::from_bytes(&priv_arr);
        let verifying = signing.verifying_key();
        return Ok((signing, verifying));
    }
    // First run — generate and persist.
    // v2.4.0 step 1 review caught a TOCTOU window: writing the key
    // first and then chmod'ing to 0600 leaves a small umask-window
    // where the file is world-readable. OpenOptions::mode() applies
    // the permission at create time on Unix, closing the window.
    let mut csprng = OsRng;
    let signing = SigningKey::generate(&mut csprng);
    let verifying = signing.verifying_key();
    {
        use std::io::Write as _;
        let mut f = open_locked_for_secret(&priv_path)
            .with_context(|| format!("create {}", priv_path.display()))?;
        f.write_all(&signing.to_bytes())
            .with_context(|| format!("write {}", priv_path.display()))?;
    }
    fs::write(&pub_path, verifying.to_bytes()).with_context(|| format!("write {}", pub_path.display()))?;
    Ok((signing, verifying))
}

/// Open `path` for writing with secret-file permissions baked in at
/// create time. Returns a fresh file (errors if it already exists)
/// so a malicious symlink can't redirect us to overwrite another
/// file the user owns.
fn open_locked_for_secret(path: &Path) -> std::io::Result<fs::File> {
    use std::fs::OpenOptions;
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        OpenOptions::new().write(true).create_new(true).open(path)
    }
}

/// Stable 32-byte peer id derived from the public key. We hash it
/// with SHA-256 so the id is fixed-length and doesn't accidentally
/// leak the raw public key in logs.
pub fn peer_id_for(public_key: &VerifyingKey) -> String {
    use std::fmt::Write as _;
    let bytes = public_key.to_bytes();
    let digest = simple_sha256(&bytes);
    let mut s = String::with_capacity(64);
    for b in digest {
        let _ = write!(&mut s, "{:02x}", b);
    }
    s
}

/// Tiny SHA-256 wrapper using sha2 (transitive via ed25519-dalek).
fn simple_sha256(data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    let result = h.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// Read the pidfile, returning Some(pid) when present + alive, else
/// None (and unlinking stale pidfiles so the next start has a clean
/// slate).
fn current_pid() -> Result<Option<u32>> {
    let path = pidfile_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let s = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let pid: u32 = match s.trim().parse() {
        Ok(p) => p,
        Err(_) => {
            let _ = fs::remove_file(&path);
            return Ok(None);
        }
    };
    if process_alive(pid) {
        Ok(Some(pid))
    } else {
        let _ = fs::remove_file(&path);
        Ok(None)
    }
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    // kill -0 returns 0 if the process exists and we can signal it.
    // For "exists at all" the signal-0 result is the right probe.
    unsafe { libc_kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    // Windows: a real implementation requires `windows-sys` for
    // `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, …)` +
    // `GetExitCodeProcess`. Phase 7.0 step 1 is macOS / Linux first;
    // we ship Windows support in a later slice rather than pretend
    // here.
    //
    // Returning `true` keeps `status` from confidently saying
    // "running: false" when we don't actually know — but `stop`
    // delegates to `taskkill` which gives an honest "no such pid"
    // error if the pidfile is stale. Trade-off: status is over-
    // reporting on Windows until the proper probe lands.
    //
    // TODO(phase-7.0-step2): replace with windows-sys OpenProcess.
    true
}

#[cfg(unix)]
extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(unix)]
#[allow(non_snake_case)]
unsafe fn libc_kill(pid: i32, sig: i32) -> i32 {
    kill(pid, sig)
}

/// `ato daemon start` — runs the daemon in the foreground. Spawning
/// it under launchd / systemd is the deployment path; for now we
/// support `ato daemon start &` for ad-hoc dev work too.
pub fn start(db_path: PathBuf) -> Result<()> {
    // Refuse to start twice. The pidfile is the canonical lock.
    if let Some(pid) = current_pid()? {
        anyhow::bail!(
            "daemon already running as pid {} ({}). Run `ato daemon stop` first.",
            pid,
            pidfile_path()?.display()
        );
    }

    let (signing, verifying) = load_or_generate_keys()?;
    let peer_id = peer_id_for(&verifying);
    let pub_b64 = base64::engine::general_purpose::STANDARD.encode(verifying.to_bytes());

    // Persist pid so `daemon stop` / `daemon status` can find us.
    // Atomic create — two `ato daemon start` invocations racing past
    // the current_pid() check above can't both win this write.
    // Review caught the TOCTOU; create_new(true) closes it.
    let pid = std::process::id();
    let pf = pidfile_path()?;
    {
        use std::fs::OpenOptions;
        use std::io::Write as _;
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pf)
            .with_context(|| {
                format!(
                    "another `ato daemon` raced past us — pidfile {} appeared between status check and write",
                    pf.display()
                )
            })?;
        writeln!(f, "{}", pid)?;
    }

    // Tokio runtime. Multi-thread because the future-self (WS + mDNS
    // + signal handling) will want it; cheap on idle either way.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;

    // Drop guard so the pidfile gets cleaned up even on panic.
    struct PidGuard(PathBuf);
    impl Drop for PidGuard {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let _guard = PidGuard(pidfile_path()?);

    rt.block_on(async {
        // Bind localhost-only in step 1. mDNS broadcast in step 2 will
        // flip this to 0.0.0.0 + an address restriction policy.
        let listener = match tokio::net::TcpListener::bind(("127.0.0.1", DEFAULT_DAEMON_PORT))
            .await
        {
            Ok(l) => l,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                // Most common cause: a previous daemon that exited
                // ungracefully without cleaning up its pidfile, OR an
                // unrelated process on the same port. Surface guidance
                // so the user knows what to try.
                anyhow::bail!(
                    "port {} is already in use. If a previous daemon crashed, try:\n  ato daemon stop\n  rm -f {}\n  ato daemon start\nIf an unrelated process owns the port, change DEFAULT_DAEMON_PORT (step 2 will make this configurable).",
                    DEFAULT_DAEMON_PORT,
                    pidfile_path().map(|p| p.display().to_string()).unwrap_or_else(|_| "~/.ato/daemon/daemon.pid".into())
                );
            }
            Err(e) => return Err(anyhow!("bind 127.0.0.1:{}: {}", DEFAULT_DAEMON_PORT, e)),
        };

        // Stamp identity to stdout so a `daemon start` invocation
        // is visibly identifiable in launchd / systemd logs.
        println!(
            "ato daemon: pid={} peer_id={} pubkey={} port={}",
            pid, peer_id, pub_b64, DEFAULT_DAEMON_PORT
        );
        // Drop the keypair var to silence the unused-warning while
        // we don't sign anything yet. Step 3 reuses it.
        let _ = signing;

        // v2.4.1 — register on mDNS and start the discovery browser.
        // Errors here don't bring the daemon down; mesh discovery is
        // best-effort (some networks block mDNS) and the local TCP
        // listener still works for direct-pair scenarios.
        let db_path_arc = Arc::new(db_path.clone());
        // Held until shutdown so its Drop fires unregister() on the
        // way out — peers see a goodbye packet instead of TTL-ing us
        // out of their list. Prefixed with `_` to silence the
        // unused-binding warning; the binding's lifetime is the
        // whole point.
        let _mdns_handle = match mdns::start_mdns(
            db_path_arc.clone(),
            &peer_id,
            DEFAULT_DAEMON_PORT,
            env!("CARGO_PKG_VERSION"),
        ) {
            Ok(h) => {
                println!(
                    "ato daemon: mdns registered as _ato._tcp.local (broadcasting + browsing)"
                );
                Some(h)
            }
            Err(e) => {
                eprintln!(
                    "ato daemon: mdns disabled — {}. Local TCP still up; pair via invite code instead.",
                    e
                );
                None
            }
        };

        // Prune stale discoveries every 60s. Anything not refreshed
        // within 5 minutes drops off the list — a peer that went
        // offline or moved networks should disappear, not linger.
        let db_path_for_prune = db_path_arc.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(60));
            tick.tick().await; // skip the immediate first tick
            loop {
                tick.tick().await;
                if let Err(e) = mdns::prune_stale(&db_path_for_prune, 300) {
                    eprintln!("ato daemon: prune_stale failed: {}", e);
                }
            }
        });

        // Signal handling: SIGTERM / SIGINT cleanly stops the loop.
        // tokio::select! doesn't accept #[cfg] attrs on match arms, so
        // we factor the shutdown future into a platform-specific
        // helper that returns the moment any termination signal fires.
        let shutdown = async {
            #[cfg(unix)]
            {
                let mut sigterm =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("install SIGTERM handler");
                let mut sigint =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                        .expect("install SIGINT handler");
                tokio::select! {
                    _ = sigterm.recv() => "SIGTERM",
                    _ = sigint.recv() => "SIGINT",
                }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
                "CTRL-C"
            }
        };
        tokio::pin!(shutdown);

        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    match accepted {
                        Ok((sock, peer_addr)) => {
                            // v2.4.3 step 3 — WS+JSON-RPC handler.
                            // Each connection runs on its own task so
                            // a slow / hostile peer can't block the
                            // accept loop. The task takes ownership of
                            // the socket; the db_path Arc lets it
                            // hit SQLite without borrowing across .await.
                            //
                            // Review finding #9 (Gemini): tokio::spawn
                            // swallows panics from the spawned future
                            // silently. JoinHandle::await surfaces
                            // them as a JoinError; we log it so a
                            // crash inside protocol handling is
                            // visible in launchctl / systemd logs.
                            let db_for_task = db_path_arc.clone();
                            let join = tokio::spawn(async move {
                                protocol::handle_connection(sock, db_for_task).await;
                            });
                            tokio::spawn(async move {
                                if let Err(e) = join.await {
                                    if e.is_panic() {
                                        eprintln!(
                                            "ato daemon: ws handler for {} PANICKED: {:?}",
                                            peer_addr, e
                                        );
                                    } else if e.is_cancelled() {
                                        eprintln!(
                                            "ato daemon: ws handler for {} cancelled",
                                            peer_addr
                                        );
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("ato daemon: accept error: {}", e);
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
                    }
                }
                sig = &mut shutdown => {
                    eprintln!("ato daemon: {} received, shutting down", sig);
                    break;
                }
            }
        }
        anyhow::Ok(())
    })?;
    Ok(())
}

/// `ato daemon stop` — sends SIGTERM to the recorded pid.
pub fn stop() -> Result<()> {
    let pid = match current_pid()? {
        Some(p) => p,
        None => {
            println!("ato daemon: not running");
            return Ok(());
        }
    };
    #[cfg(unix)]
    {
        let rc = unsafe { libc_kill(pid as i32, 15) };
        if rc != 0 {
            anyhow::bail!("kill pid {} returned {}", pid, rc);
        }
    }
    #[cfg(not(unix))]
    {
        let out = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()
            .context("spawn taskkill")?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            anyhow::bail!("taskkill failed: {}", stderr.trim());
        }
    }
    // Best-effort: pidfile may still be present until the daemon's
    // drop guard fires. Clean it up after a brief wait.
    std::thread::sleep(Duration::from_millis(200));
    let _ = fs::remove_file(pidfile_path()?);
    println!("ato daemon: stopped (pid {})", pid);
    Ok(())
}

#[derive(Debug, serde::Serialize)]
pub struct DaemonStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub peer_id: String,
    pub public_key_b64: String,
    pub port: u16,
    pub keys_path: String,
}

/// `ato daemon status` — reports running state + identity.
pub fn status() -> Result<DaemonStatus> {
    let (_, verifying) = load_or_generate_keys()?;
    let peer_id = peer_id_for(&verifying);
    let pub_b64 = base64::engine::general_purpose::STANDARD.encode(verifying.to_bytes());
    let pid = current_pid()?;
    Ok(DaemonStatus {
        running: pid.is_some(),
        pid,
        peer_id,
        public_key_b64: pub_b64,
        port: DEFAULT_DAEMON_PORT,
        keys_path: keys_dir()?.display().to_string(),
    })
}

/// Convenience: returns the daemon's directory (for callers that
/// need to drop adjacent files like sockets or logs).
#[allow(dead_code)]
pub fn root_dir() -> Result<PathBuf> {
    daemon_dir()
}

/// Used by tests / debug to bypass the global daemon_dir() and
/// resolve a known leaf.
#[allow(dead_code)]
pub fn pidfile() -> Result<PathBuf> {
    pidfile_path()
}

#[allow(dead_code)]
fn _root(_p: &Path) {}
