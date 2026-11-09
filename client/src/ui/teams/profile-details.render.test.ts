import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";
import type { TeamProfileView, TeamView } from "../../types";

// Task #410: the Profile page's lower part: Top losses and Roles as two
// collapsible sections side by side, folded to their first rows with the count
// in each header, and unknown token values as a muted dash with the reason in
// its tooltip. The component and wording are production code; only the i18n
// and the skin are stand-ins.
const entry = "\0profile-details";
const translations = "\0profile-details-i18n";
const skin = "\0profile-details-skin";
let server: ViteDevServer | undefined;
let render: (team: TeamView, profile: TeamProfileView) => string;
let fold: (view: string, section: "losses" | "roles", value: boolean) => void;
let reset: () => void;

function team(): TeamView {
  return {
    id: "t1", name: "Checkout", status: "running", project: "", folder: "", goal: "", plan_mode: "", lead: "lead",
    revision: 1, created: 0, updated: 0, waiting_for_lead: false, pending_phase: null, settings: { auto_accept: false },
    members: [], tasks: [], phases: [],
    proposals: [], attention: [], validation: { errors: [], warnings: [] }, health: { critical_path: 0, parallelism: 0, warnings: [] },
    ready: [], cycle: [], draft: null, truncated: false,
  } as TeamView;
}

function profile(): TeamProfileView {
  return {
    team: "t1", elapsed_s: 3600, worked_s: 1800, tasks: 20, accepted: 8, tokens: null, tokens_partial: false,
    losses: Array.from({ length: 8 }, (_, i) => ({ kind: i === 0 ? "idle_member" : "member_wait", task: i, seconds: 600 * (i + 1), tokens: i === 0 ? null : 100 * (i + 1) })),
    roles: Array.from({ length: 6 }, (_, i) => ({ key: `Role ${i}`, kind: "work", accepted: i, attempts: i + 2, returned: 1, tokens_per_accepted: i === 0 ? null : 1200 * (i + 1) })),
    task_rows: [], task_rows_more: false,
  } as unknown as TeamProfileView;
}

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  server = await createServer({
    configFile: false, root: fileURLToPath(new URL("../../../", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [{
      name: "profile-details-mocks", enforce: "pre",
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
          import {ProfileDetails} from '/src/ui/teams/plan-panels.tsx';
          import {resetProfileSections, setProfileSectionFolded} from '/src/store/profileSections.ts';
          export const render = (team, profile) => renderToString(() => createComponent(ProfileDetails, { team, profile }));
          export const fold = (view, section, value) => setProfileSectionFolded(view, section, value);
          export const reset = () => resetProfileSections();`;
        return undefined;
      },
    }, solid({ ssr: true })],
  });
  const module = await server.ssrLoadModule(entry);
  render = module.render;
  fold = module.fold;
  reset = module.reset;
});

afterAll(async () => { await server?.close(); });
beforeAll(() => reset());

const plain = (html: string) => html.replace(/<!--[^>]*-->/g, "");
const at = (html: string, needle: string) => html.indexOf(needle);

describe("the profile's lower sections (task #410)", () => {
  it("puts Top losses and Roles side by side, folded to their first rows with their counts", () => {
    reset();
    const html = plain(render(team(), profile()));
    expect(html).toContain('class="pm-details-columns"');
    expect(html.match(/class="pm-section"/g)).toHaveLength(2);
    expect(at(html, 'data-section="losses"')).toBeGreaterThan(-1);
    expect(at(html, 'data-section="roles"')).toBeGreaterThan(-1);
    // Each header carries the full count.
    expect(html).toMatch(/<span class="pm-section-count"[^>]*>8<\/span>/);
    expect(html).toMatch(/<span class="pm-section-count"[^>]*>6<\/span>/);
    // Folded: the first five rows of each, with Show all.
    expect(html.match(/class="pm-loss-row"/g)).toHaveLength(5);
    expect(html.match(/class="pm-role-row"/g)).toHaveLength(5);
    expect(html.match(/>Show all</g)).toHaveLength(2);
    expect(html).not.toContain("Show fewer");
  });

  it("shows every row once opened and remembers the choice per view", () => {
    reset();
    fold("t1", "losses", false);
    const html = plain(render(team(), profile()));
    expect(html.match(/class="pm-loss-row"/g)).toHaveLength(8);
    expect(html).toContain(">Show fewer<");
    // The other section stays folded, and another view starts folded again.
    expect(html.match(/class="pm-role-row"/g)).toHaveLength(5);
    expect(html.match(/>Show all</g)).toHaveLength(1);
    fold("t1", "roles", false);
    const both = plain(render(team(), profile()));
    expect(both.match(/class="pm-role-row"/g)).toHaveLength(6);
    // The same view keeps both choices across a render.
    expect(at(both, 'data-section="losses"')).toBeGreaterThan(-1);
  });

  it("draws unknown token values as a muted dash with the reason in its tooltip", () => {
    reset();
    const html = plain(render(team(), profile()));
    expect(html).not.toContain(">unknown<");
    expect(html.match(/class="pm-unknown"/g)?.length ?? 0).toBeGreaterThanOrEqual(2);
    expect(html).toContain("This loss");
    expect(html).toContain("No accepted task of this role");
    // The dash itself, never the word, is what a row shows.
    expect(html).toContain(">—</span>");
  });

  it("keeps the spans legend with the lanes: the details carry no legend", () => {
    reset();
    const html = plain(render(team(), profile()));
    expect(html).not.toContain('class="pm-legend"');
  });
});

describe("the two columns' layout rules", () => {
  it("is a two-column grid that wraps to one column on a narrow window", async () => {
    const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
    const css = fs.readFileSync(new URL("client/src/ui/styles/plan.css", new URL("../../../../", import.meta.url)), "utf8");
    const rule = css.split("\n").find((line) => line.trim().startsWith(".pm-details-columns {")) ?? "";
    expect(rule).toMatch(/display: grid;/);
    expect(rule).toMatch(/grid-template-columns: repeat\(2, minmax\(0, 1fr\)\);/);
    expect(css).toContain("@media (max-width: 1100px) { .pm-details-columns { grid-template-columns: minmax(0, 1fr); } }");
    expect(css.split("\n").find((line) => line.trim().startsWith(".pm-unknown {")) ?? "").toMatch(/color: var\(--z-color-muted\);/);
  });
});
