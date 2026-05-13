// v2.4.1 Phase 7.0 step 2 — mDNS broadcast + discovery for the ATO
// daemon mesh.
//
// Each daemon registers itself as `_ato._tcp.local` with TXT records
// carrying peer_id, friendly name, and ATO version. A parallel
// browser task watches for other instances of the same service type
// and upserts each into the `mesh_discovered` SQLite table the CLI
// reads via `ato mesh discovered`.
//
// Discovery DOES NOT imply trust — the table is just "what's on the
// network." Promoting a row into `mesh_peers` (trusted, can post
// messages) requires the pairing handshake landing in step 4.
//
// The mdns-sd crate runs its own thread internally and exposes a
// `flume::Receiver` for events; we adapt that to tokio with
// `tokio::task::spawn_blocking`. Keeps the rest of the daemon's
// async surface clean.

use anyhow::{Context, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use rusqlite::params;
use std::path::PathBuf;
use std::sync::Arc;

pub const SERVICE_TYPE: &str = "_ato._tcp.local.";

pub struct MdnsHandle {
    daemon: ServiceDaemon,
    fullname: String,
}

impl MdnsHandle {
    pub fn unregister(&self) {
        // Tell the network we're going away. The mdns-sd daemon
        // emits a goodbye packet; receivers prune our entry.
        // Best-effort — if the receiver is gone we don't care.
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

impl Drop for MdnsHandle {
    fn drop(&mut self) {
        // Called on SIGTERM-driven shutdown so peers see a clean
        // goodbye packet instead of having to TTL us out.
        self.unregister();
    }
}

/// Best-guess friendly hostname. Falls back to "ato-<peer_id[..8]>"
/// when the OS doesn't surface one (uncommon but happens in
/// containers without `/etc/hostname`).
pub fn friendly_hostname(peer_id: &str) -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("ato-{}", &peer_id[..8.min(peer_id.len())]))
}

/// Sanitize a hostname into the chars mdns-sd will accept inside an
/// instance name (letters, digits, dash). Service instance names can
/// be any UTF-8 in DNS-SD but we keep it ASCII-safe so the resulting
/// `<instance>._ato._tcp.local.` is round-trippable.
fn sanitize_instance(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    if trimmed.is_empty() {
        "ato".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Register this daemon on mDNS as `<host>.<peer_short>._ato._tcp.local`
/// and start a background browser task that upserts discoveries into
/// `mesh_discovered`. Returns a handle the caller drops at shutdown
/// to unregister + tear down the mdns thread.
pub fn start_mdns(
    db_path: Arc<PathBuf>,
    peer_id: &str,
    port: u16,
    version: &str,
) -> Result<MdnsHandle> {
    let daemon = ServiceDaemon::new().context("mdns-sd ServiceDaemon::new")?;

    let host_friendly = friendly_hostname(peer_id);
    // Use a per-instance name combining hostname + peer_id prefix so
    // two daemons on the same machine (rare but possible in dev)
    // don't collide. mDNS requires service names to be unique.
    let instance_name = format!(
        "{}-{}",
        sanitize_instance(&host_friendly),
        &peer_id[..8.min(peer_id.len())]
    );

    // mdns-sd needs a hostname ending in `.local.`; some setups have
    // a non-mDNS hostname so we synthesize one from the peer id which
    // is guaranteed unique. The friendly name still goes into a TXT
    // record so humans see something readable.
    let mdns_hostname = format!("ato-{}.local.", &peer_id[..16.min(peer_id.len())]);

    let properties: Vec<(&str, &str)> = vec![
        ("peer_id", peer_id),
        ("name", host_friendly.as_str()),
        ("version", version),
    ];

    // mdns-sd takes an &str slice for the IP; passing "" makes it
    // auto-detect non-loopback interfaces. Good default for "I want
    // peers on my LAN to find me."
    let info = ServiceInfo::new(
        SERVICE_TYPE,
        &instance_name,
        &mdns_hostname,
        "",
        port,
        &properties[..],
    )
    .context("ServiceInfo::new")?
    .enable_addr_auto();

    let fullname = format!("{}.{}", instance_name, SERVICE_TYPE);
    daemon
        .register(info)
        .context("ServiceDaemon::register")?;

    // Browser task — runs forever (until daemon.shutdown()) and
    // upserts each discovered service. Lives on a blocking thread
    // via spawn_blocking because mdns-sd's recv() is sync.
    let receiver = daemon.browse(SERVICE_TYPE).context("browse")?;
    let db_path_for_task = db_path.clone();
    let our_peer_id = peer_id.to_string();
    tokio::task::spawn_blocking(move || {
        // recv() blocks until a message lands or the daemon shuts
        // down (the channel closes). Either way we drain to completion.
        while let Ok(event) = receiver.recv() {
            match event {
                ServiceEvent::ServiceResolved(info) => {
                    if let Err(e) =
                        upsert_discovered(&db_path_for_task, &our_peer_id, &info)
                    {
                        eprintln!("ato daemon mdns: upsert failed: {}", e);
                    }
                }
                ServiceEvent::ServiceRemoved(_, fullname) => {
                    if let Err(e) = remove_discovered_by_fullname(&db_path_for_task, &fullname)
                    {
                        eprintln!("ato daemon mdns: remove failed: {}", e);
                    }
                }
                _ => {} // ignore SearchStarted / SearchStopped / etc.
            }
        }
    });

    Ok(MdnsHandle { daemon, fullname })
}

fn upsert_discovered(
    db_path: &PathBuf,
    our_peer_id: &str,
    info: &mdns_sd::ServiceInfo,
) -> Result<()> {
    let props = info.get_properties();
    let peer_id = match props.get_property_val_str("peer_id") {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => return Ok(()), // no peer_id TXT — not an ATO service
    };
    if peer_id == our_peer_id {
        // That's us, broadcasting our own service. Don't list
        // ourselves in the discovered table.
        return Ok(());
    }
    let name = props
        .get_property_val_str("name")
        .unwrap_or("(unknown)")
        .to_string();
    let version = props.get_property_val_str("version").map(|s| s.to_string());
    // Pick the first address mdns-sd resolved. They're IpAddr.
    let addrs = info.get_addresses();
    let first = addrs.iter().next();
    let addr_str = match first {
        Some(ip) => format!("{}:{}", ip, info.get_port()),
        None => return Ok(()), // not yet resolved; wait for the next event
    };
    let now = chrono::Utc::now().to_rfc3339();

    let conn = rusqlite::Connection::open(db_path).context("open ato db")?;
    conn.execute(
        "INSERT INTO mesh_discovered (peer_id, name, version, addr, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(peer_id) DO UPDATE SET
             name = excluded.name,
             version = excluded.version,
             addr = excluded.addr,
             last_seen_at = excluded.last_seen_at",
        params![peer_id, name, version, addr_str, now],
    )
    .context("upsert mesh_discovered")?;
    Ok(())
}

fn remove_discovered_by_fullname(db_path: &PathBuf, fullname: &str) -> Result<()> {
    // We don't know peer_id from `fullname` directly (mdns-sd's
    // ServiceRemoved gives the DNS name, not the TXT records). The
    // instance name we set includes the peer_id prefix, so extract
    // that and delete by prefix-match.
    let instance = fullname
        .split('.')
        .next()
        .unwrap_or(fullname);
    // The instance name format is `<host>-<peer8>`, so the suffix
    // after the LAST `-` is the prefix of peer_id we used.
    let peer_prefix = match instance.rsplit_once('-') {
        Some((_, p)) => p,
        None => return Ok(()),
    };
    if peer_prefix.is_empty() {
        return Ok(());
    }
    let conn = rusqlite::Connection::open(db_path).context("open ato db")?;
    conn.execute(
        "DELETE FROM mesh_discovered WHERE peer_id LIKE ?1",
        params![format!("{}%", peer_prefix)],
    )
    .context("delete mesh_discovered")?;
    Ok(())
}

/// Prune rows older than `max_age_seconds` from `mesh_discovered`.
/// Called from the daemon's main loop periodically so a peer going
/// offline (or moving to another network) doesn't linger forever.
pub fn prune_stale(db_path: &PathBuf, max_age_seconds: i64) -> Result<usize> {
    let conn = rusqlite::Connection::open(db_path).context("open ato db")?;
    let cutoff = chrono::Utc::now() - chrono::Duration::seconds(max_age_seconds);
    let n = conn
        .execute(
            "DELETE FROM mesh_discovered WHERE last_seen_at < ?1",
            params![cutoff.to_rfc3339()],
        )
        .context("prune mesh_discovered")?;
    Ok(n)
}

#[derive(Debug, serde::Serialize, Clone)]
pub struct DiscoveredPeer {
    pub peer_id: String,
    pub name: String,
    pub version: Option<String>,
    pub addr: String,
    pub last_seen_at: String,
}

pub fn list_discovered(db_path: &PathBuf) -> Result<Vec<DiscoveredPeer>> {
    let conn = rusqlite::Connection::open(db_path).context("open ato db")?;
    let mut stmt = conn.prepare(
        "SELECT peer_id, name, version, addr, last_seen_at
           FROM mesh_discovered
          ORDER BY last_seen_at DESC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(DiscoveredPeer {
                peer_id: r.get(0)?,
                name: r.get(1)?,
                version: r.get(2)?,
                addr: r.get(3)?,
                last_seen_at: r.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}
