import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";
import type { TeamMemberView, TeamView } from "../../types";
import type { MemberStatus } from "./team-model";

// Task #347: render the real MemberCard in Node, mocking only the stores and
// the dialogs it opens; the state model, roles and progress stay production.
const entry = "\0member-card-render";
let server: ViteDevServer | undefined;
let render: (member: TeamMemberView, status: MemberStatus, options?: { rowKey?: string; selectedKey?: string }) => string;

const mocks = new Map(["../../i18n", "../lib/icons", "../../store/catalog", "../../store/teams", "../../store/composer", "../../store/ui", "./ProposalCards", "./actions", "./TeamActions", "./SwapMemberDialog", "./roles", "./permissionAnswers", "./members"].map((id) => [id, `\0member-${id}`] as const));

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  server = await createServer({
    configFile: false, root: fileURLToPath(new URL("../../../", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "member-card-render-mocks", enforce: "pre",
      resolveId(id, importer) {
        if (id === entry) return entry;
        if (!importer?.endsWith("/MemberCard.tsx")) return undefined;
        return mocks.get(id);
      },
      load(id) {
        const base = [...mocks].find(([, virtual]) => virtual === id)?.[0];
        if (base === "../../i18n") return `export const tr = (s, ...args) => s.replace(/%(\d+)/g, (_, n) => String(args[n - 1])); export const trNoop = (s) => s;`;
        if (base === "../lib/icons") return `export const Icon = () => null; export const AgentGlyph = () => null;`;
        if (base === "../../store/catalog") return `export const agentKind = (agent) => agent || "claude";`;
        if (base === "../../store/teams") return `export const memberRow = () => (globalThis.__memberRowKey ? { key: globalThis.__memberRowKey } : undefined);`;
        if (base === "../../store/composer") return `export const composer = { view: () => undefined, locked: () => true, input() {}, text: () => "" };`;
        if (base === "../../store/ui") return `export const selected = () => (globalThis.__selectedKey ? { key: globalThis.__selectedKey } : undefined); export const setDetailTab = () => {}; export const setSelectedKey = () => {}; export const now = () => 0;`;
        if (base === "./ProposalCards") return `export const windowPort = { send: async () => ({outcome: "failed"}) };
          const methods = ["pauseMember", "resumeMember", "relaunch", "drain", "removeMember", "subLead", "reportsTo", "answerMember", "addMember"];
          export const useModel = (make) => ({ model: Object.assign(make(() => {}), Object.fromEntries(methods.map((name) => [name, async () => ({})]))), tick: () => 0 });`;
        if (base === "./actions") return `export class TeamActions { constructor() {} }`;
        if (base === "./TeamActions") return `export const TextDialog = () => null;`;
        if (base === "./SwapMemberDialog") return `export const SwapMemberDialog = () => null;`;
        if (base === "./roles") return `export const roleStyle = () => ({});`;
        if (base === "./permissionAnswers") return `export const answerRow = () => ({}); export const newestAnswers = () => ({shown: [], more: 0}); export const permissionSummary = () => "";`;
        if (base === "./members") return `export const reportsOf = () => []; export const memberMenu = () => []; export const waitingForTask = () => false;
          export const accountLabel = () => "work"; export const memberAccount = () => "work"; export const limitText = () => "";
          export const moveAffected = () => false; export const scaleRoom = () => ({enabled: false, text: "", hint: ""});`;
        if (id === entry) return `import {createComponent} from "solid-js";
          import {renderToString} from "solid-js/web";
          import {MemberCard} from "/src/ui/teams/MemberCard.tsx";
          export const render = (member, status, options = {}) => {
            globalThis.__memberRowKey = options.rowKey;
            globalThis.__selectedKey = options.selectedKey;
            return renderToString(() => createComponent(MemberCard, {
              computer: "host", team: {id: "t1", status: "running", members: [member], tasks: [{id: 3, assignee: member.id, status: "assigned", attempt_state: "delivering", title: "Fix the thing"}], settings: {}},
              member, status, depth: 1, focused: false, onFocus() {}, folded: false, onFold() {}, onTask() {},
            }));
          };`;
        return undefined;
      },
    }, solid({ ssr: true })],
  });
  render = (await server.ssrLoadModule(entry)).render;
});

afterAll(async () => { await server?.close(); });

const member = (extra: Partial<TeamMemberView> = {}): TeamMemberView => ({
  id: "m1", role: "Backend", agent: "codex", model: "gpt", account: "work", computer: "host", folder: "", state: "active",
  lead: false, sub: false, session: "", queue: [], permission_answers: [], unread: 0, checks: {}, ...extra,
} as TeamMemberView);

const status = (kind: string, extra: Partial<MemberStatus> = {}): MemberStatus => ({
  kind, text: "Working on #3 Fix the thing", short: "#3", tone: "active", task: 3, problem: false, icon: "play", ...extra,
});

