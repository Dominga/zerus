// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import { createTeams, type TeamCall, type TeamOutcome, type TeamsTransport, type TeamView } from "../../store/teams";
import { TeamWorkspace } from "./TeamTab";
import { teamView } from "./fixtures";
import { completeSettings } from "./team-settings";
import status from "../../../core/src/teams/fixtures/cli-status.json";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });

// Adapted from the independent review158 store/workspace probe.
it("REVIEW158 contract: team A's in-flight settings write does not lock team B", async () => {
  const settings = structuredClone(status.team.settings);
  if (!completeSettings(settings)) throw new Error("Incomplete product fixture");
  const teamA = teamView.bar.id;
  const other = "00000000-0000-4000-8000-00000000beee";
  let finish!: (outcome: TeamOutcome) => void;
  const act = vi.fn((_call: TeamCall) => new Promise<TeamOutcome>((resolve) => { finish = resolve; }));
  const transport: TeamsTransport = {
    follow: vi.fn(async () => {}), hidden: vi.fn(async () => {}), tasksChanged: vi.fn(async () => {}), page: vi.fn(async () => {}),
    act, actAgain: act, dismiss: vi.fn(async () => {}), catalog: vi.fn(async () => ({})), saveTemplate: vi.fn(async () => {}), project: vi.fn(async () => ({})), preview: vi.fn(async () => ({})),
  };
  const teams = createTeams(transport);
  const callA: TeamCall = { team: teamA, expected_revision: 16, request_id: "00000000-0000-4000-8000-0000000000a1", action: { kind: "settings", settings } };
  const pending = teams.run(callA);
  expect(teams.busy()).toBe(true);
  expect(teams.busy(teamA)).toBe(true);
  expect(teams.busy(other)).toBe(false);
  expect(teams.sending()).toEqual([callA.request_id]);
  const viewB: TeamView = { ...structuredClone(teamView), bar: { ...structuredClone(teamView.bar), id: other }, settings };
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(TeamWorkspace, {
    view: viewB,
    get busy() { return teams.busy(other); },
    failure: null,
    onAction: () => {}, onPage: () => {}, onAddTask: () => {}, onSettingsSave: vi.fn(),
    get settingsBlocked() { return teams.unresolved().some((entry) => entry.call.team === other && entry.call.action.kind === "settings"); },
  }), host);
  try {
    const button = host.querySelector<HTMLButtonElement>("[data-team-settings-open]")!;
    expect(button.disabled).toBe(false); button.click();
    const routing = host.querySelector<HTMLSelectElement>("[data-setting=routing]")!;
    routing.value = "lead"; routing.dispatchEvent(new Event("change", { bubbles: true }));
    expect(host.querySelector<HTMLButtonElement>("[data-settings-save]")!.disabled).toBe(false);
    expect(act).toHaveBeenCalledOnce();
  } finally {
    finish({ outcome: "done", receipt: {}, request_id: callA.request_id });
    await pending;
    expect(teams.busy()).toBe(false);
    expect(teams.busy(teamA)).toBe(false);
    expect(teams.sending()).toEqual([]);
  }
});

it("keeps each team's lock until its own request settles, including a lost reply", async () => {
  const settings = structuredClone(status.team.settings);
  if (!completeSettings(settings)) throw new Error("Incomplete product fixture");
  const pending = new Map<string, { resolve: (outcome: TeamOutcome) => void; reject: (error: Error) => void }>();
  const act = vi.fn((call: TeamCall) => new Promise<TeamOutcome>((resolve, reject) => pending.set(call.request_id, { resolve, reject })));
  const transport: TeamsTransport = {
    follow: vi.fn(async () => {}), hidden: vi.fn(async () => {}), tasksChanged: vi.fn(async () => {}), page: vi.fn(async () => {}),
    act, actAgain: act, dismiss: vi.fn(async () => {}), catalog: vi.fn(async () => ({})), saveTemplate: vi.fn(async () => {}), project: vi.fn(async () => ({})), preview: vi.fn(async () => ({})),
  };
  const teams = createTeams(transport);
  const first: TeamCall = { team: teamView.bar.id, expected_revision: 16, request_id: "00000000-0000-4000-8000-0000000000a1", action: { kind: "settings", settings } };
  const second: TeamCall = { ...first, team: "00000000-0000-4000-8000-00000000beee", request_id: "00000000-0000-4000-8000-0000000000b1" };
  const a = teams.run(first), b = teams.run(second);
  try {
    expect(teams.busy(first.team)).toBe(true); expect(teams.busy(second.team)).toBe(true);
    expect(teams.busy("unrelated-team")).toBe(false);
    pending.get(first.request_id)!.resolve({ outcome: "done", receipt: {}, request_id: first.request_id }); await a;
    expect(teams.busy(first.team)).toBe(false); expect(teams.busy(second.team)).toBe(true); expect(teams.busy()).toBe(true);
    expect(teams.sending()).toEqual([second.request_id]);
    pending.get(second.request_id)!.reject(new Error("Reply lost"));
    expect((await b).outcome).toBe("uncertain");
    expect(teams.busy()).toBe(false); expect(teams.busy(second.team)).toBe(false);
    expect(teams.unresolved().map((entry) => entry.call)).toEqual([second]);
    expect(act).toHaveBeenCalledTimes(2);
  } finally {
    for (const [request_id, deferred] of pending) deferred.resolve({ outcome: "done", receipt: {}, request_id });
    await Promise.all([a, b]);
  }
});
