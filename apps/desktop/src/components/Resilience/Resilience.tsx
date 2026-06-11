// v2.15.3 Resilience Settings tab — exhaustion-policy chooser +
// fallback-chain editor. Per war_room 27522371 (codex):
//   - Two-stage consent for fallback-chain (explicit confirm needed)
//   - Fallback chain candidates filtered to dispatchable runtimes
//   - Snapshot semantics: settings edits apply to next loop run, not
//     in-flight (no live re-read — codex's reproducibility note)
//
// v2.15.4 — pause-and-wake is now a real selectable radio option (was
// shown as a "coming soon" info note in v2.15.3 prior to the scheduler
// landing). Backend: paused_dispatches table + StepError::Paused +
// `ato loop resume` CLI + startup scanner. See ROADMAP.md.

import { useState, useMemo, useEffect } from "react";
import { GripVertical, ShieldCheck, AlertTriangle, ArrowRight } from "lucide-react";
import {
  DndContext,
  closestCenter,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { cn } from "@/lib/utils";
import { useEnabledRuntimes } from "@/lib/enabledRuntimes";
import {
  useExhaustionPolicy,
  useSetExhaustionPolicy,
  useFallbackOrder,
  useSetFallbackOrder,
  type ExhaustionPolicy,
} from "@/lib/useExhaustionPolicy";

export default function Resilience() {
  const policyQuery = useExhaustionPolicy();
  const orderQuery = useFallbackOrder();
  const runtimesQuery = useEnabledRuntimes();
  const setPolicy = useSetExhaustionPolicy();
  const setOrder = useSetFallbackOrder();

  // Local state for the fallback-chain order — committed via mutation
  // on drag end. Until that commit, the visible order is what's in
  // `localOrder`; the DB is the source of truth on first load and on
  // mutation success.
  const [localOrder, setLocalOrder] = useState<string[]>([]);
  const [showConsent, setShowConsent] = useState(false);

  useEffect(() => {
    if (orderQuery.data) setLocalOrder(orderQuery.data);
  }, [orderQuery.data]);

  const dispatchableRuntimes = useMemo(() => {
    if (!runtimesQuery.data) return [];
    // Per codex: chain candidates come from list_available_runtimes
    // filtered to dispatchable rows.
    return runtimesQuery.data.filter((r) => r.available).map((r) => r.slug);
  }, [runtimesQuery.data]);

  const unaddedRuntimes = useMemo(
    () => dispatchableRuntimes.filter((slug) => !localOrder.includes(slug)),
    [dispatchableRuntimes, localOrder]
  );

  const sensors = useSensors(
    useSensor(PointerSensor),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates })
  );

  const policy = policyQuery.data?.policy ?? "ask";
  const authorized = policyQuery.data?.authorized_auto_swap_at ?? null;

  const handlePolicyChange = (next: ExhaustionPolicy) => {
    // Two-stage consent: when the user picks fallback-chain WITHOUT
    // a prior authorization, surface the consent prompt. Persist
    // immediately for any other policy.
    if (next === "fallback-chain") {
      if (authorized) {
        // Already consented in the past — flip without re-prompting.
        setPolicy.mutate({ policy: next, confirmAutoSwap: true });
      } else {
        setShowConsent(true);
      }
    } else {
      setPolicy.mutate({ policy: next, confirmAutoSwap: false });
      setShowConsent(false);
    }
  };

  const handleConsent = () => {
    setPolicy.mutate({ policy: "fallback-chain", confirmAutoSwap: true });
    setShowConsent(false);
  };

  const handleDragEnd = (event: DragEndEvent) => {
    const { active, over } = event;
    if (!over || active.id === over.id) return;
    const oldIndex = localOrder.indexOf(String(active.id));
    const newIndex = localOrder.indexOf(String(over.id));
    if (oldIndex === -1 || newIndex === -1) return;
    const next = arrayMove(localOrder, oldIndex, newIndex);
    setLocalOrder(next);
    setOrder.mutate(next);
  };

  const addRuntime = (slug: string) => {
    const next = [...localOrder, slug];
    setLocalOrder(next);
    setOrder.mutate(next);
  };

  const removeRuntime = (slug: string) => {
    const next = localOrder.filter((s) => s !== slug);
    setLocalOrder(next);
    setOrder.mutate(next);
  };

  if (policyQuery.isLoading) {
    return <div className="p-6 text-cs-muted">Loading resilience settings…</div>;
  }

  return (
    <div className="max-w-3xl space-y-8 p-6">
      <header>
        <h2 className="text-lg font-semibold text-cs-text mb-1">Resilience</h2>
        <p className="text-sm text-cs-muted">
          Decide what ATO should do when an LLM runs out of capacity during a
          dispatch — subscription quota hit, daily rate limit reached, etc.
          Different failure class from short transient overloads, which are
          already handled by v2.15.1 retry.
        </p>
      </header>

      {/* Policy radio group */}
      <section className="space-y-3">
        <h3 className="text-sm font-medium text-cs-text">When a runtime is exhausted…</h3>

        <PolicyOption
          id="ask"
          current={policy}
          title="Ask me each time"
          description="Default. ATO emits a dispatch_exhausted event into the audit log; loops degrade to stop-and-notify until you set a real policy."
          onClick={() => handlePolicyChange("ask")}
        />

        <PolicyOption
          id="stop-and-notify"
          current={policy}
          title="Stop and notify"
          description="Fail the dispatch with a clear error. Safe default — never silently swaps runtimes. Best for loops where reproducibility matters."
          onClick={() => handlePolicyChange("stop-and-notify")}
        />

        <PolicyOption
          id="fallback-chain"
          current={policy}
          title="Switch to another runtime"
          description="ATO automatically retargets the dispatch to the next runtime in your fallback chain that still has capacity. Lets unattended Missions survive subscription exhaustion."
          requiresConsent
          consented={!!authorized}
          onClick={() => handlePolicyChange("fallback-chain")}
        />

        <PolicyOption
          id="pause-and-wake"
          current={policy}
          title="Pause and resume at reset time"
          description="When a runtime's quota resets at a known time (codex: 'try again at <date>'), ATO persists the dispatch, exits the loop cleanly, and re-fires at reset. After 3 consecutive failed wakes the dispatch is abandoned with a decision brief. Best for reproducibility — no runtime swap."
          onClick={() => handlePolicyChange("pause-and-wake")}
        />
      </section>

      {/* Consent modal */}
      {showConsent && (
        <ConsentPrompt onAccept={handleConsent} onCancel={() => setShowConsent(false)} />
      )}

      {/* Fallback chain editor — visible only when fallback-chain is selected + consented */}
      {policy === "fallback-chain" && authorized && (
        <section className="space-y-3 border-t border-cs-border pt-6">
          <header className="flex items-start justify-between gap-3">
            <div>
              <h3 className="text-sm font-medium text-cs-text">Fallback order</h3>
              <p className="text-xs text-cs-muted mt-0.5">
                Drag to reorder. When the runtime at the top exhausts, ATO tries
                the next one. Only includes runtimes you've configured (API key
                saved or CLI installed).
              </p>
            </div>
          </header>

          {localOrder.length === 0 && (
            <div className="rounded-md border border-cs-border/60 bg-cs-bg-raised/50 p-3 text-xs text-cs-muted">
              No runtimes in your chain yet. Add one from the list below.
            </div>
          )}

          <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
            <SortableContext items={localOrder} strategy={verticalListSortingStrategy}>
              <div className="space-y-1.5">
                {localOrder.map((slug, idx) => (
                  <SortableRow
                    key={slug}
                    id={slug}
                    index={idx}
                    next={localOrder[idx + 1]}
                    onRemove={() => removeRuntime(slug)}
                  />
                ))}
              </div>
            </SortableContext>
          </DndContext>

          {unaddedRuntimes.length > 0 && (
            <div className="pt-3">
              <p className="text-xs font-medium text-cs-muted mb-2">Available to add:</p>
              <div className="flex flex-wrap gap-1.5">
                {unaddedRuntimes.map((slug) => (
                  <button
                    key={slug}
                    type="button"
                    onClick={() => addRuntime(slug)}
                    className="px-2 py-1 rounded-md border border-cs-border text-xs hover:bg-cs-bg-raised hover:border-cs-accent/40 transition-colors"
                  >
                    + {slug}
                  </button>
                ))}
              </div>
            </div>
          )}

          {/* Live preview */}
          {localOrder.length > 1 && (
            <div className="rounded-md border border-cs-accent/30 bg-cs-accent/5 p-3 mt-4">
              <p className="text-xs text-cs-text">
                <span className="font-medium">Live preview:</span> if{" "}
                <code className="font-mono">{localOrder[0]}</code> exhausts, ATO
                tries{" "}
                {localOrder.slice(1).map((s, i) => (
                  <span key={s}>
                    <code className="font-mono">{s}</code>
                    {i < localOrder.length - 2 && ", then "}
                  </span>
                ))}
                . If all are exhausted, the dispatch fails with a clear
                "every-runtime-exhausted" error.
              </p>
            </div>
          )}
        </section>
      )}
    </div>
  );
}

