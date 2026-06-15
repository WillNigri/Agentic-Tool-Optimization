import { useEffect, useRef, useState } from "react";
import { Routes, Route, Navigate } from "react-router-dom";
import { useAuthStore } from "@/hooks/useAuth";
import Login from "@/pages/Login";
import Register from "@/pages/Register";
import Dashboard from "@/pages/Dashboard";
import UpdateBanner from "@/components/UpdateBanner";
import MigrationBanner from "@/components/Migration/MigrationBanner";
import IdentityProbeBanner from "@/components/Security/IdentityProbeBanner";
import TetherApprovalModal from "@/components/tether/TetherApprovalModal";

const isTauri = typeof window !== 'undefined' && '__TAURI__' in window;

// F2 / S4 follow-up — Tauri's in-place auto-swap fails with EACCES on
// .deb and Snap installs (root-owned binaries / read-only squashfs).
// Felipe spent six days on v2.4.8 because the "Update now" button looked
// like it did nothing. On those install methods we suppress the ask()
// dialog and render <UpdateBanner /> instead, which surfaces a
// copy-pasteable `apt install` / `snap refresh` command. All other
// methods (macOS DMG, Windows MSI, AppImage, dev) keep the original
// ask() + downloadAndInstall flow.
async function checkForUpdates() {
  if (!isTauri) return;
  try {
    const { check } = await import("@tauri-apps/plugin-updater");
    const { ask } = await import("@tauri-apps/plugin-dialog");
    const update = await check();
    if (update) {
      // Skip if user already dismissed (or installed) this version
      const dismissed = localStorage.getItem("ato.update.dismissedVersion");
      if (dismissed === update.version) return;

      const yes = await ask(
        `ATO ${update.version} is available. Would you like to update now?`,
        { title: "Update Available", kind: "info", okLabel: "Update", cancelLabel: "Later" }
      );
      if (yes) {
        try {
          await update.downloadAndInstall();
          localStorage.setItem("ato.update.dismissedVersion", update.version);
          const { relaunch } = await import("@tauri-apps/plugin-process");
          await relaunch();
        } catch (installErr) {
          // downloadAndInstall can fail silently on macOS (Gatekeeper,
          // code signing, permissions). Show manual download link.
          console.error("Auto-update failed:", installErr);
          const { open } = await import("@tauri-apps/plugin-shell");
          const manualUrl = `https://github.com/WillNigri/Agentic-Tool-Optimization/releases/tag/v${update.version}`;
          await ask(
            `Auto-update failed. Please download v${update.version} manually from the releases page.`,
            { title: "Update Failed", kind: "warning", okLabel: "Open Downloads", cancelLabel: "Later" }
          ).then(async (openIt) => {
            if (openIt) await open(manualUrl);
          });
          localStorage.setItem("ato.update.dismissedVersion", update.version);
        }
      } else {
        // User clicked "Later" — suppress until next app launch cycle
        localStorage.setItem("ato.update.dismissedVersion", update.version);
      }
    }
  } catch (e) {
    console.error("Update check failed:", e);
  }
}

// Read the install method ONCE on mount; the result decides whether to
// run the ask() flow or mount the manual-upgrade banner. Failure
// (Tauri not present, command missing, etc.) falls through to the
// regular updater so we never silently swallow updates on other
// platforms.
async function detectNeedsManualUpgrade(): Promise<boolean> {
  if (!isTauri) return false;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const method = await invoke<string>("get_install_method");
    return method === "deb" || method === "snap";
  } catch {
    return false;
  }
}

function ProtectedRoute({ children }: { children: React.ReactNode }) {
  const isAuthenticated = useAuthStore((s) => s.isAuthenticated);

  // Desktop app is always "authenticated" — no login needed
  if (isTauri) return <>{children}</>;

  if (!isAuthenticated) {
    return <Navigate to="/login" replace />;
  }

  return <>{children}</>;
}

function PublicRoute({ children }: { children: React.ReactNode }) {
  const isAuthenticated = useAuthStore((s) => s.isAuthenticated);

  // Desktop app skips login/register pages entirely
  if (isTauri || isAuthenticated) {
    return <Navigate to="/" replace />;
  }

  return <>{children}</>;
}

