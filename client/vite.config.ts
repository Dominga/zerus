/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import pkg from "./package.json" with { type: "json" };

// The Tauri CLI starts this dev server (tauri.conf.json devUrl) and loads the
// built `dist` in release builds. A fixed port lets `tauri dev` find it.
export default defineConfig({
  base: "./",
  plugins: [solid()],
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
    watch: { ignored: ["**/src-tauri/**", "**/core/**", "**/core-stub/**"] },
  },
  build: {
    // WebKitGTK 2.4x and WKWebView on macOS 14 are the oldest engines.
    target: "safari16",
    outDir: "dist",
    emptyOutDir: true,
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
