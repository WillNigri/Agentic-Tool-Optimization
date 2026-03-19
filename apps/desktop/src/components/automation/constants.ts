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
};
