// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import { teamView } from "./fixtures";
import providers from "../../../core/src/teams/fixtures/providers.json";
import status from "../../../core/src/teams/fixtures/cli-status.json";
import list from "../../../../crates/team-plan/fixtures/cli-list.json";
import { completeSettings } from "./team-settings";
const writtenSettings = () => { const value = structuredClone(status.team.settings); if (!completeSettings(value)) throw new Error("Incomplete product fixture"); return value; };
import type { TeamFailure, TeamOfSession, TeamView, UnresolvedCall } from "../../store/teams";

const f = vi.hoisted(() => ({
  selected: (() => undefined) as () => { key: string; computer: string; launchId: string; sessionId: string; runId: string } | undefined,
  choose: (_row: unknown) => {}, lookup: vi.fn(), close: vi.fn(), open: vi.fn(), run: vi.fn(), check: vi.fn(), dismiss: vi.fn(), clear: vi.fn(), probe: vi.fn(), hidden: vi.fn(), busy: vi.fn((_team?: string) => false),
  providers: vi.fn(), listed: vi.fn<() => typeof list.rows>(),
  setBinding: (_value: TeamOfSession) => {}, setTeam: (_value: TeamView | undefined) => {}, setFailure: (_value: TeamFailure | null) => {}, setUnresolved: (_value: UnresolvedCall[]) => {},
}));
vi.mock("../../store/ui", async () => {
  const { createSignal } = await import("solid-js");
  const [row, choose] = createSignal<ReturnType<typeof f.selected>>(); f.selected = row; f.choose = choose as typeof f.choose;
  return { selected: row, page: () => "sessions", detailTab: () => "team", setPage: vi.fn(), setMenu: vi.fn(), menu: () => undefined };
});
vi.mock("../../store/catalog", () => ({ agentKind: (agent: string) => agent }));
vi.mock("../../store/machines", () => ({ machineCards: { workstation: { machine_id: "machine-1" } } }));
vi.mock("../../store/teams", async () => {
  const { createSignal } = await import("solid-js");
  const [binding, setBinding] = createSignal<TeamOfSession>({ state: "unknown" });
  const [team, setTeam] = createSignal<TeamView>(); const [failure, setFailure] = createSignal<TeamFailure | null>(null); const [unresolved, setUnresolved] = createSignal<UnresolvedCall[]>([]);
  Object.assign(f, { setBinding, setTeam, setFailure, setUnresolved });
  return { newRequestId: () => "request-1", teams: {
    probe: f.probe, hidden: f.hidden, teamOfSession: (identity: unknown) => { f.lookup(identity); return binding(); },
    listed: f.listed, team, failure, failureFor: (source: string) => failure()?.source === source ? failure() : null, readFailureFor: (key: string) => failure()?.key === key ? failure() : null, unresolved,
    closeTeam: async () => { f.close(); setTeam(undefined); }, openTeam: async (id: string) => { f.open(id); },
    run: f.run, check: f.check, dismiss: f.dismiss, clearFailure: f.clear, busy: f.busy, page: vi.fn(), confirmation: () => null, cancelConfirmation: vi.fn(),
  } };
});
vi.mock("../../store/teamProviders", () => ({ readProviderConfig: f.providers }));
vi.mock("../sessions/SessionCard", () => ({ AgentTile: () => null }));
import { TeamTab } from "./TeamTab";
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; f.choose(undefined); f.setBinding({ state: "unknown" }); f.setTeam(undefined); f.setFailure(null); f.setUnresolved([]); document.body.replaceChildren(); vi.clearAllMocks(); f.busy.mockImplementation(() => false); });
const select = (id: string) => f.choose({ key: id, computer: "workstation", launchId: `launch-${id}`, sessionId: "not-a-launch-id", runId: `run-${id}` });
function setup() { f.listed.mockReturnValue(list.rows); const host = document.createElement("div"); document.body.append(host); dispose = render(() => createComponent(TeamTab, {}), host); return host; }

