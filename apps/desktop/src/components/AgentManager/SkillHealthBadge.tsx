import { CheckCircle, AlertTriangle, XCircle, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import type { SkillValidation } from "@/lib/tauri-api";

interface Props {
  validation?: SkillValidation | null;
  loading?: boolean;
  size?: "sm" | "md";
}

export default function SkillHealthBadge({ validation, loading, size = "sm" }: Props) {
  const iconSize = size === "sm" ? 12 : 16;

  if (loading) {
    return <Loader2 size={iconSize} className="animate-spin text-cs-muted" />;
  }

  if (!validation) {
    return null;
  }

  const hasErrors = validation.errors.length > 0;
  const hasWarnings = validation.warnings.length > 0;

  if (hasErrors) {
    return (
      <div className="flex items-center gap-1" title={`${validation.errors.length} error(s)`}>
        <XCircle size={iconSize} className="text-red-400" />
        {size === "md" && (
          <span className="text-xs text-red-400">{validation.errors.length}</span>
        )}
      </div>
    );
  }

  if (hasWarnings) {
    return (
      <div className="flex items-center gap-1" title={`${validation.warnings.length} warning(s)`}>
        <AlertTriangle size={iconSize} className="text-yellow-400" />
        {size === "md" && (
          <span className="text-xs text-yellow-400">{validation.warnings.length}</span>
        )}
      </div>
    );
  }

  return (
    <div className="flex items-center gap-1" title="Valid">
      <CheckCircle size={iconSize} className="text-green-400" />
    </div>
  );
}
