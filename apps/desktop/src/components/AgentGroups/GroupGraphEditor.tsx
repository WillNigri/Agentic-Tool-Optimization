import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Network, Sparkles, Crown, Settings, X, ArrowRight } from "lucide-react";
import { useFeatureFlag } from "@/lib/tier";
import type { GroupMemberInput, RouterConfig, RouterRule } from "@/lib/agentGroups";
import { cn } from "@/lib/utils";

// v1.4.0 Wave 3.2 — Visual graph editor for agent groups.
//
// Routes-as-edges, agents-as-nodes. The router sits in the center; children
// fan out around it on a unit circle. Each edge is labeled with the rule(s)
// that route to that child, or "default" / "LLM fallback".
//
// This view is an inspector + selector, not a layout-from-scratch tool.
// Adding/removing children + editing rules still happens via the parent
// (GroupDetail) — clicking a node focuses it there and emits onSelect.
// Pro users get the full editor; Free tier renders read-only with a hint.

interface Props {
  members: GroupMemberInput[];
  routerConfig: RouterConfig;
  /** Active runtime — used in node tooltips. */
  runtime?: string;
  /** Called when the user clicks a child node. Parent uses this to scroll
   *  the corresponding rule into view (or open a focused editor). */
  onSelectChild?: (agentSlug: string) => void;
  /** Called when the user clicks the router node. */
  onSelectRouter?: () => void;
  /** When true, all nodes are non-interactive (Free tier view-only). */
  readOnly?: boolean;
}

interface NodePos {
  slug: string;
  x: number;
  y: number;
  isRouter: boolean;
}

const CANVAS_W = 640;
const CANVAS_H = 360;
const CENTER_X = CANVAS_W / 2;
const CENTER_Y = CANVAS_H / 2;
const RADIUS = 130;
const NODE_W = 124;
const NODE_H = 56;

