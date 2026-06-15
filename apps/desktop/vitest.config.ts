import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import path from "path";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
      // libsodium-wrappers 0.7.15 ships a broken ESM bundle (.mjs imports a
      // sibling libsodium.mjs that isn't shipped). Mirror the alias from
      // vite.config.ts so transitive imports (App.tsx → e2e/* → libsodium)
      // resolve under vitest too.
      "libsodium-wrappers": path.resolve(
        __dirname,
        "../../node_modules/libsodium-wrappers/dist/modules/libsodium-wrappers.js",
      ),
    },
  },
});
