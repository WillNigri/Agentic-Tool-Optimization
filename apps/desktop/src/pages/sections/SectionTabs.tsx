import { useState, useEffect, Suspense, type ComponentType } from "react";
import { Loader2, type LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import ErrorBoundary from "@/components/ErrorBoundary";
import { useUiStore } from "@/stores/useUiStore";

// Shared tab strip + panel host for top-level sections (T1 IA collapse).
// Each tab's `Component` should be a React.lazy(...) wrapper defined at module scope
// in the parent section file — that keeps Suspense boundaries clean and avoids
// re-lazy-creating components on every render.

export type TabDef = {
  id: string;
  label: string;
  icon: LucideIcon;
  // Sections render their tab components with no props — they read their
  // own state from stores. Accept any component shape (regular + lazy)
  // since the actual props bag is always empty at the call site.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  Component: ComponentType<any>;
};

interface SectionTabsProps {
  storageKey: string;
  tabs: TabDef[];
  defaultTab?: string;
}

export default function SectionTabs({ storageKey, tabs, defaultTab }: SectionTabsProps) {
  const initial = (() => {
    try {
      const stored = localStorage.getItem(storageKey);
      if (stored && tabs.some((t) => t.id === stored)) return stored;
    } catch {
      // ignore
    }
    return defaultTab ?? tabs[0]?.id ?? "";
  })();

  const [active, setActive] = useState<string>(initial);

  // External override: if useUiStore.subTabs[storageKey] is set, honor it.
  // The demo runner uses this to switch sub-tabs without racing localStorage.
  const externalActive = useUiStore((s) => s.subTabs[storageKey] ?? null);
  useEffect(() => {
    if (externalActive && externalActive !== active && tabs.some((t) => t.id === externalActive)) {
      setActive(externalActive);
    }
  }, [externalActive, active, tabs]);

  useEffect(() => {
    try {
      localStorage.setItem(storageKey, active);
    } catch {
      // ignore
    }
  }, [storageKey, active]);

  const tab = tabs.find((t) => t.id === active) ?? tabs[0];
  const Panel = tab?.Component;

  return (
    <div className="flex flex-col h-full">
      <nav
        className="flex flex-wrap gap-1 border-b border-cs-border pb-2 mb-4"
        aria-label="Section tabs"
        role="tablist"
      >
        {tabs.map((t) => {
          const isActive = t.id === active;
          const Icon = t.icon;
          return (
            <button
              key={t.id}
              role="tab"
              aria-selected={isActive}
              onClick={() => {
                setActive(t.id);
                // Clear any external override (e.g. from a finished demo run)
                // so the user's click is the source of truth — otherwise the
                // controlling effect would race and revert.
                useUiStore.getState().setSubTab(storageKey, null);
              }}
              data-demo-id={`subtab-${storageKey}-${t.id}`}
              className={cn(
                "flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium transition-colors whitespace-nowrap",
                isActive
                  ? "bg-cs-accent/10 text-cs-accent"
                  : "text-cs-muted hover:bg-cs-border/50 hover:text-cs-text"
              )}
            >
              <Icon size={13} />
              {t.label}
            </button>
          );
        })}
      </nav>

      <div className="flex-1 min-h-0">
        {/* Re-key the boundary by tab id so switching tabs resets the
            error state cleanly. Without this, one broken tab would
            keep the fallback up after the user moves on. */}
        <ErrorBoundary key={tab?.id}>
          <Suspense
            fallback={
              <div className="flex items-center justify-center h-32">
                <Loader2 size={20} className="animate-spin text-cs-muted" />
              </div>
            }
          >
            {Panel ? <Panel /> : null}
          </Suspense>
        </ErrorBoundary>
      </div>
    </div>
  );
}
