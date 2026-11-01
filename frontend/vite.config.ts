/// <reference types="vitest/config" />
import {
  createUiDevCachePlugin,
  createUiProviderDedupe,
  createUiFontAssetsPlugin,
} from "@pdlc/ui/build";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

export default defineConfig({
  base: process.env.VITE_APP_BASE_PATH ?? "/",
  plugins: [createUiDevCachePlugin(), createUiFontAssetsPlugin(), react(), tailwindcss()],
  test: {
    environment: "jsdom",
    maxWorkers: 4,
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    setupFiles: ["./src/test-setup.ts"],
  },
  resolve: {
    dedupe: createUiProviderDedupe(),
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  server: {
    proxy: {
      "/api": {
        target:
          process.env.VITE_API_BASE_URL?.replace(/\/api\/v1\/?$/, "") ??
          "http://localhost:22801",
        changeOrigin: true,
      },
    },
  },
});