interface PolicyOptionProps {
  id: ExhaustionPolicy;
  current: ExhaustionPolicy;
  title: string;
  description: string;
  requiresConsent?: boolean;
  consented?: boolean;
  onClick: () => void;
}

function PolicyOption({
  id,
  current,
  title,
  description,
  requiresConsent,
  consented,
  onClick,
}: PolicyOptionProps) {
  const selected = current === id;
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "w-full text-left rounded-md border p-3 transition-colors",
        selected
          ? "border-cs-accent bg-cs-accent/5"
          : "border-cs-border hover:border-cs-accent/40 hover:bg-cs-bg-raised"
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className={cn("text-sm font-medium", selected ? "text-cs-accent" : "text-cs-text")}>
              {title}
            </span>
            {requiresConsent && consented && (
              <span className="inline-flex items-center gap-1 text-[10px] text-emerald-400">
                <ShieldCheck size={10} />
                consent on file
              </span>
            )}
          </div>
          <p className="text-xs text-cs-muted mt-1 leading-relaxed">{description}</p>
        </div>
        <div
          className={cn(
            "shrink-0 mt-1 h-3.5 w-3.5 rounded-full border",
            selected ? "border-cs-accent bg-cs-accent" : "border-cs-border"
          )}
        />
      </div>
    </button>
  );
}

