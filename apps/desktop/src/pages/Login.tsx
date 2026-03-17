import { useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router-dom";
import { useAuthStore } from "@/hooks/useAuth";
import { login } from "@/lib/api";
import { useTranslation } from "react-i18next";

export default function Login() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const setAuth = useAuthStore((s) => s.setAuth);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setError("");
    setLoading(true);

    try {
      const result = await login({ email, password });
      setAuth(result.user, result.accessToken, result.refreshToken);
      navigate("/");
    } catch (err: unknown) {
      const apiErr = err as { message?: string };
      setError(apiErr.message || "Login failed");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center px-4">
      <div className="card w-full max-w-sm">
        <h1 className="text-2xl font-bold mb-1">{t("app.name")}</h1>
        <p className="text-cs-muted text-sm mb-6">{t("auth.login")}</p>

        {error && (
          <div className="bg-cs-danger/10 border border-cs-danger/30 text-cs-danger text-sm rounded-md px-3 py-2 mb-4">
            {error}
          </div>
        )}

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="block text-sm text-cs-muted mb-1">{t("auth.email")}</label>
            <input
              type="email"
              className="input"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder={t("auth.emailPlaceholder")}
              required
            />
          </div>
          <div>
            <label className="block text-sm text-cs-muted mb-1">
              {t("auth.password")}
            </label>
            <input
              type="password"
              className="input"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder={t("auth.password")}
              required
            />
          </div>
          <button type="submit" className="btn-primary w-full" disabled={loading}>
            {loading ? t("auth.signingIn") : t("auth.signIn")}
          </button>
        </form>

        <p className="text-sm text-cs-muted mt-4 text-center">
          {t("auth.noAccount")}{" "}
          <Link to="/register" className="text-cs-accent hover:underline">
            {t("auth.signUp")}
          </Link>
        </p>
      </div>
    </div>
  );
}
