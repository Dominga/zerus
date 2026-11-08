import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";
import type { TeamProfileView, TeamTimelineView, TeamView } from "../../types";

// Task #339: the Profile page's order, as Qt's TeamProfile::timePage has it.
// The totals come first, the member lanes follow, and the legend and the
// estimate row sit directly under the lanes they explain, with the Top losses
// after them. The page and its wording are production code; only the i18n and
// the skin are stand-ins.
const entry = "\0profile-order";
const translations = "\0profile-order-i18n";
const skin = "\0profile-order-skin";
let server: ViteDevServer | undefined;
let renderTime: (view: TeamTimelineView, profile: TeamProfileView) => string;
let renderPage: (team: TeamView, profile: TeamProfileView) => string;

const NOW = 1_800_000_000;

function timeline(): TeamTimelineView {
  const span = (task: number, kind: string, from: number, to: number) =>
    ({ task, attempt: 1, kind, stage_kind: "", member: "dev", from, to, running: false });
  return {
    team: "t1", phase: null, from: NOW - 3600, to: NOW, elapsed_s: 3600, worked_s: 1800,
    lanes: [
      { member: "dev", role: "Backend", model: "gpt", utilization: 0.5, busy_s: 1800, idle_s: 600,
        spans: [span(1, "work", NOW - 3000, NOW - 1800)], waits: [span(1, "deps", NOW - 1800, NOW - 900)], idle: [] },
      { member: "qa", role: "Tester", model: "kimi", utilization: 0, busy_s: 0, idle_s: 0,
        spans: [], waits: [], idle: [] },
    ],
    real: [span(1, "work", NOW - 3000, NOW - 1800)], planned: [span(1, "work", NOW - 3000, NOW - 1800)],
    real_s: 1200, planned_s: 1200, tests: [], tests_more: false,
    estimates: [{ task: 1, estimate_min: 30, worked_s: 1200, running: false }],
    titles: { "1": "Saved cards" }, tasks: 1, truncated: false,
  };
}

function profile(): TeamProfileView {
  return {
    team: "t1", elapsed_s: 3600, worked_s: 1800, tasks: 4, accepted: 1, tokens: 1200, tokens_partial: false,
    losses: [{ kind: "rework", task: 1, member: "dev", seconds: 600, tokens: 100, share: 0.2 }],
    roles: [{ key: "Backend", kind: "work", accepted: 1, attempts: 2, returned: 1, tokens_per_accepted: 1200 }],
    task_rows: [], task_rows_more: false,
  } as unknown as TeamProfileView;
}

function team(): TeamView {
  return {
    id: "t1", name: "Checkout", status: "running", project: "", folder: "", goal: "", plan_mode: "", lead: "lead",
    revision: 1, created: 0, updated: 0, waiting_for_lead: false, pending_phase: null, settings: { auto_accept: false },
    members: [{ id: "lead", role: "Lead", agent: "claude", lead: true }] as never,
    tasks: [], phases: [],
    proposals: [], attention: [], validation: { errors: [], warnings: [] }, health: { critical_path: 0, parallelism: 0, warnings: [] },
    ready: [], cycle: [], draft: null, truncated: false,
  } as TeamView;
}

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  server = await createServer({
    configFile: false, root: fileURLToPath(new URL("../../../", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "profile-order-mocks", enforce: "pre",
      resolveId(id) {
        if ([entry, translations, skin].includes(id)) return id;
        if (id === "../../i18n") return translations;
        if (id === "../../skin/runtime") return skin;
        return undefined;
      },
      load(id) {
        if (id === translations) return `export const trNoop = (s) => s;
          export const tr = (s, ...args) => s.replace(/%(\\d+)/g, (_, n) => String(args[n - 1]));`;
        if (id === skin) return `export const skin = () => ({ appearance: 'light' });`;
        if (id === entry) return `import {createComponent} from 'solid-js';
          import {renderToString} from 'solid-js/web';
          import {TimeSection} from '/src/ui/teams/TimeView.tsx';
          import {ProfilePage} from '/src/ui/teams/profile-page.tsx';
          export const renderTime = (view, profile) => renderToString(() => createComponent(TimeSection,
            { phases: [], phase: null, onPhase: () => {}, view, profile, statusName: (s) => s }));
          export const renderPage = (team, profile) => renderToString(() => createComponent(ProfilePage,
            { team, profile, loadTimeline: async () => ({}), statusName: (s) => s }));`;
        return undefined;
      },
    }, solid({ ssr: true })],
  });
  const module = await server.ssrLoadModule(entry);
  renderTime = module.renderTime;
  renderPage = module.renderPage;
});

afterAll(async () => { await server?.close(); });

/** The HTML without Solid's hydration markers, as the text reads. */
const plain = (html: string) => html.replace(/<!--[^>]*-->/g, "");
const at = (html: string, needle: string) => html.indexOf(needle);

describe("the profile's order (task #339)", () => {
  it("puts the legend and the estimates directly under the lanes", () => {
    const html = plain(renderTime(timeline(), profile()));
    const card = at(html, 'class="tl"');
    const host = at(html, "tl-host");
    const legend = at(html, 'class="pm-legend"');
    const estimates = at(html, 'class="tl-estimates"');
    expect(card).toBeGreaterThan(-1);
    expect(host).toBeGreaterThan(card);
    // The legend follows the lanes card at once, never after another block.
    expect(legend).toBeGreaterThan(host);
    expect(estimates).toBeGreaterThan(legend);
    // It is the legend of the lanes, not a panel that follows them later.
    expect(html.match(/class="pm-legend"/g)).toHaveLength(1);
    expect(at(html, "SPANS")).toBeGreaterThan(host);
    expect(at(html, "SPANS")).toBeLessThan(estimates);
    expect(at(html, "ESTIMATE AND ACTUAL")).toBeGreaterThan(legend);
    // Nothing of the profile panel sits between the lanes and the legend.
    const between = html.slice(host, legend);
    expect(between).not.toContain("pm-tiles");
    expect(between).not.toContain("pm-loss-row");
    expect(between).not.toContain("pm-profile");
  });

  it("puts the totals above the lanes and the Top losses after them, as Qt", () => {
    const html = plain(renderPage(team(), profile()));
    const tiles = at(html, 'class="pm-tiles"');
    const section = at(html, 'class="tl-section"');
    const details = at(html, "pm-profile-details");
    const losses = at(html, 'class="pm-loss-row"');
    expect(tiles).toBeGreaterThan(-1);
    // Qt's summaryTiles sits before the timeline card.
    expect(tiles).toBeLessThan(section);
    // The Top losses follow the lanes and their legend.
    expect(details).toBeGreaterThan(section);
    expect(losses).toBeGreaterThan(section);
    // The summary is its own block above, never between the lanes and the legend.
    expect(html.slice(tiles, section)).not.toContain("tl-host");
  });
});
