import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";
import type { TeamList, TeamListedMember, TeamMemberView, TeamProposalView, TeamSummary, TeamView } from "../../types";

// Task 217 (Qt U57, SessionsWindow.cpp:1083, 1711, 7616-7626): the lead's
// open proposals are cards in its own Activity above the composer, from the
// live team read — not only in the Team tab and Waiting for you. Rendered as
// the Tauri client draws it; the stores are the only isolated modules, the
// component, the card model and the lead lookup are production code.
const entry = "\0lead-proposals-render";
const stores = "\0lead-proposals-stores";
const translations = "\0lead-proposals-i18n";
const bridge = "\0lead-proposals-bridge";
const ui = "\0lead-proposals-ui";
const confirm = "\0lead-proposals-confirm";

let server: ViteDevServer | undefined;
let render: () => string;
let state: { selected?: { computer: string; state: string; name: string }; list: TeamList; views: Record<string, TeamView>; subagent?: string };

const NAME = "claude/p/lead";
const DEV = "claude/p/dev";

function listed(id: string, name: string, lead: boolean): TeamListedMember {
  return { id, role: lead ? "Lead" : id, agent: "claude", state: "active", lead, session: { host: "", name, run_id: "r", conversation_id: "c" } };
}

function member(id: string, name: string, lead: boolean): TeamMemberView {
  return {
    id, role: lead ? "Lead" : id, agent: "claude", model: "", effort: "", account: "", computer: "", folder: "",
    state: "active", activity: "idle", phase: "idle", error: "", lead, session: { host: "", name, run_id: "r", conversation_id: "c" },
    live: true, stuck: false, question: false, queue: [], minutes_without_progress: 0, host_shared: false,
  };
}

function summary(id: string, members: TeamListedMember[], over: Partial<TeamSummary> = {}): TeamSummary {
  return {
    id, name: id, status: "running", project: "", folder: "/work/p", plan_mode: "standard", lead: "lead",
    members, tasks: 0, accepted: 0, submitted: 0, in_review: 0, in_test: 0,
    attention: 0, open_proposals: 0, waiting_for_lead: false, pending_phase: null, updated: 1, revision: 1,
    phase: 0, phase_done: 0, phase_total: 0, phases_done: 0, phases_total: 0, ...over,
  };
}

function view(id: string, members: TeamMemberView[], proposals: TeamProposalView[]): TeamView {
  return {
    id, name: id, status: "running", project: "", folder: "/work/p", goal: "", plan_mode: "standard", lead: "lead",
    revision: 1, created: 1, updated: 1, waiting_for_lead: false, pending_phase: null,
    settings: {}, members, tasks: [], phases: [], proposals, attention: [],
    validation: { errors: [], warnings: [] }, health: { critical_path: 0, parallelism: 0, warnings: [] },
    ready: [], cycle: [], draft: null, truncated: false,
  };
}

function proposal(over: Partial<TeamProposalView> & { id: string; kind: string }): TeamProposalView {
  return { by: "Lead", at: 2000, reason: "more hands", hash: `h${over.id.slice(1)}`, summary: over.kind, lines: [], change: null, member: null, ...over };
}

const list = (...teams: TeamSummary[]): TeamList => ({ teams, truncated: false });

