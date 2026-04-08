import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, '../desktop/src'),
    },
  },
  define: {
    // Ensure Tauri is never detected in web builds
    '__TAURI__': 'undefined',
    '__TAURI_INTERNALS__': 'undefined',
  },
  envPrefix: 'VITE_',
  build: {
    outDir: 'dist',
    sourcemap: true,
  },
});
