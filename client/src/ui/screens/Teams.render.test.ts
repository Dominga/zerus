// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent, createRoot } from "solid-js";
import { render } from "solid-js/web";
import fixture from "./fixtures/teams-screen.json";
import presentation from "../sessions/fixtures/team-session-presentation.json";
import type { TeamList, TeamSummary, TeamView } from "../../store/teams";
import type { Session } from "../sessions/types";
import type { TeamsOut, ViewRow } from "../../types";

// Every Teams result row/cursor is pinned by the Rust product-writer test.
// Session inputs are synthetic core rows, mapped by the product's toSession.
const data = fixture as unknown as { pages: TeamList[]; empty: TeamList; running: TeamList; stuck: TeamList; team: TeamView };
const f = vi.hoisted(() => ({ rows: [] as Session[], pages: [] as [string, string | null][], follows: [] as unknown[], calls: [] as string[] }));
vi.mock("../../bridge", async (original) => ({ ...(await original<object>()), inTauri: () => true }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: async (command: string, args?: { key: string; cursor: string | null }) => {
  f.calls.push(command);
  if (command === "teams_page") f.pages.push([args!.key, args!.cursor]);
  if (command === "teams_follow") f.follows.push(args);
  return undefined;
}, isTauri: () => false }));
vi.mock("../../store/machines", async (original) => ({ ...(await original<object>()), machineCards: { "": { machine_id: "00000000-0000-4000-8000-0000000000aa" } } }));
vi.mock("../../store/catalog", async (original) => ({ ...(await original<object>()), orderedRows: () => f.rows }));
vi.mock("../../store/projects", () => ({ projectsPage: () => ({ projects: [{ id: "example-project", name: "Example project" }] }) }));
vi.mock("../../store/ui", async (original) => {
  const ui = await original<typeof import("../../store/ui")>();
  return { ...ui, selected: () => f.rows.find((row) => row.key === ui.selectedKey()) };
});
let store: typeof import("../../store/teams").teams;
let ui: typeof import("../../store/ui");
let nav: typeof import("../teams/lead-navigation");
let TeamsScreen: typeof import("./Teams").TeamsScreen;
let toSession: typeof import("../../store/catalog").toSession;
const disposers: (() => void)[] = [];
const entry = (key: string, kind: string, payload: unknown): TeamsOut => ({ computer: "", key, kind, payload });
const publish = (view: TeamList) => store.apply([entry("list", "list", view)]);
function session(team: TeamSummary): Session {
  return toSession("", `lead:${team.id}`, { ...presentation.ready as unknown as ViewRow,
    name: `codex/unrelated/${team.id}`, title: "Lead display title", launch_id: team.lead_session!.session_id.slice("launch_id:".length),
    session_id: "native-id-is-not-the-launch", run_id: "unrelated-run" });
}
function show(component = TeamsScreen) {
  const host = document.createElement("div"); document.body.append(host);
  disposers.push(render(() => createComponent(component, {}), host)); return host;
}
const button = (host: HTMLElement, text: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === text)!;
beforeEach(async () => {
  vi.resetModules(); f.rows = []; f.pages = []; f.follows = []; f.calls = [];
  ({ teams: store } = await import("../../store/teams")); ui = await import("../../store/ui");
  nav = await import("../teams/lead-navigation"); ({ TeamsScreen } = await import("./Teams"));
  ({ toSession } = await import("../../store/catalog")); ui.setPage("teams"); ui.setSelectedKey(undefined); ui.setDetailTab("activity");
});
afterEach(() => { while (disposers.length) disposers.pop()!(); document.body.replaceChildren(); });

it("renders the real running and stuck list projections, without another follow or per-team read", () => {
  f.rows = [session(data.running.teams[0])]; publish(data.running);
  const host = show();
  expect(host.textContent).toContain("Fixes"); expect(host.textContent).toContain("Example project");
  expect(host.textContent).toContain("running"); expect(host.textContent).toContain("codex/unrelated/");
  expect(host.textContent).toContain("1/3"); expect(host.textContent).toContain("?1"); expect(host.textContent).toContain("4 members");
  expect(host.querySelector('[role="progressbar"]')?.getAttribute("aria-valuenow")).toBe("1");
  expect(host.textContent).not.toContain("Stuck"); publish(data.stuck); expect(host.textContent).toContain("Stuck");
  expect(f.follows).toEqual([]); expect(f.pages).toEqual([]); expect(f.calls).toEqual([]);
});

it("browses every real cursor beyond the five-page lookup cache, then returns to the first page", async () => {
  publish(data.pages[0]); const host = show();
  for (let i = 1; i < data.pages.length; i++) {
    button(host, "More teams").click();
    expect(f.pages.at(-1)).toEqual(["list", data.pages[i - 1].next]);
    expect(button(host, "More teams").disabled).toBe(true);
    publish(data.pages[i]); await Promise.resolve();
    expect(host.querySelectorAll("[data-team-row]")).toHaveLength(data.pages[i].teams.length);
    if (data.pages[i].teams.length) expect(host.textContent).toContain(data.pages[i].teams[0].name);
    expect(store.listPages().length).toBeLessThanOrEqual(5);
  }
  expect(host.textContent).toContain("No teams on this page."); expect(host.textContent).not.toContain("No teams yet.");
  expect(button(host, "More teams")).toBeUndefined();
  button(host, "First page").click(); expect(f.pages.at(-1)).toEqual(["list", null]);
  publish(data.pages[0]); expect(host.textContent).toContain("Team 1");
  expect(f.follows).toEqual([]);
});