const limits = () => proposal({
  id: "p1", kind: "settings", summary: "Team limits", lines: ["max_members: 6 → 8"],
  change: { op: "settings", input: { settings: { max_members: 8 } }, summary: "Team limits", lines: ["max_members: 6 → 8"] },
});
const designer = () => proposal({ id: "p2", kind: "add-member", change: { members: [{ id: "designer", role: "Designer", agent: "claude", model: "sonnet" }] } });

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  server = await createServer({
    configFile: false, root: fileURLToPath(new URL("../../../", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "lead-proposals-mocks", enforce: "pre",
      resolveId(id, importer) {
        if ([entry, stores, translations, bridge, ui, confirm].includes(id)) return id;
        if (importer?.endsWith("/ui/teams/ProposalCards.tsx") || importer?.endsWith("/ui/teams/decisions.ts")) {
          if (id === "../../i18n") return translations;
          if (id === "../../bridge") return bridge;
          if (id === "../../store/teams") return stores;
          if (id === "../../store/ui") return ui;
          if (id === "../../store/teamConfirm") return confirm;
        }
        return undefined;
      },
      load(id) {
        if (id === stores) return `export const state = { selected: undefined, list: { teams: [], truncated: false }, views: {} };
          export const teams = { list: () => state.list, team: (computer, id) => state.views[id] };
          export const showTeam = () => () => {};`;
        if (id === translations) return `export const trNoop = (s) => s;
          export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));`;
        if (id === bridge) return `export const teamAction = async () => ({ outcome: "failed", detail: "the render test sends nothing" });
          export const teamRead = async () => ({});
          export const inTauri = () => false;`;
        if (id === confirm) return `export const teamConfirm = { ask: async () => false, askChoice: async () => undefined };`;
        if (id === ui) return `import { state } from '${stores}';
          export const selected = () => state.selected;
          export const subagentOpen = () => state.subagent !== undefined;`;
        if (id === entry) return `import { createComponent } from 'solid-js';
          import { renderToString } from 'solid-js/web';
          import { LeadProposals } from '/src/ui/teams/ProposalCards.tsx';
          import { state } from '${stores}';
          export { state };
          export const render = () => renderToString(() => createComponent(LeadProposals, {}));`;
      },
    }, solid({ ssr: true })],
  });
  const module = await server.ssrLoadModule(entry);
  render = module.render;
  state = module.state;
});

afterAll(async () => { await server?.close(); });

/** The lead's own session, with the team it leads in the polled list and its live read. */
function aLead(listedTeams: TeamSummary[], views: Record<string, TeamView>, over: { name?: string; state?: string; subagent?: string } = {}) {
  state.selected = { computer: "", state: over.state ?? "running", name: over.name ?? NAME };
  state.list = list(...listedTeams);
  state.views = views;
  state.subagent = over.subagent;
}

