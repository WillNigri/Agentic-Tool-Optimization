// Single source of truth for every runtime the desktop knows about.
//
// Before this module: RUNTIME_COLORS was declared in 10 different
// component files (drift waiting to happen — adding MiniMax meant
// hand-editing 10 places); AgentRuntime was typed as
// "claude" | "codex" | "openclaw" | "hermes" but the codebase
// actively dispatches to gemini / minimax / grok / deepseek / qwen
// / openrouter too; PromptBar's RUNTIME_OPTIONS picker only listed
// the four CLI runtimes and never gained the API providers.
//
// One import. One canonical map. Adding a new runtime now means
// one entry here + (if it has special dispatch rules) the matching
// backend changes — every UI surface picks it up for free.

import {
  Bot,
  Cloud,
  Cpu,
  Globe,
  Layers,
  Network,
  Server,
  Sparkles,
  Terminal,
  Zap,
  type LucideIcon,
} from "lucide-react";

interface RuntimeMeta {
  /** Display label for badges, picker rows, tooltips. */
  label: string;
  /** Icon for picker rows + headers. */
  icon: LucideIcon;
  /** Hex color for inline styles (PromptBar send-button background,
   *  workspace canvas nodes, chart series). */
  hex: string;
  /** Tailwind class string for badges (matches the existing
   *  `runtimeBadge` shape — text + bg tints from the same color
   *  family). */
  tw: string;
  /** How this runtime dispatches:
   *   - "cli"  → shells out to a binary (claude/codex/gemini/openclaw/hermes)
   *   - "api"  → direct HTTPS via stored API key (minimax/grok/deepseek/qwen/openrouter)
   *  The PromptBar uses this to decide whether to show a CLI-path
   *  hint vs an API-key hint when the runtime isn't ready. */
  kind: "cli" | "api";
}

/** Canonical map. Keys are the wire identifiers used by the CLI's
 *  `ato dispatch <runtime>`, by `execution_logs.runtime`, by every
 *  `useQuery` keyed on a runtime, and by the runtime filter chips.
 *
 *  Adding a runtime: drop one entry. Removing a runtime: see all
 *  callsites by removing the entry — tsc will tell you.
 */
// Hex values lean on each provider's brand color (so chart series +
// PromptBar's send button feel recognizable); tw values are
// tailwind-palette-derived for badge differentiation (small chips
// in the Sessions feed need to be visually distinct, even if that
// means the chip color doesn't perfectly match the brand hex —
// brand hex on minimax/deepseek/openrouter are all bluish but the
// badge palette spreads them across pink/indigo/violet so the
// feed stays scannable).
export const RUNTIME_REGISTRY = {
  claude:     { label: "Claude",     icon: Terminal, hex: "#f97316", tw: "text-orange-400 bg-orange-400/10", kind: "cli" },
  codex:      { label: "Codex",      icon: Cpu,      hex: "#22c55e", tw: "text-green-400 bg-green-400/10",  kind: "cli" },
  gemini:     { label: "Gemini",     icon: Sparkles, hex: "#4285f4", tw: "text-blue-400 bg-blue-400/10",    kind: "cli" },
  openclaw:   { label: "OpenClaw",   icon: Server,   hex: "#06b6d4", tw: "text-cyan-400 bg-cyan-400/10",    kind: "cli" },
  hermes:     { label: "Hermes",     icon: Globe,    hex: "#a855f7", tw: "text-purple-400 bg-purple-400/10", kind: "cli" },
  minimax:    { label: "MiniMax",    icon: Zap,      hex: "#1456ff", tw: "text-pink-400 bg-pink-400/10",    kind: "api" },
  grok:       { label: "Grok",       icon: Cloud,    hex: "#94a3b8", tw: "text-slate-400 bg-slate-400/10",  kind: "api" },
  deepseek:   { label: "DeepSeek",   icon: Layers,   hex: "#4d6bfe", tw: "text-indigo-400 bg-indigo-400/10", kind: "api" },
  qwen:       { label: "Qwen",       icon: Bot,      hex: "#7c3aed", tw: "text-amber-400 bg-amber-400/10",  kind: "api" },
  openrouter: { label: "OpenRouter", icon: Network,  hex: "#10b981", tw: "text-violet-400 bg-violet-400/10", kind: "api" },
} as const satisfies Record<string, RuntimeMeta>;

/** Every known runtime id, derived from the registry keys. Use this
 *  instead of hand-maintained string-literal unions. */
export type RuntimeId = keyof typeof RUNTIME_REGISTRY;

/** #82 — Curated list of well-known models per CLI runtime, surfaced by
 *  the PromptBar's ModelPicker when the runtime is `kind: "cli"`. CLI
 *  binaries can't enumerate their own models for us (no list endpoint),
 *  so we hardcode the canonical ones each vendor ships. The backend
 *  already pipes `--model <id>` through to claude/codex/gemini CLI
 *  spawns (apps/desktop/src-tauri/src/commands/mod.rs:1362,1444,9418,
 *  9442) — this list just makes the UI surface it.
 *
 *  When the picker has no override saved, the CLI runs with its own
 *  default. Updating: keep the list short and current (newest model
 *  first). For runtimes that don't ship a `--model` flag (openclaw,
 *  hermes), omit them here — the picker stays hidden.
 */
