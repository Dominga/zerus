/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

// A cross-origin isolated page gets precise timers (performance.now), which
// the frame-pacing measurement needs; tauri.conf.json sets the same headers on
// the app's own protocol for release builds.
const ISOLATION = { "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp" };

// Vitest renders in Node, where solid-refresh's HMR runtime cannot load: its
// virtual module resolves to file:///@solid-refresh on Windows and the render
// suites fail with "filename must be a file URL". The tests keep Solid's own
// runtime; only the dev server gets hot refresh.
const TESTING = !!(globalThis as { process?: { env?: Record<string, string | undefined> } }).process?.env?.VITEST;

// The Tauri CLI starts this dev server (tauri.conf.json devUrl) and loads the
// built `dist` in release builds. A fixed port lets `tauri dev` find it.
export default defineConfig({
  base: "./",
  plugins: [solid({ hot: !TESTING })],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
    watch: { ignored: ["**/src-tauri/**", "**/core/**", "**/core-stub/**"] },
    headers: ISOLATION,
  },
  preview: { headers: ISOLATION },
  build: {
    // WebKitGTK 2.4x and WKWebView on macOS 14 are the oldest engines.
    target: "safari16",
    outDir: "dist",
    emptyOutDir: true,
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
    // Task 245: numbers and dates read the same on every machine's locale.
    setupFiles: ["src/test-locale.ts"],
  },
});
