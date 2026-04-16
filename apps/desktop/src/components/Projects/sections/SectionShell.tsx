import { ReactNode } from "react";
import { cn } from "@/lib/utils";
import { LucideIcon } from "lucide-react";

interface SectionShellProps {
  icon: LucideIcon;
  title: string;
  subtitle?: string;
  count?: number | string;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}

export default function SectionShell({
  icon: Icon,
  title,
  subtitle,
  count,
  actions,
  children,
  className,
}: SectionShellProps) {
  return (
    <section className={cn("rounded-xl border border-cs-border bg-cs-card", className)} aria-label={title}>
      <header className="flex items-start justify-between gap-3 border-b border-cs-border px-4 py-3">
        <div className="flex items-start gap-3 min-w-0">
          <div className="mt-0.5 rounded-md bg-cs-accent/10 p-1.5 text-cs-accent shrink-0">
            <Icon size={14} />
          </div>
          <div className="min-w-0">
            <h2 className="text-sm font-semibold flex items-center gap-2">
              {title}
              {count !== undefined && (
                <span className="rounded-full bg-cs-border/60 px-2 py-0.5 text-[10px] font-normal text-cs-muted">
                  {count}
                </span>
              )}
            </h2>
            {subtitle && <p className="mt-0.5 text-[11px] text-cs-muted">{subtitle}</p>}
          </div>
        </div>
        {actions && <div className="shrink-0">{actions}</div>}
      </header>
      <div className="p-4">{children}</div>
    </section>
  );
}

export function ScopeBadge({ scope }: { scope: "user" | "project" | "nested" | "merged" }) {
  const styles: Record<string, string> = {
    user: "bg-blue-500/10 text-blue-300",
    project: "bg-cs-accent/10 text-cs-accent",
    nested: "bg-purple-500/10 text-purple-300",
    merged: "bg-cs-border text-cs-muted",
  };
  return (
    <span className={cn("rounded px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide", styles[scope])}>
      {scope}
    </span>
  );
}

export function EmptyRow({
  message,
  actionLabel,
  onAction,
}: {
  message: string;
  actionLabel?: string;
  onAction?: () => void;
}) {
  return (
    <div className="rounded-md border border-dashed border-cs-border/60 bg-cs-bg/40 px-3 py-4 text-center text-xs text-cs-muted">
      <p>{message}</p>
      {actionLabel && onAction && (
        <button
          onClick={onAction}
          className="mt-2 inline-flex items-center gap-1 rounded-md border border-cs-accent/30 bg-cs-accent/10 px-2.5 py-1 text-[11px] font-medium text-cs-accent transition-colors hover:bg-cs-accent/20"
        >
          {actionLabel}
        </button>
      )}
    </div>
  );
}

export function formatBytes(bytes: number): string {
  if (bytes === 0) return "—";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
