import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";

// Task 206 (owner, 2026-11-07): in the Compact skins (layout `side-panel:
// sections`) the side panel is Qt's InspectorCards, one scroll of collapsible
// sections in its order and with its folds; every other skin keeps the four
// tabs. The Inspector and its section data are production code; the stores,
// the bridge and the panels it embeds are stand-ins fed from globalThis.
const entry = "\0inspector-render";
const STUBS: Record<string, string> = {
  "src/i18n": `export const trNoop = (s) => s;
    export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));`,
  "src/skin/runtime": `export const skin = () => globalThis.__inspector.skin;`,
  "src/store/ui": `export const selected = () => globalThis.__inspector.session; export const subagentOpen = () => undefined;
    export const setSubagentOpen = () => {}; export const setDetailTab = () => {}; export const setSelectedKey = () => {}; export const setMenu = () => {};`,
  "src/store/inspector": `export const INSPECTOR_TABS = ["plan", "details", "worktrees", "tasks"];
    export const inspectorOpen = () => true; export const inspectorTab = () => "plan"; export const inspectorWidth = () => 400;
    export const inspectorClosed = () => new Set(globalThis.__inspector.closed ?? []);
    export const setInspectorOpen = () => {}; export const setInspectorTab = () => {}; export const setInspectorWidth = () => {}; export const toggleInspectorCard = () => {};
    export const panelGeometry = () => ({ width: 400, floating: false }); export const inspectorHint = (s) => s;`,
  "src/store/actions": `export const followedFor = () => ({ value: globalThis.__inspector.details, settled: true });
    export const detailsFor = () => undefined; export const copyIdentity = async () => {}; export const copy = async () => {};`,
  "src/ui/sessions/newsession-view": `export const newSessionFromWorktree = () => {};`,
  "src/ui/sessions/worktree-dialogs": `export const catalogStatus = () => ""; export const catalogRoots = () => [];`,
  "src/store/worktreeCleanup": `export const openWorktreeReview = () => {}; export const worktreeReviewDialog = () => undefined;`,
  "src/store/catalog": `export const computerOffline = () => false; export const orderedRows = () => []; export const agentKind = (agent) => agent || "claude";`,
  "src/store/organization": `export const INBOX = "\\u0000inbox"; export const groupOf = () => "orbit"; export const grouping = () => ({ headers: [{ id: "orbit", name: "Orbit" }] });`,
  "src/ui/sessions/actions-ui": `export const sessionIdentity = (s) => ({ title: "Session ID", caption: "Copy session ID", value: s.conversationId }); export const snapshotOf = (s) => s;`,
  "src/store/tasks": `export const tasks = { project: () => undefined, total: () => 0, loaded: () => false };`,
  "src/store/taskContext": `export const taskProject = () => undefined;`,
  "src/store/machines": `export const machineCards = {};`,
  "src/store/settings": `export const flag = () => false; export const settings = {};`,
  "src/ui/sessions/SocratiCodePanel": `export const SocratiCodePanel = () => null;`,
  "src/ui/sessions/SessionLeadPanel": `export const SessionLeadPanel = () => "Reports to";`,
  "src/ui/sessions/IronRulesPanel": `export const IronRulesPanel = () => "No iron rules";`,
  "src/store/privateStoreNotices": `export const privateStoreNotices = () => ({ observe() {} });`,
  "src/store/notices": `export const toast = () => {};`,
  "src/store/folderFilter": `export const filterFolder = () => {}; export const clearFolderFilter = () => {};`,
  "src/ui/sessions/SessionCard": `export const captionText = (c) => String(c ?? ""); export const childStatus = () => ({ caption: "" }); export const AgentBadge = (p) => "[" + p.agent + "]";`,
  "src/store/accounts": `export const accountsPage = () => ({ hosts: [], accounts: [], overview: [] });`,
  "src/store/accountFilters": `export const openFilterAccounts = async () => {};`,
  "src/store/composer": `export const composer = { view: () => ({ context: { text: "36%" }, clear: { enabled: true } }), openClear() {} };`,
  "src/bridge": `export const worktreeCatalog = async () => ({}); export const worktreeSnapshot = async () => ({});`,
  "src/ui/lib/dates": `export const eventTimeOf = () => "27 Oct 01:40";`,
  "src/ui/lib/icons": `export const Icon = () => null;`,
  "src/store/teams": `export const selectedTeam = () => globalThis.__inspector.team?.summary; export const teams = { team: () => globalThis.__inspector.team?.view };`,
  "src/ui/screens/Tasks": `export const TasksPanel = () => "Project task board";`,
};
const stubId = (key: string) => `\0inspector-stub:${key}`;
let server: ViteDevServer | undefined;
let render: () => string;