describe("the lead's proposal cards in its Activity (task 217)", () => {
  it("draws the open cards of the team it leads, with the change lines and Allow and Deny", () => {
    const members = [member("lead", NAME, true)];
    aLead([summary("t1", [listed("lead", NAME, true)])], { t1: view("t1", members, [limits(), designer()]) });
    const html = render();
    expect(html).toContain('class="team-proposals"');
    expect(html).toContain('data-proposal="p1"');
    expect(html).toContain('data-kind="settings"');
    expect(html).toContain('data-hash="h1"');
    expect(html).toContain("Team limits");
    expect(html).toContain("max_members: 6 → 8");
    expect(html).toContain("Add Designer (claude, sonnet, default account)");
    expect(html).toContain(">Allow<");
    expect(html).toContain(">Deny<");
    // One section over every card, as the Team tab draws it.
    expect(html).toContain('aria-label="Proposals"');
  });

  it("clamps a long reason and keeps Allow and Deny in the card", async () => {
    const members = [member("lead", NAME, true)];
    const long = proposal({ id: "p3", kind: "settings", reason: "a long reason ".repeat(20), change: { op: "settings", summary: "Change the team's settings", lines: ["integration_folder: old → new"] } });
    aLead([summary("t1", [listed("lead", NAME, true)])], { t1: view("t1", members, [long]) });
    const html = render();
    expect(html).toContain('class="team-proposal-reason clamped"');
    expect(html).toContain(">Show more<");
    expect(html).toContain(">Allow<");
    expect(html).toContain(">Deny<");
    // Task 367 (owner, 2026-11-08): the strip in the owner's Team pane grows
    // with its cards instead of scrolling inside a box shorter than one card,
    // and only a long reason is clamped, to four lines with More.
    const { readFileSync } = await import(/* @vite-ignore */ ["node", "fs"].join(":")) as { readFileSync(path: URL): string };
    const css = String(readFileSync(new URL("../styles/teams.css", import.meta.url))).replace(/\r\n/g, "\n");
    const rule = (selector: string) => {
      const at = css.indexOf(`${selector} {`);
      expect(at, selector).toBeGreaterThan(-1);
      return css.slice(at, css.indexOf("}", at));
    };
    const strip = rule(".team-conversation > .team-proposals");
    expect(strip).toContain("max-height: none");
    expect(strip).toContain("overflow: visible");
    expect(rule(".team-proposal-reason.clamped")).toMatch(/-webkit-line-clamp:\s*4/);
    expect(rule(".team-proposal-answers")).toContain("flex: none");
  });

  // Task #338: one card to the owner is drawn in full once, with its Allow and
  // Deny, and nowhere twice (the wait strip only counts and links to it).
  it("draws one card once", () => {
    const members = [member("lead", NAME, true)];
    aLead([summary("t1", [listed("lead", NAME, true)])], { t1: view("t1", members, [limits()]) });
    const html = render();
    expect(html.match(/class="team-proposals"/g)?.length).toBe(1);
    expect(html.match(/data-proposal="p1"/g)?.length).toBe(1);
    expect(html.match(/>Allow</g)?.length).toBe(1);
    expect(html.match(/>Deny</g)?.length).toBe(1);
  });

  it("draws nothing for a member's session", () => {
    aLead([summary("t1", [listed("lead", NAME, true), listed("dev", DEV, false)])],
      { t1: view("t1", [member("lead", NAME, true), member("dev", DEV, false)], [limits()]) }, { name: DEV });
    const html = render();
    expect(html).not.toContain("team-proposals");
    expect(html).not.toContain("Team limits");
  });

  it("draws the sub-team a sub-lead leads, not the parent team it also belongs to", () => {
    aLead([
      summary("parent", [listed("lead", NAME, true), listed("dev", DEV, false)]),
      summary("child", [listed("dev", DEV, true)], { parent: "parent" }),
    ], {
      parent: view("parent", [member("lead", NAME, true), member("dev", DEV, false)], [limits()]),
      child: view("child", [member("dev", DEV, true)], [proposal({ id: "p9", kind: "relaunch", summary: "Relaunch dev", change: { op: "relaunch", summary: "Relaunch dev", lines: [] } })]),
    }, { name: DEV });
    const html = render();
    expect(html).toContain('data-proposal="p9"');
    expect(html).toContain("Relaunch dev");
    expect(html).not.toContain("Team limits");
  });

  it("draws nothing while a subagent's conversation is open, nor for an archived session", () => {
    const members = [member("lead", NAME, true)];
    aLead([summary("t1", [listed("lead", NAME, true)])], { t1: view("t1", members, [limits()]) }, { subagent: "child-1" });
    expect(render()).not.toContain("team-proposals");
    aLead([summary("t1", [listed("lead", NAME, true)])], { t1: view("t1", members, [limits()]) }, { state: "archived" });
    expect(render()).not.toContain("team-proposals");
  });

  it("follows the live read: decided cards go away and a changed roster drops them", () => {
    const members = [member("lead", NAME, true)];
    aLead([summary("t1", [listed("lead", NAME, true)])], { t1: view("t1", members, [limits()]) });
    expect(render()).toContain('data-proposal="p1"');
    // hgs confirmed the decision: the next read no longer lists the proposal.
    state.views = { t1: view("t1", members, []) };
    expect(render()).not.toContain("team-proposals");
    // The same proposals, but the live read no longer names this session lead.
    state.views = { t1: { ...view("t1", [member("lead", NAME, false)], [limits()]), lead: "someone-else" } };
    expect(render()).not.toContain("team-proposals");
  });
});
