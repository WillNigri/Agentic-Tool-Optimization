import { lazy, Suspense, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Plus } from "lucide-react";
import AddMcpModal from "./AddMcpModal";

// T4 — MCP manager. Wraps existing McpDashboard (which is the read-only view of
// running MCP servers) and adds a "+ Add MCP" entry point that opens the install
// modal (registry browser + custom install).

const McpDashboard = lazy(() => import("@/components/McpDashboard"));

export default function McpManager() {
  const { t } = useTranslation();
  const [addOpen, setAddOpen] = useState(false);

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-end">
        <button
          type="button"
          onClick={() => setAddOpen(true)}
          className="inline-flex items-center gap-2 rounded-lg bg-cs-accent px-4 py-2 text-sm font-medium text-cs-bg hover:bg-cs-accent-hover"
        >
          <Plus size={14} />
          {t("mcpManager.addButton", "Add MCP")}
        </button>
      </div>

      <Suspense
        fallback={
          <div className="flex items-center justify-center h-32">
            <Loader2 size={20} className="animate-spin text-cs-muted" />
          </div>
        }
      >
        <McpDashboard />
      </Suspense>

      <AddMcpModal open={addOpen} onClose={() => setAddOpen(false)} />
    </div>
  );
}
