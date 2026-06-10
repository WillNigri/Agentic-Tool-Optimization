import type { ServiceAction, NodeTemplate, RetryConfig } from "./types";

// ---------------------------------------------------------------------------
// Service catalog — each service has actions with typed param schemas
// ---------------------------------------------------------------------------

export const SERVICE_ACTIONS: Record<string, ServiceAction[]> = {
  cron: [
    {
      id: "schedule",
      label: "Scheduled Trigger",
      description: "Trigger on a cron schedule (linked to Cron Monitor)",
      params: [
        { key: "schedule", label: "Cron Expression", type: "text", placeholder: "0 7 * * *", required: true },
      ],
    },
  ],
  github: [
    {
      id: "pr_opened",
      label: "PR Opened",
      description: "Trigger when a new PR is created",
      params: [
        { key: "repo", label: "Repository", type: "text", placeholder: "owner/repo", required: true },
        { key: "branch", label: "Base Branch", type: "text", placeholder: "main" },
      ],
    },
    {
      id: "fetch_diff",
      label: "Fetch Diff",
      description: "Get changed files from a PR",
      params: [
        { key: "repo", label: "Repository", type: "text", placeholder: "owner/repo", required: true },
        { key: "pr_number", label: "PR Number", type: "text", placeholder: "{{trigger.pr_number}}" },
      ],
    },
    {
      id: "comment_pr",
      label: "Comment on PR",
      description: "Post a comment on a pull request",
      params: [
        { key: "repo", label: "Repository", type: "text", placeholder: "owner/repo", required: true },
        { key: "pr_number", label: "PR Number", type: "text", placeholder: "{{trigger.pr_number}}" },
        { key: "body", label: "Comment Body", type: "textarea", placeholder: "Review comment..." },
      ],
    },
    {
      id: "merge_pr",
      label: "Merge PR",
      description: "Merge a pull request",
      params: [
        { key: "repo", label: "Repository", type: "text", placeholder: "owner/repo", required: true },
        { key: "pr_number", label: "PR Number", type: "text", placeholder: "{{trigger.pr_number}}" },
        { key: "method", label: "Merge Method", type: "select", options: ["merge", "squash", "rebase"] },
      ],
    },
  ],
  slack: [
    {
      id: "send_message",
      label: "Send Message",
      description: "Post a message to a Slack channel",
      params: [
        { key: "channel", label: "Channel", type: "text", placeholder: "#general", required: true },
        { key: "message", label: "Message", type: "textarea", placeholder: "Message text...", required: true },
      ],
    },
    {
      id: "read_channel",
      label: "Read Channel",
      description: "Read recent messages from a channel",
      params: [
        { key: "channel", label: "Channel", type: "text", placeholder: "#general", required: true },
        { key: "limit", label: "Message Limit", type: "text", placeholder: "10" },
      ],
    },
  ],
  gmail: [
    {
      id: "fetch_unread",
      label: "Fetch Unread",
      description: "Get unread emails from inbox",
      params: [
        { key: "label", label: "Label", type: "text", placeholder: "INBOX" },
        { key: "limit", label: "Max Emails", type: "text", placeholder: "20" },
      ],
    },
    {
      id: "send_email",
      label: "Send Email",
      description: "Send an email",
      params: [
        { key: "to", label: "To", type: "text", placeholder: "user@example.com", required: true },
        { key: "subject", label: "Subject", type: "text", placeholder: "Subject line", required: true },
        { key: "body", label: "Body", type: "textarea", placeholder: "Email body...", required: true },
      ],
    },
  ],
  postgres: [
    {
      id: "query",
      label: "Run Query",
      description: "Execute a SQL query",
      params: [
        { key: "connection", label: "Connection", type: "text", placeholder: "postgres://..." },
        { key: "query", label: "SQL Query", type: "textarea", placeholder: "SELECT ...", required: true },
      ],
    },
    {
      id: "schema_diff",
      label: "Schema Diff",
      description: "Compare database schema changes",
      params: [
        { key: "connection", label: "Connection", type: "text", placeholder: "postgres://..." },
        { key: "table", label: "Table", type: "text", placeholder: "users" },
      ],
    },
  ],
  notion: [
    {
      id: "create_page",
      label: "Create Page",
      description: "Create a new Notion page",
      params: [
        { key: "database_id", label: "Database ID", type: "text", placeholder: "abc123...", required: true },
        { key: "title", label: "Page Title", type: "text", placeholder: "New page", required: true },
        { key: "content", label: "Content", type: "textarea", placeholder: "Page content..." },
      ],
    },
  ],
  linear: [
    {
      id: "create_issue",
      label: "Create Issue",
      description: "Create a new Linear issue",
      params: [
        { key: "team", label: "Team", type: "text", placeholder: "ENG", required: true },
        { key: "title", label: "Title", type: "text", placeholder: "Issue title", required: true },
        { key: "description", label: "Description", type: "textarea", placeholder: "Issue description..." },
        { key: "priority", label: "Priority", type: "select", options: ["urgent", "high", "medium", "low"] },
      ],
    },
    {
      id: "update_status",
      label: "Update Status",
      description: "Update issue status",
      params: [
        { key: "issue_id", label: "Issue ID", type: "text", placeholder: "ENG-123", required: true },
        { key: "status", label: "Status", type: "select", options: ["backlog", "todo", "in_progress", "in_review", "done"] },
      ],
    },
  ],
};

