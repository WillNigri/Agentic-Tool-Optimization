import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import { QueryClientProvider } from "@tanstack/react-query";
import App from "./App";
import { queryClient } from "@/lib/queryClient";
import "./i18n";
import "./index.css";

// queryClient lives in @/lib/queryClient now so non-React callers can
// import it without triggering the createRoot below in test envs.
// Vitest's jsdom has no #root → importing main.tsx blew up. Keeping
// the cache hydration + persistence logic here (it must run exactly
// once on app boot, not on every queryClient import).

const PERSISTED_QUERY_PREFIXES = [
  "mcp-servers",
  "mcp-servers-with-tools",
  "skills",
  "agents",
  "agent-statuses",
  "context-files",
];
const PERSIST_KEY = "ato.query-cache.v1";

// Hydrate from localStorage before any component mounts.
try {
  const raw = localStorage.getItem(PERSIST_KEY);
  if (raw) {
    const cached: Array<{ k: unknown[]; d: unknown; t: number }> = JSON.parse(raw);
    const horizon = Date.now() - 24 * 60 * 60 * 1000;
    for (const { k, d, t } of cached) {
      if (t > horizon) {
        queryClient.setQueryData(k, d);
      }
    }
  }
} catch {
  // Bad JSON or schema mismatch — clear and move on.
  localStorage.removeItem(PERSIST_KEY);
}

// Persist on every successful query update, debounced.
let persistTimer: number | null = null;
queryClient.getQueryCache().subscribe(() => {
  if (persistTimer !== null) window.clearTimeout(persistTimer);
  persistTimer = window.setTimeout(() => {
    try {
      const queries = queryClient.getQueryCache().getAll();
      const snapshot = queries
        .filter((q) => q.state.status === "success" && q.state.data !== undefined)
        .filter((q) => {
          const head = q.queryKey[0];
          return typeof head === "string" && PERSISTED_QUERY_PREFIXES.includes(head);
        })
        .map((q) => ({
          k: q.queryKey as unknown[],
          d: q.state.data,
          t: q.state.dataUpdatedAt,
        }));
      localStorage.setItem(PERSIST_KEY, JSON.stringify(snapshot));
    } catch {
      // localStorage full or serialization error — silent.
    }
  }, 500);
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <App />
      </BrowserRouter>
    </QueryClientProvider>
  </React.StrictMode>
);
