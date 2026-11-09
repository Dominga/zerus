import { afterAll, beforeAll, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";
import type { CleanupRow, CleanupView } from "../../store/terminationCleanup";

const entry = "\0background-tasks-render", translations = "\0background-tasks-i18n", bridge = "\0background-tasks-bridge";
let server: ViteDevServer;
let render: (view: Partial<CleanupView>) => string;
beforeAll(async () => {
  server = await createServer({
    configFile: false, server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "background-tasks-render", enforce: "pre",
      resolveId(id) {
        if ([entry, translations, bridge].includes(id)) return id;
        if (id.endsWith("/i18n") || id === "./i18n") return translations;
        if (id.endsWith("/bridge")) return bridge;
      },
      load(id) {
        if (id === translations) return `const fill = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));
          export const tr = fill; export const trNoop = (s) => s;`;
        if (id === bridge) return `export const inTauri = () => false;`;
        if (id === entry) return `import {createComponent} from 'solid-js';
          import {renderToString} from 'solid-js/web';
          import {BackgroundTasksChip} from '/src/ui/screens/BackgroundTasks.tsx';
          import {CleanupChip,CleanupReviewButton} from '/src/ui/screens/CleanupChip.tsx';
          import {applyCleanup, emptyCleanup} from '/src/store/terminationCleanup.ts';
          let revision = 0;
          export const render = (view) => { applyCleanup({...emptyCleanup(), ...view, revision: ++revision});
            return renderToString(() => [createComponent(CleanupReviewButton, {}), createComponent(BackgroundTasksChip, {}), createComponent(CleanupChip, {})]); };`;
      },
    }, solid({ ssr: true })],
  });
  render = (await server.ssrLoadModule(entry)).render;
});
afterAll(async () => { await server?.close(); });

const row = (over: Partial<CleanupRow>): CleanupRow => ({ id: "job", label: "work", path: "/srv/example/work", machine: "this computer", computer: "",
  branch: "work", size: "1 KiB", status: "ready", reason: "", details: "", can_check: false, ready: false, ...over });
const running = row({ busy: true, status: "checking", activity: "checking" });

it("shows the count while work runs and no cleanup chip beside it", () => {
  const html = render({ rows: [running, { ...running, id: "other" }], busy: true, cleaning: true, text: "Cleaning working folders…" });
  expect(html).toContain("Background tasks 2");
  expect(html).not.toContain("Cleaning working folders…");
});
it("always offers worktree review even when nothing is held", () => { expect(render({ rows: [] })).toContain('aria-label="Review working folders"'); });
it("keeps a kept result on its warning chip beside running work", () => {
  const html = render({ rows: [running, row({ id: "kept", status: "kept" })], busy: true, cleaning: true, kept: 1, text: "Folder kept: work.", title: "Folder kept: work." });
  expect(html).toContain("Background tasks 1");
  expect(html).toContain("Folder kept: work.");
  expect(html).toMatch(/warn-chip/);
});
it("keeps an unknown result on its warning chip when only running rows are listed", () => {
  const html = render({ rows: [running], busy: true, cleaning: true, unknown: 1, text: "Working folders: 1 unknown" });
  expect(html).toContain("Background tasks 1");
  expect(html).toContain("Working folders: 1 unknown");
  expect(html).toMatch(/warn-chip/);
});
it("keeps a kept or unknown result once the running chip has gone", () => {
  for (const view of [{ kept: 1, text: "Working folders: 1 kept" }, { unknown: 1, text: "Working folders: 1 unknown" }]) {
    const html = render({ rows: [], ...view });
    expect(html).not.toContain("Background tasks");
    expect(html).toContain(view.text);
    expect(html).toMatch(/warn-chip/);
  }
});
