import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";

// Task 243: the Compact skins' one title bar, Qt's WindowTitleBar with the
// rail's controls (placeRailControls): the mark and name, the sessions
// toggle (Sessions page only), New session and the screens, the Go to
// search, the pin and Settings, then the window buttons; the bar is the drag
// region. Task 409: beside the search stand the client log's last line and,
// with the update notice, the version; the bottom status bar is gone.
const entry = "\0titlebar-render";
const STUBS: Record<string, string> = {
  "src/i18n": `export const trNoop = (s) => s; export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));`,
  "src/store/ui": `export const page = () => globalThis.__bar.page; export const setPage = () => {}; export const counts = () => ({ attention: 2 });
    export const logoMotion = () => "still"; export const setMenu = () => {}; export const stripMode = () => globalThis.__bar.strip; export const toggleSessionsList = () => {};
    export const toast = () => {};`,
  "src/ui/sessions/mark-entries": `export const markAllEntry = () => ({});`,
  "src/ui/sessions/background-menu": `export const expandAllCollapsedEntry = () => ({});`,
  "src/store/windowLayer": `export const windowLayer = () => ({ on: false, supported: true, hint: "" }); export const setWindowLayer = async () => {};`,
  "src/ui/sessions/newsession-view": `export const newSessionFromRail = () => {};`,
  "src/store/shortcuts": `export const shortcutTooltip = (text) => text; export const chord = () => "Ctrl+K";`,
  "src/store/goTo": `export const openGoTo = () => {};`,
  "src/store/teams": `export const waiting = () => ({ total: 1 });`,
  "src/store/modelServers": `export const modelServers = { available: () => false, listed: () => true, refreshList: async () => {} };`,
  "src/bridge": `export const windowControl = async () => false;`,
  "src/store/buildInfo": `export const refreshAppBuildInfo = async () => {}; export const appBuildInfo = () => ({ version: "0.49.0" });`,
  "src/ui/components/ZerusLogo": `export const ZerusLogo = () => "[logo]";`,
  "src/ui/lib/icons": `export const Icon = (p) => "[" + p.name + "]";`,
  "src/store/desk": `export const openAbout = () => {}; export const openUpdates = () => {};`,
  "src/ui/screens/settings/about-version": `export const installedAppName = () => "Zerus"; export const railVersion = () => "0.48";`,
  "src/skin/runtime": `export const skin = () => ({ layout: { rail: "icons" } });`,
  // Task 409: the title bar's own new parts, read from the test's global state.
  "src/store/appLog": `export const appLog = { latest: () => globalThis.__bar.log ?? "", level: () => globalThis.__bar.logLevel ?? "connection", open: () => { globalThis.__bar.opened = (globalThis.__bar.opened ?? 0) + 1; }, record: () => {}, start: () => {}, stop: () => {} };`,
  "src/store/updates": `export const updates = () => ({ quiet: false, available: !!globalThis.__bar.update, restart_required: false,
    login_item_deprioritized: false, version: "0.49.0" }); export const updateCommands = { restart: async () => {} };`,
  "src/ui/components/DesktopServiceStatus": `export const DesktopServiceStatus = () => "[service]";`,
  "src/ui/screens/BackgroundTasks": `export const BackgroundTasksChip = () => null;`,
  "src/ui/screens/CleanupChip": `export const CleanupChip = () => null;`,
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

const state = (value: { page: string; strip: boolean; log?: string; logLevel?: string; update?: boolean }) => { (globalThis as Record<string, unknown>).__bar = value; };
const plain = (html: string) => html.replace(/<!--[^>]*-->/g, "");
const order = (html: string, needles: string[]) => needles.map((needle) => html.indexOf(needle));

describe("the Compact skins' title bar (task 243)", () => {
  it("holds the controls in Qt's order and is the window's drag region", () => {
    state({ page: "sessions", strip: false });
    const html = render();
    expect(html).toMatch(/<header[^>]*class="titlebar"[^>]*data-tauri-drag-region/);
    const positions = order(html, ["[logo]", ">Zerus<", "data-sessions-toggle", "[add]", 'data-page="sessions"', 'data-page="projects"', 'data-page="machines"',
      'data-page="accounts"', 'data-page="waiting"', "data-log-line", "[unpinned]", "[settings]", 'data-window="minimize"', 'data-window="maximize"', 'data-window="close"']);
    expect(positions.every((at) => at >= 0)).toBe(true);
    expect([...positions].sort((a, b) => a - b)).toEqual(positions);
    // The Sessions badge and the waiting count.
    expect(plain(html)).toMatch(/\[sessions\]<span[^>]*class="tb-badge"[^>]*>2<\/span>/);
    // Task 409: the search field no longer carries the Ctrl+K hint. Owner,
    // 2026-11-09 (07e0ddacb): the title bar has no search field at all.
    expect(plain(html)).not.toContain("Ctrl+K");
    expect(html).not.toContain("tb-search");
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

describe("the title bar's log line and version (task 409)", () => {
  it("shows the client log's last line beside the search and opens the log from it", () => {
    state({ page: "sessions", strip: false, log: "msi-old: reconnecting (attempt 2)" });
    const html = plain(render());
    const line = html.slice(html.indexOf("data-log-line"), html.indexOf("[unpinned]"));
    expect(line).toContain("msi-old: reconnecting (attempt 2)");
    expect(line).toContain('title="Open log"');
    expect(line).toContain('aria-label="Open log"');
    // The next change replaces it, as the log's own tail does.
    state({ page: "sessions", strip: false, log: "5 of 5 machines connected" });
    expect(plain(render())).toContain("5 of 5 machines connected");
    expect(plain(render())).not.toContain("msi-old: reconnecting");
  });

  it("keeps the version here and the update notice beside it", () => {
    state({ page: "sessions", strip: false, update: true });
    const html = plain(render());
    expect(html).toContain("data-version");
    expect(html).toContain(">0.48<");
    expect(html).toContain(">Update available<");
    expect(html).toContain("Zerus 0.49.0 is available. Open Updates.");
    // With no update the version stays and the notice is gone.
    state({ page: "sessions", strip: false, update: false });
    const calm = plain(render());
    expect(calm).toContain("data-version");
    expect(calm).not.toContain("Update available");
  });

  it("leaves no status bar in the app shell", async () => {
    const { readFileSync } = await import(/* @vite-ignore */ ["node", "fs"].join(":")) as { readFileSync(path: URL): string };
    const app = String(readFileSync(new URL("../../App.tsx", import.meta.url)));
    expect(app).not.toContain("statusbar");
    expect(app).not.toContain("<StatusBar");
    expect(app).toContain("<TitleBar mode={props.mode}");
    const css = String(readFileSync(new URL("../styles/app.css", import.meta.url)));
    expect(css).not.toContain(".statusbar");
    expect(css).toContain(".tb-log {");
  });
});

// Task 424 (owner, 2026-11-09): the log line and its icon are pale grey in every
// skin, brighter only on hover or focus, and an error line is a muted red.
describe("the title bar's pale log line (task 424)", () => {
  it("carries the muted class and draws in the skin's dim token", async () => {
    state({ page: "sessions", strip: false, log: "msi-old: reconnecting (attempt 4)" });
    const html = plain(render());
    const line = html.slice(html.lastIndexOf("<button", html.indexOf("data-log-line")), html.indexOf("[unpinned]"));
    expect(line).toMatch(/class="tb-log muted\b/);
    expect(line).toContain("[file-text]");
    expect(line).toContain("msi-old: reconnecting (attempt 4)");
    const { readFileSync } = await import(/* @vite-ignore */ ["node", "fs"].join(":")) as { readFileSync(path: URL): string };
    const css = String(readFileSync(new URL("../styles/app.css", import.meta.url)));
    // The faint token, and the icon inherits it (currentColor).
    expect(css).toMatch(/\.tb-log \{[^}]*color: var\(--z-color-dim\);/);
    expect(css).not.toMatch(/\.tb-log \{[^}]*color: var\(--z-color-muted\);/);
    // Bright only on hover and focus.
    expect(css).toMatch(/\.tb-log:hover, \.tb-log:focus-visible \{[^}]*color: var\(--z-color-text\);/);
    // An error line is a muted red, and still brightens to the danger colour.
    expect(css).toMatch(/\.tb-log\.tb-log-error \{[^}]*color: color-mix\(in srgb, var\(--z-color-danger\) 60%, var\(--z-color-dim\)\);/);
    expect(css).toMatch(/\.tb-log\.tb-log-error:hover, \.tb-log\.tb-log-error:focus-visible \{[^}]*color: var\(--z-color-danger\);/);
  });

  it("adds the error class only while the last line is an error", () => {
    state({ page: "sessions", strip: false, log: "Windows: the runtime failed to start", logLevel: "error" });
    const error = plain(render());
    expect(error).toMatch(/class="tb-log muted tb-log-error\b/);
    state({ page: "sessions", strip: false, log: "5 of 5 machines connected", logLevel: "connection" });
    expect(plain(render())).toMatch(/class="tb-log muted\b/);
    expect(plain(render())).not.toContain("tb-log-error");
  });
});
