import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "./App";
import "./i18n";
import "./index.css";

// Tabs that hit Tauri-side discovery (Skills, MCPs, Health) used to freeze
// for several seconds on every open because each query refetched from
// scratch. The cache below makes them paint instantly with last-known
// data, then revalidates in the background. Felipe's report: "quando
// abre a aba ele demora uns 5seg pra responder travado".
//
// Strategy:
//   1. gcTime = 24h so within-session navigation never evicts data
//   2. Snapshot a small whitelist of expensive queries to localStorage
//      after each successful fetch
//   3. On boot, prime queryCache from that snapshot — `useQuery` then
//      serves cached data immediately while the background refetch runs
const PERSISTED_QUERY_PREFIXES = [
  "mcp-servers",
  "mcp-servers-with-tools",
  "skills",
  "agents",
  "agent-statuses",
  "context-files",
];
const PERSIST_KEY = "ato.query-cache.v1";

// Exported so non-React code (the demo store) can invalidate cached
// queries after Tauri-side mutations land. Without this, demos that
// createAgent/createGroup then immediately try to use the new entity
// hit the 30s staleTime and components don't re-render until then.
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      gcTime: 24 * 60 * 60 * 1000,
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});

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
