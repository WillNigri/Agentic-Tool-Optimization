import { QueryClient } from "@tanstack/react-query";

// Lives in its own module so non-React callers (the demo store) can
// import it without dragging in main.tsx — which calls
// `ReactDOM.createRoot(document.getElementById("root")!)` at module
// top and crashes vitest's jsdom env (no #root). main.tsx and
// useDemoStore now both import from here.
//
// Cache strategy: tabs that hit Tauri-side discovery (Skills, MCPs,
// Health) used to freeze for seconds on every open because each query
// refetched from scratch. gcTime = 24h means within-session navigation
// never evicts; localStorage snapshotting in main.tsx primes the cache
// on boot so panes paint instantly with last-known data.
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
