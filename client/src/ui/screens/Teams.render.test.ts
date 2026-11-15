// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent, createRoot, createSignal, Show } from "solid-js";
import { render } from "solid-js/web";
import fixture from "./fixtures/teams-screen.json";
import rebind from "./fixtures/teams-rebind.json";
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
  return toSession("", `lead:${team.lead_session!.session_id}`, { ...presentation.ready as unknown as ViewRow,
    name: `codex/unrelated/${team.id}`, title: "Lead display title", launch_id: team.lead_session!.session_id.slice("launch_id:".length),
    session_id: "native-id-is-not-the-launch", run_id: "unrelated-run" });
}
function show(component = TeamsScreen) {
  const host = document.createElement("div"); document.body.append(host);
  disposers.push(render(() => createComponent(component, {}), host)); return host;
}
const button = (host: HTMLElement, text: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === text)!;
beforeEach(async (context) => {
  vi.resetModules(); f.rows = []; f.pages = []; f.follows = []; f.calls = [];
  ({ teams: store } = await import("../../store/teams")); ui = await import("../../store/ui");
  nav = await import("../teams/lead-navigation"); ({ TeamsScreen } = await import("./Teams"));
  ({ toSession } = await import("../../store/catalog")); ui.setPage("teams"); ui.setSelectedKey(undefined); ui.setDetailTab("activity");
  if (context.task.name.includes("Russian")) {
    const preferences = await import("../../store/settings"); await preferences.settingsReady;
    await preferences.setSetting("workspace/language", "ru", { persist: false });
    const language = await import("../../i18n"); language.installUiLanguage();
    await vi.waitFor(() => expect(language.uiLanguage()).toBe("ru"));
  }
});
afterEach(() => { while (disposers.length) disposers.pop()!(); document.body.replaceChildren(); });

