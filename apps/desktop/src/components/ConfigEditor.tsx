import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { FileCheck, FileX } from "lucide-react";
import { getConfigFiles } from "@/lib/api";
import { cn } from "@/lib/utils";

export default function ConfigEditor() {
  const { t } = useTranslation();
  const { data: configs = [], isLoading } = useQuery({
    queryKey: ["config-files"],
    queryFn: getConfigFiles,
  });

  if (isLoading) {
    return <LoadingSkeleton />;
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-xl font-semibold mb-1">{t('config.title')}</h2>
        <p className="text-cs-muted text-sm">
          {t('config.subtitle')}
        </p>
      </div>

      {configs.length === 0 ? (
        <div className="card text-center py-12">
          <p className="text-cs-muted text-sm">
            {t('config.noConfig')}
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {configs.map((config) => (
            <div
              key={config.path}
              className="card flex items-center gap-3"
            >
              {config.exists ? (
                <FileCheck size={18} className="text-cs-success shrink-0" />
              ) : (
                <FileX size={18} className="text-cs-muted shrink-0" />
              )}
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <p className="text-sm font-medium truncate">{config.path}</p>
                  <span
                    className={cn(
                      "text-[10px] font-medium uppercase tracking-wider px-1.5 py-0.5 rounded shrink-0",
                      config.exists
                        ? "bg-cs-success/15 text-cs-success"
                        : "bg-cs-border/50 text-cs-muted"
                    )}
                  >
                    {config.exists ? t('common.success') : t('config.noConfig')}
                  </span>
                </div>
                <p className="text-xs text-cs-muted mt-0.5">{config.scope}</p>
              </div>
            </div>
          ))}
        </div>
      )}

      <div className="card bg-cs-bg border-dashed">
        <p className="text-sm text-cs-muted">
          {t('config.subtitle')}
        </p>
      </div>
    </div>
  );
}

function LoadingSkeleton() {
  return (
    <div className="space-y-6 animate-pulse">
      <div>
        <div className="h-6 w-36 bg-cs-border rounded" />
        <div className="h-4 w-64 bg-cs-border rounded mt-2" />
      </div>
      {[1, 2, 3, 4].map((i) => (
        <div key={i} className="card h-14" />
      ))}
    </div>
  );
}
