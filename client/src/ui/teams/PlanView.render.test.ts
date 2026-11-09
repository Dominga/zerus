import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";
import type { JSX } from "solid-js";
import type { TeamTaskView, TeamView } from "../../types";
import type { PlanGraph } from "./types";

// Task 197: the plan's Graph, Board and Layers as the Tauri client draws them,
// against Qt's TeamWorkspace: one graph per phase with routed links (P1), the
// problem tint with its reason (P2), each phase's own sideways scroll (P3),
// the toolbar (P6), the Board's columns inside each phase (P7) and the Layers'
// compact rows (P8). The plan, its layout and its wording are production
// code; only the i18n, the stores and the session card are stand-ins.
const entry = "\0plan-render";
const translations = "\0plan-render-i18n";
const settingsStore = "\0plan-render-settings";
const sessionCard = "\0plan-render-session-card";
const catalog = "\0plan-render-catalog";
const timeView = "\0plan-render-time";
const actions = "\0plan-render-actions";
const skin = "\0plan-render-skin";
let server: ViteDevServer | undefined;
let render: (props: Record<string, unknown>) => string;
type Hover = { enter(id: number): number | null; leave(current: number | null, id: number): number | null; focus(current: number | null, id: number, keyboard: boolean): number | null };
let planHover: Hover;
let clearsSelection: (target: { closest(selector: string): unknown } | null) => boolean;

const NOW = 1_800_000_000;

function task(id: number, status: string, extra: Partial<TeamTaskView> = {}): TeamTaskView {
  return { id, title: `Task ${id}`, status, assignee: "dev", phase: 1, deps: [], estimate_minutes: 0, attempts: 0, attempt_state: "",
    decided_by: "", pinned: false, output: [], waits: "", ...extra };
}

/** Phase 1: #1 -> #2, #1 -> #3, #2 and #3 -> #4, and #1 -> #4 across the
 * middle column; #5 is a dead-end beside #2 and #3. Phase 2 waits on #4. */
function fixture(): { team: TeamView; graph: PlanGraph } {
  const tasks: TeamTaskView[] = [
    task(1, "accepted", { title: "API", worked_s: 40 * 60, estimate_minutes: 45, estimate_source: "team" }),
    task(2, "in_progress", { title: "Store", deps: [{ task: 1, hard: true }], attempts: 2, attempt_state: "delivered", attempt_begun: true,
      running_since: NOW - 12 * 60, estimate_minutes: 30, estimate_source: "role" }),
    task(3, "assigned", { title: "Cache", deps: [{ task: 1, hard: true }], assignee: "qa" }),
    task(4, "pending", { title: "Wire it", deps: [{ task: 2, hard: true }, { task: 3, hard: true }, { task: 1, hard: true }],
      wait_for: "deps", wait_on: ["2", "3"], estimate_minutes: 45, estimate_source: "lead" }),
    task(5, "draft", { title: "Docs" }),
    task(6, "pending", { title: "Release", phase: 2, deps: [{ task: 4, hard: true }] }),
  ];
  const layer: Record<number, number> = { 1: 0, 2: 1, 3: 1, 5: 1, 4: 2, 6: 0 };
  const team = {
    id: "t1", name: "Checkout", status: "running", project: "", folder: "", goal: "", plan_mode: "", lead: "lead", revision: 1, created: 0, updated: 0,
    waiting_for_lead: false, pending_phase: null, settings: { auto_accept: true },
    members: [
      { id: "lead", role: "Lead", agent: "claude", lead: true },
      { id: "dev", role: "Backend", agent: "codex" },
      { id: "qa", role: "Tester", agent: "kimi" },
    ] as never,
    tasks,
    phases: [
      { n: 1, title: "Build", state: "approved", status: "in_work", done: 1, total: 5, barrier: false, auto: false, acceptance: "", paused: false, health: "" },
      { n: 2, title: "Ship", state: "approved", status: "approved", done: 0, total: 1, barrier: false, auto: false, acceptance: "", paused: false, health: "" },
    ] as never,
    proposals: [], attention: [], validation: { errors: [], warnings: [] }, health: { critical_path: 0, parallelism: 0, warnings: [] },
    ready: [], cycle: [], draft: null, truncated: false,
  } as TeamView;
  const graph: PlanGraph = {
    team: "t1", layers: 3,
    nodes: tasks.map((found) => ({ id: found.id, title: found.title, status: found.status, assignee: found.assignee, role: "", phase: found.phase,
      layer: layer[found.id], ready: false, in_cycle: false })),
    edges: tasks.flatMap((found) => found.deps.map((dep) => ({ from: dep.task, to: found.id, hard: dep.hard }))),
  };
  return { team, graph };
}