it.each(["settings", "validation_config"])("sends settings from the actual Team tab and blocks unresolved %s", async (kind) => {
  select("a"); f.setBinding({ state: "lead", team: teamView.bar.id }); const host = setup();
  await vi.waitFor(() => expect(f.open).toHaveBeenCalled());
  f.setTeam({ ...teamView, settings: writtenSettings() });
  await vi.waitFor(() => expect(host.querySelector("[data-team-settings-open]")).not.toBeNull());
  f.run.mockResolvedValueOnce({ outcome: "uncertain", request_id: "request-1", detail: "Read the team before sending it again." });
  host.querySelector<HTMLButtonElement>("[data-team-settings-open]")!.click();
  const routing = host.querySelector<HTMLSelectElement>("[data-setting=routing]")!;
  routing.value = "lead"; routing.dispatchEvent(new Event("change", { bubbles: true }));
  host.querySelector<HTMLButtonElement>("[data-settings-save]")!.click();
  await vi.waitFor(() => expect(f.run).toHaveBeenCalledOnce());
  const call = f.run.mock.calls[0][0];
  expect(call).toEqual({ team: teamView.bar.id, team_name: "Fixes", expected_revision: 16, request_id: "request-1", action: { kind: "settings", settings: { ...status.team.settings, routing: "lead" } } });
  const held = kind === "settings" ? call : { ...call, action: { kind, selection: { source_digest: providers.inspection.source_digest, digest: providers.inspection.digest, override_config: null } } };
  f.setUnresolved([{ call: held, detail: "Read the team before sending it again.", state: "uncertain" }]);
  await vi.waitFor(() => expect(host.querySelector("[data-team-settings]")).toBeNull());
  expect(host.querySelector<HTMLButtonElement>("[data-team-settings-open]")!.disabled).toBe(true);
  host.querySelector<HTMLButtonElement>("[data-team-request] button")!.click();
  expect(f.check.mock.calls[0][0].call).toEqual(held); expect(f.run).toHaveBeenCalledOnce();
  host.querySelectorAll<HTMLButtonElement>("[data-team-request] button")[1].click();
  expect(f.dismiss.mock.calls[0][0].call).toEqual(held);
});

it("closes a settings draft on a session change and never sends it to the next team", async () => {
  select("a"); f.setBinding({ state: "lead", team: teamView.bar.id }); const host = setup();
  await vi.waitFor(() => expect(f.open).toHaveBeenCalled()); f.setTeam({ ...teamView, settings: writtenSettings() });
  await vi.waitFor(() => expect(host.querySelector("[data-team-settings-open]")).not.toBeNull());
  host.querySelector<HTMLButtonElement>("[data-team-settings-open]")!.click();
  expect(host.querySelector("[data-team-settings]")).not.toBeNull(); select("b");
  expect(host.querySelector("[data-team-settings]")).toBeNull(); expect(f.run).not.toHaveBeenCalled();
});

it("uses the displayed team's in-flight lock for Settings and its open dialog", async () => {
  select("a"); f.setBinding({ state: "lead", team: teamView.bar.id });
  // Another team is in flight, so the legacy global accessor is busy.
  f.busy.mockImplementation((id?: string) => id === undefined || id === "other-team");
  const host = setup(); await vi.waitFor(() => expect(f.open).toHaveBeenCalled());
  f.setTeam({ ...teamView, settings: writtenSettings() });
  await vi.waitFor(() => expect(host.querySelector("[data-team-settings-open]")).not.toBeNull());
  expect(f.busy).toHaveBeenCalledWith(teamView.bar.id);
  const open = host.querySelector<HTMLButtonElement>("[data-team-settings-open]")!;
  expect(open.disabled).toBe(false); open.click();
  const routing = host.querySelector<HTMLSelectElement>("[data-setting=routing]")!;
  routing.value = "lead"; routing.dispatchEvent(new Event("change", { bubbles: true }));
  expect(host.querySelector<HTMLButtonElement>("[data-settings-save]")!.disabled).toBe(false);
  expect(f.run).not.toHaveBeenCalled();
});

it("reads provider metadata for a real Team view whose home is supplied by the list", async () => {
  select("a"); f.setBinding({ state: "lead", team: teamView.bar.id }); const host = setup();
  await vi.waitFor(() => expect(f.open).toHaveBeenCalled());
  const revision = providers.inspection.revision;
  f.setTeam({ ...teamView, revision, bar: { ...teamView.bar, revision }, settings: writtenSettings() });
  f.providers.mockResolvedValue(structuredClone(providers.inspection));
  host.querySelector<HTMLButtonElement>("[data-team-settings-open]")!.click();
  await vi.waitFor(() => expect(host.querySelector("[data-provider-source]")?.textContent).toContain("Detected from project metadata"));
  expect(f.providers).toHaveBeenCalledWith({ team: teamView.bar.id });
  host.querySelector<HTMLButtonElement>("[data-provider-check]")!.click();
  await vi.waitFor(() => expect(host.querySelector<HTMLButtonElement>("[data-provider-save]")!.disabled).toBe(false));
  expect(f.run).not.toHaveBeenCalled();
});

it.each(["id", "project"] as const)("does not use a list row with another %s for provider inspection", async (field) => {
  select("a"); f.setBinding({ state: "lead", team: teamView.bar.id }); const host = setup();
  await vi.waitFor(() => expect(f.open).toHaveBeenCalled());
  f.setTeam({ ...teamView, settings: writtenSettings() });
  const foreign = structuredClone(list.rows); foreign[0][field] = "foreign";
  f.listed.mockReturnValue(foreign);
  host.querySelector<HTMLButtonElement>("[data-team-settings-open]")!.click();
  await vi.waitFor(() => expect(host.textContent).toContain("The team’s home computer is missing from the loaded team list."));
  expect(f.providers).not.toHaveBeenCalled(); expect(f.run).not.toHaveBeenCalled();
});
