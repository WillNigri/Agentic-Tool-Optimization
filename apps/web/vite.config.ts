import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'node:path';

export default defineConfig({
  plugins: [react()],
  envPrefix: 'VITE_',
  // `@` alias resolves to `apps/web/src` so imports written as
  // `@/lib/runtimes` (the convention apps/desktop already uses) work
  // here too. PR #73 (Wave 1 browser) introduced the first such
  // import without the matching config change; main was building
  // green only because vite dev-mode is more lenient than rollup
  // production builds about unresolved aliases. Caught 2026-06-16
  // during the pre-merge QA sweep.
  resolve: {
    alias: {
      '@': path.resolve(__dirname, 'src'),
    },
  },
  build: {
    outDir: 'dist',
    sourcemap: true,
  },
});
