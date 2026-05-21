import { useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, X, ShoppingBag } from "lucide-react";
import { useUiStore } from "@/stores/useUiStore";

// Felipe P6 — when a user lands on the MCP-selection field in the Create
// Agent wizard but no MCPs are installed yet, the list is silently empty.
// Surface a warning that explains MCPs and routes to the catalog so new
// users don't ship agents with zero tools by accident.

interface Props {
  /** Called after navigating to the catalog so the wizard modal closes. */
  onClose: () => void;
}

export default function EmptyMcpsWarning({ onClose }: Props) {
  const { t } = useTranslation();
  const setSection = useUiStore((s) => s.setSection);
  const setSubTab = useUiStore((s) => s.setSubTab);
  const [dismissed, setDismissed] = useState(false);

  if (dismissed) return null;

  const browseCatalog = () => {
    setSection("skills");
    setSubTab("ato.subtab.skills", "mcps");
    onClose();
  };

  return (
    <div
      role="alert"
      data-testid="empty-mcps-warning"
      className="rounded-lg border border-cs-warning/40 bg-cs-warning/10 p-3 flex items-start gap-3"
    >
      <AlertTriangle size={16} className="text-cs-warning shrink-0 mt-0.5" />
      <div className="flex-1 min-w-0">
        <h4 className="text-xs font-semibold text-cs-text">
          {t("createAgent.emptyMcps.title", "No MCPs registered")}
        </h4>
        <p className="mt-1 text-[11px] text-cs-muted leading-relaxed">
          {t(
            "createAgent.emptyMcps.body",
            "MCPs give your agent tools like gmail, github, filesystem. Browse the catalog or skip and add them later.",
          )}
        </p>
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={browseCatalog}
            className="inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1.5 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover"
          >
            <ShoppingBag size={11} />
            {t("createAgent.emptyMcps.browse", "Browse catalog")}
          </button>
          <button
            type="button"
            onClick={() => setDismissed(true)}
            className="inline-flex items-center gap-1 rounded-md border border-cs-border bg-cs-bg-raised px-3 py-1.5 text-xs font-medium text-cs-muted hover:text-cs-text"
          >
            <X size={11} />
            {t("createAgent.emptyMcps.skip", "Skip for now")}
          </button>
        </div>
      </div>
    </div>
  );
}
