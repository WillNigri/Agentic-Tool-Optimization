import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
      // libsodium-wrappers 0.7.15 ships a broken ESM bundle: the .mjs
      // re-exports a sibling libsodium.mjs that is absent from node_modules.
      // Redirect to the CJS build so the Vite dev server and Vitest can
      // both resolve the package correctly.
      "libsodium-wrappers": path.resolve(
        __dirname,
        "../../node_modules/libsodium-wrappers/dist/modules/libsodium-wrappers.js",
      ),
    },
  },
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: "http://localhost:3000",
        changeOrigin: true,
      },
    },
  },
  // Vitest configuration — kept here so tests share the same aliases and
  // plugin stack as the Vite build, avoiding a separate vitest.config.ts.
  test: {
    environment: "jsdom",
    globals: false,
  },
} as Parameters<typeof defineConfig>[0]);
