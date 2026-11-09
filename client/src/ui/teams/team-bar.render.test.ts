import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";
import type { TeamView } from "../../types";

// Task #427: the lead session's team strip is one row at 1200, 900 and 700 px:
// nothing overlaps or wraps, the time stays on one line, and the lower-priority
// actions fold into a More menu as the width falls. The strip is production
// code; only the i18n and the stores are stand-ins.
const entry = "\0team-bar";
const translations = "\0team-bar-i18n";
const skin = "\0team-bar-skin";
const teamsStore = "\0team-bar-teams";
const catalogStore = "\0team-bar-catalog";
const composerStore = "\0team-bar-composer";
const uiStore = "\0team-bar-ui";
const memberCard = "\0team-bar-member-card";
let server: ViteDevServer | undefined;
let render: (team: TeamView, width: number) => string;

function team(): TeamView {
  const now = Math.floor(Date.now() / 1000);
  return {
    id: "t1", name: "Zerus Next for the fork", status: "running", project: "", folder: "", goal: "Move the UI", plan_mode: "standard",
    lead: "lead", revision: 1, created: now - 3 * 86400 - 18 * 3600, updated: now, waiting_for_lead: false, pending_phase: 9,
    settings: { auto_accept: false },
    members: [
      { id: "lead", role: "Lead", agent: "claude", computer: "c1", lead: true, session: { name: "claude/x/lead" } },
      { id: "dev", role: "Frontend", agent: "codex", computer: "c1", lead: false, session: { name: "codex/x/dev" } },
    ] as never,
    tasks: [
      { id: 1, title: "One", status: "accepted", assignee: "dev", phase: 1, deps: [], estimate_minutes: 0, attempts: 1, attempt_state: "", decided_by: "", pinned: false, output: [], waits: "" },
      { id: 2, title: "Two", status: "submitted", assignee: "dev", phase: 1, deps: [], estimate_minutes: 0, attempts: 1, attempt_state: "", decided_by: "", pinned: false, output: [], waits: "" },
    ] as never,
    phases: [{ n: 9, title: "Phase 9", state: "approved", status: "in_work", done: 1, total: 2, barrier: false, auto: false, acceptance: "", paused: false, health: "" }] as never,
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
      name: "team-bar-mocks", enforce: "pre",
      resolveId(id) {
        if ([entry, translations, skin, teamsStore, catalogStore, composerStore, uiStore, memberCard].includes(id)) return id;
        if (id === "../../i18n") return translations;
        if (id === "../../skin/runtime") return skin;
        if (id === "../../store/teams") return teamsStore;
        if (id === "../../store/catalog") return catalogStore;
        if (id === "../../store/composer") return composerStore;
        if (id === "../../store/ui") return uiStore;
        if (id === "./MemberCard") return memberCard;
        return undefined;
      },
      load(id) {
        if (id === translations) return `export const trNoop = (s) => s;
          export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));`;
        if (id === skin) return `export const skin = () => ({ appearance: 'light' });`;
        if (id === teamsStore) return `export const teams = { list: () => ({ teams: [] }), inbox: () => ({ items: [], truncated: false }) };
          export const memberRow = () => null;
          export const showTeam = () => {};`;
        if (id === catalogStore) return `export const orderedRows = () => []; export const agentKind = (agent) => agent || 'claude';`;
        if (id === composerStore) return `export const composer = { offer: async () => true }; export const draftKey = () => "k";`;
        if (id === uiStore) return `export const setSelectedKey = () => {}; export const setDetailTab = () => {}; export const setPage = () => {};
          export const selected = () => undefined; export const subagentOpen = () => false; export const toast = () => {};`;
        if (id === memberCard) return `export const toComposer = () => {}; export const memberRow = () => null; export const MemberCard = () => null; export const AddMemberDialog = () => null;`;
        if (id === entry) return `import {createComponent} from 'solid-js';
          import {renderToString} from 'solid-js/web';
          import {TeamBar} from '/src/ui/teams/team-bar.tsx';
          export const render = (team, width) => renderToString(() => createComponent(TeamBar,
            { computer: 'c1', team, state: 'running', problemTasks: new Set(), onPhase: () => {}, onSettings: () => {}, onCatalog: () => {}, width }));`;
        return undefined;
      },
    }, solid({ ssr: true })],
  });
  const module = await server.ssrLoadModule(entry);
  render = module.render;
});

afterAll(async () => { await server?.close(); });
const plain = (html: string) => html.replace(/<!--[^>]*-->/g, "");
const has = (html: string, needle: string) => plain(html).includes(needle);

/** Every action the running team's bar offers. */
const ACTIONS = ["resumeLimits", "pause", "stop"];

