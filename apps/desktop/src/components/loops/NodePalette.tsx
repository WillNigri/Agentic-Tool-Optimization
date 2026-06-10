import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { Globe, Activity } from "lucide-react";
import { PALETTE_W, TYPE_COLORS, SERVICE_COLORS, SERVICE_ICONS, NODE_ICONS } from "./constants";
import { NODE_TEMPLATES } from "./service-catalog";
import type { NodeTemplate } from "./types";

interface NodePaletteProps {
  onDragStart: (template: NodeTemplate) => void;
}

const CATEGORIES = [
  { key: "triggers" as const, labelKey: "automation.builder.triggers" },
  { key: "services" as const, labelKey: "automation.builder.services" },
  { key: "actions" as const, labelKey: "automation.builder.actions" },
  // v0.8.0: New categories
  { key: "flow-control" as const, labelKey: "automation.builder.flowControl" },
  { key: "variables" as const, labelKey: "automation.builder.variables" },
];

export default function NodePalette({ onDragStart }: NodePaletteProps) {
  const { t } = useTranslation();

  return (
    <div
      className="flex-shrink-0 border-r border-[#2a2a3a] overflow-y-auto"
      style={{ width: PALETTE_W, background: "#0e0e16" }}
    >
      <div className="p-3">
        <h3 className="text-xs font-semibold text-[#e8e8f0] mb-3">
          {t("automation.builder.palette", "Node Palette")}
        </h3>

        {CATEGORIES.map((cat) => {
          const templates = NODE_TEMPLATES.filter((t) => t.category === cat.key);
          return (
            <div key={cat.key} className="mb-4">
              <h4 className="text-[10px] text-[#8888a0] uppercase tracking-wider mb-2 font-medium">
                {t(cat.labelKey, cat.key)}
              </h4>
              <div className="flex flex-col gap-1">
                {templates.map((tmpl) => {
                  const isService = tmpl.service;
                  const color = isService
                    ? SERVICE_COLORS[tmpl.service!] || TYPE_COLORS.service
                    : TYPE_COLORS[tmpl.type];
                  const Icon = isService
                    ? SERVICE_ICONS[tmpl.service!] || Globe
                    : NODE_ICONS[tmpl.type] || Activity;

                  return (
                    <div
                      key={`${tmpl.type}-${tmpl.service || ""}-${tmpl.label}`}
                      draggable
                      onDragStart={(e) => {
                        e.dataTransfer.setData("application/automation-node", JSON.stringify(tmpl));
                        e.dataTransfer.effectAllowed = "copy";
                        onDragStart(tmpl);
                      }}
                      className={cn(
                        "flex items-center gap-2 px-2.5 py-2 rounded-md cursor-grab border border-transparent",
                        "hover:bg-[#16161e] hover:border-[#2a2a3a] transition-colors active:cursor-grabbing"
                      )}
                    >
                      <div
                        className="flex items-center justify-center w-6 h-6 rounded shrink-0"
                        style={{ background: `${color}18` }}
                      >
                        <Icon size={12} style={{ color }} />
                      </div>
                      <div className="min-w-0">
                        <p className="text-[11px] font-medium text-[#e8e8f0] truncate">
                          {tmpl.label}
                        </p>
                        <p className="text-[9px] text-[#8888a0] truncate">
                          {tmpl.description}
                        </p>
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
