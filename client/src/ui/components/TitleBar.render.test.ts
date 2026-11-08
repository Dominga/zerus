import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";

// Task 243: the Compact skins' one title bar, Qt's WindowTitleBar with the
// rail's controls (placeRailControls): the mark and name, the sessions
// toggle (Sessions page only), New session and the screens, the Go to
// search, the pin and Settings, then the window buttons; the bar is the drag
// region. The bar is production code; the stores it reads are stand-ins.
const entry = "\0titlebar-render";
const STUBS: Record<string, string> = {
  "src/i18n": `export const trNoop = (s) => s; export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));`,
  "src/store/ui": `export const page = () => globalThis.__bar.page; export const setPage = () => {}; export const counts = () => ({ attention: 2 });
    export const logoMotion = () => "still"; export const setMenu = () => {}; export const stripMode = () => globalThis.__bar.strip; export const toggleSessionsList = () => {};`,
  "src/ui/sessions/mark-entries": `export const markAllEntry = () => ({});`,
  "src/ui/sessions/background-menu": `export const expandAllCollapsedEntry = () => ({});`,
  "src/store/windowLayer": `export const windowLayer = () => ({ on: false, supported: true, hint: "" }); export const setWindowLayer = async () => {};`,
  "src/ui/sessions/newsession-view": `export const newSessionFromRail = () => {};`,
  "src/store/shortcuts": `export const shortcutTooltip = (text) => text; export const chord = () => "Ctrl+K";`,
  "src/store/goTo": `export const openGoTo = () => {};`,
  "src/store/teams": `export const waiting = () => ({ total: 1 });`,
  "src/store/modelServers": `export const modelServers = { available: () => false, listed: () => true, refreshList: async () => {} };`,
  "src/bridge": `export const windowControl = async () => false;`,
  "src/store/buildInfo": `export const refreshAppBuildInfo = async () => {}; export const appBuildInfo = () => undefined;`,
  "src/ui/components/ZerusLogo": `export const ZerusLogo = () => "[logo]";`,
  "src/ui/lib/icons": `export const Icon = (p) => "[" + p.name + "]";`,
  "src/store/desk": `export const openAbout = () => {};`,
  "src/ui/screens/settings/about-version": `export const installedAppName = () => "Zerus"; export const railVersion = () => "0.48";`,
  "src/skin/runtime": `export const skin = () => ({ layout: { rail: "icons" } });`,
};
const stubId = (key: string) => `\0titlebar-stub:${key}`;
let server: ViteDevServer | undefined;
let render: () => string;

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  server = await createServer({
    configFile: false, root: fileURLToPath(new URL("../../../", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "titlebar-render-mocks", enforce: "pre",
      async resolveId(id, importer) {
        if (id === entry || id.startsWith("\0titlebar-stub:")) return id;
        if (!importer || id.startsWith("\0") || !id.startsWith(".")) return undefined;
        const resolved = await this.resolve(id, importer, { skipSelf: true });
        const path = resolved?.id.replace(/\\/g, "/").replace(/\.(tsx?|jsx?)$/, "") ?? "";
        const key = Object.keys(STUBS).find((stub) => path.endsWith(`/${stub}`));
        return key ? stubId(key) : undefined;
      },
      load(id) {
        if (id.startsWith("\0titlebar-stub:")) return STUBS[id.slice("\0titlebar-stub:".length)];
        if (id === entry) return `import {createComponent} from 'solid-js';
          import {renderToString} from 'solid-js/web';
          import {TitleBar} from '/src/ui/components/TitleBar.tsx';
          export const render = () => renderToString(() => createComponent(TitleBar, {}));`;
        return undefined;
      },
    }, solid({ ssr: true })],
  });
  render = (await server.ssrLoadModule(entry)).render;
});

afterAll(async () => { await server?.close(); });

const state = (value: { page: string; strip: boolean }) => { (globalThis as Record<string, unknown>).__bar = value; };
const plain = (html: string) => html.replace(/<!--[^>]*-->/g, "");
const order = (html: string, needles: string[]) => needles.map((needle) => html.indexOf(needle));

describe("the Compact skins' title bar (task 243)", () => {
  it("holds the controls in Qt's order and is the window's drag region", () => {
    state({ page: "sessions", strip: false });
    const html = render();
    expect(html).toMatch(/<header[^>]*class="titlebar"[^>]*data-tauri-drag-region/);
    const positions = order(html, ["[logo]", ">Zerus<", "data-sessions-toggle", "[add]", 'data-page="sessions"', 'data-page="projects"', 'data-page="machines"',
      'data-page="accounts"', 'data-page="waiting"', "tb-search", "[unpinned]", "[settings]", 'data-window="minimize"', 'data-window="maximize"', 'data-window="close"']);
    expect(positions.every((at) => at >= 0)).toBe(true);
    expect([...positions].sort((a, b) => a - b)).toEqual(positions);
    // The Sessions badge and the waiting count.
    expect(plain(html)).toMatch(/\[sessions\]<span[^>]*class="tb-badge"[^>]*>2<\/span>/);
    expect(plain(html)).toContain("Ctrl+K");
    // The model servers tab shows only while a server exists.
    expect(html).not.toContain('data-page="modelservers"');
  });

  it("toggles the sessions list on the Sessions page only, saying which way", () => {
    state({ page: "sessions", strip: false });
    expect(render()).toContain('aria-label="Hide the session list"');
    state({ page: "sessions", strip: true });
    const hidden = render();
    expect(hidden).toContain('aria-label="Show the session list"');
    expect(hidden).toContain("[expand-sessions]");
    state({ page: "machines", strip: false });
    expect(render()).not.toContain("data-sessions-toggle");
  });

  // Task #361: New team left the top bar; the session's own tab row offers it
  // (Qt's m_teamNewTab), as the rail does not.
  it("holds no New team icon", () => {
    state({ page: "sessions", strip: false });
    const html = plain(render());
    expect(html).not.toContain('data-page="newteam"');
    expect(html).not.toContain('aria-label="New team"');
    expect(html).toContain('data-page="sessions"');
  });
});
