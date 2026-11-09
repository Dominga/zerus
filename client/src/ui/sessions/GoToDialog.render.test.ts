import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";

// Task 423: the title-bar search opens in place. The palette, as the client
// draws it, lies over the title bar's search field (its rect at open time) and
// shows the field's own placeholder; its rows are sessions and messages. The
// stores are stand-ins; the dialog and the palette model are production code.
const entry = "\0goto-render";
const goToStore = "\0goto-render-store";
const messages = "\0goto-render-messages";
const shortcuts = "\0goto-render-shortcuts";
const translations = "\0goto-render-i18n";
let server: ViteDevServer | undefined;
let render: (anchor: unknown) => string;

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  (globalThis as Record<string, unknown>).innerWidth = 1400;
  (globalThis as Record<string, unknown>).innerHeight = 900;
  server = await createServer({
    configFile: false, root: fileURLToPath(new URL("../../../", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "goto-render-mocks", enforce: "pre",
      resolveId(id) {
        if ([entry, goToStore, messages, shortcuts, translations].includes(id)) return id;
        if (id === "../../store/goTo") return goToStore;
        if (id === "../../store/goToMessages") return messages;
        if (id === "../../store/shortcuts") return shortcuts;
        if (id === "../../i18n" || id === "../../../i18n") return translations;
        return undefined;
      },
      load(id) {
        if (id === translations) return `export const trNoop = (s) => s;
          export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));`;
        if (id === shortcuts) return `export const keys = () => [];
          export const comboFromEvent = () => ""; export const matchSequence = () => "none";`;
        if (id === goToStore) return `export const state = { anchor: null };
          export const goToOrigin = () => ({});
          export const goToAnchor = () => state.anchor;
          export const goToItems = () => [
            { kind: 0, id: "run:a", host: "", title: "Checkout", detail: ["Shop"], recency: 0, keys: ["claude/shop/checkout", "conv-7f3a"] },
            { kind: 5, id: "session.new", host: "", title: "New session", detail: "Sessions", recency: -1 },
          ];
          export const activateGoTo = () => {}; export const closeGoTo = () => {};`;
        if (id === messages) return `export const setGoToMessageQuery = () => {};
          export const openGoToMessage = () => {};
          export const goToMessages = () => [{ item: { kind: 6, id: "\\nrun:a\\nm1", host: "", title: "Checkout", detail: ["the refund webhook retries twice"], recency: -1 } }];`;
        if (id === entry) return `import {createComponent} from 'solid-js';
          import {renderToString} from 'solid-js/web';
          import {GoToDialog} from '/src/ui/sessions/GoToDialog.tsx';
          import {state} from '${goToStore}';
          export const render = (anchor) => { state.anchor = anchor; return renderToString(() => createComponent(GoToDialog, {})); };`;
        return undefined;
      },
    }, solid({ ssr: true })],
  });
  render = (await server.ssrLoadModule(entry)).render;
});

afterAll(async () => { await server?.close(); });

const PLACEHOLDER = "Search sessions, IDs and messages…";

describe("the title-bar search opens in place (task 423)", () => {
  it("lies over the search field's rect with the field's own placeholder", async () => {
    const html = render({ left: 300, top: 6, width: 340, height: 28 });
    expect(html).toContain('data-anchored="1"');
    expect(html).toMatch(/class="goto anchored"/);
    expect(html).toMatch(/left:\s*300px/);
    expect(html).toMatch(/top:\s*6px/);
    expect(html).toMatch(/width:\s*560px/);
    expect(html).toContain(`placeholder="${PLACEHOLDER}"`);
    expect(html).not.toContain("Go to… (type &gt; for commands)");
    // The title bar's field says the same words.
    const fs = await import(/* @vite-ignore */ ["node", "fs"].join(":")) as { readFileSync(path: URL, encoding: "utf8"): string };
    const bar = fs.readFileSync(new URL("../components/TitleBar.tsx", import.meta.url), "utf8");
    expect(bar).toContain(`tr("${PLACEHOLDER}")`);
  });

  it("lists sessions and the messages that match", () => {
    const html = render({ left: 300, top: 6, width: 340, height: 28 });
    expect(html).toContain("SESSION");
    expect(html).toContain("MESSAGE");
    expect(html).toContain("the refund webhook retries twice");
  });

  it("opens centred when there is no title-bar field to lie over", () => {
    const html = render(null);
    expect(html).not.toContain("data-anchored");
    expect(html).toMatch(/class="goto"/);
    expect(html).toContain(`placeholder="${PLACEHOLDER}"`);
  });
});