export default function GroupGraphEditor({
  members,
  routerConfig,
  runtime,
  onSelectChild,
  onSelectRouter,
  readOnly,
}: Props) {
  const { t } = useTranslation();
  const editorAllowed = useFeatureFlag("groups.editor");
  const interactive = !readOnly && editorAllowed;
  const [hovered, setHovered] = useState<string | null>(null);

  const children = useMemo(
    () => members.filter((m) => m.role === "child"),
    [members]
  );

  // Lay out children on a circle around the router.
  const positions = useMemo<NodePos[]>(() => {
    const out: NodePos[] = [
      { slug: "__router__", x: CENTER_X, y: CENTER_Y, isRouter: true },
    ];
    const n = children.length;
    if (n === 0) return out;
    children.forEach((c, i) => {
      const angle = (i / n) * 2 * Math.PI - Math.PI / 2; // start at top
      out.push({
        slug: c.agentSlug,
        x: CENTER_X + RADIUS * Math.cos(angle),
        y: CENTER_Y + RADIUS * Math.sin(angle),
        isRouter: false,
      });
    });
    return out;
  }, [children]);

  // Index rules → which child they route to.
  const rulesByChild = useMemo(() => {
    const m = new Map<string, RouterRule[]>();
    for (const r of routerConfig.rules) {
      if (!r.then) continue;
      const arr = m.get(r.then) ?? [];
      arr.push(r);
      m.set(r.then, arr);
    }
    return m;
  }, [routerConfig.rules]);

  const handleChildClick = (slug: string) => {
    if (!interactive) return;
    onSelectChild?.(slug);
  };

  const handleRouterClick = () => {
    if (!interactive) return;
    onSelectRouter?.();
  };

  if (children.length === 0) {
    return (
      <div className="flex items-center justify-center rounded-lg border border-dashed border-cs-border bg-cs-bg-raised/40 p-8 text-xs text-cs-muted">
        {t(
          "agentGroups.graph.empty",
          "Add at least one child agent to see the graph."
        )}
      </div>
    );
  }

  return (
    <div className="rounded-lg border border-cs-border bg-cs-bg-raised overflow-hidden">
      {!editorAllowed && (
        <div className="flex items-center gap-2 border-b border-cs-border bg-cs-accent/10 px-3 py-2 text-[11px] text-cs-accent">
          <Crown size={12} />
          {t(
            "agentGroups.graph.viewOnly",
            "Free tier: graph view is read-only. Upgrade to Pro to edit by drag and drop."
          )}
        </div>
      )}

      <div className="relative">
        <svg
          viewBox={`0 0 ${CANVAS_W} ${CANVAS_H}`}
          className="w-full h-auto block"
          role="img"
          aria-label={t("agentGroups.graph.aria", "Agent group routing diagram")}
        >
          <defs>
            <marker
              id="grp-arrow"
              viewBox="0 0 10 10"
              refX="9"
              refY="5"
              markerWidth="6"
              markerHeight="6"
              orient="auto-start-reverse"
            >
              <path d="M 0 0 L 10 5 L 0 10 z" fill="rgb(120 120 130)" />
            </marker>
            <marker
              id="grp-arrow-active"
              viewBox="0 0 10 10"
              refX="9"
              refY="5"
              markerWidth="6"
              markerHeight="6"
              orient="auto-start-reverse"
            >
              <path d="M 0 0 L 10 5 L 0 10 z" fill="rgb(0 255 178)" />
            </marker>
          </defs>

          {/* Edges: router → each child */}
          {positions
            .filter((p) => !p.isRouter)
            .map((p) => {
              const rules = rulesByChild.get(p.slug) ?? [];
              const hasRule = rules.length > 0;
              const isActive = hovered === p.slug;
              const stroke = isActive
                ? "rgb(0 255 178)"
                : hasRule
                ? "rgb(140 140 150)"
                : "rgb(80 80 90)";
              const dash = hasRule ? undefined : "4 4";

              // Edge: from router edge → child top edge.
              const dx = p.x - CENTER_X;
              const dy = p.y - CENTER_Y;
              const len = Math.hypot(dx, dy);
              const ux = dx / len;
              const uy = dy / len;
              // Stop short of the node so the arrow lands on its border.
              const stop = NODE_W / 2 - 8;
              const x2 = p.x - ux * stop;
              const y2 = p.y - uy * stop;

              const labelX = (CENTER_X + p.x) / 2;
              const labelY = (CENTER_Y + p.y) / 2;
              const labelText = hasRule
                ? rules.length === 1
                  ? `${rules[0].if?.keyword?.[0] ?? "rule"}${
                      (rules[0].if?.keyword?.length ?? 0) > 1 ? ` +${(rules[0].if?.keyword?.length ?? 0) - 1}` : ""
                    }`
                  : `${rules.length} rules`
                : routerConfig.llmFallback?.enabled
                ? "llm fallback"
                : "default";

              return (
                <g key={p.slug}>
                  <line
                    x1={CENTER_X + ux * (NODE_W / 2 - 8)}
                    y1={CENTER_Y + uy * (NODE_W / 2 - 8)}
                    x2={x2}
                    y2={y2}
                    stroke={stroke}
                    strokeWidth={isActive ? 2 : 1.5}
                    strokeDasharray={dash}
                    markerEnd={isActive ? "url(#grp-arrow-active)" : "url(#grp-arrow)"}
                  />
                  <rect
                    x={labelX - 36}
                    y={labelY - 8}
                    width={72}
                    height={16}
                    rx={3}
                    fill="rgb(20 20 28)"
                    stroke={stroke}
                    strokeOpacity={0.4}
                  />
                  <text
                    x={labelX}
                    y={labelY + 3}
                    fontSize="9"
                    fontFamily="ui-monospace, monospace"
                    fill={hasRule ? "rgb(200 200 210)" : "rgb(140 140 150)"}
                    textAnchor="middle"
                  >
                    {labelText.length > 12 ? labelText.slice(0, 11) + "…" : labelText}
                  </text>
                </g>
              );
            })}

          {/* Nodes */}
          {positions.map((p) => {
            const isHovered = hovered === p.slug;
            const isInteractive =
              interactive && (p.isRouter ? !!onSelectRouter : !!onSelectChild);
            return (
              <g
                key={p.slug}
                onClick={() => (p.isRouter ? handleRouterClick() : handleChildClick(p.slug))}
                onMouseEnter={() => setHovered(p.slug)}
                onMouseLeave={() => setHovered(null)}
                style={{ cursor: isInteractive ? "pointer" : "default" }}
              >
                <rect
                  x={p.x - NODE_W / 2}
                  y={p.y - NODE_H / 2}
                  width={NODE_W}
                  height={NODE_H}
                  rx={8}
                  fill={p.isRouter ? "rgb(0 255 178 / 0.08)" : "rgb(28 28 36)"}
                  stroke={
                    isHovered
                      ? "rgb(0 255 178)"
                      : p.isRouter
                      ? "rgb(0 255 178 / 0.6)"
                      : "rgb(80 80 90)"
                  }
                  strokeWidth={isHovered ? 2 : 1.5}
                />
                <foreignObject
                  x={p.x - NODE_W / 2}
                  y={p.y - NODE_H / 2}
                  width={NODE_W}
                  height={NODE_H}
                >
                  <div
                    className="h-full w-full flex items-center justify-center gap-1.5 px-2"
                    style={{
                      // Inline style — `foreignObject` strips a lot of cascading
                      // styles otherwise. Keep this minimal.
                      color: p.isRouter ? "rgb(0 255 178)" : "rgb(220 220 230)",
                      fontFamily: "ui-monospace, SFMono-Regular, monospace",
                      fontSize: 11,
                      lineHeight: "1.2",
                      textAlign: "center",
                    }}
                  >
                    {p.isRouter ? (
                      <>
                        <Network size={12} />
                        <span style={{ fontWeight: 600 }}>router</span>
                      </>
                    ) : (
                      <>
                        <Sparkles size={11} />
                        <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                          {p.slug}
                        </span>
                      </>
                    )}
                  </div>
                </foreignObject>
              </g>
            );
          })}
        </svg>

        {/* Hover detail panel */}
        {hovered && hovered !== "__router__" && (
          <div className="absolute top-2 right-2 max-w-[260px] rounded-md border border-cs-border bg-cs-card/95 backdrop-blur-sm p-3 text-[11px] shadow-lg">
            <ChildHoverDetail
              slug={hovered}
              rules={rulesByChild.get(hovered) ?? []}
              llmFallback={routerConfig.llmFallback?.enabled ?? false}
              runtime={runtime}
              onClose={() => setHovered(null)}
            />
          </div>
        )}
        {hovered === "__router__" && (
          <div className="absolute top-2 right-2 max-w-[260px] rounded-md border border-cs-border bg-cs-card/95 backdrop-blur-sm p-3 text-[11px] shadow-lg">
            <RouterHoverDetail
              ruleCount={routerConfig.rules.length}
              llmFallback={routerConfig.llmFallback}
              onClose={() => setHovered(null)}
            />
          </div>
        )}
      </div>

      {/* Footer hint */}
      <div className={cn(
        "flex items-center justify-between border-t border-cs-border px-3 py-2 text-[10px]",
        interactive ? "text-cs-muted" : "text-cs-muted/70"
      )}>
        <span>
          {interactive
            ? t("agentGroups.graph.clickHint", "Click a node to focus it in the form below.")
            : t("agentGroups.graph.viewOnlyHint", "Read-only view. Upgrade to Pro to edit.")}
        </span>
        <span className="font-mono">
          {children.length} {t("agentGroups.graph.children", "children")}
          {" · "}
          {routerConfig.rules.length} {t("agentGroups.graph.rules", "rules")}
        </span>
      </div>
    </div>
  );
}