const base = { appearance: "dark", components: {}, tokens: {} };
const sections = { ...base, id: "whitewh-dark", layout: { rail: "icons", "sessions-pane": "left", inspector: "docked", "activity-column": "centered", "side-panel": "sections" } };
const tabs = { ...base, id: "zerus-dark", layout: { ...sections.layout, "side-panel": "tabs" } };
const session = {
  key: "\nrun:run-1", computer: "", name: "codex/hgs/dashboard", title: "Checkout dashboard", machine: "workstation", cwdPath: "/work/orbit", branch: "feat/checkout-v2",
  model: "gpt-5.5", effort: "high", updatedAt: 1793050800, caption: "Working", summary: "Wiring the saved cards API", conversationId: "5f0c2a1e-7d3b-4c8e-9a61-2b7e4d9c0f13",
  cmd: "codex", subagents: [],
};
const details = {
  session_usage: { context: { used: 72_000, limit: 200_000 } },
  task_lists: { main: { items: [{ title: "Plan Checkout v2 with the team", status: "completed" }, { title: "Wire the API", status: "in_progress" }] } },
};
const phase = (n: number, done: number, total: number) => ({ n, title: "", state: "", status: "", done, total, barrier: false, auto: false, acceptance: "", paused: false, health: "" });
const member = (id: string, role: string, agent: string, lead = false) => ({ id, role, agent, lead, state: "active", activity: "idle", phase: "", error: "", session: null,
  live: true, stuck: false, question: false, queue: [], minutes_without_progress: 0, host_shared: false, model: "", effort: "", account: "", computer: "", folder: "" });
const team = {
  summary: { id: "team-1", name: "Checkout", status: "running", pending_phase: 3 },
  view: { id: "team-1", name: "Checkout", status: "running", pending_phase: 3, created: 0, ready: [],
    phases: [phase(1, 1, 1), phase(2, 1, 4), phase(3, 0, 1)],
    tasks: [{ id: 1, status: "accepted", assignee: "lead", attempt_state: "" }, { id: 2, status: "accepted", assignee: "backend", attempt_state: "" }, { id: 3, status: "pending", assignee: "tester", attempt_state: "" }],
    members: [member("lead", "Lead", "codex", true), member("backend", "Backend", "claude"), member("tester", "Tester", "deepseek")] },
};

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  server = await createServer({
    configFile: false, root: fileURLToPath(new URL("../../../", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "inspector-render-mocks", enforce: "pre",
      async resolveId(id, importer) {
        if (id === entry || id.startsWith("\0inspector-stub:")) return id;
        if (!importer || id.startsWith("\0") || !id.startsWith(".")) return undefined;
        const resolved = await this.resolve(id, importer, { skipSelf: true });
        const path = resolved?.id.replace(/\\/g, "/").replace(/\.(tsx?|jsx?)$/, "") ?? "";
        const key = Object.keys(STUBS).find((stub) => path.endsWith(`/${stub}`));
        return key ? stubId(key) : undefined;
      },
      load(id) {
        if (id.startsWith("\0inspector-stub:")) return STUBS[id.slice("\0inspector-stub:".length)];
        if (id === entry) return `import {createComponent} from 'solid-js';
          import {renderToString} from 'solid-js/web';
          import {Inspector} from '/src/ui/sessions/Inspector.tsx';
          export const render = () => renderToString(() => createComponent(Inspector, { available: 1200 }));`;
        return undefined;
      },
    }, solid({ ssr: true })],
  });
  render = (await server.ssrLoadModule(entry)).render;
});

