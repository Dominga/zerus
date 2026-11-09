import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";

// Task #361 (owner, screenshot of package 161): New team left the top bar and
// belongs on the selected session's tab row, after Activity, as Qt's
// m_teamNewTab has it. The row is production code (TAB_ORDER and tabs() in
// SessionDetail.tsx); the stores it reads are stand-ins fed from globalThis.
// Header is not rendered here: its createResource needs a hydrating context.
const entry = "\0sessiondetail-render";
const STUBS: Record<string, string> = {
  "src/i18n": `export const trNoop = (s) => s; export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));`,
  "src/store/ui": `export const page = () => "sessions"; export const detailTab = () => "activity";
    export const selected = () => globalThis.__detail?.session; export const setDetailTab = () => {}; export const setMenu = () => {};
    export const subagentOpen = () => undefined;`,
  "src/store/visibility": `export const pageHidden = () => false;`,
  "src/store/teams": `export const selectedTeam = () => globalThis.__detail?.team;`,
  "src/store/drops": `export const drops = { hint: () => undefined };`,
  "src/store/processes": `export const processes = { view: () => ({ count: 0 }) };
    export const processesTab = () => ({ id: "processes", label: "Processes", show: true });`,
  "src/store/settings": `export const flag = () => false;`,
  "src/store/terminal": `export const terminal = { leaveSession() {}, leavePage() {}, opened() {}, setTarget: async () => {} };`,
  "src/store/native-ui": `export const nativeUi = { opened() {} };`,
  "src/store/catalog": `export const computerOffline = () => false; export const catalog = { rows: {} };`,
  "src/store/actions": `export const copyIdentity = async () => {}; export const detailsFor = () => undefined;
    export const followedFor = () => ({ value: undefined });`,
  "src/store/session": `export const activity = { loaded: () => true }; export const subagentActivity = { loaded: () => true };`,
  "src/store/activitySource": `export const activityReadOnly = () => false;`,
  "src/store/archiveActivity": `export const archiveActivity = { loaded: () => true };`,
  "src/store/inspector": `export const inspectorOpen = () => false; export const setInspectorOpen = () => {};
    export const openInspector = () => {}; export const inspectorHint = (s) => s; export const inspectorTab = () => "plan";`,
  "src/store/shortcuts": `export const onCommand = () => () => {};`,
  "src/store/navigation": `export const stepArea = () => {}; export const stepTab = () => undefined;`,
  "src/store/composer": `export const composer = { view: () => ({ clear: { enabled: false } }), openClear() {} };`,
  "src/store/subagentIdentity": `export const createSubagentIdentity = () => ({ follow() {}, child: () => undefined });`,
  "src/store/taskNavigation": `export const selectedTask = () => undefined;`,
  "src/store/taskContext": `export const followProjectTasks = () => {};`,
  "src/store/sessionFolders": `export const folderRequest = () => ({ target: { computer: "", directory: "" } });
    export const folderEntries = () => ({ folder: { hint: "Folder", disabled: true }, shell: { hint: "Shell", disabled: true } });`,
  "src/bridge": `export const sessionFolderStatus = async () => undefined; export const inspectSubagentIdentity = async () => undefined;
    export const terminalStatus = async () => ({});`,
  "src/ui/lib/icons": `export const Icon = (p) => "[" + p.name + "]";`,
  "src/ui/sessions/SessionCard": `export const AgentBadge = (p) => "[" + p.agent + "]"; export const MachineBadge = (p) => "[" + p.computer + "]";
    export const StatusBadge = (p) => "[" + p.kind + "]"; export const captionText = (c) => String(c ?? "");
    export const headerState = () => undefined; export const statusOf = () => "working"; export const folderOf = () => "/work";`,
  "src/ui/sessions/Activity": `export const ActivityView = () => "[activity]"; export const columnStyle = () => ({});`,
  "src/ui/sessions/actions-ui": `export const sessionIdentity = () => ({ caption: "Session ID", value: "id" });
    export const headerClearSlot = () => "fresh"; export const snapshotOf = (s) => s;`,
  "src/ui/sessions/ActionDialogs": `export const actionEntries = () => ({ rename: {}, fork: {}, change: {}, archive: undefined, end: {} });
    export const copyEntries = () => ({ command: {}, name: {}, reference: {}, folder: {} }); export const freshActionEntry = () => ({ label: "Restart", action() {} });`,
  "src/ui/sessions/mark-entries": `export const markEntry = () => undefined;`,
  "src/ui/sessions/viewed-reply": `export const followViewedReplies = () => {};`,
  "src/ui/sessions/Composer": `export const Composer = () => "[composer]";`,
  "src/ui/sessions/ComposerDialogs": `export const ComposerDialogs = () => null;`,
  "src/ui/sessions/QuestionCard": `export const QuestionCard = () => null;`,
  "src/ui/sessions/QueuePanel": `export const QueuePanel = () => null;`,
  "src/ui/sessions/HeldMessage": `export const HeldMessage = () => null; export const HeldInputBar = () => null; export const TerminalDraftBar = () => null;`,
  "src/ui/sessions/Inspector": `export const Inspector = () => "[inspector]";`,
  "src/ui/sessions/KeepWarmChip": `export const KeepWarmChip = () => null;`,
  "src/ui/sessions/session-extra-entries": `export const expandedEntries = () => [];`,
  "src/ui/sessions/detail-removal": `export const watchDetailRemoval = () => ({ update() {}, dispose() {} });`,
  "src/ui/sessions/lazy-slots": `export const ComposerDialogsSlot = () => null; export const QuestionSlot = () => null;`,
  "src/ui/lazy-view": `export const lazyView = () => () => null;`,
  "src/ui/lazy-boundary": `export const LazyBoundary = (p) => p.children;`,
  "src/ui/sessions/Terminal": `export const TerminalTab = () => "[terminal]";`,
  "src/ui/sessions/NativeUi": `export const NativeUiTab = () => "[native]";`,
  "src/ui/sessions/Processes": `export const ProcessesTab = () => "[processes]";`,
  "src/ui/sessions/native-entry": `export const nativeBinding = () => undefined; export const nativeEntry = () => undefined; export const nativeRoute = () => () => {};`,
  "src/ui/sessions/inspector-data": `export const sessionGoal = () => ({ objective: undefined, caption: "" });`,
  "src/ui/teams/TeamTab": `export const LeadTeamHeader = () => null; export const TeamTab = () => "[team]";`,
  "src/ui/teams/NewTeamPage": `export const NewTeamPage = () => "[new team form]";`,
  "src/ui/teams/Waiting": `export const WaitingBanner = () => null;`,
  "src/ui/teams/ProposalCards": `export const LeadProposals = () => null;`,
  "src/ui/teams/leadTeam": `export const teamAnswers = () => 0;`,
  "src/ui/components/DesktopServiceStatus": `export const PausedServiceDetail = () => null; export const servicePausedEmpty = () => false;`,
  "src/ui/screens/TaskView": `export default () => "[task view]";`,
};
const stubId = (key: string) => `\0sessiondetail-stub:${key}`;
let server: ViteDevServer | undefined;
let render: () => string;
let shown: (id: string) => boolean;