const problems = new Map([[3, { tone: "danger", text: "Offline" }]]);

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  server = await createServer({
    configFile: false, root: fileURLToPath(new URL("../../../", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    // The virtualizer must use the same Solid as the plan it windows.
    ssr: { noExternal: ["@tanstack/solid-virtual", "@tanstack/virtual-core"] },
    plugins: [{
      name: "plan-render-mocks", enforce: "pre",
      resolveId(id) {
        if ([entry, translations, settingsStore, sessionCard, catalog, timeView, actions, skin].includes(id)) return id;
        if (id === "../../i18n") return translations;
        if (id === "../../store/settings") return settingsStore;
        if (id === "../sessions/SessionCard") return sessionCard;
        if (id === "../../store/catalog") return catalog;
        if (id === "./TimeView") return timeView;
        if (id === "./actions") return actions;
        if (id === "../../skin/runtime") return skin;
        return undefined;
      },
      load(id) {
        if (id === translations) return `export const trNoop = (s) => s;
          export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));`;
        if (id === settingsStore) return `export const settings = {}; export const settingsReady = new Promise(() => {});`;
        if (id === sessionCard) return `export const AgentBadge = () => null;`;
        if (id === catalog) return `export const agentKind = (agent) => agent || 'claude';`;
        if (id === timeView) return `export const ProfileTime = () => null;`;
        if (id === actions) return `export const stageName = (kind) => kind;`;
        if (id === skin) return `export const skin = () => ({ appearance: 'light' });`;
        if (id === entry) return `import {createComponent} from 'solid-js';
          import {renderToString} from 'solid-js/web';
          import {PlanView} from '/src/ui/teams/PlanView.tsx';
          export {planHover, clearsSelection} from '/src/ui/teams/PlanView.tsx';
          export const render = (props) => renderToString(() => createComponent(PlanView, props));`;
        return undefined;
      },
    }, solid({ ssr: true })],
  });
  const module = await server.ssrLoadModule(entry);
  render = module.render;
  planHover = module.planHover;
  clearsSelection = module.clearsSelection;
});

afterAll(async () => { await server?.close(); });

const now = () => NOW;
/** The HTML without Solid's hydration markers, as the text reads. */
const plain = (html: string) => html.replace(/<!--[^>]*-->/g, "");

