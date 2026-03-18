import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Search, Download, Star, CheckCircle } from "lucide-react";
import { cn } from "@/lib/utils";
import {
  MOCK_MARKETPLACE_SKILLS,
  MARKETPLACE_CATEGORIES,
  type MarketplaceSkill,
  type MarketplaceCategory,
} from "@/lib/marketplace-mock";
import { installMarketplaceSkill } from "@/lib/tauri-api";
import MarketplaceSkillDetail from "./MarketplaceSkillDetail";

export default function MarketplaceGrid() {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [selectedCategory, setSelectedCategory] = useState<MarketplaceCategory | "all">("all");
  const [selectedSkill, setSelectedSkill] = useState<MarketplaceSkill | null>(null);
  const [installedIds, setInstalledIds] = useState<Set<string>>(new Set());
  const [installingId, setInstallingId] = useState<string | null>(null);

  const filtered = MOCK_MARKETPLACE_SKILLS.filter((skill) => {
    const matchesSearch =
      !search.trim() ||
      skill.name.toLowerCase().includes(search.toLowerCase()) ||
      skill.description.toLowerCase().includes(search.toLowerCase()) ||
      skill.tags.some((tag) => tag.toLowerCase().includes(search.toLowerCase()));
    const matchesCategory = selectedCategory === "all" || skill.category === selectedCategory;
    return matchesSearch && matchesCategory;
  });

  async function handleInstall(skill: MarketplaceSkill) {
    setInstallingId(skill.id);
    try {
      await installMarketplaceSkill({
        id: skill.id,
        name: skill.name,
        content: skill.content,
      });
      setInstalledIds((prev) => new Set([...prev, skill.id]));
    } catch {
      // Handled by tauri-api fallback
      setInstalledIds((prev) => new Set([...prev, skill.id]));
    } finally {
      setInstallingId(null);
    }
  }

  return (
    <>
      <div className="space-y-4">
        {/* Search */}
        <div className="relative">
          <Search size={16} className="absolute left-3 top-1/2 -translate-y-1/2 text-cs-muted" />
          <input
            type="text"
            className="input pl-9"
            placeholder={t("marketplace.search")}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>

        {/* Category filter */}
        <div className="flex items-center gap-1.5 flex-wrap">
          <button
            onClick={() => setSelectedCategory("all")}
            className={cn(
              "px-2.5 py-1 text-[11px] font-medium rounded-full border transition-colors",
              selectedCategory === "all"
                ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                : "border-cs-border text-cs-muted hover:text-cs-text"
            )}
          >
            {t("marketplace.allCategories")}
          </button>
          {MARKETPLACE_CATEGORIES.map((cat) => (
            <button
              key={cat.id}
              onClick={() => setSelectedCategory(cat.id)}
              className={cn(
                "px-2.5 py-1 text-[11px] font-medium rounded-full border transition-colors",
                selectedCategory === cat.id
                  ? "border-cs-accent bg-cs-accent/10 text-cs-accent"
                  : "border-cs-border text-cs-muted hover:text-cs-text"
              )}
            >
              {t(`marketplace.categories.${cat.id}`)}
            </button>
          ))}
        </div>

        {/* Skills grid */}
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
          {filtered.map((skill) => {
            const isInstalled = installedIds.has(skill.id);
            const isInstalling = installingId === skill.id;
            const categoryLabel = MARKETPLACE_CATEGORIES.find((c) => c.id === skill.category)?.label || skill.category;

            return (
              <div
                key={skill.id}
                onClick={() => setSelectedSkill(skill)}
                className="card cursor-pointer transition-colors hover:border-cs-border/80"
              >
                {/* Header */}
                <div className="flex items-start justify-between gap-2 mb-2">
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-semibold truncate">{skill.name}</p>
                    <p className="text-[10px] text-cs-muted">{t("marketplace.author", { author: skill.author })}</p>
                  </div>
                  {/* Install button */}
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      if (!isInstalled && !isInstalling) handleInstall(skill);
                    }}
                    disabled={isInstalled || isInstalling}
                    className={cn(
                      "flex items-center gap-1 px-2.5 py-1 text-[11px] font-medium rounded-lg border transition-colors shrink-0",
                      isInstalled
                        ? "border-green-500/30 text-green-400 bg-green-500/10"
                        : isInstalling
                          ? "border-yellow-500/30 text-yellow-400 bg-yellow-500/10"
                          : "border-cs-accent/30 text-cs-accent bg-cs-accent/10 hover:bg-cs-accent/20"
                    )}
                  >
                    {isInstalled ? (
                      <>
                        <CheckCircle size={12} />
                        {t("marketplace.installed")}
                      </>
                    ) : isInstalling ? (
                      t("marketplace.installing")
                    ) : (
                      <>
                        <Download size={12} />
                        {t("marketplace.install")}
                      </>
                    )}
                  </button>
                </div>

                {/* Description */}
                <p className="text-xs text-cs-muted line-clamp-2 mb-2">{skill.description}</p>

                {/* Footer: category, tags, stats */}
                <div className="flex items-center gap-2 flex-wrap">
                  <span className="px-2 py-0.5 text-[10px] font-medium rounded-full border border-purple-500/30 bg-purple-500/10 text-purple-400">
                    {categoryLabel}
                  </span>
                  {skill.tags.slice(0, 3).map((tag) => (
                    <span
                      key={tag}
                      className="px-1.5 py-0.5 text-[10px] font-mono rounded border border-cs-border text-cs-muted"
                    >
                      {tag}
                    </span>
                  ))}
                  <div className="ml-auto flex items-center gap-3 text-[10px] text-cs-muted">
                    <span className="flex items-center gap-0.5">
                      <Download size={10} />
                      {skill.installs.toLocaleString()}
                    </span>
                    <span className="flex items-center gap-0.5">
                      <Star size={10} className="text-yellow-400" />
                      {skill.rating}
                    </span>
                  </div>
                </div>
              </div>
            );
          })}
        </div>

        {filtered.length === 0 && (
          <p className="text-cs-muted text-sm text-center py-8">
            {t("common.noResults")}
          </p>
        )}
      </div>

      {/* Detail panel */}
      {selectedSkill && (
        <MarketplaceSkillDetail
          skill={selectedSkill}
          isInstalled={installedIds.has(selectedSkill.id)}
          onInstall={() => handleInstall(selectedSkill)}
          onClose={() => setSelectedSkill(null)}
        />
      )}
    </>
  );
}
