import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { FolderOpen, ExternalLink } from "lucide-react";
import { cn } from "@/lib/utils";

interface ClickablePathProps {
  path: string;
  /**
   * `reveal` (default) shows the path in the OS file manager with the
   * entry selected; `open` hands it to the default handler.
   */
  mode?: "reveal" | "open";
  className?: string;
}

/**
 * A monospace path that opens in the OS file manager on click, plus an
 * existence pill that reflects whether the path still resolves on disk.
 *
 * Backed by the `reveal_path` / `open_path` / `path_exists` Tauri
 * commands (see commands/fs_actions.rs).
 */
export default function ClickablePath({ path, mode = "reveal", className }: ClickablePathProps) {
  const { t } = useTranslation();
  const [error, setError] = useState<string | null>(null);

  const { data: exists, isLoading } = useQuery({
    queryKey: ["path-exists", path],
    queryFn: () => invoke<boolean>("path_exists", { path }),
    // Cheap fs::metadata call, but the answer can go stale — let callers
    // refetch on focus rather than holding a forever-fresh value.
    staleTime: 10_000,
  });

  const handleClick = async () => {
    setError(null);
    try {
      await invoke(mode === "open" ? "open_path" : "reveal_path", { path });
    } catch (e) {
      setError(String(e));
    }
  };

  const Icon = mode === "open" ? ExternalLink : FolderOpen;
  const actionLabel = mode === "open" ? t("clickablePath.open") : t("clickablePath.reveal");

  return (
    <span className={cn("inline-flex items-center gap-1.5 max-w-full", className)}>
      <button
        type="button"
        onClick={handleClick}
        title={error ?? `${actionLabel} ${path}`}
        aria-label={`${actionLabel} ${path}`}
        className={cn(
          "inline-flex items-center gap-1 min-w-0 font-mono text-xs text-cs-text",
          "hover:text-cs-accent transition-colors truncate",
          error && "text-cs-danger",
        )}
      >
        <Icon className="w-3 h-3 shrink-0" />
        <span className="truncate">{path}</span>
      </button>

      {!isLoading && (
        <span
          className={cn(
            "shrink-0 inline-flex items-center rounded-full px-1.5 py-0.5 text-[10px] font-medium",
            exists
              ? "bg-cs-accent/10 text-cs-accent"
              : "bg-cs-danger/10 text-cs-danger",
          )}
        >
          {exists ? t("clickablePath.exists") : t("clickablePath.gone")}
        </span>
      )}
    </span>
  );
}
