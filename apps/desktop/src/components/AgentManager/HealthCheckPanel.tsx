import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  AlertTriangle,
  XCircle,
  CheckCircle,
  RefreshCw,
  FileText,
  Lightbulb,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { validateSkill, validateAllSkills, type SkillValidation, type ValidationIssue } from "@/lib/api";

interface Props {
  selectedPath?: string | null;
}

export default function HealthCheckPanel({ selectedPath }: Props) {
  const { t } = useTranslation();

  // Validate selected file
  const { data: selectedValidation, isLoading: isLoadingSelected, refetch: refetchSelected } = useQuery({
    queryKey: ["skill-validation", selectedPath],
    queryFn: () => selectedPath ? validateSkill(selectedPath) : null,
    enabled: !!selectedPath,
  });

  // Validate all skills
  const { data: allValidations = [], isLoading: isLoadingAll, refetch: refetchAll } = useQuery({
    queryKey: ["all-skill-validations"],
    queryFn: validateAllSkills,
  });

  const errorCount = allValidations.reduce((sum, v) => sum + v.errors.length, 0);
  const warningCount = allValidations.reduce((sum, v) => sum + v.warnings.length, 0);
  const validCount = allValidations.filter((v) => v.valid).length;

  return (
    <div className="h-full flex flex-col bg-cs-card">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-cs-border">
        <h3 className="font-medium">
          {t("agentManager.health.title", "Skill Health Check")}
        </h3>
        <button
          onClick={() => refetchAll()}
          disabled={isLoadingAll}
          className="p-1.5 rounded-md border border-cs-border hover:bg-cs-border/50 transition-colors"
          title={t("common.refresh", "Refresh")}
        >
          <RefreshCw size={14} className={isLoadingAll ? "animate-spin" : ""} />
        </button>
      </div>

      {/* Summary */}
      <div className="flex items-center gap-4 px-4 py-3 border-b border-cs-border bg-cs-bg/50">
        <div className="flex items-center gap-1.5">
          <CheckCircle size={14} className="text-green-400" />
          <span className="text-sm">{validCount} valid</span>
        </div>
        <div className="flex items-center gap-1.5">
          <AlertTriangle size={14} className="text-yellow-400" />
          <span className="text-sm">{warningCount} warnings</span>
        </div>
        <div className="flex items-center gap-1.5">
          <XCircle size={14} className="text-red-400" />
          <span className="text-sm">{errorCount} errors</span>
        </div>
      </div>

      {/* Selected file validation */}
      {selectedPath && selectedValidation && (
        <div className="px-4 py-3 border-b border-cs-border">
          <div className="flex items-center gap-2 mb-2">
            <FileText size={14} className="text-cs-muted" />
            <span className="text-sm font-medium truncate">
              {selectedPath.split("/").pop()}
            </span>
            <span className="text-xs text-cs-muted">~{selectedValidation.tokenCount} tokens</span>
          </div>

          {selectedValidation.errors.length === 0 && selectedValidation.warnings.length === 0 ? (
            <div className="flex items-center gap-2 text-sm text-green-400">
              <CheckCircle size={14} />
              <span>No issues found</span>
            </div>
          ) : (
            <div className="space-y-2">
              {selectedValidation.errors.map((issue, i) => (
                <IssueItem key={`error-${i}`} issue={issue} />
              ))}
              {selectedValidation.warnings.map((issue, i) => (
                <IssueItem key={`warning-${i}`} issue={issue} />
              ))}
            </div>
          )}
        </div>
      )}

      {/* All validations list */}
      <div className="flex-1 overflow-y-auto">
        {allValidations.filter((v) => !v.valid || v.warnings.length > 0).map((validation) => (
          <div
            key={validation.path}
            className="px-4 py-2 border-b border-cs-border/50 hover:bg-cs-border/20"
          >
            <div className="flex items-center justify-between mb-1">
              <span className="text-sm font-medium truncate max-w-[200px]">
                {validation.skillName || validation.path.split("/").pop()}
              </span>
              <div className="flex items-center gap-2">
                {validation.errors.length > 0 && (
                  <span className="flex items-center gap-1 text-xs text-red-400">
                    <XCircle size={10} />
                    {validation.errors.length}
                  </span>
                )}
                {validation.warnings.length > 0 && (
                  <span className="flex items-center gap-1 text-xs text-yellow-400">
                    <AlertTriangle size={10} />
                    {validation.warnings.length}
                  </span>
                )}
              </div>
            </div>
            <p className="text-xs text-cs-muted truncate">{validation.path}</p>
          </div>
        ))}

        {allValidations.filter((v) => !v.valid || v.warnings.length > 0).length === 0 && (
          <div className="flex flex-col items-center justify-center h-full text-cs-muted p-8">
            <CheckCircle size={32} className="mb-2 text-green-400 opacity-50" />
            <p className="text-sm">All skills are healthy</p>
          </div>
        )}
      </div>
    </div>
  );
}

function IssueItem({ issue }: { issue: ValidationIssue }) {
  const isError = issue.severity === "error";

  return (
    <div
      className={cn(
        "rounded-md p-2 text-sm",
        isError ? "bg-red-500/10 border border-red-500/20" : "bg-yellow-500/10 border border-yellow-500/20"
      )}
    >
      <div className="flex items-start gap-2">
        {isError ? (
          <XCircle size={14} className="text-red-400 mt-0.5 shrink-0" />
        ) : (
          <AlertTriangle size={14} className="text-yellow-400 mt-0.5 shrink-0" />
        )}
        <div className="flex-1 min-w-0">
          <p className={isError ? "text-red-400" : "text-yellow-400"}>
            {issue.message}
          </p>
          {issue.line && (
            <p className="text-xs text-cs-muted mt-0.5">Line {issue.line}</p>
          )}
          {issue.suggestion && (
            <div className="flex items-start gap-1.5 mt-1.5 text-xs text-cs-muted">
              <Lightbulb size={10} className="mt-0.5 shrink-0" />
              <span>{issue.suggestion}</span>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
