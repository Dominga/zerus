// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import { teamView } from "./fixtures";
import type { TeamFailure, TeamOfSession, TeamView, UnresolvedCall } from "../../store/teams";

const f = vi.hoisted(() => ({
  selected: (() => undefined) as () => { key: string; computer: string; launchId: string; sessionId: string; runId: string } | undefined,
  choose: (_row: unknown) => {}, lookup: vi.fn(), close: vi.fn(), open: vi.fn(), run: vi.fn(), check: vi.fn(), dismiss: vi.fn(), clear: vi.fn(), probe: vi.fn(), hidden: vi.fn(),
  setBinding: (_value: TeamOfSession) => {}, setTeam: (_value: TeamView | undefined) => {}, setFailure: (_value: TeamFailure | null) => {}, setUnresolved: (_value: UnresolvedCall[]) => {},
}));
vi.mock("../../store/ui", async () => {
  const { createSignal } = await import("solid-js");
  const [row, choose] = createSignal<ReturnType<typeof f.selected>>(); f.selected = row; f.choose = choose as typeof f.choose;
  return { selected: row, page: () => "sessions", detailTab: () => "team", setPage: vi.fn(), setMenu: vi.fn(), menu: () => undefined };
});
vi.mock("../../store/taskNavigation", () => ({ openTask: vi.fn() }));
vi.mock("../../store/catalog", () => ({ agentKind: (agent: string) => agent }));
vi.mock("../../store/machines", () => ({ machineCards: { workstation: { machine_id: "machine-1" } } }));
vi.mock("../../store/teams", async () => {
  const { createSignal } = await import("solid-js");
  const [binding, setBinding] = createSignal<TeamOfSession>({ state: "unknown" });
  const [team, setTeam] = createSignal<TeamView>(); const [failure, setFailure] = createSignal<TeamFailure | null>(null); const [unresolved, setUnresolved] = createSignal<UnresolvedCall[]>([]);
  Object.assign(f, { setBinding, setTeam, setFailure, setUnresolved });
  return { newRequestId: () => "request-1", teams: {
    probe: f.probe, hidden: f.hidden, teamOfSession: (identity: unknown) => { f.lookup(identity); return binding(); },
    team, failure, failureFor: (source: string) => failure()?.source === source ? failure() : null, readFailureFor: (key: string) => failure()?.key === key ? failure() : null, unresolved,
    closeTeam: async () => { f.close(); setTeam(undefined); }, openTeam: async (id: string) => { f.open(id); },
    run: f.run, check: f.check, dismiss: f.dismiss, clearFailure: f.clear, busy: () => false, page: vi.fn(), confirmation: () => null, cancelConfirmation: vi.fn(),
  } };
});
vi.mock("../sessions/SessionCard", () => ({ AgentTile: () => null }));
import { TeamTab } from "./TeamTab";
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; f.choose(undefined); f.setBinding({ state: "unknown" }); f.setTeam(undefined); f.setFailure(null); f.setUnresolved([]); document.body.replaceChildren(); vi.clearAllMocks(); });
const select = (id: string) => f.choose({ key: id, computer: "workstation", launchId: `launch-${id}`, sessionId: "not-a-launch-id", runId: `run-${id}` });
function setup() { const host = document.createElement("div"); document.body.append(host); dispose = render(() => createComponent(TeamTab, {}), host); return host; }

it("leaves discovery and document visibility to the application", () => {
  select("a"); setup(); select("b");
  document.dispatchEvent(new Event("visibilitychange"));
  expect(f.probe).not.toHaveBeenCalled(); expect(f.hidden).not.toHaveBeenCalled();
});

it("reacts to selection and list changes using machine/launch identity, retiring old actions (review 4)", async () => {
  select("a"); const host = setup();
  expect(f.lookup).toHaveBeenLastCalledWith({ machine: "machine-1", launchId: "launch-a", runId: "run-a" });
  expect(host.textContent).toContain("Finding the team");
  f.setBinding({ state: "lead", team: teamView.bar.id }); await vi.waitFor(() => expect(f.open).toHaveBeenCalledWith(teamView.bar.id));
  f.setTeam(teamView); await vi.waitFor(() => expect(host.querySelector("[data-team-tab]")).not.toBeNull());
  const pause = host.querySelector<HTMLButtonElement>('[data-strip-action="pause"]')!; pause.click();
  expect(f.run.mock.calls.at(-1)?.[0].team).toBe(teamView.bar.id);
  select("b");
  expect(host.querySelector("[data-team-tab]")).toBeNull();
  pause.click(); expect(f.run).toHaveBeenCalledTimes(1);
  expect(f.lookup).toHaveBeenLastCalledWith({ machine: "machine-1", launchId: "launch-b", runId: "run-b" });
  f.setBinding({ state: "lead", team: "team-b" });
  await vi.waitFor(() => expect(f.open).toHaveBeenCalledWith("team-b"));
  f.setTeam({ ...teamView, bar: { ...teamView.bar, id: "team-b", name: "Team B" } });
  await vi.waitFor(() => expect(host.querySelector('[data-strip-action="pause"]')).not.toBeNull());
  host.querySelector<HTMLButtonElement>('[data-strip-action="pause"]')!.click();
  expect(f.run.mock.calls.at(-1)?.[0].team).toBe("team-b");
  f.setBinding({ state: "none" }); await vi.waitFor(() => expect(host.textContent).toContain("not a team lead"));
});

it("renders the asking view's failed outcome verbatim and fences late reasons from another selection (review 5)", async () => {
  select("a"); f.setBinding({ state: "lead", team: teamView.bar.id }); const host = setup();
  await vi.waitFor(() => expect(f.open).toHaveBeenCalled()); f.setTeam(teamView);
  await vi.waitFor(() => expect(host.querySelector('[data-strip-action="pause"]')).not.toBeNull());
  host.querySelector<HTMLButtonElement>('[data-strip-action="pause"]')!.click();
  const source = f.run.mock.calls.at(-1)![1] as string;
  f.setFailure({ key: "action", code: "refused", message: "This member cannot be paused now.", source });
  expect(host.querySelector("[data-team-action-failure]")?.textContent).toBe("This member cannot be paused now.");
  select("b"); expect(host.textContent).not.toContain("This member cannot be paused now.");
  f.setFailure({ key: "action", code: "refused", message: "Late refusal from a", source });
  expect(host.textContent).not.toContain("Late refusal from a");
});

it("keeps every uncertain/pending request on its own Check/Dismiss callbacks", () => {
  select("a");
  const entries: UnresolvedCall[] = ["one", "two"].map((id) => ({ call: { team: "team-a", team_name: "Example", request_id: id, action: { kind: "pause" } }, detail: id, state: id === "one" ? "uncertain" : "pending" }));
  f.setUnresolved(entries); const host = setup();
  const rows = host.querySelectorAll<HTMLElement>("[data-team-request]"); expect(rows).toHaveLength(2);
  rows[1].querySelector<HTMLButtonElement>("button")!.click(); expect(f.check.mock.calls.at(-1)?.[0]).toBe(entries[1]);
  rows[0].querySelectorAll<HTMLButtonElement>("button")[1].click(); expect(f.dismiss.mock.calls.at(-1)?.[0]).toBe(entries[0]);
});

it("waits for a verified machine UUID instead of doing an unscoped lookup", () => {
  f.choose({ key: "missing-machine", computer: "unknown", launchId: "launch-a", sessionId: "different-value", runId: "run-a" });
  const host = setup(); expect(host.textContent).toContain("Finding the team"); expect(f.lookup).not.toHaveBeenCalled();
});