describe("the compact member card (task 347)", () => {
  it.each([
    ["working", "info"], ["sent", "info"], ["submitted", "info"], ["planning", "info"],
    ["stuck", "warn"], ["no_report", "warn"], ["input_blocked", "warn"], ["asking", "warn"],
    ["answer", "danger"], ["failed", "danger"], ["unconfirmed", "danger"],
    ["paused", "muted"], ["offline", "muted"], ["idle", "dim"], ["done", "dim"],
  ])("shows %s with its state colour", (kind, colour) => {
    const html = render(member(), status(kind));
    expect(html).toContain(`data-kind="${kind}"`);
    expect(html).toContain(`data-state-color="${colour}"`);
    expect(html).toContain("team-member-card");
    expect(html).toContain("team-status");
  });

  it("keeps one header and one status line, with the rest folded", () => {
    const html = render(member(), status("working"));
    // Header (role, model, progress, actions) and the state line only.
    expect(html.match(/class="team-member-line"/g)).toHaveLength(1);
    expect(html.match(/class="team-member-line sub"/g)).toHaveLength(1);
    expect(html).toContain("team-member-progress");
    expect(html).toContain("data-action=\"member-details\"");
    expect(html).not.toContain("team-account");
    expect(html).not.toContain("team-member-tasks");
    expect(html).not.toContain("team-member-marks");
    expect(html).not.toContain("team-member-queue");
  });

  it("keeps an approval request in full beside the compact lines", () => {
    const html = render(member({ permission: { question_id: "q1", question_hash: "h", answer: { question_id: "q1", allow: "yes", deny: "no" }, tool: "Bash", command: "rm -rf build", reason: "why" } } as never), status("answer"));
    expect(html).toContain("team-member-permission");
    expect(html).toContain("permission-deny");
    expect(html).toContain("permission-allow");
    expect(html).toContain("rm -rf build");
    // The compact lines still stand; the request is the only extra block.
    expect(html.match(/class="team-member-line"/g)?.length).toBeGreaterThanOrEqual(1);
  });
  it("draws a compact card: two lines and a tighter box (height check)", async () => {
    const { readFileSync } = await import(/* @vite-ignore */ ["node", "fs"].join(":")) as { readFileSync(path: URL): string };
    const css = String(readFileSync(new URL("../../ui/styles/teams.css", import.meta.url))).replace(/\r\n/g, "\n");
    const rule = (selector: string) => {
      const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      return [...css.matchAll(new RegExp(`${escaped} \\{([^}]*)\\}`, "g"))].at(-1)?.[1] ?? "";
    };
    expect(rule(".team-member-card")).toMatch(/padding:\s*5px 7px 5px 11px/);
    expect(rule(".team-member-card .team-member-main")).toMatch(/gap:\s*1px/);
    expect(rule(".team-member-card")).toMatch(/--member-state:/);
    for (const colour of ["info", "warn", "danger", "muted", "dim"]) {
      expect(rule(`.team-member-card[data-state-color="${colour}"]`)).toContain(`var(--z-color-${colour})`);
    }
    const html = render(member(), status("working"));
    expect(html.match(/class="team-member-line"/g)).toHaveLength(1);
    expect(html.match(/class="team-member-line sub"/g)).toHaveLength(1);
    expect(html).not.toContain("team-member-pulse");
    expect(html).not.toContain("team-member-marks");
  });
});

describe("the member row's state and its session (task 355)", () => {
  /** Solid's SSR comments sit between a text node's markers; strip them to read the row. */
  const plain = (html: string) => html.replace(/<!--\$-->|<!--\/-->/g, "");

  it("shows the state as a word in its colour with the task number beside it", () => {
    const cases: [string, string, number][] = [
      ["working", "Working", 3], ["idle", "Idle", 0], ["paused", "Paused", 0],
      ["offline", "Offline", 0], ["stuck", "Stuck", 3], ["answer", "Answer", 3],
      ["failed", "Failed", 0], ["input_blocked", "Blocked", 3],
    ];
    for (const [kind, word, task] of cases) {
      const html = plain(render(member(), status(kind, { short: word, task })));
      expect(html, kind).toContain(`data-state="${kind}"`);
      expect(html, kind).toContain(`>${word}<`);
      if (task > 0) expect(html, kind).toContain(`data-task-number="${task}">#${task}<`);
      else expect(html, kind).not.toContain("data-task-number");
    }
  });

  it("shows a queued task beside an idle member, so it does not read as work", () => {
    const html = plain(render(member({ queue: [342] }), status("idle", { short: "Idle", task: 0 })));
    expect(html).toContain(">Idle<");
    expect(html).toContain('data-task-number="342">#342<');
    expect(html).toContain("data-state=\"idle\"");
  });

  it("marks the row of the session you are looking at as you", () => {
    const session = (name: string) => ({ host: "", name, run_id: "r", conversation_id: "c" });
    const you = render(member({ session: session("claude/p/me") }), status("working"), { rowKey: "row-me", selectedKey: "row-me" });
    expect(you).toContain("data-you");
    expect(you).toContain(">you<");
    const other = render(member({ session: session("claude/p/other") }), status("working"), { rowKey: "row-other", selectedKey: "row-me" });
    expect(other).not.toContain("data-you");
  });
});