interface ConsentPromptProps {
  onAccept: () => void;
  onCancel: () => void;
}

function ConsentPrompt({ onAccept, onCancel }: ConsentPromptProps) {
  return (
    <div className="rounded-md border border-amber-500/40 bg-amber-500/5 p-4">
      <div className="flex items-start gap-3">
        <AlertTriangle size={18} className="text-amber-400 shrink-0 mt-0.5" />
        <div className="flex-1 space-y-3">
          <div>
            <h4 className="text-sm font-medium text-cs-text">
              Allow ATO to switch runtimes automatically?
            </h4>
            <p className="text-xs text-cs-muted mt-1 leading-relaxed">
              When a runtime exhausts mid-dispatch, ATO will silently retarget
              to the next runtime in your fallback chain. This means a loop
              that started on Claude may finish on Gemini — different model,
              potentially different output. Critical for unattended Missions
              that can't ask a human; risky for reproducibility-sensitive runs.
            </p>
            <p className="text-xs text-cs-muted/80 mt-2 italic">
              You can change this back to "Stop and notify" at any time.
            </p>
          </div>
          <div className="flex gap-2">
            <button
              type="button"
              onClick={onAccept}
              className="px-3 py-1.5 rounded-md bg-cs-accent text-cs-bg text-xs font-medium hover:bg-cs-accent/90"
            >
              Yes, allow auto-swap
            </button>
            <button
              type="button"
              onClick={onCancel}
              className="px-3 py-1.5 rounded-md border border-cs-border text-xs hover:bg-cs-bg-raised"
            >
              Cancel
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

interface SortableRowProps {
  id: string;
  index: number;
  next?: string;
  onRemove: () => void;
}

function SortableRow({ id, index, next, onRemove }: SortableRowProps) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } =
    useSortable({ id });
  const style: React.CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.6 : 1,
  };
  return (
    <div
      ref={setNodeRef}
      style={style}
      className="flex items-center gap-2 rounded-md border border-cs-border bg-cs-bg-raised px-3 py-2"
    >
      <button
        type="button"
        {...attributes}
        {...listeners}
        className="cursor-grab active:cursor-grabbing text-cs-muted hover:text-cs-text"
        aria-label="Drag to reorder"
      >
        <GripVertical size={14} />
      </button>
      <span className="font-mono text-xs text-cs-text flex-1">
        {index + 1}. {id}
      </span>
      {next && (
        <span className="text-[10px] text-cs-muted/70 flex items-center gap-1">
          on exhaust <ArrowRight size={10} /> <code>{next}</code>
        </span>
      )}
      <button
        type="button"
        onClick={onRemove}
        className="text-[10px] text-cs-muted hover:text-red-400 transition-colors"
      >
        remove
      </button>
    </div>
  );
}
