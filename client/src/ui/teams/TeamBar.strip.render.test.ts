import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";

// Task #426 (owner, screenshots of package 167): the team bar's "Switch to
// standard…" text was covered by the green phase progress bar that starts right
// after it. The strip lays the bar out so the bar keeps its width and the text
// shortens with an ellipsis first: the mode control clips, its button label is
// an ellipsis span, and the progress bar never shrinks.
const entry = "\0team-bar-strip";
const STUBS: Record<string, string> = {
  "src/i18n": `export const trNoop = (s) => s; export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));`,
  "src/store/teams": `export const teams = { list: () => [] }; export const memberRow = () => undefined;`,
  "src/store/ui": `export const setDetailTab = () => {}; export const setPage = () => {}; export const setSelectedKey = () => {};`,
  "src/ui/teams/ProposalCards": `export const useModel = () => ({ model: { phaseMode: async () => {} }, tick: () => {} }); export const windowPort = {};`,
  "src/ui/teams/noReport": `export const noReportReply = () => ""; export const remindMember = () => {};`,
  "src/ui/lib/TextField": `export const TextArea = () => null; export const TextInput = () => null;`,
  "src/ui/lib/icons": `export const Icon = (p) => "[" + p.name + "]";`,
};
const stubId = (key: string) => `\0team-bar-stub:${key}`;
let server: ViteDevServer | undefined;
let render: () => string;

// The live strip of the owner's screenshot: free mode in phase 9 offers the
// switch back to standard planning right before the progress bar.
const team = {
  id: "team-1", name: "Zerus Next for the fork", status: "running", phase_modes_known: true,
  plan_mode: "free", free_phase: 9, phases: [{ n: 9, mode: "free", state: "running", title: "Phase 9" }],
};

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  const root = fileURLToPath(new URL("../../../", import.meta.url));
  server = await createServer({
    configFile: false, root,
    cacheDir: fileURLToPath(new URL("../../../node_modules/.vite-teambar", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "team-bar-strip-mocks", enforce: "pre",
      async resolveId(id, importer) {
        if (id === entry || id.startsWith("\0team-bar-stub:")) return id;
        if (!importer || id.startsWith("\0") || !id.startsWith(".")) return undefined;
        const resolved = await this.resolve(id, importer, { skipSelf: true });
        const path = resolved?.id.replace(/\\/g, "/").replace(/\.(tsx?|jsx?)$/, "") ?? "";
        const key = Object.keys(STUBS).find((stub) => path.endsWith(`/${stub}`));
        return key ? stubId(key) : undefined;
      },
      load(id) {
        if (id.startsWith("\0team-bar-stub:")) return STUBS[id.slice("\0team-bar-stub:".length)];
        if (id === entry) return `import {createComponent} from 'solid-js';
          import {renderToString} from 'solid-js/web';
          import {TeamModeControl} from '/src/ui/teams/TeamActions.tsx';
          export const render = () => renderToString(() => createComponent(TeamModeControl, {computer: "", team: ${JSON.stringify(team)}}));`;
        return undefined;
      },
    }, solid({ ssr: true })],
  });
  render = (await server.ssrLoadModule(entry)).render;
});

afterAll(async () => { await server?.close(); });

describe("the team bar's strip layout (task #426)", () => {
  it("renders the switch's label in its own ellipsis span beside the free-mode chip", () => {
    const html = render();
    expect(html).toContain('class="team-mode"');
    expect(html).toContain("Free mode: phase 9");
    // The switch's text can shorten on its own, so the bar never covers it.
    expect(html).toMatch(/<button[^>]*data-action="phase-mode"[^>]*><span class="ell">Switch to standard…<\/span><\/button>/);
  });

  it("lays the strip out so the progress bar keeps its width and the text shortens first", async () => {
    const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
    const text = fs.readFileSync(new URL("client/src/ui/styles/teams.css", new URL("../../../../", import.meta.url)), "utf8");
    const rule = (prefix: string) => text.split("\n").find((line) => line.trim().startsWith(prefix)) ?? "";
    // The bar never shrinks; the mode control and its button clip and ellipsize.
    // Task #427 keeps the bar inside a min and a max width instead of fixed.
    expect(rule(".team-progress {")).toMatch(/min-width: 80px;/);
    expect(rule(".team-bar {")).toMatch(/overflow: hidden;/);
    expect(rule(".team-bar .team-mode {")).toMatch(/min-width: 0;/);
    expect(rule(".team-bar .team-mode {")).toMatch(/overflow: hidden;/);
    expect(rule(".team-mode .btn {")).toMatch(/min-width: 0;/);
    expect(rule(".team-mode .btn > .ell {")).toMatch(/min-width: 0;/);
  });
});
