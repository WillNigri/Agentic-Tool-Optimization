import { useState } from "react";
import { X, Crown, Loader2 } from "lucide-react";
import { useAuthStore } from "@/hooks/useAuth";
import { login, register } from "@/lib/api";

interface LoginModalProps {
  onClose: () => void;
}

export default function LoginModal({ onClose }: LoginModalProps) {
  const [mode, setMode] = useState<"login" | "register">("login");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [name, setName] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const setAuth = useAuthStore((s) => s.setAuth);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!email || !password) return;
    setLoading(true);
    setError(null);

    try {
      let result;
      if (mode === "login") {
        result = await login(email, password);
      } else {
        if (!name.trim()) { setError("Name is required"); setLoading(false); return; }
        result = await register(email, password, name);
      }
      setAuth(result.user, result.tokens.accessToken, result.tokens.refreshToken);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Authentication failed");
    } finally {
      setLoading(false);
    }
  }

  return (
    <>
      <div className="fixed inset-0 bg-black/50 z-50" onClick={onClose} />
      <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
        <div
          className="bg-cs-card border border-cs-border rounded-xl w-full max-w-sm shadow-2xl"
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div className="flex items-center justify-between p-4 border-b border-cs-border">
            <div className="flex items-center gap-2">
              <Crown size={18} className="text-cs-accent" />
              <h3 className="text-lg font-semibold">
                {mode === "login" ? "Sign In" : "Create Account"}
              </h3>
            </div>
            <button
              onClick={onClose}
              className="p-1.5 rounded hover:bg-cs-border transition-colors text-cs-muted"
            >
              <X size={16} />
            </button>
          </div>

          <form onSubmit={handleSubmit} className="p-4 space-y-3">
            {/* Pro benefits */}
            <div className="rounded-lg border border-cs-accent/20 bg-cs-accent/5 p-3 mb-4">
              <p className="text-xs text-cs-accent font-medium mb-1.5">Pro includes:</p>
              <ul className="text-[11px] text-cs-muted space-y-1">
                <li>Cloud sync across machines</li>
                <li>Real-time cron monitoring</li>
                <li>Usage analytics dashboard</li>
                <li>Team workspaces</li>
              </ul>
            </div>

            {mode === "register" && (
              <input
                type="text"
                className="input"
                placeholder="Name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                required
              />
            )}

            <input
              type="email"
              className="input"
              placeholder="Email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
            />

            <input
              type="password"
              className="input"
              placeholder="Password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
            />

            {error && (
              <p className="text-xs text-red-400">{error}</p>
            )}

            <button
              type="submit"
              disabled={loading}
              className="w-full flex items-center justify-center gap-2 px-4 py-2.5 text-sm rounded-lg bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50"
            >
              {loading ? (
                <Loader2 size={14} className="animate-spin" />
              ) : (
                mode === "login" ? "Sign In" : "Create Account"
              )}
            </button>

            <p className="text-xs text-cs-muted text-center">
              {mode === "login" ? (
                <>Don't have an account?{" "}
                  <button type="button" onClick={() => setMode("register")} className="text-cs-accent hover:underline">
                    Sign up
                  </button>
                </>
              ) : (
                <>Already have an account?{" "}
                  <button type="button" onClick={() => setMode("login")} className="text-cs-accent hover:underline">
                    Sign in
                  </button>
                </>
              )}
            </p>
          </form>
        </div>
      </div>
    </>
  );
}
