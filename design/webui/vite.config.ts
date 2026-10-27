import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import { resolve } from "node:path";

// Two pages: the Russian review page (index.html) and the clickable mockup
// (app.html). Relative base so the build works from any static folder.
export default defineConfig({
  base: "./",
  plugins: [solid()],
  build: {
    target: "safari16",
    outDir: "dist",
    emptyOutDir: true,
    rollupOptions: {
      input: {
        review: resolve(import.meta.dirname, "index.html"),
        app: resolve(import.meta.dirname, "app.html"),
      },
    },
  },
});