describe("the plan's Graph, Board and Layers (task 197)", () => {
  it("draws each phase as one graph with routed links and its own sideways scroll", () => {
    const { team, graph } = fixture();
    const html = render({ team, graph, problems, now });
    // One SVG per expanded phase, never one per layer row.
    expect(html.match(/data-edges="1"/g)).toHaveLength(1);
    expect(html.match(/data-edges="2"/g)).toHaveLength(1);
    // Every dependency inside phase 1 is a routed path with rounded bends.
    for (const edge of ["1-2", "1-3", "2-4", "3-4", "1-4"]) expect(html).toContain(`data-edge="${edge}"`);
    expect(html).toMatch(/ d="M [\d.]+ [\d.]+ L [\d. LQ]+"/);
    // Phase 2's task names its dependency in phase 1 instead of a link.
    expect(plain(html)).toContain("↤#4");
    // Each phase scrolls on its own, keyed by team and phase.
    expect(html).toContain('data-scroll="graph/t1/1"');
    expect(html).toContain('data-scroll="graph/t1/2"');
    // Layers as columns with their labels.
    expect(html).toContain('data-layer="1"');
    // Qt numbers a phase's layers from 1 inside it.
    expect(html).toContain("Layer 3");
  });

  it("tints a problem task with its reason and says what each task does and how long", () => {
    const { team, graph } = fixture();
    const html = render({ team, graph, problems, now });
    expect(html).toContain('data-problem="3"');
    expect(html).toContain("Offline");
    expect(html).toContain("pt-card-problem");
    expect(html).toContain("attempt 2, idle");
    expect(html).toContain("12 of ~30 min");
    expect(html).toContain("after #2, #3");
    expect(html).toContain("~45 min");
    expect(html).toContain("accepted");
    expect(html).toContain("40 of ~45 min");
    // The phase header's health: four of five tasks have no problem, once.
    expect(html.match(/Healthy 4\/5/g)).toHaveLength(1);
    // A draft task keeps its card, dashed.
    expect(html).toContain("pt-card-draft");
  });

  it("offers the toolbar: the state filter, the heat modes, Hide finished, Auto-accept and zoom", () => {
    const { team, graph } = fixture();
    const html = render({ team, graph, problems, now, onAutoAccept: () => {} });
    for (const text of ['data-filter-value="all"', 'data-filter-value="active"', 'data-filter-value="problems"']) expect(html).toContain(text);
    expect(html).toContain('<option value="tokens">Tokens</option>');
    expect(html).toContain('<option value="time">Lateness</option>');
    expect(html).toContain("Auto-accept drafts");
    expect(html).toMatch(/class="pm-auto-accept"[^>]*checked/);
    expect(html).toContain('data-zoom="1"');
    expect(html).toContain("Fit");
    // A member filter names the member and dims the others' cards.
    const focused = render({ team, graph, problems, now, member: "qa" });
    expect(focused).toContain("Only Tester's tasks");
    expect(focused.match(/pt-card-off/g)?.length).toBe(5);
  });

  it("collapses a phase to one line with its tasks' titles", () => {
    const { team, graph } = fixture();
    team.phases[1].status = "done";
    team.tasks[5].status = "accepted";
    const html = render({ team, graph, problems, now });
    expect(html).toContain('data-summary="1"');
    expect(html).toContain("#6 Release");
    // One summary per phase, not the same list twice.
    expect(html).not.toContain('data-inline=');
    expect(html).not.toContain('data-scroll="graph/t1/2"');
  });

  it("shows one health figure and no 'lead' label in the phase header, in every mode (task 356)", () => {
    const { team, graph } = fixture();
    // The core names who accepts a phase in acceptance: lead or owner.
    team.phases[0].acceptance = "lead";
    team.phases[1].acceptance = "owner";
    for (const initialMode of ["graph", "board", "layers"]) {
      const html = plain(render({ team, graph, problems, now, initialMode }));
      const head = html.slice(html.indexOf('data-phase-head="1"'), html.indexOf('data-phase-head="2"'));
      expect(head.match(/Healthy \d+\/\d+/g), initialMode).toEqual(["Healthy 4/5"]);
      expect(html, initialMode).not.toContain("pt-acceptance");
      expect(html, initialMode).not.toMatch(/>\s*(lead|owner)\s*</);
    }
  });

  it("gives a folded phase card its closing border and four round corners, with one frame when open (task 332)", async () => {
    const { team, graph } = fixture();
    team.phases[1].status = "done";
    team.tasks[5].status = "accepted";
    const html = render({ team, graph, problems, now });
    // The folded header carries pt-phase-closed beside its state class; the open one does not.
    expect(html).toMatch(/class="pt-phase pt-phase-done pt-phase-closed"[^>]*data-phase-head="2"/);
    expect(html).toMatch(/class="pt-phase pt-phase-[a-z]+"[^>]*aria-expanded="true"[^>]*data-phase-head="1"/);
    const fs = await import(/* @vite-ignore */ ["node", "fs"].join(":")) as { readFileSync(path: URL, encoding: "utf8"): string };
    const css = fs.readFileSync(new URL("../styles/plan.css", import.meta.url), "utf8").replace(/\r\n/g, "\n");
    expect(css).toContain(".pt-phase.pt-phase-closed { border-bottom: 1px solid var(--z-color-card-border); border-radius: 10px; }");
    // Wider content is clipped inside the padding, never against the right border.
    expect(css).toContain(".pt-phase { overflow: clip; overflow-clip-margin: content-box; }");
    // The open card's fold and body take the header's state border.
    for (const state of ["done", "running", "draft", "rejected", "awaiting"]) {
      expect(css).toContain(`.pt-row:has(> .pt-phase-${state}) > :is(.pt-free-accepted, .pt-phase-body)`);
    }
    expect(css).toMatch(/\.pt-row > \.pt-free-accepted \{ border: 1px solid var\(--z-color-card-border\); border-top: 0; border-bottom: 0;/);
  });

  it("folds the open free phase's accepted tasks into one row (task 316 F6)", () => {
    const { team, graph } = fixture();
    team.free_phase = 1;
    team.phases[0].state = "running";
    const html = render({ team, graph, problems, now });
    expect(html).toContain('data-accepted-fold="1"');
    expect(plain(html)).toContain("1 accepted");
    expect(plain(html)).toContain("#1");
    expect(plain(html)).toContain("Show");
    // The accepted card is folded away; the open ones stay.
    expect(html).not.toContain('data-task="1"');
    expect(html).toContain('data-task="2"');
  });

  it("shows the Board inside each phase with Qt's columns, so assigned and draft tasks stay", () => {
    const { team, graph } = fixture();
    const html = render({ team, graph, problems, now, initialMode: "board" });
    for (const key of ["pending", "active", "problems", "submitted", "accepted"]) expect(html).toContain(`data-column="${key}"`);
    for (const title of ["PENDING", "ACTIVE", "PROBLEMS", "SUBMITTED", "ACCEPTED"]) expect(html).toContain(title);
    expect(html.match(/data-column="pending"/g)).toHaveLength(2);
    // Every task of the phase is on the board: the draft under Pending, the
    // problem (an assigned task) under Problems.
    for (const id of [1, 2, 3, 4, 5, 6]) expect(html).toContain(`data-task="${id}"`);
    expect(plain(html)).toContain("←#2 #3 #1");
    expect(html).toContain("Backend");
    expect(html).toContain('data-scroll="board/t1/1"');
  });

  it("shows a No report card's kept last reply and 'Use last reply as report' (task #415)", () => {
    const { team, graph } = fixture();
    const target = team.tasks.find((task) => task.id === 2)!;
    target.no_report = true;
    target.attempt_id = "a7";
    const html = plain(render({ team, graph, problems, now, initialMode: "board",
      noReportReply: (id: number) => (id === 2 ? "Done: the parser and its tests" : ""),
      onUseLastReply: () => {} }));
    expect(html).toContain("Done: the parser and its tests");
    expect(html).toContain("Use last reply as report");
    expect(html).toContain('data-action="use-last-reply"');
    expect(html).toContain('data-no-report="true"');
    // The card is still the card: its task, title and column stay.
    expect(html).toContain('data-task="2"');
    expect(html).toContain("Store");
  });

  it("marks a Crit node and its links violet and a Minor phase and task quietly", () => {
    const { team, graph } = fixture();
    team.phases[1].kind = "minor";
    team.phases[1].crit = [6];
    team.phases[1].crit_gate = "critical task #6 in phase 2 is open";
    team.tasks[5].severity = "crit";
    team.tasks[5].origin = 2;
    team.tasks[3].severity = "crit";
    team.tasks[4].severity = "minor";
    const html = render({ team, graph, problems, now });
    expect(html).toContain('data-severity="crit"');
    expect(html).toContain("pt-card-crit");
    expect(html).toContain("pt-edge-crit");
    expect(plain(html)).toContain("Crit #2");
    expect(html).toContain('data-phase-kind="minor"');
    expect(html).toContain(">Minor<");
    expect(html).toContain("critical task #6 in phase 2 is open");
    const board = render({ team, graph, problems, now, initialMode: "board" });
    expect(board).toContain('data-severity="minor"');
    expect(board).toContain("Minor");
    const layers = render({ team, graph, problems, now, initialMode: "layers" });
    expect(layers).toContain('data-severity="crit"');
  });

  it("lists Layers as compact rows by layer then id, filtered by member", () => {
    const { team, graph } = fixture();
    const html = render({ team, graph, problems, now, initialMode: "layers" });
    const order = [...html.matchAll(/class="pm-layer-row[^"]*"[^>]*data-task="(\d+)"/g)].map((match) => Number(match[1]));
    expect(order).toEqual([1, 2, 3, 5, 4, 6]);
    // Each row names its layer inside the phase: phase 1's L1 to L3, phase 2's L1.
    expect(html.match(/class="pm-layer-label">L1</g)?.length).toBe(2);
    expect(html).toContain('class="pm-layer-label">L3<');
    const qa = render({ team, graph, problems, now, initialMode: "layers", member: "qa" });
    expect([...qa.matchAll(/class="pm-layer-row[^"]*"[^>]*data-task="(\d+)"/g)].map((match) => Number(match[1]))).toEqual([3]);
  });

  it("keeps the title visible and folds a long dependency list (task 333)", () => {
    const base = fixture().team;
    const tasks: TeamTaskView[] = [];
    for (let id = 1; id <= 18; id++) tasks.push(task(id, "accepted", { phase: 1, title: `Dep ${id}` }));
    const deps = (ids: number[]) => ids.map((id) => ({ task: id, hard: true }));
    tasks.push(task(204, "pending", { phase: 2, title: "Long dependency card", deps: deps(Array.from({ length: 18 }, (_, i) => i + 1)) }));
    tasks.push(task(205, "pending", { phase: 2, title: "One dependency", deps: deps([1]) }));
    tasks.push(task(206, "pending", { phase: 2, title: "Two dependencies", deps: deps([1, 2]) }));
    const team = { ...base, tasks, phases: [{ ...base.phases[0], n: 1, title: "Deps", total: 18 }, { ...base.phases[1], n: 2, title: "Cards", total: 3 }] };
    const graph: PlanGraph = {
      team: "t1", layers: 2,
      nodes: tasks.map((found) => ({ id: found.id, title: found.title, status: found.status, assignee: found.assignee, role: "", phase: found.phase,
        layer: found.phase === 1 ? 0 : 1, ready: false, in_cycle: false })),
      edges: tasks.flatMap((found) => found.deps.map((dep) => ({ from: dep.task, to: found.id, hard: dep.hard }))),
    };
    const html = render({ team, graph, problems: new Map(), now });
    const card = plain(html.slice(html.indexOf('data-task="204"')));
    expect(card).toContain("Long dependency card");
    expect(card).toContain("↤#1 #2 +16");
    expect(card.indexOf("Long dependency card")).toBeLessThan(card.indexOf("pt-deps"));
    expect(card).toContain("#18");
    const one = plain(html.slice(html.indexOf('data-task="205"')));
    expect(one).toContain("↤#1");
    expect(one).not.toContain("+0");
    const two = plain(html.slice(html.indexOf('data-task="206"')));
    expect(two).toContain("↤#1 #2");
    expect(two).not.toContain("+0");
  });

  // Task 335 (owner, 2026-11-08): the strip of phase chips 219 added above the
  // plan is gone. Qt keeps the Standard/Free toggle in each phase's own header;
  // Board and Layers draw that header, Lanes its own, and Profile and Tests
  // have no phase headers at all.
  it("has no phase strip above the plan and keeps the toggle in each phase header", () => {
    const { team, graph } = fixture();
    const phaseActions = ((n: number) => `TOGGLE-${n}`) as unknown as (n: number) => JSX.Element;
    for (const initialMode of ["graph", "board", "layers", "lanes", "profile", "tests"]) {
      const html = render({ team, graph, problems, now, initialMode, phaseActions });
      expect(html, initialMode).not.toContain("pm-phase-strip");
    }
    for (const initialMode of ["board", "layers", "lanes"]) {
      const html = render({ team, graph, problems, now, initialMode, phaseActions });
      // Each phase's header, once: no repeated chip row above the view.
      for (const n of [1, 2]) expect(html, `${initialMode}/${n}`).toContain(`TOGGLE-${n}`);
      expect(html, initialMode).not.toContain("TOGGLE-undefined");
    }
  });

  // Task 342 (owner, 2026-11-08): the five task states are told apart by colour
  // in every plan mode - done green, in work light blue, in review a deeper
  // blue, not started a pale grey and cancelled a grey hatch - and the plan
  // header carries their legend.
  it("tints each task by its state and names the five states in a legend", () => {
    const { team, graph } = fixture();
    team.tasks[0].status = "accepted";
    team.tasks[1].status = "in_progress";
    team.tasks[3].status = "submitted";
    team.tasks[4].status = "pending";
    team.tasks[5].status = "cancelled";
    team.tasks[3].deps = [];
    const graphHtml = render({ team, graph, problems, now, initialShowCancelled: true });
    for (const cls of ["pt-card-done", "pt-card-work", "pt-card-review", "pt-card-notstarted", "pt-card-cancelled"]) {
      expect(graphHtml, cls).toContain(`pt-card-${cls.slice(8)}`);
    }
    // The same tones on Board and Layers, where the cards are rows.
    for (const initialMode of ["board", "layers"]) {
      const html = render({ team, graph, problems, now, initialMode, initialShowCancelled: true });
      for (const cls of ["pt-tone-done", "pt-tone-work", "pt-tone-review", "pt-tone-notstarted", "pt-tone-cancelled"]) {
        expect(html, `${initialMode}/${cls}`).toContain(cls);
      }
    }
    // The legend: one swatch per state, in the plan header of every mode.
    for (const initialMode of ["graph", "board", "layers", "lanes", "profile"]) {
      const html = render({ team, graph, problems, now, initialMode, initialShowCancelled: true });
      expect(html, initialMode).toContain("data-state-legend");
      for (const label of ["Done", "In progress", "In review", "Not started", "Cancelled"]) expect(html, initialMode).toContain(label);
      expect(html, initialMode).toContain("pt-swatch-cancelled");
    }
    // The side panel takes the selected task's colour.
    const detail = render({ team, graph, problems, now, initialShowCancelled: true, openTask: { task: 1 } });
    expect(detail).toContain("pt-detail-done");
    expect(detail).toContain('data-task-tone="done"');
  });

  // Task 342: Hide finished hides accepted tasks in every plan mode, not only
  // the phases whose every task is decided.
  it("Hide finished leaves accepted tasks out of every plan mode", () => {
    const { team, graph } = fixture();
    const modes = ["graph", "board", "layers"];
    for (const initialMode of modes) {
      const shown = render({ team, graph, problems, now, initialMode });
      expect(shown, initialMode).toContain('data-task="1"');
      const hidden = render({ team, graph, problems, now, initialMode, initialHideFinished: true });
      expect(hidden, initialMode).not.toContain('data-task="1"');
      // The tasks still open stay.
      expect(hidden, initialMode).toContain('data-task="2"');
    }
    const lanes = render({ team, graph, problems, now, initialMode: "lanes" });
    expect(lanes).toContain("pm-lanes");
    expect(lanes).toContain("API");
    const lanesHidden = render({ team, graph, problems, now, initialMode: "lanes", initialHideFinished: true });
    expect(lanesHidden).not.toContain("API");
    expect(lanesHidden).toContain("Store");
  });
});


// Task 428 (owner, 2026-11-09): "the cursor is here, and the last chosen one stayed".
describe("the plan board's hover and selection (task 428)", () => {
  it("lights a card only while the pointer is over it or the keyboard focuses it", () => {
    let hover = planHover.enter(4);
    expect(hover).toBe(4);
    // The pointer moves to empty space: the card's own leave ends it.
    hover = planHover.leave(hover, 4);
    expect(hover).toBeNull();
    // Entering another card before the first one's leave arrives never loses the new one.
    hover = planHover.enter(2);
    hover = planHover.leave(planHover.enter(5), 2);
    expect(hover).toBe(5);
    // A click's focus does not light a card; keyboard focus does, and blur ends it.
    expect(planHover.focus(null, 3, false)).toBeNull();
    expect(planHover.focus(null, 3, true)).toBe(3);
    expect(planHover.leave(3, 3)).toBeNull();
  });

  it("marks only the selected card and nothing as hovered on a fresh board", () => {
    const { team, graph } = fixture();
    const html = render({ team, graph, problems, now, openTask: { task: 2 } });
    const card = (id: number) => html.match(new RegExp(`<button[^>]*data-task="${id}"[^>]*>`))?.[0] ?? "";
    expect(card(2)).toContain("pt-card-selected");
    expect(card(2)).toContain('aria-pressed="true"');
    for (const id of [1, 3, 4, 5]) {
      expect(card(id), `#${id}`).not.toContain("pt-card-selected");
      expect(card(id)).toContain('aria-pressed="false"');
    }
    expect(html).not.toContain("pt-card-on");
    expect(html).not.toMatch(/data-lit="(lit|dim)"/);
    // Nothing chosen: no card is marked.
    expect(render({ team, graph, problems, now })).not.toContain("pt-card-selected");
  });

  it("clears the selection on a click on empty board space, never on a card, header or control", () => {
    const at = (inside: string[]) => ({ closest: (selector: string) => (inside.some((part) => selector.split(", ").includes(part)) ? {} : null) });
    expect(clearsSelection(at([]))).toBe(true);
    for (const inside of [".pt-card", ".pt-phase", "button", ".pt-detail", "input"]) expect(clearsSelection(at([inside])), inside).toBe(false);
    expect(clearsSelection(null)).toBe(false);
  });

  it("styles the selection apart from the hover, and shows a focus ring only for the keyboard", async () => {
    const fs = await import(/* @vite-ignore */ ["node", "fs"].join(":")) as { readFileSync(path: URL, encoding: "utf8"): string };
    const css = fs.readFileSync(new URL("../styles/plan.css", import.meta.url), "utf8").replace(/\r\n/g, "\n");
    expect(css).toMatch(/\.pt-card\.pt-card-selected \{[^}]*border-color: var\(--z-color-accent\)/);
    expect(css).toMatch(/\.pt-card:focus \{ outline: none; \}/);
    expect(css).toMatch(/\.pt-card:focus-visible \{ outline: 2px solid/);
    const source = fs.readFileSync(new URL("./PlanView.tsx", import.meta.url), "utf8");
    // The card ends its own hover when the pointer leaves it, and the window's blur ends any.
    expect(source).toContain("onPointerLeave={() => setHover(planHover.leave(hover(), task.id))}");
    expect(source).toContain('window.addEventListener("blur", clear)');
  });
});
