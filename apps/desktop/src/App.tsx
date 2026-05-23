import { useEffect, useState } from "react";
import { Routes, Route, Navigate } from "react-router-dom";
import { useAuthStore } from "@/hooks/useAuth";
import Login from "@/pages/Login";
import Register from "@/pages/Register";
import Dashboard from "@/pages/Dashboard";
import UpdateBanner from "@/components/UpdateBanner";
import MigrationBanner from "@/components/Migration/MigrationBanner";
import IdentityProbeBanner from "@/components/Security/IdentityProbeBanner";

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
        await update.downloadAndInstall();
        // Mark as handled so we don't re-prompt if relaunch fails
        localStorage.setItem("ato.update.dismissedVersion", update.version);
        const { relaunch } = await import("@tauri-apps/plugin-process");
        await relaunch();
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
  const [needsManualUpgrade, setNeedsManualUpgrade] = useState(false);

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
