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

// The packaged build links the lazy chunks' stylesheets before the entry one
// (which carries base.css and its `@layer reset, base, components, ...`), so a
// chunk opened `components` before `reset` existed and the reset's
// `button { padding: 0; border: 0; background: none }` outranked every button
// style in the app (the owner's 168/169: tabs, folder headers, card actions).
// Every emitted stylesheet now starts with the one layer order, so whichever
// file the document reads first fixes it. A <style> in index.html is no option:
// Tauri's CSP would then block the runtime skin (task 454).
export const LAYER_ORDER = "@layer reset, base, components, variants, utilities;";
function layerOrderFirst() {
  return {
    name: "zerus-layer-order-first",
    apply: "build" as const,
    enforce: "post" as const,
    generateBundle(_options: unknown, bundle: Record<string, { type: string; fileName: string; source?: string | Uint8Array }>) {
      for (const asset of Object.values(bundle)) {
        if (asset.type !== "asset" || !asset.fileName.endsWith(".css")) continue;
        const text = typeof asset.source === "string" ? asset.source : new TextDecoder().decode(asset.source);
        if (!text.startsWith(LAYER_ORDER)) asset.source = `${LAYER_ORDER}\n${text}`;
      }
    },
  };
}

// The Tauri CLI starts this dev server (tauri.conf.json devUrl) and loads the
// built `dist` in release builds. A fixed port lets `tauri dev` find it.
export default defineConfig({
  base: "./",
  plugins: [solid({ hot: !TESTING }), layerOrderFirst()],
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
    // Task #28 (team ede724c4): 24 render suites start a Vite SSR server and
    // transform a large component graph in beforeAll; on a loaded machine that
    // one-time setup passed the default 10 s ("Hook timed out in 10000ms",
    // seven suites at once, each passing alone). Setup gets its own bound;
    // the tests keep the default 5 s.
    hookTimeout: 60_000,
  },
});
