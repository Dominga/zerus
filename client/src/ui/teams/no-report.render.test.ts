import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";
import type { TeamTaskView, TeamView } from "../../types";

// Task #415: a task whose attempt ended without a report shows the member's
// last reply (first lines, expandable) and the three choices the owner decides
// with: use the reply as the report, remind the member, or retry. The
// component and wording are production code; only the i18n, the stores and
// the skin are stand-ins.
const entry = "\0no-report-actions";
const translations = "\0no-report-actions-i18n";
const skin = "\0no-report-actions-skin";
const teamsStore = "\0no-report-actions-teams";
const catalogStore = "\0no-report-actions-catalog";
const composerStore = "\0no-report-actions-composer";
const uiStore = "\0no-report-actions-ui";
let server: ViteDevServer | undefined;
let render: (team: TeamView, task: TeamTaskView) => string;

function task(over: Partial<TeamTaskView> = {}): TeamTaskView {
  return { id: 268, title: "Paging API", status: "in_progress", assignee: "api", phase: 1, deps: [], estimate_minutes: 30,
    attempts: 1, attempt_state: "delivered", decided_by: "", pinned: false, output: [], waits: "", no_report: true, attempt_id: "a7", ...over };
}

function team(): TeamView {
  return {
    id: "t1", name: "Checkout", status: "running", project: "", folder: "", goal: "", plan_mode: "", lead: "lead",
    revision: 1, created: 0, updated: 0, waiting_for_lead: false, pending_phase: null, settings: { auto_accept: false },
    members: [
      { id: "lead", role: "Lead", agent: "claude", computer: "c1", lead: true, session: { name: "claude/x/lead" } },
      { id: "api", role: "Backend", agent: "codex", computer: "c1", lead: false, session: { name: "codex/x/api", host: "studio" } },
    ] as never,
    tasks: [], phases: [],
    proposals: [], attention: [], validation: { errors: [], warnings: [] }, health: { critical_path: 0, parallelism: 0, warnings: [] },
    ready: [], cycle: [], draft: null, truncated: false,
  } as unknown as TeamView;
}

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  server = await createServer({
    configFile: false, root: fileURLToPath(new URL("../../../", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "no-report-actions-mocks", enforce: "pre",
      resolveId(id) {
        if ([entry, translations, skin, teamsStore, catalogStore, composerStore, uiStore].includes(id)) return id;
        if (id === "../../i18n") return translations;
        if (id === "../../skin/runtime") return skin;
        if (id === "../../store/teams") return teamsStore;
        if (id === "../../store/catalog") return catalogStore;
        if (id === "../../store/composer") return composerStore;
        if (id === "../../store/ui") return uiStore;
        return undefined;
      },
      load(id) {
        if (id === translations) return `export const trNoop = (s) => s;
          export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));`;
        if (id === skin) return `export const skin = () => ({ appearance: 'light' });`;
        if (id === teamsStore) return `export const teams = { inbox: () => ({ items: [{ kind: "no_report", task: 268, detail: "Done: the parser and its tests" }], truncated: false }) };
          export const memberRow = () => null;
          export const showTeam = () => {};`;
        if (id === catalogStore) return `export const orderedRows = () => [];`;
        if (id === composerStore) return `export const composer = { offer: async () => true }; export const draftKey = () => "k";`;
        if (id === uiStore) return `export const setSelectedKey = () => {}; export const setDetailTab = () => {}; export const setPage = () => {}; export const selected = () => undefined; export const subagentOpen = () => false;`;
        if (id === entry) return `import {createComponent} from 'solid-js';
          import {renderToString} from 'solid-js/web';
          import {TaskActions} from '/src/ui/teams/TeamActions.tsx';
          export const render = (team, task) => renderToString(() => createComponent(TaskActions, { computer: 'c1', team, task }));`;
        return undefined;
      },
    }, solid({ ssr: true })],
  });
  const module = await server.ssrLoadModule(entry);
  render = module.render;
});

afterAll(async () => { await server?.close(); });
const plain = (html: string) => html.replace(/<!--[^>]*-->/g, "");

describe("a No report task's card (task #415)", () => {
  it("shows the member's last reply and the three choices", () => {
    const html = plain(render(team(), task()));
    expect(html).toContain('class="team-no-report"');
    expect(html).toContain("Done: the parser and its tests");
    expect(html).toContain("The member's last reply");
    expect(html).toContain('class="team-no-report-text"');
    expect(html).toContain(">Show more<");
    // The owner decides from the card: use the reply, remind, retry.
    expect(html).toContain('data-action="use-last-reply"');
    expect(html).toContain("Use last reply as report");
    expect(html).toContain('data-action="remind-member"');
    expect(html).toContain(">Remind<");
    expect(html).toContain('data-action="retry-no-report"');
    expect(html).toContain(">Retry<");
  });

  it("says so when the attempt kept no reply", () => {
    const bare = task({ id: 300 });
    const empty = render(team(), bare);
    expect(empty).toContain("No reply was kept for this attempt");
    expect(empty).toContain('data-action="use-last-reply"');
  });

  it("adds nothing to a task with a report", () => {
    const html = plain(render(team(), task({ no_report: false, attempt_id: "a7" })));
    expect(html).not.toContain('class="team-no-report"');
    expect(html).not.toContain("Use last reply as report");
  });
});

describe("the No report card's rules", () => {
  it("folds the reply to three lines and keeps the choices in one row", async () => {
    const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
    const css = fs.readFileSync(new URL("client/src/ui/styles/teams.css", new URL("../../../../", import.meta.url)), "utf8");
    const text = css.split("\n").find((line) => line.trim().startsWith(".team-no-report-text {")) ?? "";
    expect(text).toMatch(/-webkit-line-clamp: 3;/);
    expect(text).toMatch(/white-space: pre-wrap;/);
    expect(css).toContain(".team-no-report-actions { display: flex;");
    const plan = fs.readFileSync(new URL("client/src/ui/styles/plan.css", new URL("../../../../", import.meta.url)), "utf8");
    expect(plan.split("\n").find((line) => line.trim().startsWith(".pm-card-noreport {")) ?? "").toMatch(/display: flex;/);
  });
});
