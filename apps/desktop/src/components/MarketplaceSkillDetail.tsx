import { useTranslation } from "react-i18next";
import { X, Download, Star, CheckCircle, Tag } from "lucide-react";
import { cn } from "@/lib/utils";
import type { MarketplaceSkill } from "@/lib/marketplace-mock";
import { MARKETPLACE_CATEGORIES } from "@/lib/marketplace-mock";

interface MarketplaceSkillDetailProps {
  skill: MarketplaceSkill;
  isInstalled: boolean;
  onInstall: () => void;
  onClose: () => void;
}

export default function MarketplaceSkillDetail({
  skill,
  isInstalled,
  onInstall,
  onClose,
}: MarketplaceSkillDetailProps) {
  const { t } = useTranslation();
  const categoryLabel = MARKETPLACE_CATEGORIES.find((c) => c.id === skill.category)?.label || skill.category;

  return (
    <>
      {/* Backdrop */}
      <div className="fixed inset-0 bg-black/30 z-40 lg:hidden" onClick={onClose} />

      {/* Panel */}
      <div className="fixed right-0 top-0 bottom-0 w-full lg:w-1/2 max-w-2xl bg-cs-card border-l border-cs-border z-50 flex flex-col shadow-2xl animate-slide-in-right">
        {/* Header */}
        <div className="flex items-start justify-between p-4 border-b border-cs-border">
          <div className="min-w-0 flex-1">
            <h3 className="text-lg font-semibold truncate">{skill.name}</h3>
            <p className="text-xs text-cs-muted mt-0.5">
              {t("marketplace.author", { author: skill.author })}
            </p>
          </div>
          <button
            onClick={onClose}
            className="p-1.5 rounded hover:bg-cs-border transition-colors text-cs-muted hover:text-cs-text"
          >
            <X size={16} />
          </button>
        </div>

        <div className="p-4 space-y-5 overflow-y-auto flex-1">
          {/* Stats row */}
          <div className="flex items-center gap-4">
            <div className="flex items-center gap-1.5 text-sm">
              <Download size={14} className="text-cs-muted" />
              <span className="font-medium">{skill.installs.toLocaleString()}</span>
              <span className="text-cs-muted text-xs">installs</span>
            </div>
            <div className="flex items-center gap-1.5 text-sm">
              <Star size={14} className="text-yellow-400" />
              <span className="font-medium">{skill.rating}</span>
              <span className="text-cs-muted text-xs">/ 5</span>
            </div>
            <span className="px-2.5 py-0.5 text-[11px] font-medium rounded-full border border-purple-500/30 bg-purple-500/10 text-purple-400">
              {categoryLabel}
            </span>
          </div>

          {/* Description */}
          <div>
            <h4 className="text-xs font-medium text-cs-muted uppercase tracking-wider mb-1">
              Description
            </h4>
            <p className="text-sm text-cs-text">{skill.description}</p>
          </div>

          {/* Tags */}
          <div>
            <h4 className="text-xs font-medium text-cs-muted uppercase tracking-wider mb-2">
              Tags
            </h4>
            <div className="flex flex-wrap gap-1.5">
              {skill.tags.map((tag) => (
                <span
                  key={tag}
                  className="flex items-center gap-1 px-2 py-0.5 text-xs font-mono rounded-full border border-cs-border text-cs-muted"
                >
                  <Tag size={10} />
                  {tag}
                </span>
              ))}
            </div>
          </div>

          {/* Content preview */}
          <div>
            <h4 className="text-xs font-medium text-cs-muted uppercase tracking-wider mb-1">
              Skill Content
            </h4>
            <pre className="w-full p-3 bg-cs-bg border border-cs-border rounded-lg text-sm font-mono text-cs-text whitespace-pre-wrap overflow-x-auto max-h-80 overflow-y-auto">
              {skill.content}
            </pre>
          </div>

          {/* Dates */}
          <div className="flex items-center gap-4 text-[11px] text-cs-muted">
            <span>Created: {skill.createdAt}</span>
            <span>Updated: {skill.updatedAt}</span>
          </div>
        </div>

        {/* Footer */}
        <div className="flex gap-2 p-4 border-t border-cs-border">
          <button
            onClick={() => {
              if (!isInstalled) onInstall();
            }}
            disabled={isInstalled}
            className={cn(
              "flex-1 flex items-center justify-center gap-2 px-4 py-2.5 text-sm rounded-lg font-medium transition-colors",
              isInstalled
                ? "bg-green-500/10 border border-green-500/30 text-green-400"
                : "bg-cs-accent text-cs-bg hover:bg-cs-accent/90"
            )}
          >
            {isInstalled ? (
              <>
                <CheckCircle size={16} />
                {t("marketplace.installed")}
              </>
            ) : (
              <>
                <Download size={16} />
                {t("marketplace.install")}
              </>
            )}
          </button>
          <button
            onClick={onClose}
            className="px-4 py-2.5 text-sm rounded-lg border border-cs-border text-cs-muted hover:text-cs-text transition-colors"
          >
            {t("common.close")}
          </button>
        </div>
      </div>
    </>
  );
}
