// Builds the synthetic pacing harness (client/bench/harness) with the client's
// dependencies: npx vite build --config client/bench/harness/vite.config.ts
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

const here = fileURLToPath(new URL(".", import.meta.url));

export default defineConfig({
  root: here,
  base: "./",
  plugins: [solid()],
  build: { outDir: here + "../build/harness", emptyOutDir: true, target: "es2022" },
});