afterAll(async () => { await server?.close(); });

const state = (value: Record<string, unknown>) => { (globalThis as Record<string, unknown>).__inspector = { session, details, team, ...value }; };
/** The HTML without Solid's hydration markers, as the text reads. */
const plain = (html: string) => html.replace(/<!--[^>]*-->/g, "");
/** A stylesheet's rule body, read from disk (as task 317 does). */
const read = async (path: string) => {
  const fs = await import(/* @vite-ignore */ ["node", "fs"].join(":")) as { readFileSync(path: URL, encoding: "utf8"): string };
  return fs.readFileSync(new URL(path, import.meta.url), "utf8").replace(/\r\n/g, "\n");
};
const rule = (css: string, selector: string) => {
  const at = css.indexOf(`${selector} {`);
  expect(at, selector).toBeGreaterThan(-1);
  return css.slice(at, css.indexOf("}", at));
};
/** A drawn line, not the rounded corner's radius. */
const borderLine = /\bborder(?:-(?:top|right|bottom|left|width|style|color))?\s*:/;
const shown = (html: string) => [...html.matchAll(/data-section="([^"]+)"/g)].map((match) => match[1]);
const folded = (html: string) => [...html.matchAll(/<section([^>]*)>/g)].map((match) => match[1])
  .filter((attributes) => /class="[^"]*\bclosed\b/.test(attributes)).map((attributes) => /data-section="([^"]+)"/.exec(attributes)?.[1]);

