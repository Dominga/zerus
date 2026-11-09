import { afterAll, beforeAll, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";
import { usageOf, projected, NOW } from "../../store/testing";
import type { Usage } from "../../types";

const entry = "\0usage-render", stores = "\0usage-stores", translations = "\0usage-i18n", bridge = "\0usage-bridge";
let server: ViteDevServer;
let render: (usage: Usage, now: number) => string;
beforeAll(async () => {
  server = await createServer({
    configFile: false, server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "usage-render", enforce: "pre",
      resolveId(id, importer) {
        if ([entry, stores, translations, bridge].includes(id)) return id;
        if (id.endsWith("/i18n")) return translations;
        if (id.endsWith("/bridge")) return bridge;
        if (importer?.endsWith("/UsageChips.tsx") && (id.startsWith("../../store/") || id === "./SessionCard")) return stores;
      },
      load(id) {
        if (id === translations) return `export const trNoop = s => s; export const uiLanguage = () => "en";
          export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));`;
        if (id === bridge) return `export const inTauri = () => false;`;
        if (id === stores) return `export const state = {usage: {}, now: 0};
          export const sessionUsage = () => state.usage, useNow = () => () => state.now, computerOffline = () => false, text = () => 'rails';
          export const startSessionUsage = () => {}, stopSessionUsage = () => {}, followSessionUsage = () => {}, refreshSessionUsage = () => {};
          export const setAccountMachine = () => {}, setRequestedProfile = () => {}, loadAccounts = () => {}, setPage = () => {}, openInspector = () => {}, AgentBadge = () => "";
          export const composer = {view: () => undefined}, followedFor = () => undefined;`;
        if (id === entry) return `import {createComponent} from 'solid-js';
          import {renderToString} from 'solid-js/web';
          import {AccountUsageChip} from '/src/ui/sessions/UsageChips.tsx';
          import {state} from '${stores}';
          export const render = (usage, now) => { state.usage = usage; state.now = now;
            return renderToString(() => createComponent(AccountUsageChip, {session: {tracked: true, computer: '', agent: 'claude'}})); };`;
      },
    }, solid({ ssr: true })],
  });
  render = (await server.ssrLoadModule(entry)).render;
});
afterAll(async () => { await server?.close(); });

it("keeps the real footer meters visible with the sample age and cooldown", () => {
  const usage = usageOf({ status: "rate_limited", checked_at: NOW - 120, retry_at: NOW + 100, windows: [projected({ used: 42 })] });
  const html = render(usage, NOW);
  expect(html).toContain("5h 42%");
  expect(html).toContain("Limits 2 min ago, rate limited. Retry in 100 s");
  expect(html).toMatch(/class="icon-btn small usage-refresh" disabled/);
  expect(render(usage, NOW + 100)).not.toMatch(/class="icon-btn small usage-refresh" disabled/);
  expect(render({ ...usage, status: "ok", retry_at: null }, NOW)).not.toContain("rate limited");
});
