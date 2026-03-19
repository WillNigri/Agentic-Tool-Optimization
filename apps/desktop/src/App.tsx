import { useEffect } from "react";
import { Routes, Route, Navigate } from "react-router-dom";
import { useAuthStore } from "@/hooks/useAuth";
import Login from "@/pages/Login";
import Register from "@/pages/Register";
import Dashboard from "@/pages/Dashboard";

const isTauri = typeof window !== 'undefined' && '__TAURI__' in window;

async function checkForUpdates() {
  if (!isTauri) return;
  try {
    const { check } = await import("@tauri-apps/plugin-updater");
    const { ask, message } = await import("@tauri-apps/plugin-dialog");
    const update = await check();
    if (update) {
      const yes = await ask(
        `ATO ${update.version} is available. Would you like to update now?`,
        { title: "Update Available", kind: "info", okLabel: "Update", cancelLabel: "Later" }
      );
      if (yes) {
        await update.downloadAndInstall();
        const { relaunch } = await import("@tauri-apps/plugin-process");
        await relaunch();
      }
    }
  } catch (e) {
    console.error("Update check failed:", e);
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
  useEffect(() => {
    checkForUpdates();
  }, []);

  return (
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
  );
}
