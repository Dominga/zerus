import { afterAll, beforeAll, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";
import type { CleanupProgram } from "./termination-offer";

const entry = "\0termination-program-render", translations = "\0termination-program-i18n";
let server: ViteDevServer;
let render: (programs: readonly CleanupProgram[], force?: boolean) => string;
beforeAll(async () => {
  server = await createServer({
    configFile: false, server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "termination-program-render", enforce: "pre",
      resolveId(id, importer) {
        if ([entry, translations].includes(id)) return id;
        if (importer?.endsWith("/TerminationPrograms.tsx") && id === "../../i18n") return translations;
      },
      load(id) {
        if (id === translations) return `export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));`;
        if (id === entry) return `import {createComponent} from 'solid-js';
          import {renderToString} from 'solid-js/web';
          import {TerminationPrograms} from '/src/ui/sessions/TerminationPrograms.tsx';
          export const render = (programs, force = false) => renderToString(() => createComponent(TerminationPrograms, {programs, force}));`;
      },
    }, solid({ ssr: true })],
  });
  render = (await server.ssrLoadModule(entry)).render;
});
afterAll(async () => { await server?.close(); });

it("starts collapsed with an accurate count and retains all 64 rows for expansion", () => {
  const programs = Array.from({ length: 64 }, (_, i) => ({ pid: 100 + i, label: `program-${i}` }));
  const html = render(programs);
  expect(html.match(/<li\b/g)).toHaveLength(64);
  for (const program of programs) expect(html).toContain(`${program.label} (PID ${program.pid})`);
  expect(html).toContain("64 other programs will be asked to stop. Save their work before continuing.");
  expect(html).toMatch(/<details[^>]*><summary>/);
  expect(html).not.toMatch(/<details[^>]*\bopen\b/);
  expect(html).not.toContain("<button");
});
it("escapes native labels and distinguishes the separate Force confirmation", () => {
  const html = render([{ pid: 123, label: "<script>editor</script>" }], true);
  expect(html).not.toContain("<script>");
  expect(html).toContain("&lt;script>");
  expect(html).toContain("Force stop may discard unsaved work");
  expect(html).not.toContain("will be asked to stop");
  expect(html).not.toContain("<details");
  expect(render([])).not.toContain("termination-programs");
});

it("uses a singular warning and keeps the complete Force list expanded", () => {
  const single = render([{ pid: 10, label: "editor" }]);
  expect(single).toContain("1 other program will be asked to stop. Save its work before continuing.");
  const all = Array.from({ length: 64 }, (_, i) => ({pid: 100 + i, label: `program-${i}`}));
  const force = render(all, true);
  expect(force.match(/<li\b/g)).toHaveLength(64);
  expect(force).toContain("program-63 (PID 163)");
  expect(force).not.toContain("<details");
});