it("renders the real running and stuck list projections, without another follow or per-team read", () => {
  f.rows = [session(data.running.teams[0])]; publish(data.running);
  const host = show();
  expect(host.textContent).toContain("Fixes");
  expect(host.textContent).toContain("Running"); expect(host.textContent).toContain("codex/unrelated/");
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
  // Leave the actual screen; the component must restore its reader itself.
  disposers.shift()!(); host.remove();
  expect(f.pages.at(-1)).toEqual(["list", null]); publish(data.pages[0]);
  expect(store.listPages().some((row) => row.id === team.id)).toBe(false);
  const { TeamTab } = await import("../teams/TeamTab"); const tab = show(TeamTab);
  await vi.waitFor(() => expect(store.followed()).toBe(team.id));
  store.apply([entry(`team:${team.id}`, "team", data.team)]);
  await vi.waitFor(() => expect(tab.querySelector("[data-team-tab]")).not.toBeNull());
  expect(tab.textContent).toContain("Team 7");
  disposers.pop()!(); tab.remove(); ui.setDetailTab("activity");
  await vi.waitFor(() => expect(store.followed()).toBeUndefined());
  expect(nav.navigationTeamFor(f.rows[0])).toBe(team.id);
  ui.setDetailTab("team"); const again = show(TeamTab);
  await vi.waitFor(() => expect(store.followed()).toBe(team.id));
  store.apply([entry(`team:${team.id}`, "team", data.team)]);
  await vi.waitFor(() => expect(again.querySelector("[data-team-tab]")).not.toBeNull());
  expect(again.textContent).not.toContain("Finding the team");
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

it("fences an explicit opening when product-written unbind and rebind rows change its lead", async () => {
  const before = rebind.before_list as unknown as TeamList, after = rebind.after_list as unknown as TeamList;
  const original = session(before.teams[0]), replacement = session(after.teams[0]);
  publish(before); f.rows = [original, replacement];
  createRoot((dispose) => { disposers.push(dispose); nav.watchTeamNavigation(); });
  expect(nav.openTeamLead(before.teams[0], original)).toBe(true);
  const { TeamTab } = await import("../teams/TeamTab"); const host = show(TeamTab);
  await vi.waitFor(() => expect(store.followed()).toBe(before.teams[0].id));
  store.apply([entry(`team:${before.teams[0].id}`, "team", rebind.before_team)]);
  await vi.waitFor(() => expect(host.querySelector("[data-team-tab]")).not.toBeNull());
  store.apply([entry(`team:${before.teams[0].id}`, "team", rebind.after_team)]);
  await vi.waitFor(() => expect(host.textContent).toContain("The team's lead session has changed."));
  expect(host.querySelector("[data-team-tab]")).toBeNull(); expect(ui.selectedKey()).toBe(original.key);
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

it("restores the first-page reader on mount without a helper call from the test", async () => {
  publish(data.pages[0]); await store.page("list", data.pages[1].after!); publish(data.pages[1]); f.pages = [];
  show(); await vi.waitFor(() => expect(f.pages).toEqual([["list", null]]));
});

it("restores the first-page reader when the screen unmounts", async () => {
  publish(data.pages[0]); const host = show(); button(host, "More teams").click(); publish(data.pages[1]);
  await Promise.resolve(); f.pages = []; disposers.pop()!();
  await vi.waitFor(() => expect(f.pages).toEqual([["list", null]]));
});

it("re-enables paging after a new read failure while keeping the old rows", async () => {
  publish(data.pages[0]); const host = show(); button(host, "More teams").click();
  expect(button(host, "More teams").disabled).toBe(true);
  store.apply([entry("list", "failed", { code: "invalid_reply", message: "fixture read refused" })]);
  await vi.waitFor(() => expect(button(host, "More teams").disabled).toBe(false));
  expect(host.querySelector('[role="alert"]')?.textContent).toContain("fixture read refused");
  expect(host.querySelectorAll("[data-team-row]")).toHaveLength(1);
});

it("refuses a self-repeating cursor injected into a product-written page", () => {
  publish(data.pages[0]); const host = show(); button(host, "More teams").click();
  // Protocol fault injection changes cursor metadata, never a Teams row.
  publish({ ...data.pages[1], next: data.pages[1].after! });
  expect(button(host, "More teams")).toBeUndefined(); expect(button(host, "First page")).toBeDefined();
});

it("omits the phase metric for a single queue phase", () => {
  publish(data.pages[0]); const host = show();
  expect(data.pages[0].teams[0].progress?.phases).toBe(1);
  expect(host.textContent).not.toContain("Phases finished:");
});

it("omits the question marker when the product reports zero questions", () => {
  publish(data.pages[0]); const host = show();
  expect(data.pages[0].teams[0].progress?.questions).toBe(0);
  expect(host.querySelector(".teams-questions")).toBeNull(); expect(host.textContent).not.toContain("?0");
});

it("rejects a navigation record after a same-key synthetic Session identity replacement", () => {
  const team = data.running.teams[0], row = session(team); publish(data.running); f.rows = [row];
  expect(nav.openTeamLead(team, row)).toBe(true);
  // Deliberately changed Session inputs; the listed team is product-written.
  expect(nav.navigationTeamFor({ ...row, launchId: "replacement-launch" })).toBeUndefined();
  expect(nav.navigationTeamFor({ ...row, computer: "another-computer" })).toBeUndefined();
});

it("reports a product-unbound lead separately from a missing session-list row", () => {
  const view = rebind.between_list as unknown as TeamList;
  publish(view); const host = show(); host.querySelector<HTMLButtonElement>("[data-team-row]")!.click();
  expect(host.textContent).toContain("The team's lead has no session yet.");
  expect(ui.selectedKey()).toBeUndefined();
});

it("uses Russian team statuses consistently in the list, session strip and Team tab bar", async () => {
  const { tr } = await import("../../i18n"); publish(data.running); const host = show();
  expect(host.querySelector(".tm-state-badge")?.textContent).toBe(tr("Running"));
  expect(host.querySelector(".teams-row-meta .dim")?.textContent).toBe("Лид");
  expect(host.querySelector(".teams-member-count")?.textContent).toBe(tr("%n members", 4));
  const { TeamStrip } = await import("../sessions/TeamStrip");
  const strip = show(() => createComponent(TeamStrip, { team: data.running.teams[0], onOpen() {} }));
  expect(strip.querySelector(".sc-team-status")?.textContent).toBe(tr("Running"));
  const { TeamBar } = await import("../teams/team-bar");
  const bar = show(() => createComponent(TeamBar, { view: data.team, busy: false, width: 1000, onAction() {} }));
  expect(bar.querySelector("[data-state]")?.textContent).toBe(tr("Running"));
  const { MemberCard } = await import("../teams/MemberCard");
  const card = show(() => createComponent(MemberCard, { member: data.team.members.find((member) => member.state === "not_created")!, teamStatus: data.team.bar.status, busy: false, onAction() {} }));
  expect(card.querySelector(".team-member-state")?.textContent).toBe("Не создан");
});

it("offers sibling header and row action hooks without opening the lead from a drawer control", () => {
  const [opened, setOpened] = createSignal<string>(); let catalogs = 0;
  const team = data.pages[0].teams[0]; f.rows = [session(team)]; publish(data.pages[0]);
  const host = show(() => createComponent(TeamsScreen, {
    get headerActions() {
      const action = document.createElement("button"); action.textContent = "Catalog"; action.onclick = () => { catalogs++; }; return action;
    },
    rowActions: (team: TeamSummary) => {
      const action = document.createElement("button"); action.textContent = "Drawer"; action.onclick = () => setOpened(team.id); return action;
    },
    rowDetails: (team: TeamSummary) => createComponent(Show, { keyed: true, get when() { return opened() === team.id; }, children: "Team drawer content" }),
  }));
  expect(button(host, "Catalog")).toBeDefined(); button(host, "Catalog").click(); expect(catalogs).toBe(1); expect(store.newTeamOpen()).toBe(false);
  const drawer = button(host, "Drawer"); expect(drawer.parentElement!.closest("button")).toBeNull();
  drawer.click(); expect(host.textContent).toContain("Team drawer content"); expect(ui.selectedKey()).toBeUndefined();
  host.querySelector<HTMLButtonElement>("[data-team-row]")!.click(); expect(ui.selectedKey()).toBe(f.rows[0].key);
});

it("opens the new product-bound lead from a refreshed list rather than its old session", () => {
  const before = rebind.before_list as unknown as TeamList, after = rebind.after_list as unknown as TeamList;
  const old = session(before.teams[0]), fresh = session(after.teams[0]); f.rows = [old, fresh];
  publish(after); const host = show(); host.querySelector<HTMLButtonElement>("[data-team-row]")!.click();
  expect(ui.selectedKey()).toBe(fresh.key); expect(nav.navigationTeamFor(old)).toBeUndefined();
});
