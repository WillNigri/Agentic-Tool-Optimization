// ---------------------------------------------------------------------------
// Cron expression parsing & utilities
// ---------------------------------------------------------------------------

const DAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS = [
  "Jan", "Feb", "Mar", "Apr", "May", "Jun",
  "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

export interface ParsedCron {
  minute: string;
  hour: string;
  dayOfMonth: string;
  month: string;
  dayOfWeek: string;
}

/**
 * Parse a 5-field cron expression into its components.
 * Returns null if invalid.
 */
export function parseCron(expression: string | unknown): ParsedCron | null {
  if (typeof expression !== "string") return null;
  const parts = expression.trim().split(/\s+/);
  if (parts.length !== 5) return null;

  const [minute, hour, dayOfMonth, month, dayOfWeek] = parts;

  // Basic validation
  if (!isValidField(minute, 0, 59)) return null;
  if (!isValidField(hour, 0, 23)) return null;
  if (!isValidField(dayOfMonth, 1, 31)) return null;
  if (!isValidField(month, 1, 12)) return null;
  if (!isValidField(dayOfWeek, 0, 7)) return null;

  return { minute, hour, dayOfMonth, month, dayOfWeek };
}

function isValidField(field: string, min: number, max: number): boolean {
  if (field === "*") return true;

  // Handle */n step syntax
  if (field.startsWith("*/")) {
    const step = parseInt(field.slice(2), 10);
    return !isNaN(step) && step >= 1 && step <= max;
  }

  // Handle comma-separated values
  const parts = field.split(",");
  for (const part of parts) {
    // Handle range
    if (part.includes("-")) {
      const [start, end] = part.split("-").map(Number);
      if (isNaN(start) || isNaN(end) || start < min || end > max || start > end)
        return false;
    } else {
      const val = parseInt(part, 10);
      if (isNaN(val) || val < min || val > max) return false;
    }
  }
  return true;
}

/**
 * Convert a cron expression to a human-readable string.
 */
export function cronToHuman(expression: string | unknown): string {
  if (typeof expression !== "string") return String(expression ?? "Unknown schedule");
  if (expression.startsWith("Every ")) return expression; // Already human-readable
  const parsed = parseCron(expression);
  if (!parsed) return expression; // Return as-is instead of "Invalid expression"

  const { minute, hour, dayOfMonth, month, dayOfWeek } = parsed;

  // Every minute
  if (minute === "*" && hour === "*" && dayOfMonth === "*" && month === "*" && dayOfWeek === "*") {
    return "Every minute";
  }

  // Every N minutes
  if (minute.startsWith("*/") && hour === "*" && dayOfMonth === "*") {
    return `Every ${minute.slice(2)} minutes`;
  }

  // Every N hours
  if (minute !== "*" && hour.startsWith("*/") && dayOfMonth === "*") {
    return `Every ${hour.slice(2)} hours at :${minute.padStart(2, "0")}`;
  }

  const parts: string[] = [];

  // Time
  if (hour !== "*" && minute !== "*") {
    const h = parseInt(hour, 10);
    const m = parseInt(minute, 10);
    const ampm = h >= 12 ? "PM" : "AM";
    const h12 = h === 0 ? 12 : h > 12 ? h - 12 : h;
    parts.push(`at ${h12}:${String(m).padStart(2, "0")} ${ampm}`);
  }

  // Day of week
  if (dayOfWeek !== "*") {
    if (dayOfWeek.includes(",")) {
      const days = dayOfWeek.split(",").map((d) => DAYS[parseInt(d, 10)] || d);
      parts.unshift(`On ${days.join(", ")}`);
    } else if (dayOfWeek.includes("-")) {
      const [start, end] = dayOfWeek.split("-").map(Number);
      parts.unshift(`${DAYS[start]}-${DAYS[end]}`);
    } else {
      const d = parseInt(dayOfWeek, 10);
      parts.unshift(`Every ${DAYS[d] || dayOfWeek}`);
    }
  } else if (dayOfMonth !== "*") {
    // Day of month
    if (dayOfMonth.includes(",")) {
      parts.unshift(`On day ${dayOfMonth}`);
    } else {
      const d = parseInt(dayOfMonth, 10);
      const suffix = d === 1 ? "st" : d === 2 ? "nd" : d === 3 ? "rd" : "th";
      parts.unshift(`On the ${d}${suffix}`);
    }
  } else {
    parts.unshift("Every day");
  }

  // Month
  if (month !== "*") {
    if (month.includes(",")) {
      const ms = month.split(",").map((m) => MONTHS[parseInt(m, 10) - 1] || m);
      parts.push(`in ${ms.join(", ")}`);
    } else {
      const m = parseInt(month, 10);
      parts.push(`in ${MONTHS[m - 1] || month}`);
    }
  }

  return parts.join(" ");
}

/**
 * Validate a cron expression. Returns error message or null if valid.
 */
export function validateCron(expression: string): string | null {
  if (!expression.trim()) return "Expression is required";

  const parts = expression.trim().split(/\s+/);
  if (parts.length !== 5)
    return `Expected 5 fields, got ${parts.length}`;

  const parsed = parseCron(expression);
  if (!parsed) return "Invalid cron expression";

  return null;
}

/**
 * Check if a cron expression would fire on a given date (ignoring time).
 * Used by the calendar view to show which jobs run on which days.
 */
export function matchesCronDate(expression: string, date: Date): boolean {
  const parsed = parseCron(expression);
  if (!parsed) return false;

  return (
    matchesField(parsed.dayOfMonth, date.getDate()) &&
    matchesField(parsed.month, date.getMonth() + 1) &&
    matchesField(parsed.dayOfWeek, date.getDay())
  );
}

/**
 * Calculate the next run time from a cron expression (simplified).
 * For accurate scheduling, a proper cron library should be used.
 * This provides a rough estimate for display purposes.
 */
export function getNextRun(expression: string | unknown, fromDate: Date = new Date()): Date | null {
  if (typeof expression !== "string") return null;
  const parsed = parseCron(expression);
  if (!parsed) return null;

  const next = new Date(fromDate);
  next.setSeconds(0, 0);

  // Simple: advance minute by minute up to 48 hours to find next match
  const maxIterations = 48 * 60;
  for (let i = 0; i < maxIterations; i++) {
    next.setMinutes(next.getMinutes() + 1);
    if (matchesCron(parsed, next)) return next;
  }

  return null;
}

function matchesCron(parsed: ParsedCron, date: Date): boolean {
  return (
    matchesField(parsed.minute, date.getMinutes()) &&
    matchesField(parsed.hour, date.getHours()) &&
    matchesField(parsed.dayOfMonth, date.getDate()) &&
    matchesField(parsed.month, date.getMonth() + 1) &&
    matchesField(parsed.dayOfWeek, date.getDay())
  );
}

function matchesField(field: string, value: number): boolean {
  if (field === "*") return true;
  if (field.startsWith("*/")) {
    const step = parseInt(field.slice(2), 10);
    return value % step === 0;
  }
  const parts = field.split(",");
  for (const part of parts) {
    if (part.includes("-")) {
      const [start, end] = part.split("-").map(Number);
      if (value >= start && value <= end) return true;
    } else {
      if (parseInt(part, 10) === value) return true;
    }
  }
  return false;
}

/**
 * Format a relative time string (e.g., "2h ago", "in 30m")
 */
export function formatRelativeTime(date: Date | string): string {
  const now = new Date();
  const d = typeof date === "string" ? new Date(date) : date;
  const diffMs = d.getTime() - now.getTime();
  const absDiff = Math.abs(diffMs);
  const isFuture = diffMs > 0;

  if (absDiff < 60_000) return isFuture ? "in <1m" : "<1m ago";
  if (absDiff < 3_600_000) {
    const m = Math.floor(absDiff / 60_000);
    return isFuture ? `in ${m}m` : `${m}m ago`;
  }
  if (absDiff < 86_400_000) {
    const h = Math.floor(absDiff / 3_600_000);
    return isFuture ? `in ${h}h` : `${h}h ago`;
  }
  const days = Math.floor(absDiff / 86_400_000);
  return isFuture ? `in ${days}d` : `${days}d ago`;
}
