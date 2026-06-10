import {
  MessageSquare,
  Layers,
  ShieldCheck,
  Play,
  CheckCircle,
  Globe,
  Mail,
  MessageCircle,
  Database,
  GitBranch,
  FileText,
  Activity,
  Wrench,
  Calendar,
  Clock,
  Eye,
  Filter,
  Cpu,
  Bell,
  // v0.8.0: New icons for advanced nodes
  Webhook,
  GitMerge,
  ShieldAlert,
  RefreshCw,
  Variable,
  LayoutTemplate,
  // v2.14: LLM-aware Loop Composer kinds
  Zap,            // dispatch — single LLM call
  Beaker,         // methodology_run — variant matrix experiment
  Stethoscope,    // diagnose — LLM-driven refinement
  Hammer,         // apply — patch application
  ScanSearch,     // review — multi-LLM code review
  Swords,         // war_room — multi-seat debate
  Award,          // score — rubric application
  Inbox,          // input — markdown context bundle
} from "lucide-react";

// ---------------------------------------------------------------------------
// Dimensions
// ---------------------------------------------------------------------------

export const NODE_W = 200;
export const NODE_H = 100;
export const PORT_SIZE = 8;
export const PALETTE_W = 220;
export const CONFIG_PANEL_W = 280;

// ---------------------------------------------------------------------------
// Colors
// ---------------------------------------------------------------------------

export const TYPE_COLORS: Record<string, string> = {
  trigger: "#00FFB2",
  process: "#3b82f6",
  decision: "#FFB800",
  action: "#a78bfa",
  output: "#00FFB2",
  service: "#f97316",
  // v0.8.0: New node type colors
  parallel: "#06b6d4",    // Cyan for parallel execution
  "try-catch": "#ef4444", // Red for error handling
  retry: "#f59e0b",       // Amber for retry
  variable: "#8b5cf6",    // Purple for variables
  template: "#6366f1",    // Indigo for templates
  // v2.14: LLM-aware kinds — adjacent hues so the palette reads as
  // "one family" without losing per-kind distinguishability.
  dispatch: "#10b981",        // Emerald — single shot of execution
  methodology_run: "#14b8a6", // Teal — variant matrix
  diagnose: "#f43f5e",        // Rose — diagnostic
  apply: "#84cc16",           // Lime — patch landed
  review: "#0ea5e9",          // Sky — eyes on the diff
  war_room: "#d946ef",        // Fuchsia — multi-seat debate
  score: "#facc15",           // Yellow-400 — rubric verdict
  input: "#94a3b8",           // Slate — neutral context container
};

export const SERVICE_COLORS: Record<string, string> = {
  gmail: "#EA4335",
  slack: "#4A154B",
  github: "#8b5cf6",
  postgres: "#336791",
  notion: "#000000",
  linear: "#5E6AD2",
  jira: "#0052CC",
  discord: "#5865F2",
  calendar: "#4285F4",
};

export const SERVICE_ICONS: Record<string, React.ElementType> = {
  gmail: Mail,
  slack: MessageCircle,
  github: GitBranch,
  postgres: Database,
  notion: FileText,
  linear: Activity,
  jira: Wrench,
  discord: MessageCircle,
  calendar: Calendar,
};

export const NODE_ICONS: Record<string, React.ElementType> = {
  trigger: MessageSquare,
  process: Layers,
  decision: ShieldCheck,
  action: Play,
  output: CheckCircle,
  service: Globe,
  // v0.8.0: New node type icons
  parallel: GitMerge,
  "try-catch": ShieldAlert,
  retry: RefreshCw,
  variable: Variable,
  template: LayoutTemplate,
  // v2.14: LLM-aware Loop Composer kinds
  dispatch: Zap,
  methodology_run: Beaker,
  diagnose: Stethoscope,
  apply: Hammer,
  review: ScanSearch,
  war_room: Swords,
  score: Award,
  input: Inbox,
  // Note: "output" already maps above (legacy CheckCircle). The v2.14
  // Output BUNDLE shares the kind name; if we add a distinct
  // packaged-bundle icon later (lucide's `Package`), swap it in here.
};

// v0.8.0: Webhook-specific icon
export const WEBHOOK_ICON = Webhook;
