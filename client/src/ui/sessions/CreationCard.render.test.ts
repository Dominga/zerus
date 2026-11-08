import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";
import type { CreationCard } from "../../types";

// Render production cards in Node with only their application stores isolated.
const entry = "\0creation-card-render";
const stores = "\0creation-card-stores";
const translations = "\0creation-card-i18n";
const creations = "\0creation-card-creations";
const organize = "\0creation-card-organize";
let server: ViteDevServer | undefined;
let render: (card: CreationCard) => { full: string; strip: string };

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  server = await createServer({
    configFile: false, root: fileURLToPath(new URL("../../../", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "creation-card-render-stores", enforce: "pre",
      resolveId(id, importer) {
        if ([entry, stores, translations, creations, organize].includes(id)) return id;
        if (["../../store/catalog", "../../store/ui", "../../store/settings", "../../store/accountMarks"].includes(id)) return stores;
        if (id === "../../i18n") return translations;
        if (importer?.endsWith("/CreationCard.tsx")) {
          if (id === "../../store/creations") return creations;
          if (id === "./organize") return organize;
        }
      },
      load(id) {
        if (id === stores) return `export const agentKind = (cmd) => cmd;
          export const computerFresh = () => true, computerOffline = () => false;
          export const selectedKey = () => undefined, peek = () => false, setPeek = () => {}, setMenu = () => {};
          export const now = () => 0, settings = {}, sessionMark = () => undefined;`;
        if (id === translations) return `export const trNoop = s => s;
          export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));`;
        if (id === creations) return `export const selectCreation = () => {}, hideCreation = async () => {};`;
        if (id === organize) return `export const clearMulti = () => {};`;
        if (id === entry) return `import {createComponent} from 'solid-js';
          import {renderToString} from 'solid-js/web';
          import {CreationCard, CreationTile} from '/src/ui/sessions/CreationCard.tsx';
          export const render = (card) => ({
            full: renderToString(() => createComponent(CreationCard, {card})),
            strip: renderToString(() => createComponent(CreationTile, {card})),
          });`;
      },
    }, solid({ ssr: true })],
  });
  ({ render } = await server.ssrLoadModule(entry) as { render: typeof render });
});

afterAll(async () => { await server?.close(); });

const card = (extra: Partial<CreationCard> = {}): CreationCard => ({
  id: "launch-fixture", launch_id: "attempt-1", tag: "Preparing review", state: "confirming", caption: "Confirming session…", error: "",
  created: 100, group: "ungrouped", host: "", machine: "Fixture PC", cmd: "codex", account: "", target: "/fixture/next",
  show_pending: true, launch_dispatched: true, stop_requested: false, worktree_receipts: false, can_resume: false, can_edit: false, bound_session: "",
  ...extra,
});

describe("creation card geometry and progress", () => {
  it("puts active progress in the ordinary provider slot without moving the other lines", () => {
    const { full, strip } = render(card());
    expect(full).toContain("scard tiled");
    expect(full).toContain("sc-tile");
    expect(full).toContain("agent-tile");
    expect(full).toContain("set-spinner");
    expect(full).not.toContain("agent-badge");
    for (const slot of ["sc-l1", "sc-l2", "sc-l3", "sc-foot", "sc-title", "sc-meta", "sc-meta-ico", "sc-model"]) {
      expect(full).toContain(slot);
    }
    expect(full).toContain("Preparing review");
    expect(full).toContain("Confirming session…");
    expect(full).toContain("/fixture/next");
    expect(strip).toContain("st-provider");
    expect(strip).toContain("creation-progress");
    expect(strip).toContain("set-spinner");
    expect(strip).toContain("Confirming session…");
  });

  it("keeps recovery, unknown and settled states truthful and still", () => {
    for (const state of ["unknown", "recover", "stopping", "stopped", "failed", "storage_error"]) {
      const { full, strip } = render(card({ state }));
      expect(full).not.toContain("set-spinner");
      expect(strip).not.toContain("set-spinner");
      expect(strip).not.toContain("creation-progress");
      expect(full).toContain("scard tiled");
      expect(full).toContain("agent-tile");
      expect(strip).toContain("agent-badge");
    }
  });

  it("retains the actual error and folder while leaving absent model metadata empty", () => {
    const { full } = render(card({ state: "failed", caption: "Creation failed", error: "No folder." }));
    expect(full).toContain("Creation failed");
    expect(full).toContain("No folder.");
    expect(full).toContain("sc-meta-ico");
    expect(full).toContain("/fixture/next");
    expect(full).toContain("sc-model ell");
    expect(full).not.toContain("sc-effort");
    expect(full).not.toContain("acct-chip");
  });
});
