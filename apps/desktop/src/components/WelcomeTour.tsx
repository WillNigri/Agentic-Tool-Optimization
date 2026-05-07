import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Sparkles, ArrowRight, X, ChevronLeft, ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/useUiStore";

// v1.5.5 — First-run welcome tour.
//
// 3-slide modal that fires the first time a user opens ATO. The goal
// isn't to walk them through every feature — it's to plant the
// "your agents can adapt at fire time" mental model before they fall
// into the same trap Felipe did (building agents in static mode without
// realizing dynamic was an option).
//
// Gated by `localStorage["ato.welcome-tour.shown"]`. Dismiss → never
// fires again. Users can re-open via Settings → Help (TODO) or by
// clearing the localStorage key manually.

const STORAGE_KEY = "ato.welcome-tour.shown";

interface Slide {
  title: string;
  body: string;
  example?: string;
}

const SLIDES: Slide[] = [
  {
    title: "ATO builds agents that adapt — not repeat",
    body: "Most tools give you a single system-prompt textarea and call it a day. The agents you build here resolve their prompts at fire time — variables pulled from files, env vars, databases, and other LLMs.",
  },
  {
    title: "Variables are real — they resolve every turn",
    body: "Reference {user_name}, {project_root}, {today}, {recent_orders} in your prompt. Each has a resolver: env var, file read, db query, MCP call, or a JS expression you write. Resolution happens before every dispatch, on every conversation turn.",
    example: "You are a context-aware assistant for {user_name} on {project_name}.\nToday is {today}. Recent orders: {recent_orders}",
  },
  {
    title: "Pick the Production-grade Agent template to see it work",
    body: "Templates → Production-grade Agent comes pre-wired with 4 variables, a pre-call hook reading your CHANGELOG.md, and a memory policy. Click it once to see the dynamic pattern end-to-end. Then customize.",
  },
];

export default function WelcomeTour() {
  const { t } = useTranslation();
  const setSection = useUiStore((s) => s.setSection);
  const openCreateAgent = useUiStore((s) => s.openCreateAgent);
  const [open, setOpen] = useState(false);
  const [slide, setSlide] = useState(0);

  useEffect(() => {
    try {
      const seen = localStorage.getItem(STORAGE_KEY);
      if (!seen) setOpen(true);
    } catch {
      // localStorage unavailable — silent. Don't block the app.
    }
  }, []);

  const dismiss = () => {
    try {
      localStorage.setItem(STORAGE_KEY, "1");
    } catch {
      // ignore
    }
    setOpen(false);
  };

  const finish = () => {
    dismiss();
    // Send the user straight to the production template so they can
    // see the dynamic pattern instead of starting from the blank form.
    setSection("agents");
    openCreateAgent("quick", "production-grade");
  };

  if (!open) return null;
  const isLast = slide === SLIDES.length - 1;
  const current = SLIDES[slide];

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/70 backdrop-blur-sm p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) dismiss();
      }}
    >
      <div className="w-full max-w-lg rounded-2xl border border-cs-accent/30 bg-cs-card shadow-2xl overflow-hidden">
        {/* Header with close + progress dots */}
        <header className="flex items-center justify-between px-5 pt-4 pb-2">
          <span className="inline-flex items-center gap-1.5 text-[10px] font-mono uppercase tracking-wider text-cs-accent">
            <Sparkles size={11} />
            Welcome to ATO
          </span>
          <button
            type="button"
            onClick={dismiss}
            aria-label={t("common.close", "Close")}
            className="text-cs-muted hover:text-cs-text"
          >
            <X size={16} />
          </button>
        </header>

        {/* Slide body */}
        <div className="px-6 py-5 min-h-[260px] flex flex-col">
          <h2 className="text-xl font-semibold text-cs-text leading-snug">
            {current.title}
          </h2>
          <p className="mt-3 text-sm text-cs-muted leading-relaxed">
            {current.body}
          </p>
          {current.example && (
            <div className="mt-4 rounded-md border border-cs-border bg-cs-bg p-3 font-mono text-[11px] text-cs-text whitespace-pre-wrap leading-relaxed">
              {current.example}
            </div>
          )}
        </div>

        {/* Footer with progress + CTA */}
        <footer className="flex items-center justify-between gap-3 px-5 pb-4 pt-2 border-t border-cs-border">
          <div className="flex items-center gap-1.5">
            {SLIDES.map((_, i) => (
              <span
                key={i}
                className={cn(
                  "w-1.5 h-1.5 rounded-full transition-colors",
                  i === slide ? "bg-cs-accent" : "bg-cs-border"
                )}
              />
            ))}
          </div>
          <div className="flex items-center gap-2">
            {slide > 0 && (
              <button
                type="button"
                onClick={() => setSlide((s) => Math.max(0, s - 1))}
                className="inline-flex items-center gap-1 rounded-md border border-cs-border px-3 py-1.5 text-xs text-cs-muted hover:text-cs-text"
              >
                <ChevronLeft size={12} />
                Back
              </button>
            )}
            {!isLast ? (
              <button
                type="button"
                onClick={() => setSlide((s) => s + 1)}
                className="inline-flex items-center gap-1 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover"
              >
                Next
                <ChevronRight size={12} />
              </button>
            ) : (
              <button
                type="button"
                onClick={finish}
                className="inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover"
              >
                Open the template
                <ArrowRight size={12} />
              </button>
            )}
          </div>
        </footer>
      </div>
    </div>
  );
}