// ---------------------------------------------------------------------------
// Node templates for the palette
// ---------------------------------------------------------------------------

// Default retry configuration
const DEFAULT_RETRY_CONFIG: RetryConfig = {
  maxAttempts: 3,
  backoffType: "exponential",
  initialDelayMs: 1000,
  maxDelayMs: 30000,
};

export const NODE_TEMPLATES: NodeTemplate[] = [
  // Triggers
  {
    type: "trigger",
    label: "Webhook",
    description: "HTTP webhook trigger",
    category: "triggers",
    defaultConfig: {
      webhook: {
        path: "",
        method: "POST",
      },
    },
  },
  { type: "trigger", service: "cron", label: "Cron / Schedule", description: "Time-based trigger (linked to Cron Monitor)", category: "triggers" },
  { type: "trigger", label: "File Watcher", description: "File change trigger", category: "triggers" },
  { type: "trigger", label: "Manual", description: "Manual trigger", category: "triggers" },
  { type: "trigger", service: "github", label: "GitHub PR", description: "PR opened trigger", action: "pr_opened", category: "triggers" },

  // Services
  { type: "service", service: "github", label: "GitHub", description: "GitHub operations", category: "services" },
  { type: "service", service: "slack", label: "Slack", description: "Slack messaging", category: "services" },
  { type: "service", service: "gmail", label: "Gmail", description: "Email operations", category: "services" },
  { type: "service", service: "postgres", label: "Postgres", description: "Database operations", category: "services" },
  { type: "service", service: "notion", label: "Notion", description: "Notion pages", category: "services" },
  { type: "service", service: "linear", label: "Linear", description: "Issue tracking", category: "services" },

  // Actions
  { type: "action", label: "Claude Process", description: "Claude AI processing", category: "actions" },
  { type: "process", label: "Filter / Transform", description: "Filter or transform data", category: "actions" },
  { type: "decision", label: "Decision", description: "Conditional branching", category: "actions" },
  { type: "output", label: "Notify", description: "Send notification", category: "actions" },

  // v0.8.0: Flow Control
  {
    type: "parallel",
    label: "Parallel",
    description: "Execute branches in parallel",
    category: "flow-control",
  },
  {
    type: "try-catch",
    label: "Try / Catch",
    description: "Error handling wrapper",
    category: "flow-control",
  },
  {
    type: "retry",
    label: "Retry",
    description: "Retry on failure with backoff",
    category: "flow-control",
    defaultRetryConfig: DEFAULT_RETRY_CONFIG,
  },

  // v0.8.0: Variables
  {
    type: "variable",
    label: "Set Variable",
    description: "Set or transform a variable",
    category: "variables",
  },
  {
    type: "template",
    label: "Template",
    description: "Use a reusable template",
    category: "variables",
  },
];

// v0.8.0: Service actions for webhook triggers
SERVICE_ACTIONS.webhook = [
  {
    id: "http_trigger",
    label: "HTTP Webhook",
    description: "Receive HTTP requests to trigger workflow",
    params: [
      { key: "path", label: "Path", type: "text", placeholder: "/my-workflow", required: true },
      { key: "method", label: "Method", type: "select", options: ["GET", "POST", "PUT", "DELETE"] },
      { key: "secret", label: "Secret (optional)", type: "text", placeholder: "HMAC secret for validation" },
    ],
  },
];

// v0.8.0: Service actions for variable operations
SERVICE_ACTIONS.variable = [
  {
    id: "set",
    label: "Set Variable",
    description: "Set a variable value",
    params: [
      { key: "name", label: "Variable Name", type: "text", placeholder: "myVar", required: true },
      { key: "value", label: "Value", type: "textarea", placeholder: "Value or expression", required: true },
    ],
  },
  {
    id: "transform",
    label: "Transform",
    description: "Transform data using jq expression",
    params: [
      { key: "input", label: "Input Variable", type: "text", placeholder: "$trigger.data", required: true },
      { key: "expression", label: "jq Expression", type: "textarea", placeholder: ".items | map(.name)", required: true },
      { key: "output", label: "Output Variable", type: "text", placeholder: "result", required: true },
    ],
  },
];

// v0.8.0: Service actions for retry configuration
SERVICE_ACTIONS.retry = [
  {
    id: "configure",
    label: "Configure Retry",
    description: "Configure retry behavior for wrapped nodes",
    params: [
      { key: "maxAttempts", label: "Max Attempts", type: "text", placeholder: "3", required: true },
      { key: "backoffType", label: "Backoff Type", type: "select", options: ["fixed", "exponential", "linear"] },
      { key: "initialDelayMs", label: "Initial Delay (ms)", type: "text", placeholder: "1000" },
      { key: "maxDelayMs", label: "Max Delay (ms)", type: "text", placeholder: "30000" },
    ],
  },
];
