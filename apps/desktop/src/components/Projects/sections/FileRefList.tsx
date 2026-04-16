import { FileText, ExternalLink, Plus, Loader2 } from "lucide-react";
import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { cn } from "@/lib/utils";
import { writeAgentConfigFile, type ProjectFileRef } from "@/lib/api";
import { ScopeBadge, formatBytes } from "./SectionShell";

interface FileRefListProps {
  files: ProjectFileRef[];
  onOpen: (path: string) => void;
  emptyMessage?: string;
}

export default function FileRefList({ files, onOpen, emptyMessage }: FileRefListProps) {
  const queryClient = useQueryClient();
  const [creatingPath, setCreatingPath] = useState<string | null>(null);

  const createMutation = useMutation({
    mutationFn: (path: string) => writeAgentConfigFile(path, "", { skipValidation: true }),
    onSuccess: (_res, path) => {
      queryClient.invalidateQueries({ queryKey: ["project-bundle"] });
      queryClient.invalidateQueries({ queryKey: ["config-file", path] });
      setCreatingPath(null);
      onOpen(path);
    },
    onError: () => setCreatingPath(null),
  });

  const handleCreate = (path: string) => {
    setCreatingPath(path);
    createMutation.mutate(path);
  };

  if (files.length === 0) {
    return (
      <div className="rounded-md border border-dashed border-cs-border/60 bg-cs-bg/40 px-3 py-4 text-center text-xs text-cs-muted">
        {emptyMessage ?? "Nothing here yet."}
      </div>
    );
  }

  return (
    <ul className="space-y-1.5">
      {files.map((file) => {
        const isCreating = creatingPath === file.path;
        return (
          <li key={file.path}>
            <div
              className={cn(
                "group flex w-full items-center gap-3 rounded-md border border-cs-border/60 px-3 py-2 transition-colors",
                file.exists && "hover:border-cs-accent/40 hover:bg-cs-bg"
              )}
            >
              <button
                onClick={() => file.exists && onOpen(file.path)}
                disabled={!file.exists}
                className={cn(
                  "flex flex-1 items-center gap-3 text-left min-w-0",
                  !file.exists && "cursor-default"
                )}
              >
                <FileText
                  size={14}
                  className={cn("shrink-0", file.exists ? "text-cs-accent" : "text-cs-muted/60")}
                />
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span
                      className={cn(
                        "truncate text-sm",
                        !file.exists && "text-cs-muted"
                      )}
                    >
                      {file.label}
                    </span>
                    <ScopeBadge scope={file.scope} />
                    {!file.exists && (
                      <span className="rounded bg-cs-border/60 px-1.5 py-0.5 text-[10px] text-cs-muted">
                        not created
                      </span>
                    )}
                  </div>
                  <p className="mt-0.5 truncate font-mono text-[10px] text-cs-muted">{file.path}</p>
                </div>
                <div className="shrink-0 text-right text-[10px] text-cs-muted">
                  {file.exists && (
                    <>
                      <div>{formatBytes(file.sizeBytes)}</div>
                      <div>~{file.tokenEstimate.toLocaleString()} tok</div>
                    </>
                  )}
                </div>
                {file.exists && (
                  <ExternalLink
                    size={12}
                    className="shrink-0 text-cs-muted opacity-0 transition-opacity group-hover:opacity-100"
                  />
                )}
              </button>
              {!file.exists && (
                <button
                  onClick={() => handleCreate(file.path)}
                  disabled={isCreating || createMutation.isPending}
                  className="flex shrink-0 items-center gap-1 rounded-md border border-cs-border px-2 py-1 text-[11px] text-cs-muted transition-colors hover:border-cs-accent/40 hover:bg-cs-accent/10 hover:text-cs-accent disabled:opacity-50"
                  title={`Create ${file.path}`}
                >
                  {isCreating ? (
                    <Loader2 size={11} className="animate-spin" />
                  ) : (
                    <Plus size={11} />
                  )}
                  Create
                </button>
              )}
            </div>
          </li>
        );
      })}
    </ul>
  );
}
