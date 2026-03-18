// ---------------------------------------------------------------------------
// Mock marketplace data — 10 community skills across 9 categories
// ---------------------------------------------------------------------------

export type MarketplaceCategory =
  | "library-reference"
  | "product-verification"
  | "data-fetching"
  | "business-process"
  | "code-scaffolding"
  | "code-quality"
  | "ci-cd"
  | "runbooks"
  | "infra-ops";

export interface MarketplaceSkill {
  id: string;
  name: string;
  description: string;
  category: MarketplaceCategory;
  tags: string[];
  author: string;
  installs: number;
  rating: number; // 1-5
  content: string;
  createdAt: string;
  updatedAt: string;
}

export const MARKETPLACE_CATEGORIES: { id: MarketplaceCategory; label: string }[] = [
  { id: "library-reference", label: "Library Reference" },
  { id: "product-verification", label: "Product Verification" },
  { id: "data-fetching", label: "Data Fetching" },
  { id: "business-process", label: "Business Process" },
  { id: "code-scaffolding", label: "Code Scaffolding" },
  { id: "code-quality", label: "Code Quality" },
  { id: "ci-cd", label: "CI/CD" },
  { id: "runbooks", label: "Runbooks" },
  { id: "infra-ops", label: "Infra Ops" },
];

export const MOCK_MARKETPLACE_SKILLS: MarketplaceSkill[] = [
  {
    id: "mp-react-patterns",
    name: "React Best Practices",
    description: "Enforces React 19 patterns: server components, use() hook, proper Suspense boundaries, and avoiding common anti-patterns.",
    category: "library-reference",
    tags: ["react", "frontend", "typescript"],
    author: "community",
    installs: 2340,
    rating: 4.7,
    content: `---
name: react-best-practices
description: Enforces React 19 patterns including server components, use() hook, and Suspense boundaries
---

When writing React code:
- Prefer server components by default; add "use client" only when state/effects are needed
- Use the use() hook for promises instead of useEffect + useState
- Wrap async boundaries with <Suspense fallback={...}>
- Never pass functions as props from server → client components
- Use React.cache() for deduplicating data fetches in server components`,
    createdAt: "2025-11-01",
    updatedAt: "2026-02-15",
  },
  {
    id: "mp-api-contract-check",
    name: "API Contract Validator",
    description: "Verifies API responses match OpenAPI schemas before and after changes. Flags breaking changes automatically.",
    category: "product-verification",
    tags: ["api", "testing", "openapi"],
    author: "api-guild",
    installs: 1820,
    rating: 4.5,
    content: `---
name: api-contract-validator
description: Verifies API responses match OpenAPI schemas and flags breaking changes
allowed-tools: Read, Bash(npx openapi-diff *)
---

When reviewing API changes:
1. Find the OpenAPI/Swagger spec in the project
2. Compare the before/after schemas for breaking changes
3. Flag removed fields, type changes, and new required parameters
4. Suggest backwards-compatible alternatives when possible`,
    createdAt: "2025-10-15",
    updatedAt: "2026-01-20",
  },
  {
    id: "mp-graphql-fetcher",
    name: "GraphQL Data Fetcher",
    description: "Generates type-safe GraphQL queries and mutations from schema. Supports fragments and pagination patterns.",
    category: "data-fetching",
    tags: ["graphql", "codegen", "typescript"],
    author: "graphql-community",
    installs: 1560,
    rating: 4.3,
    content: `---
name: graphql-data-fetcher
description: Generates type-safe GraphQL queries and mutations from schema with fragments and pagination
---

When working with GraphQL:
- Read the schema first to understand available types
- Generate queries with proper fragment composition
- Always include pagination (first/after cursor pattern)
- Generate TypeScript types from the schema
- Use query variables, never inline arguments`,
    createdAt: "2025-12-01",
    updatedAt: "2026-02-10",
  },
  {
    id: "mp-invoice-processor",
    name: "Invoice Processor",
    description: "Extracts structured data from invoices: line items, totals, tax, payment terms. Outputs JSON matching accounting schemas.",
    category: "business-process",
    tags: ["finance", "extraction", "automation"],
    author: "finops-team",
    installs: 980,
    rating: 4.1,
    content: `---
name: invoice-processor
description: Extracts structured data from invoices including line items, totals, tax, and payment terms
---

When processing invoices:
1. Extract: vendor name, invoice number, date, due date
2. Parse line items: description, quantity, unit price, total
3. Calculate subtotal, tax (identify tax type), total
4. Extract payment terms and bank details
5. Output as structured JSON matching the accounting schema`,
    createdAt: "2026-01-10",
    updatedAt: "2026-03-01",
  },
  {
    id: "mp-nextjs-scaffold",
    name: "Next.js App Scaffolder",
    description: "Scaffolds Next.js 15 App Router pages with layouts, loading states, error boundaries, and route handlers.",
    category: "code-scaffolding",
    tags: ["nextjs", "react", "scaffold"],
    author: "vercel-community",
    installs: 3100,
    rating: 4.8,
    content: `---
name: nextjs-app-scaffolder
description: Scaffolds Next.js 15 App Router pages with layouts, loading states, error boundaries, and route handlers
allowed-tools: Read, Write, Bash(npx *)
---

When creating Next.js pages:
- Create page.tsx, layout.tsx, loading.tsx, error.tsx, not-found.tsx
- Use generateMetadata for SEO
- Add route handlers in route.ts with proper HTTP method exports
- Use server actions for form submissions
- Add proper TypeScript types for params and searchParams`,
    createdAt: "2025-09-20",
    updatedAt: "2026-03-10",
  },
  {
    id: "mp-code-reviewer",
    name: "PR Code Review",
    description: "Performs comprehensive code review: security, performance, maintainability, and test coverage analysis.",
    category: "code-quality",
    tags: ["review", "security", "quality"],
    author: "engineering-leads",
    installs: 4200,
    rating: 4.9,
    content: `---
name: pr-code-review
description: Comprehensive code review covering security, performance, maintainability, and test coverage
---

Review checklist:
1. **Security**: injection risks, auth checks, data exposure, OWASP top 10
2. **Performance**: N+1 queries, unnecessary re-renders, missing indexes, large payloads
3. **Maintainability**: naming, complexity, DRY violations, proper abstractions
4. **Tests**: coverage gaps, edge cases, flaky test patterns
5. **Style**: consistent with codebase conventions

Output format: table with severity (critical/warning/info), file:line, issue, suggestion`,
    createdAt: "2025-08-15",
    updatedAt: "2026-02-28",
  },
  {
    id: "mp-gh-actions-gen",
    name: "GitHub Actions Generator",
    description: "Generates CI/CD workflows for GitHub Actions with caching, matrix builds, and deployment stages.",
    category: "ci-cd",
    tags: ["github-actions", "ci", "deployment"],
    author: "devops-guild",
    installs: 2100,
    rating: 4.4,
    content: `---
name: github-actions-generator
description: Generates GitHub Actions CI/CD workflows with caching, matrix builds, and deployment stages
allowed-tools: Read, Write
---

When creating GitHub Actions workflows:
- Use composite actions for reusable steps
- Enable dependency caching (npm, pip, cargo)
- Use matrix strategy for cross-platform/version testing
- Separate lint, test, build, deploy into distinct jobs
- Use environment protection rules for production
- Pin action versions to specific SHAs, not tags`,
    createdAt: "2025-11-20",
    updatedAt: "2026-01-15",
  },
  {
    id: "mp-incident-runbook",
    name: "Incident Response Runbook",
    description: "Guided incident response: severity assessment, impact analysis, communication templates, and post-mortem structure.",
    category: "runbooks",
    tags: ["incident", "oncall", "sre"],
    author: "sre-team",
    installs: 890,
    rating: 4.6,
    content: `---
name: incident-response-runbook
description: Guided incident response with severity assessment, impact analysis, and communication templates
---

On incident trigger:
1. **Assess severity**: P0 (customer-facing outage), P1 (degraded), P2 (internal), P3 (minor)
2. **Impact**: users affected, revenue impact, data integrity risk
3. **Communicate**: update status page, notify stakeholders, create incident channel
4. **Investigate**: check dashboards, recent deploys, error rates
5. **Mitigate**: rollback, feature flag, scale, failover
6. **Post-mortem**: timeline, root cause, action items, blameless review`,
    createdAt: "2026-01-05",
    updatedAt: "2026-03-05",
  },
  {
    id: "mp-k8s-troubleshoot",
    name: "K8s Troubleshooter",
    description: "Diagnoses Kubernetes issues: pod crashes, OOM kills, networking, ingress misconfigs, and resource quotas.",
    category: "infra-ops",
    tags: ["kubernetes", "debugging", "infrastructure"],
    author: "platform-team",
    installs: 1450,
    rating: 4.5,
    content: `---
name: k8s-troubleshooter
description: Diagnoses Kubernetes issues including pod crashes, OOM kills, networking, and ingress misconfigs
allowed-tools: Bash(kubectl *)
---

Troubleshooting flow:
1. kubectl get pods -A | grep -v Running — find non-running pods
2. kubectl describe pod <name> — check events, conditions
3. kubectl logs <name> --previous — check crash logs
4. Common fixes:
   - CrashLoopBackOff: check readiness/liveness probes, resource limits
   - OOMKilled: increase memory limits or fix memory leak
   - ImagePullBackOff: check registry credentials, image tag
   - Pending: check node resources, PVC bindings, taints`,
    createdAt: "2025-10-30",
    updatedAt: "2026-02-20",
  },
  {
    id: "mp-db-migration-safe",
    name: "Safe DB Migrations",
    description: "Validates database migrations for safety: no locks on large tables, backwards-compatible changes, rollback scripts.",
    category: "code-quality",
    tags: ["database", "migrations", "safety"],
    author: "data-team",
    installs: 1750,
    rating: 4.6,
    content: `---
name: safe-db-migrations
description: Validates database migrations for safety including lock analysis, backwards compatibility, and rollback scripts
---

Migration review checklist:
1. **No long locks**: ALTER TABLE on large tables must use pt-online-schema-change or equivalent
2. **Backwards compatible**: old code must work with new schema during deploy window
3. **Rollback**: every migration must have a corresponding down migration
4. **Data**: data migrations separate from schema migrations
5. **Indexes**: CREATE INDEX CONCURRENTLY to avoid blocking reads
6. **NOT NULL**: add column nullable first, backfill, then add constraint`,
    createdAt: "2025-12-15",
    updatedAt: "2026-03-12",
  },
];
