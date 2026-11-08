import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";
import type { TeamWaitingItem, TeamWaitingView } from "../../types";

// Task 116: one fixture with every kind that waits for the owner, rendered as
// the Tauri client draws it — the Waiting page and the lead's Activity strip
// (WaitingBanner). Each row carries its answer control (Approve…, Answer,
// Resume team, Open Build folders, Open team) and opens its card in place; the
// payload each answer sends is covered by decisions.test.ts (the same model as
// Qt's TeamWaitingAnswer tests). The stores are the only isolated modules; the
// page, strip, row and kind/action mapping are production code.
const entry = "\0waiting-render";
const stores = "\0waiting-render-stores";
const translations = "\0waiting-render-i18n";
const bridge = "\0waiting-render-bridge";
const notices = "\0waiting-render-notices";
const settings = "\0waiting-render-settings";
const proposals = "\0waiting-render-proposals";
const noticesStore = "\0waiting-render-store-notices";
const sections = "\0waiting-render-sections";
const uiStore = "\0waiting-render-ui";
const catalog = "\0waiting-render-catalog";
let server: ViteDevServer | undefined;
let renderPage: (view: TeamWaitingView) => string;
let renderBanner: (view: TeamWaitingView) => string;

const NOW = 1_800_000_000;

function item(kind: string, action: string, title: string, detail: Record<string, unknown> = {}, team = "team-1", name = "Live"): TeamWaitingItem {
  const id = `${team}/${kind}/${title.replace(/\W+/g, "-")}`;
  return { id, kind, team, team_name: name, title, action, since: NOW - 60, proposal: detail.proposal ?? null,
    detail: { id, kind, team, title, action, since: NOW - 60, ...detail } };
}

/** Every kind of the task, one each: the lead's questions and proposals, a
 * phase acceptance, a member question and permission, a held and a retrying
 * task, the team's end, a paused team, a stalled lead, a deferred request, a
 * missing tool, a low drive and the test decisions. */
function fixture(): TeamWaitingItem[] {
  return [
    item("plan_draft", "approve_plan", "Plan of 6 tasks waits for Approve & Run", {}, "team-plan", "Billing export"),
    item("phase_draft", "approve_phase", "Phase 2 Screens waits for your approval", { phase: 2 }),
    item("phase_acceptance", "accept_phase", "Phase 2 is ready for your acceptance",
      { phase: 2, summary: { accepted: 3, rejected: 1, cancelled: 0, tests: "tests pass", minutes: 42 } }),
    item("report_escalated", "decide_report", "Report on #4 Ranking from Backend waits for your decision", { task: 4, member: "Backend" }),
    item("permission_escalated", "answer_permission", "Tester waits for your answer: Bash npm run e2e",
      { member: "tester", tool: "Bash", command: "npm run e2e" }),
    item("owner_word_asked", "decide_proposal", "The lead asks: should the second reviewer be a lead?", { proposal: "p3" }),
    item("task_waits_owner", "release_task", "#7 Publish the guide is held by you", { task: 7 }),
    item("task_waits_owner", "retry_task", "#8 Index waits: hgs: unknown model gpt-5.5", { task: 8 }),
    item("team_finished", "finish", "All 12 tasks decided: waits for you to finish"),
    item("team_paused", "resume_team", "Team paused 5 min ago by the lead: members get no tasks"),
    item("lead_stalled", "open_lead", "The lead stalled while its decisions wait", { member: "Lead", reason: "quiet" }),
    item("tool_missing", "decide_tool", "Install cmake on this computer?", { tool: "cmake", command: "apt install cmake", state: "missing" }, "", "This computer"),
    item("disk_low", "open_disk", "C: has 8 GB free, below its threshold", { drive: "C:" }, "", "This computer"),
    item("deferred", "answer_deferred", "Deferred permission of Backend: Bash: cargo test --locked",
      { deferred: "d1", request: "permission", text: "Bash: cargo test --locked", session: "codex/x/dev", outcome: "Zerus denied it for now" }, "", "codex/x/dev"),
    item("permission_escalated", "open_terminal", "Tester waits in its Terminal: a dialog Zerus cannot answer",
      { member: "tester", member_id: "tester", session: "claude/p/tester" }),
    item("input_blocked", "open_terminal", "Backend's Terminal refused 3 hand-outs",
      { member: "backend", member_id: "backend", session: "codex/p/backend", count: 3 }),
    item("test_full_run", "open_team", "Run the full test suite? About 1 h 20 min", {}),
    item("test_quarantine", "open_team", "e2e is quarantined as flaky (2 episodes)", {}),
  ];
}