describe("the lead session's team strip (task #427)", () => {
  it("at 1200 px keeps everything on one row: no More menu, every action inline", () => {
    const html = plain(render(team(), 1200));
    expect(html).toContain('class="team-bar');
    expect(html).toContain('data-compact="none"');
    expect(html).not.toContain("data-more");
    for (const id of ACTIONS) expect(html).toContain(`data-action="${id}"`);
    expect(html).toContain('data-strip-action="settings"');
    expect(html).toContain('data-strip-action="catalog"');
    // The name, the state, the progress with its count and the one-line time.
    expect(html).toContain("Zerus Next for the fork");
    expect(html).toContain("team-progress");
    expect(html).toContain("team-count");
    expect(html).toContain('data-strip-time');
    expect(html).toContain("3d 18h");
  });

  it("at 900 px folds the lower-priority actions into More, keeping Pause and Stop inline", () => {
    const html = plain(render(team(), 900));
    expect(html).toContain('data-compact="low"');
    expect(html).toContain("data-more");
    expect(html).toContain('data-overflow="settings catalog resumeLimits"');
    expect(html).toContain('data-more-item="resumeLimits"');
    expect(html).toContain('data-more-item="settings"');
    expect(html).toContain('data-more-item="catalog"');
    expect(html).toContain('data-action="pause"');
    expect(html).toContain('data-action="stop"');
    expect(html).not.toContain('data-action="resumeLimits"');
    // The progress, its count and the time are still there, never hidden.
    expect(html).toContain("team-progress");
    expect(html).toContain("team-count");
    expect(html).toContain('data-strip-time');
  });

  it("at 700 px folds the team actions too: nothing overlaps or wraps", () => {
    const html = plain(render(team(), 700));
    expect(html).toContain('data-compact="high"');
    expect(html).toContain("data-more");
    for (const id of ACTIONS) {
      expect(html).toContain(`data-more-item="${id}"`);
      expect(html).not.toContain(`data-action="${id}"`);
    }
    expect(html).toContain('data-overflow="settings catalog resumeLimits pause stop"');
    // The strip's essentials stay: the name (ellipsis), the state, the progress
    // with its count and the time on one line.
    expect(html).toContain("team-name");
    expect(html).toContain("team-progress");
    expect(html).toContain("team-count");
    expect(html).toContain('data-strip-time');
  });

  it("joins the progress segments into one track, a minimum for a non-zero one and a per-state tooltip (task #438)", async () => {
    const html = plain(render(team(), 1200));
    // The fixture has one accepted and one in-progress task: two segments joined
    // edge to edge, and no zero-width sliver for the empty states.
    expect(html).toContain('class="seg accepted"');
    expect(html).toContain('class="seg submitted"');
    expect(html).toMatch(/flex-grow:\s*1/);
    expect(html).not.toContain('class="seg active"');
    expect(html).not.toContain('class="seg problem"');
    // The tooltip carries every state's count, the empty ones included.
    expect(html).toContain("Accepted 1, Submitted 1, In progress 0, Problems 0");
    const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
    const css = fs.readFileSync(new URL("client/src/ui/styles/teams.css", new URL("../../../../", import.meta.url)), "utf8");
    const track = css.split("\n").find((line) => line.trim().startsWith(".team-progress {")) ?? "";
    expect(track).toMatch(/gap: 0;/);
    expect(track).toMatch(/overflow: hidden;/);
    expect(track).toMatch(/border-radius: 3px;/);
    const segment = css.split("\n").find((line) => line.trim().startsWith(".team-progress .seg {")) ?? "";
    expect(segment).toMatch(/min-width: 3px;/);
    expect(segment).toMatch(/border-radius: 0;/);
    expect(css).toContain(".team-progress .seg:first-child { border-top-left-radius: 3px;");
    expect(css).toContain(".team-progress .seg:last-child { border-top-right-radius: 3px;");
    // No detached sliver rules from the old painting.
    expect(css).not.toContain('.team-progress .seg:not([style*="width: 0%"])');
  });

  it("takes its own height and pushes the row below it (task #438)", async () => {
    const html = plain(render(team(), 1200));
    expect(html).toContain("data-strip");
    const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
    const css = fs.readFileSync(new URL("client/src/ui/styles/teams.css", new URL("../../../../", import.meta.url)), "utf8");
    const bar = css.split("\n").find((line) => line.trim().startsWith(".team-bar {")) ?? "";
    expect(bar).toMatch(/min-height: 44px;/);
    expect(bar).toMatch(/height: auto;/);
    expect(bar).toMatch(/flex: none;/);
    const lead = css.split("\n").find((line) => line.trim().startsWith(".lead-team {")) ?? "";
    expect(lead).toMatch(/display: flex;/);
    expect(lead).toMatch(/flex-direction: column;/);
    expect(lead).toMatch(/overflow: visible;/);
  });

  it("keeps the strip's one-row CSS: no wrap, a shrinking name, a bounded progress and a nowrap time", async () => {
    const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
    const css = fs.readFileSync(new URL("client/src/ui/styles/teams.css", new URL("../../../../", import.meta.url)), "utf8");
    const bar = css.split("\n").find((line) => line.trim().startsWith(".team-bar {")) ?? "";
    expect(bar).toMatch(/flex-wrap: nowrap;/);
    expect(bar).toMatch(/min-width: 0;/);
    expect(bar).toMatch(/overflow: hidden;/);
    const name = css.split("\n").find((line) => line.trim().startsWith(".team-bar .team-name {")) ?? "";
    expect(name).toMatch(/min-width: 0;/);
    expect(name).toMatch(/text-overflow: ellipsis;/);
    const barProgress = css.split("\n").find((line) => line.trim().startsWith(".team-progress {")) ?? "";
    expect(barProgress).toMatch(/min-width: 80px;/);
    expect(barProgress).toMatch(/max-width: 220px;/);
    const time = css.split("\n").find((line) => line.trim().startsWith(".team-age {")) ?? "";
    expect(time).toMatch(/white-space: nowrap;/);
    // The old rules hid the progress or the time at narrow widths; they are gone.
    expect(css).not.toMatch(/\.team-age \{ display: none;/);
    expect(css).not.toMatch(/\.team-progress, \.team-count, \.team-age \{ display: none;/);
  });
});