const session = {
  key: "run:run-1", computer: "", name: "codex/hgs/dashboard", title: "Checkout dashboard", machine: "workstation",
  project: "Orbit", agent: "claude", model: "gpt-5.5", effort: "high", caption: "Working", state: "running", status: "running",
  cwd: "/work/orbit", cwdPath: "/work/orbit", branch: "feat/checkout-v2", conversationId: "5f0c2a1e-7d3b-4c8e-9a61-2b7e4d9c0f13",
  cmd: "claude", subagents: [],
};
const team = { id: "team-1", name: "Checkout", status: "running" };

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  const root = fileURLToPath(new URL("../../../", import.meta.url));
  server = await createServer({
    configFile: false, root,
    // Its own dependency cache: several render tests run at once, and a shared
    // node_modules/.vite/deps makes them race on the same folder (EPERM).
    cacheDir: fileURLToPath(new URL("../../../node_modules/.vite-sessiondetail", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "sessiondetail-render-mocks", enforce: "pre",
      async resolveId(id, importer) {
        if (id === entry || id.startsWith("\0sessiondetail-stub:")) return id;
        if (!importer || id.startsWith("\0") || !id.startsWith(".")) return undefined;
        const resolved = await this.resolve(id, importer, { skipSelf: true });
        const path = resolved?.id.replace(/\\/g, "/").replace(/\.(tsx?|jsx?)$/, "") ?? "";
        const key = Object.keys(STUBS).find((stub) => path.endsWith(`/${stub}`));
        return key ? stubId(key) : undefined;
      },
      load(id) {
        if (id.startsWith("\0sessiondetail-stub:")) return STUBS[id.slice("\0sessiondetail-stub:".length)];
        if (id === entry) return `import {createComponent} from 'solid-js';
          import {renderToString} from 'solid-js/web';
          import {TAB_ORDER, tabs} from '/src/ui/sessions/SessionDetail.tsx';
          const Row = () => TAB_ORDER.map((id) => {
            const tab = tabs().find((found) => found.id === id);
            return '<button class="tab" role="tab" data-tab-id="' + id + '"' + (tab && tab.show ? '' : ' hidden') + '>'
              + (tab ? tab.label : '') + '</button>';
          }).join('');
          export const render = () => renderToString(() => createComponent(Row, {}));
          export const shown = (id) => !!(tabs().find((tab) => tab.id === id)?.show);`;
        return undefined;
      },
    }, solid({ ssr: true })],
  });
  const loaded = await server.ssrLoadModule(entry);
  render = loaded.render;
  shown = loaded.shown;
});

afterAll(async () => { await server?.close(); });

const state = (value: { session: unknown; team: unknown }) => { (globalThis as Record<string, unknown>).__detail = value; };
const plain = (html: string) => html.replace(/<!--[^>]*-->/g, "");
const buttonOf = (html: string, id: string) => html.match(new RegExp(`<button[^>]*data-tab-id="${id}"[^>]*>[^<]*`))?.[0] ?? "";

describe("the session tab row (task #361)", () => {
  it("offers '+ New team' on the row of a session that has no team", () => {
    state({ session, team: undefined });
    const html = plain(render());
    expect(html).toContain('data-tab-id="newteam"');
    expect(html).toContain("+ New team");
    // The row's order: Activity first, the new-team tab last (Qt appends it).
    const ids = [...html.matchAll(/data-tab-id="([a-z]+)"/g)].map((match) => match[1]);
    expect(ids[0]).toBe("activity");
    expect(ids[ids.length - 1]).toBe("newteam");
    expect(buttonOf(html, "newteam")).not.toMatch(/\shidden/);
  });

  it("hides the tab once the session has a team", () => {
    state({ session, team });
    // The row's data: the fixed id keeps its button mounted, hidden (Qt removes
    // the tab; the strip hides the button so a data update cannot remount it).
    expect(shown("newteam")).toBe(false);
    expect(shown("team")).toBe(true);
  });
});