it("opens a seventh-page lead on its Team tab and keeps that binding after restoring the first page", async () => {
  publish(data.pages[0]); const host = show();
  for (let i = 1; i <= 6; i++) { button(host, "More teams").click(); publish(data.pages[i]); await Promise.resolve(); }
  const team = data.pages[6].teams[0]; f.rows = [session(team)];
  createRoot((dispose) => { disposers.push(dispose); nav.watchTeamNavigation(); });
  host.querySelector<HTMLButtonElement>("[data-team-row]")!.click();
  expect(ui.page()).toBe("sessions"); expect(ui.selectedKey()).toBe(f.rows[0].key); expect(ui.detailTab()).toBe("team");
  expect(nav.navigationTeamFor(f.rows[0])).toBe(team.id);
  await store.firstListPage(); publish(data.pages[0]);
  expect(store.listPages().some((row) => row.id === team.id)).toBe(false);
  const { TeamTab } = await import("../teams/TeamTab"); const tab = show(TeamTab);
  await vi.waitFor(() => expect(store.followed()).toBe(team.id));
  store.apply([entry(`team:${team.id}`, "team", data.team)]);
  await vi.waitFor(() => expect(tab.querySelector("[data-team-tab]")).not.toBeNull());
  expect(tab.textContent).toContain("Team 7");
  ui.setDetailTab("activity"); expect(nav.navigationTeamFor(f.rows[0])).toBeUndefined();
});

it("refuses synthetic lookalikes with another machine or only the native session id", () => {
  const team = data.running.teams[0]; const row = session(team); publish(data.running);
  // Deliberately corrupted Session inputs: no hand-authored Teams result rows.
  for (const changed of [{ ...row, computer: "another" }, { ...row, launchId: row.sessionId, runId: team.lead_session!.session_id.slice(10) }]) {
    f.rows = [changed]; const host = show(); host.querySelector<HTMLButtonElement>("[data-team-row]")!.click();
    expect(host.textContent).toContain("The team's lead session is not on the session list.");
    expect(ui.selectedKey()).toBeUndefined();
  }
});

it("fences an explicit opening if the fresh team view no longer binds this lead", async () => {
  const team = data.pages[6].teams[0]; publish(data.pages[0]);
  // The row is stale, not the product result: a later launch cannot be adopted.
  const stale = { ...session(team), launchId: "stale-launch" }; f.rows = [stale];
  expect(nav.openTeamLead(team, stale)).toBe(false);
  f.rows = [session(team)]; expect(nav.openTeamLead(team, f.rows[0])).toBe(true);
  const { TeamTab } = await import("../teams/TeamTab"); const host = show(TeamTab);
  await vi.waitFor(() => expect(store.followed()).toBe(team.id));
  // A real view of another team's members, relabelled as a stale-cache fault.
  const changed = structuredClone(data.team); changed.members.find((m) => m.id === changed.bar.lead)!.session = data.running.teams[0].lead_session!;
  store.apply([entry(`team:${team.id}`, "team", changed)]);
  await vi.waitFor(() => expect(host.textContent).toContain("The team's lead session has changed."));
  expect(host.querySelector("[data-team-tab]")).toBeNull();
});

it("keeps New team available in the header and in the empty state", async () => {
  publish(data.empty); const host = show(); expect(host.textContent).toContain("No teams yet.");
  const buttons = [...host.querySelectorAll<HTMLButtonElement>("button")].filter((b) => b.textContent === "New team");
  expect(buttons).toHaveLength(2); buttons[1].click();
  await vi.waitFor(() => expect(store.newTeamOpen()).toBe(true)); expect(f.calls).toContain("teams_catalog");
});

it.each(["teams_inactive", "unavailable"])("hides cached rows and disables creation after %s", async (code) => {
  publish(data.running); const host = show(); store.apply([entry("list", "failed", { code, message: "fixture refusal" })]);
  expect(host.querySelector("[data-team-row]")).toBeNull(); expect(button(host, "New team").disabled).toBe(true);
  expect(host.textContent).toContain(code === "unavailable" ? "Teams are unavailable" : "Teams are off");
  expect(store.listPage()).toBeUndefined();
});

it("clears the bounded navigation when its selected session or preview changes", () => {
  const team = data.running.teams[0]; f.rows = [session(team)]; publish(data.running);
  createRoot((dispose) => { disposers.push(dispose); nav.watchTeamNavigation(); });
  expect(nav.openTeamLead(team, f.rows[0])).toBe(true); expect(nav.navigationTeamFor(f.rows[0])).toBe(team.id);
  ui.setSelectedKey("elsewhere"); expect(nav.navigationTeamFor(f.rows[0])).toBeUndefined();
  nav.openTeamLead(team, f.rows[0]); store.apply([entry("list", "failed", { code: "teams_inactive", message: "off" })]);
  expect(nav.navigationTeamFor(f.rows[0])).toBeUndefined();
});

it("mounts the actual lazy Teams route from the application's rail", async () => {
  publish(data.running);
  const { App } = await import("../../App");
  const host = show(() => createComponent(App, { mode: () => "replay" }));
  await vi.waitFor(() => expect(host.querySelector("[data-teams-page]")).not.toBeNull());
  expect(host.querySelector("[data-team-row]")?.textContent).toContain("Fixes");
  const rail = host.querySelector('button[aria-label="Teams"]') as HTMLButtonElement;
  expect(rail.getAttribute("aria-current")).toBe("page");
  const waiting = host.querySelector('button[aria-label="Waiting for you"]') as HTMLButtonElement;
  waiting.click(); expect(ui.page()).toBe("waiting");
  rail.click(); expect(ui.page()).toBe("teams");
  await vi.waitFor(() => expect(host.querySelector("[data-teams-page]")).not.toBeNull());
});