function ChildHoverDetail({
  slug,
  rules,
  llmFallback,
  runtime,
  onClose,
}: {
  slug: string;
  rules: RouterRule[];
  llmFallback: boolean;
  runtime?: string;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-1.5">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="flex items-center gap-1.5">
            <Sparkles size={11} className="text-cs-accent" />
            <code className="font-mono text-cs-text truncate">{slug}</code>
          </div>
          {runtime && (
            <p className="mt-0.5 text-cs-muted text-[10px] uppercase tracking-wide">
              {runtime}
            </p>
          )}
        </div>
        <button
          type="button"
          onClick={onClose}
          className="text-cs-muted hover:text-cs-text shrink-0"
        >
          <X size={11} />
        </button>
      </div>
      {rules.length === 0 ? (
        <p className="text-cs-muted">
          {llmFallback
            ? t(
                "agentGroups.graph.noRulesLlm",
                "No rules route here. The LLM fallback may pick this child."
              )
            : t(
                "agentGroups.graph.noRulesDefault",
                "No rules route here. Reachable only as the default fallback."
              )}
        </p>
      ) : (
        <ul className="space-y-1">
          {rules.map((r, i) => {
            const kw = r.if?.keyword ?? [];
            return (
              <li key={i} className="flex items-start gap-1 text-cs-text">
                <ArrowRight size={10} className="text-cs-muted shrink-0 mt-0.5" />
                <span className="font-mono text-[10px]">
                  {kw.length > 0
                    ? kw.join(", ")
                    : t("agentGroups.graph.unconfigured", "(no keywords)")}
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

function RouterHoverDetail({
  ruleCount,
  llmFallback,
  onClose,
}: {
  ruleCount: number;
  llmFallback: RouterConfig["llmFallback"];
  onClose: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-1.5">
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-1.5">
          <Settings size={11} className="text-cs-accent" />
          <span className="text-cs-text font-medium">
            {t("agentGroups.graph.routerDetail", "Router")}
          </span>
        </div>
        <button
          type="button"
          onClick={onClose}
          className="text-cs-muted hover:text-cs-text shrink-0"
        >
          <X size={11} />
        </button>
      </div>
      <p className="text-cs-muted">
        {t("agentGroups.graph.routerSummary", "{{count}} rule(s) configured.", {
          count: ruleCount,
        })}
      </p>
      <p className="text-cs-muted">
        {llmFallback?.enabled
          ? t("agentGroups.graph.llmOn", "LLM fallback: enabled{{model}}", {
              model: llmFallback.model ? ` (${llmFallback.model})` : "",
            })
          : t("agentGroups.graph.llmOff", "LLM fallback: disabled")}
      </p>
    </div>
  );
}