/** The title as the HTML carries it (Solid escapes &, < and >). */
const escaped = (text: string) => text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** The answer control each row must carry, the same wording the strip and the
 * page use (`waitingActionLabel` and the row's own buttons). */
function control(found: TeamWaitingItem): string {
  if (found.action === "approve_phase" || found.action === "approve_plan") return "Approve…";
  if (["decide_proposal", "confirm_addition", "answer_permission", "decide_tool", "accept_phase", "answer_deferred",
    "decide_report", "release_task", "retry_task", "finish"].includes(found.action)) return "Answer";
  if (found.action === "resume_team") return "Resume team";
  if (found.action === "open_disk") return "Open Build folders";
  if (found.action === "open_terminal") return "Open Terminal";
  return "Open team";
}

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  server = await createServer({
    configFile: false, root: fileURLToPath(new URL("../../../", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "waiting-render-mocks", enforce: "pre",
      resolveId(id, importer) {
        if ([entry, stores, translations, bridge, notices, settings, proposals, noticesStore, sections, uiStore, catalog].includes(id)) return id;
        if (importer?.endsWith("/ui/teams/Waiting.tsx") || importer?.endsWith("/ui/teams/ProposalCards.tsx") || importer?.endsWith("/ui/teams/decisions.ts")) {
          if (id === "../../i18n") return translations;
          if (id === "../../bridge") return bridge;
          if (id === "../../store/teams") return stores;
          if (id === "../../store/waitingNotices") return notices;
          if (id === "../../store/notices") return noticesStore;
          if (id === "../../store/teamConfirm") return settings;
          if (id === "../../store/ui") return uiStore;
          if (id === "../../store/catalog") return catalog;
          if (id === "../screens/settings/sections") return sections;
          if (importer?.endsWith("/ui/teams/Waiting.tsx") && id === "./ProposalCards") return proposals;
        }
        return undefined;
      },
      load(id) {
        if (id === stores) return `export const state = {view: null};
          export const waiting = () => state.view;
          export const showWaiting = () => () => {};
          export const memberRow = () => undefined;
          export const answeredWaiting = () => {};
          export const teams = { list: () => undefined, failure: () => "", followError: () => "" };`;
        if (id === translations) return `export const trNoop = (s) => s;
          export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));`;
        if (id === bridge) return `export const teamsWaitingOlder = async () => false;
          export const teamAction = async () => ({outcome: "failed", detail: "the render test sends nothing"});
          export const teamRead = async () => ({});
          export const inTauri = () => false;`;
        if (id === notices) return `export const waitingNotices = () => ({ observe: () => [{ title: "Waiting for you in X", body: "A card" }], withdraw: () => {} });`;
        if (id === noticesStore) return `export const toast = () => { globalThis.__waitingToasts = (globalThis.__waitingToasts ?? 0) + 1; };`;
        if (id === settings) return `export const teamConfirm = async () => false;`;
        if (id === sections) return `export const openSettings = () => {};`;
        if (id === uiStore) return `export const setPage = () => {};
          export const setSelectedKey = () => {};
          export const setDetailTab = () => {};`;
        if (id === catalog) return `export const orderedRows = () => [];`;
        if (id === proposals) return `export const windowPort = { send: async () => ({outcome: "failed"}) };
          export const ProposalCardList = () => null;
          export const useModel = (make) => { const model = make(() => {});
            return { model: Object.assign(model, { open: async () => {}, approve: async () => {}, answerPermission: async () => {}, answerQuestion: async () => {}, answerAddition: async () => {}, acceptPhase: async () => {}, sendBack: async () => {}, answerDeferred: async () => {}, decideTool: async () => {}, acceptReport: async () => {}, rejectReport: async () => {}, release: async () => {}, retry: async () => {}, finishTeam: async () => {} }), tick: () => 0 };
          };`;
        if (id === entry) return `import {createComponent} from 'solid-js';
          import {renderToString} from 'solid-js/web';
          import {WaitingPage, WaitingBanner} from '/src/ui/teams/Waiting.tsx';
          import {state} from '${stores}';
          const show = (view, component) => { state.view = view; return renderToString(() => createComponent(component, {})); };
          export const renderPage = (view) => show(view, WaitingPage);
          export const renderBanner = (view) => show(view, WaitingBanner);`;
      },
    }, solid({ ssr: true })],
  });
  const module = await server.ssrLoadModule(entry);
  renderPage = module.renderPage;
  renderBanner = module.renderBanner;
});

afterAll(async () => { await server?.close(); });

describe("Waiting for you and the Activity strip (task 116)", () => {
  it("the page lists every kind with its answer control", () => {
    const items = fixture();
    const html = renderPage({ items, total: items.length, more: false, next: null });
    expect(html).toContain("Waiting for you");
    for (const found of items) {
      expect(html, found.kind).toContain(escaped(found.title));
      expect(html, `${found.kind} control`).toContain(control(found));
    }
    // A deferred request keeps its own group; a tool offer and a drive belong to no team.
    expect(html).toContain("Deferred");
    expect(html).toContain("Open Build folders");
  });

  it("the strip shows the count and the newest item, and each kind answers in its row", () => {
    const items = fixture();
    const all = renderBanner({ items, total: items.length, more: false, next: null });
    expect(all).toContain("18 wait for you");
    expect(all).toContain(items[0].title.replace(/&/g, "&amp;"));
    expect(all).toContain("Show all");
    // The single item the strip shows is a full row with its own answer.
    for (const found of items) {
      const one = renderBanner({ items: [found], total: 1, more: false, next: null });
      expect(one, found.kind).toContain(escaped(found.title));
      expect(one, `${found.kind} control`).toContain(control(found));
    }
  });

  it("does not stack in-window notices, as Qt (task 323 N7)", () => {
    const calls = globalThis as typeof globalThis & { __waitingToasts?: number };
    calls.__waitingToasts = 0;
    renderBanner({ items: fixture(), total: 18, more: false, next: null });
    expect(calls.__waitingToasts).toBe(0);
  });

  it("nothing waits for you: the strip draws nothing and the page says so", () => {
    expect(renderBanner({ items: [], total: 0, more: false, next: null })).not.toContain("wait for you");
    expect(renderPage({ items: [], total: 0, more: false, next: null })).toContain("Nothing waits for you");
  });
});
