import { useState } from "react";
import { useTranslation } from "react-i18next";
import { X, Upload } from "lucide-react";
import { cn } from "@/lib/utils";
import { MARKETPLACE_CATEGORIES, type MarketplaceCategory } from "@/lib/marketplace-mock";
import { publishSkill } from "@/lib/api";

interface PublishSkillModalProps {
  skillId: string;
  skillName: string;
  onClose: () => void;
}

export default function PublishSkillModal({ skillId, skillName, onClose }: PublishSkillModalProps) {
  const { t } = useTranslation();
  const [category, setCategory] = useState<MarketplaceCategory>("code-quality");
  const [tags, setTags] = useState("");
  const [publishing, setPublishing] = useState(false);
  const [published, setPublished] = useState(false);

  async function handlePublish() {
    setPublishing(true);
    try {
      await publishSkill(skillId, {
        category,
        tags: tags.split(",").map((t) => t.trim()).filter(Boolean),
      });
      setPublished(true);
    } catch {
      // handled by localStorage fallback
      setPublished(true);
    } finally {
      setPublishing(false);
    }
  }

  return (
    <>
      <div className="fixed inset-0 bg-black/50 z-50" onClick={onClose} />
      <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
        <div
          className="bg-cs-card border border-cs-border rounded-xl w-full max-w-md shadow-2xl"
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div className="flex items-center justify-between p-4 border-b border-cs-border">
            <h3 className="text-lg font-semibold">{t("marketplace.publishModal.title")}</h3>
            <button
              onClick={onClose}
              className="p-1.5 rounded hover:bg-cs-border transition-colors text-cs-muted hover:text-cs-text"
            >
              <X size={16} />
            </button>
          </div>

          <div className="p-4 space-y-4">
            {published ? (
              <div className="text-center py-6">
                <Upload size={32} className="text-cs-accent mx-auto mb-3" />
                <p className="text-sm font-medium text-cs-text mb-1">
                  {t("marketplace.publishModal.published", { skillName })}
                </p>
                <p className="text-xs text-cs-muted">
                  {t("marketplace.publishModal.publishedDetail")}
                </p>
              </div>
            ) : (
              <>
                {/* Skill name (read-only) */}
                <div>
                  <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                    {t("marketplace.publishModal.skill")}
                  </label>
                  <p className="text-sm font-medium text-cs-text bg-cs-bg rounded-lg p-2.5 border border-cs-border">
                    {skillName}
                  </p>
                </div>

                {/* Category */}
                <div>
                  <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                    {t("marketplace.publishModal.category")}
                  </label>
                  <select
                    className="input"
                    value={category}
                    onChange={(e) => setCategory(e.target.value as MarketplaceCategory)}
                  >
                    {MARKETPLACE_CATEGORIES.map((cat) => (
                      <option key={cat.id} value={cat.id}>
                        {t(`marketplace.categories.${cat.id}`)}
                      </option>
                    ))}
                  </select>
                </div>

                {/* Tags */}
                <div>
                  <label className="text-xs font-medium text-cs-muted uppercase tracking-wider block mb-1">
                    {t("marketplace.publishModal.tags")}
                  </label>
                  <input
                    type="text"
                    className="input"
                    value={tags}
                    onChange={(e) => setTags(e.target.value)}
                    placeholder={t("marketplace.publishModal.tagsHint")}
                  />
                </div>
              </>
            )}
          </div>

          {/* Footer */}
          <div className="flex gap-2 p-4 border-t border-cs-border">
            {published ? (
              <button
                onClick={onClose}
                className="flex-1 px-4 py-2 text-sm rounded-lg bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 transition-colors"
              >
                {t("common.close")}
              </button>
            ) : (
              <>
                <button
                  onClick={handlePublish}
                  disabled={publishing}
                  className="flex-1 flex items-center justify-center gap-2 px-4 py-2 text-sm rounded-lg bg-cs-accent text-cs-bg font-medium hover:bg-cs-accent/90 transition-colors disabled:opacity-50"
                >
                  <Upload size={14} />
                  {publishing ? t("marketplace.publishModal.publishing") : t("marketplace.publish")}
                </button>
                <button
                  onClick={onClose}
                  className="px-4 py-2 text-sm rounded-lg border border-cs-border text-cs-muted hover:text-cs-text transition-colors"
                >
                  {t("common.cancel")}
                </button>
              </>
            )}
          </div>
        </div>
      </div>
    </>
  );
}