describe("the side panel (task 206)", () => {
  it("is one scroll of Qt's sections in its order and folds in the Compact skins", () => {
    state({ skin: sections });
    const html = render();
    expect(html).not.toContain('role="tablist"');
    expect(html.match(/class="insp-scroll[^"]*"/g)).toEqual(['class="insp-scroll sections"']);
    expect(shown(html)).toEqual(["sec-environment", "sec-context", "sec-team", "sec-tasks", "sec-rules", "sec-worktrees", "sec-changes", "sec-projecttasks"]);
    expect(folded(html)).toEqual(["sec-rules", "sec-worktrees", "sec-changes", "sec-projecttasks"]);
    const text = plain(html);
    // Environment: the session ID with its copy button, machine, folder, branch, model and effort, last event.
    for (const row of ["Environment", "5f0c2a1e-7d3b-4c8e-9a61-2b7e4d9c0f13", "workstation", "/work/orbit", "feat/checkout-v2", "gpt-5.5 high", "27 Oct 01:40"]) expect(text).toContain(row);
    expect(html).toContain('aria-label="Copy Session ID: 5f0c2a1e-7d3b-4c8e-9a61-2b7e4d9c0f13"');
    // Context: Qt's used share of the window, the tokens and what is left.
    for (const row of ["36% used", "72k/200k", "128k left in the window", "Clear session…"]) expect(text).toContain(row);
    // Team: the card, its chips, Phase and Phases bars, accepted, Open Team, Members and the foot line.
    for (const row of ["Checkout", "Running", "phase 3 pending approval", "Phase 2", "1/4", "Phases", "1/3", "2/3 accepted", "Open Team", "Members", "Lead", "Backend", "[claude]",
      "Plan, graph and member changes live in the Team tab."]) expect(text).toContain(row);
    // Tasks: the main agent, its plan and Reports to.
    for (const row of ["Wiring the saved cards API", "Plan Checkout v2 with the team", "1 of 2 completed (last recorded)", "Subagents", "Reports to"]) expect(text).toContain(row);
  });

  it("keeps the owner's folds against each section's default", () => {
    state({ skin: sections, closed: ["sec-environment", "+sec-changes"] });
    const html = render();
    expect(folded(html)).toEqual(["sec-environment", "sec-rules", "sec-worktrees", "sec-projecttasks"]);
    // A folded section keeps its title and its short value.
    expect(plain(html)).toContain("Environment");
    expect(plain(html)).not.toContain("feat/checkout-v2");
  });

  it("shows the Team section only for a team's session", () => {
    state({ skin: sections, team: undefined });
    expect(shown(render())).toEqual(["sec-environment", "sec-context", "sec-tasks", "sec-rules", "sec-worktrees", "sec-changes", "sec-projecttasks"]);
  });

  it("folds finished subagents into a count in both variants, open per session (task 262)", () => {
    const children = [
      { id: "a", name: "Reader", state: "ready", summary: "" },
      { id: "b", name: "Writer", state: "working", summary: "" },
      { id: "c", name: "Checker", state: "ended", summary: "" },
    ];
    const running = { ...session, state: "running", activity: "busy", processState: "running", subagents: children };
    for (const skin of [sections, tabs]) {
      state({ skin, session: running });
      const text = plain(render());
      expect(text).toContain("Writer");
      expect(text).not.toContain("Reader");
      if (skin === tabs) {
        // The tabs' Plan & agents list is upstream's bounded one: "Finished: N", unfolded by its own button.
        expect(text).toContain("Finished: 2");
        continue;
      }
      expect(text).toContain("2 finished");
      expect(text).toContain("show");
      // Opened for this session: every child, running first, and the way back.
      state({ skin, session: running, closed: [`+finished:${session.key}`] });
      const open = plain(render());
      expect(open.indexOf("Writer")).toBeLessThan(open.indexOf("Reader"));
      expect(open).toContain("Checker");
      expect(open).toContain("hide finished");
      // Another session's fold does not open this one.
      state({ skin, session: running, closed: ["+finished:\nrun:other"] });
      expect(plain(render())).not.toContain("Reader");
    }
  });

  it("keeps the four tabs in every other skin", () => {
    state({ skin: tabs });
    const html = render();
    expect(html).toContain('role="tablist"');
    expect(html.match(/role="tab"/g)).toHaveLength(4);
    expect(shown(html).filter((id) => id.startsWith("sec-"))).toEqual([]);
    expect(plain(html)).toContain("Plan &amp; agents");
  });
});

// Task 354 (owner, 2026-11-08): the panel's outer lines and the "Session"
// caption go; the sections keep their own cards and only the collapse button
// stays above them.
describe("the side panel's frame and caption (task 354)", () => {
  it("shows only the collapse button above the Compact panel's sections, with no 'Session' caption", () => {
    state({ skin: sections });
    const html = plain(render());
    expect(html).not.toContain("insp-title");
    const head = /<div[^>]*class="insp-head"[^>]*>([\s\S]*?)<\/div>/.exec(html)?.[1];
    expect(head).toBeDefined();
    expect((head!.match(/<button/g) ?? []).length).toBe(1);
    expect(head).toContain('aria-label="Collapse side panel"');
    // Nothing but the button and its spacer: the caption row is gone.
    expect(head!.replace(/<[^>]+>/g, "").trim()).toBe("");
    expect(shown(html)).toHaveLength(8);
  });

  it("draws no caption in the tabbed skins either, and keeps one collapse button", () => {
    state({ skin: tabs });
    const html = plain(render());
    expect(html).not.toContain("insp-title");
    expect(html).not.toContain('class="insp-head"');
    expect(html).toContain('role="tablist"');
    expect((html.match(/aria-label="Collapse side panel"/g) ?? []).length).toBe(1);
  });

  it("removes the panel's outer border and the line under its head, and keeps the sections' cards", async () => {
    const css = await read("../styles/inspector.css");
    expect(rule(css, ".inspector")).not.toMatch(borderLine);
    expect(rule(css, ".insp-head")).not.toMatch(borderLine);
    expect(css).not.toContain(".insp-title");
    // The sections inside keep their own cards.
    expect(rule(css, ".icard")).toContain("border: 1px solid var(--z-color-card-border)");
    // detail.css still carries an older copy of the panel's rules; it drops the frame as well.
    expect(rule(await read("../styles/detail.css"), ".inspector")).not.toMatch(borderLine);
  });
});