export default function App() {
  const refreshTier = useAuthStore((s) => s.refreshTier);
  // v2.8.x chunks 3+4 war-room AMEND: MigrationBanner must not render
  // on /login or /register routes (UX leak per claude + minimax both).
  // Mirrors the existing isAuthenticated pattern at line 59.
  const isAuthenticatedForBanner = useAuthStore((s) => s.isAuthenticated);
  const isCloudUser = useAuthStore((s) => s.isCloudUser);
  const [needsManualUpgrade, setNeedsManualUpgrade] = useState(false);

  // v2.15 Wave 1 — E2E key bootstrap.
  // Fires once per session when the user becomes a cloud user (logged-in
  // Pro+). If no keypair exists in the keychain, generates one, stores it,
  // and pushes the public keys to the cloud so team admins can seal Team
  // Keys to this member. Wrapped in try/catch: E2E is opt-in per-resource,
  // so a keychain error here must not block the rest of the login flow.
  const e2eBootstrappedRef = useRef(false);
  useEffect(() => {
    if (!isCloudUser || e2eBootstrappedRef.current) return;
    e2eBootstrappedRef.current = true;
    void (async () => {
      try {
        const { hasE2eKeypair, storeE2eKeypair } = await import("@/lib/e2e/keychain");
        const { generateE2eKeypair, ensureSodiumReady } = await import("@/lib/e2e/crypto");
        const { pushE2ePublicKeys } = await import("@/lib/cloud-api");
        await ensureSodiumReady();
        if (!(await hasE2eKeypair())) {
          const kp = await generateE2eKeypair();
          await storeE2eKeypair(kp.x25519PrivateKey, kp.ed25519PrivateKey);
          await pushE2ePublicKeys(kp.x25519PublicKey, kp.ed25519PublicKey);
        }
      } catch (err) {
        // E2E bootstrap failure is non-fatal — log and continue.
        console.warn("[e2e] keypair bootstrap failed:", err);
      }
    })();
  }, [isCloudUser]);

  // v2.17 Wave 2 — tether host lifecycle.
  // Spawns the Rust tether-host WS task on Pro+ login; tears it down on
  // logout. Also starts the JS decrypt bridge so tether_decrypt events
  // from Rust are handled by the existing v2.15 crypto stack.
  // Both operations are non-fatal: a tether failure must never block the
  // rest of the app.
  const tetherStartedRef = useRef(false);
  useEffect(() => {
    if (!isTauri) return;
    if (isCloudUser && !tetherStartedRef.current) {
      tetherStartedRef.current = true;
      void (async () => {
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          const { getStoredTokens } = await import("@/lib/cloud-api");
          const tokens = getStoredTokens();
          if (!tokens?.accessToken) return;
          const { startTetherDecryptBridge } = await import("@/lib/tether/host");
          await startTetherDecryptBridge();
          await invoke("start_tether_host", { accessToken: tokens.accessToken });
        } catch (err) {
          console.warn("[tether] host start failed (non-fatal):", err);
        }
      })();
    } else if (!isCloudUser && tetherStartedRef.current) {
      tetherStartedRef.current = false;
      void (async () => {
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          const { stopTetherDecryptBridge } = await import("@/lib/tether/host");
          stopTetherDecryptBridge();
          await invoke("stop_tether_host");
        } catch (err) {
          console.warn("[tether] host stop failed (non-fatal):", err);
        }
      })();
    }
  }, [isCloudUser]);

  useEffect(() => {
    void (async () => {
      const manual = await detectNeedsManualUpgrade();
      if (manual) {
        // F2: render <UpdateBanner /> instead of the failing in-place
        // updater. The banner does its own check() + install-method
        // round-trip and renders null when no upgrade is pending, so
        // we just mount it and let it decide.
        setNeedsManualUpgrade(true);
      } else {
        checkForUpdates();
      }
    })();
    // Pull the latest subscription_tier on mount. If the user upgraded on
    // the web, the desktop tier badge updates without a re-login.
    void refreshTier();
  }, [refreshTier]);

  // v2.15 Wave 3 — Anon telemetry drain timer.
  //
  // Fires once per hour (3_600_000ms). Drains up to 100 oldest entries from
  // the local anon_telemetry_queue SQLite table, POSTs them to the cloud
  // batch endpoint, and clears the entries on success. Best-effort: any error
  // is silently swallowed so a telemetry failure never surfaces to the user.
  //
  // Only runs when in a Tauri context (the Tauri commands don't exist in
  // web preview mode).
  useEffect(() => {
    if (!isTauri) return;

    const drainTelemetry = async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const { postAnonTelemetryBatch } = await import("@/lib/cloud-api");

        type QueueEntry = { id: number; data_json: string };
        const entries = await invoke<QueueEntry[]>("anon_telemetry_drain_for_post");
        if (!entries || entries.length === 0) return;

        await postAnonTelemetryBatch(entries);
        const ids = entries.map((e) => e.id);
        await invoke("anon_telemetry_clear_ids", { ids });
      } catch {
        // Silently ignore — telemetry errors must never surface to users.
      }
    };

    // Run once on mount (in case there's a backlog from a previous session),
    // then every hour.
    void drainTelemetry();
    const timerId = setInterval(() => { void drainTelemetry(); }, 3_600_000);

    // Flush-on-exit best-effort via a Tauri window close listener.
    let removeCloseListener: (() => void) | undefined;
    void (async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const win = getCurrentWindow();
        const unlisten = await win.onCloseRequested(async () => {
          await drainTelemetry();
        });
        removeCloseListener = unlisten;
      } catch {
        // Window events not available (web preview etc.).
      }
    })();

    return () => {
      clearInterval(timerId);
      removeCloseListener?.();
    };
  }, []);

  return (
    <>
      {needsManualUpgrade && (
        <div className="fixed top-4 right-4 z-50 w-96 max-w-[90vw]">
          <UpdateBanner />
        </div>
      )}
      {/* v2.8.x Phase A chunk 4 — one-time re-tier migration banner. */}
      {(isTauri || isAuthenticatedForBanner) && <MigrationBanner />}
      {/* PR-5 master_key_v2 — identity-probe mismatch banner.
          v2.7.15 — moved to top-RIGHT, stacked under UpdateBanner via top-32. */}
      <div className="fixed top-32 right-4 z-50 w-96 max-w-[90vw]">
        <IdentityProbeBanner />
      </div>
      {/* v2.17 Wave 2 — tether approval modal (shown over any view). */}
      <TetherApprovalModal />
    <Routes>
      <Route
        path="/"
        element={
          <ProtectedRoute>
            <Dashboard />
          </ProtectedRoute>
        }
      />
      <Route
        path="/login"
        element={
          <PublicRoute>
            <Login />
          </PublicRoute>
        }
      />
      <Route
        path="/register"
        element={
          <PublicRoute>
            <Register />
          </PublicRoute>
        }
      />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
    </>
  );
}