export const CLI_RUNTIME_MODELS: Partial<Record<RuntimeId, ReadonlyArray<{ id: string; display: string }>>> = {
  claude: [
    { id: "claude-opus-4-7",    display: "Opus 4.7 (most capable)" },
    { id: "claude-sonnet-4-6",  display: "Sonnet 4.6 (balanced)" },
    { id: "claude-haiku-4-5",   display: "Haiku 4.5 (fastest)" },
  ],
  codex: [
    { id: "gpt-4.1",            display: "GPT-4.1" },
    { id: "gpt-4o",             display: "GPT-4o" },
    { id: "gpt-4o-mini",        display: "GPT-4o mini" },
    { id: "o1-preview",         display: "o1-preview (reasoning)" },
    { id: "o1-mini",            display: "o1-mini" },
  ],
  gemini: [
    { id: "gemini-2.5-pro",     display: "Gemini 2.5 Pro" },
    { id: "gemini-2.5-flash",   display: "Gemini 2.5 Flash (fast)" },
    { id: "gemini-2.0-flash",   display: "Gemini 2.0 Flash" },
  ],
};

/** Stable backwards-compat alias. `AgentRuntime` historically only
 *  covered the four CLI runtimes; many callsites already import it
 *  generically. Aliasing to `RuntimeId` widens the type for those
 *  callers without forcing a rename PR. New code should import
 *  `RuntimeId` directly. */
export type AgentRuntime = RuntimeId;

/** Provider slug → runtime id. The LLM-API-keys table uses provider
 *  names ("anthropic", "openai", "google") that route to a runtime
 *  ("claude", "codex", "gemini"). Pure aliases — both forms point at
 *  the same color/icon/label set. */
export const PROVIDER_TO_RUNTIME: Record<string, RuntimeId> = {
  anthropic: "claude",
  openai: "codex",
  google: "gemini",
};

/** All ids in canonical display order (CLI runtimes first, then API
 *  providers). The PromptBar picker, the FirstChatWizard counter,
 *  and the runtime filter chips all walk this list. */
export const RUNTIME_IDS: RuntimeId[] = Object.keys(RUNTIME_REGISTRY) as RuntimeId[];

/** Pre-baked Record<id, hex> map for components that just need the
 *  hex color (workspace canvas, analytics charts). Same shape as the
 *  legacy in-component RUNTIME_COLORS objects that used to be
 *  copy-pasted 10 places — drop-in replacement. Prefer
 *  `runtimeHex(rt)` in new code so unknown values get a fallback. */
export const RUNTIME_HEX_COLORS: Readonly<Record<RuntimeId, string>> =
  Object.freeze(
    Object.fromEntries(
      RUNTIME_IDS.map((id) => [id, RUNTIME_REGISTRY[id].hex]),
    ),
  ) as Readonly<Record<RuntimeId, string>>;

/** All runtimes as a tuple ready for `[id, meta]` iteration in picker
 *  UIs. Frozen by `as const` so TS infers literal types. */
export const RUNTIME_ENTRIES = RUNTIME_IDS.map(
  (id) => [id, RUNTIME_REGISTRY[id]] as const,
);

/** Type guard — does this string identify a known runtime? */
export function isRuntimeId(rt: string): rt is RuntimeId {
  return rt in RUNTIME_REGISTRY;
}

/** Resolve a string (which may be a runtime id, a provider alias, or
 *  unknown legacy data) to a registry entry, or `undefined`. */
export function resolveRuntime(rt: string): { id: RuntimeId; meta: RuntimeMeta } | undefined {
  if (isRuntimeId(rt)) return { id: rt, meta: RUNTIME_REGISTRY[rt] };
  const aliased = PROVIDER_TO_RUNTIME[rt.toLowerCase()];
  if (aliased) return { id: aliased, meta: RUNTIME_REGISTRY[aliased] };
  return undefined;
}

/** Tailwind class string for a runtime badge. Falls back to a neutral
 *  muted treatment for unknown values — protects against legacy data
 *  in execution_logs that predates a runtime being in the registry. */
export function runtimeTw(rt: string): string {
  return resolveRuntime(rt)?.meta.tw ?? "text-cs-muted bg-cs-border";
}

/** Hex color (for inline styles). Falls back to a neutral slate. */
export function runtimeHex(rt: string): string {
  return resolveRuntime(rt)?.meta.hex ?? "#94a3b8";
}

/** Human-readable label. Falls back to the raw id so unknown runtimes
 *  still render *something* recognizable. */
export function runtimeLabel(rt: string): string {
  return resolveRuntime(rt)?.meta.label ?? rt;
}

/** Icon for picker rows. Falls back to the generic Bot icon. */
export function runtimeIcon(rt: string): LucideIcon {
  return resolveRuntime(rt)?.meta.icon ?? Bot;
}
