import { defineConfig } from "vite";
import { fileURLToPath } from "node:url";

// xterm.js and Terminal's real copy and paste handler and menu; no session,
// bridge or clipboard write is involved.
export default defineConfig({
  root: fileURLToPath(new URL("../..", import.meta.url)), base: "./",
  build: { target: "safari16", rollupOptions: { input: fileURLToPath(new URL("index.html", import.meta.url)) } },
});
