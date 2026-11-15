import { describe, expect, it } from "vitest";
import { createTeams, newRequestId, type TeamCall, type TeamCatalogTemplate, type TeamFollow, type TeamOutcome, type TeamsTransport, type TeamView } from "./teams";
import { catalog, teamView, TEAM_ID } from "../ui/teams/fixtures";
import type { TeamsOut } from "../types";

// The store alone: a fake transport stands for package 12's commands, and the
// pinned fixture stands for a `team:<id>` reply. No `hgs`, no timer, no Tauri.

const list = {
  teams: [{
    id: TEAM_ID, project: "example-project", name: "Fixes", status: "running", lead: "lead",
    home: "00000000-0000-4000-8000-0000000000aa", revision: 16,
    lead_session: { machine: "00000000-0000-4000-8000-0000000000aa", session_id: "launch_id:00000000-0000-4000-8000-000000000064" },
  }],
  truncated: false,
  next: null,
};

function entry(key: string, kind: string, payload: unknown): TeamsOut {
  return { computer: "", key, kind, payload };
}

function fake() {
  const calls: { follow: TeamFollow[]; hidden: boolean[]; pages: [string, string | null][]; acts: TeamCall[]; again: TeamCall[]; dismissed: TeamCall[] } = {
    follow: [], hidden: [], pages: [], acts: [], again: [], dismissed: [],
  };
  const outcomes: TeamOutcome[] = [];
  const transport = {
    async follow(follow: TeamFollow) { calls.follow.push(follow); },
    async hidden(hidden: boolean) { calls.hidden.push(hidden); },
    async tasksChanged() {},
    async page(key: string, cursor: string | null) { calls.pages.push([key, cursor]); },
    async act(call: TeamCall) { calls.acts.push(call); return outcomes.shift() ?? { outcome: "done", receipt: { ok: true }, request_id: call.request_id }; },
    async actAgain(call: TeamCall) { calls.again.push(call); return outcomes.shift() ?? { outcome: "done", receipt: {}, request_id: call.request_id }; },
    async dismiss(call: TeamCall) { calls.dismissed.push(call); },
    async catalog() { return { ok: true, catalog }; },
    async saveTemplate() {},
    async project() { return { state: "ok", head: "0123456789abcdef0123456789abcdef01234567", branch: "main" }; }, async preview() { return {}; },
  };
  return { transport, calls, outcomes };
}

const call = (kind = "accept", fields: Record<string, unknown> = { node: 2 }): TeamCall => ({
  team: TEAM_ID, team_name: "Fixes", expected_revision: 16, request_id: newRequestId(), action: { kind, ...fields },
});

describe("the Teams store", () => {
  it("takes every view from the teams key and hides the entry points when Teams are off", () => {
    const { transport } = fake();
    const teams = createTeams(transport);
    expect(teams.available()).toBeUndefined();
    teams.apply([entry("list", "failed", { code: "teams_inactive", message: "Teams are off." })]);
    expect(teams.available()).toBe(false);
    expect(teams.off()).toBe("inactive");
    const other = createTeams(transport);
    other.apply([entry("list", "failed", { code: "unavailable", message: "hgs is missing" })]);
    expect(other.available()).toBe(false);
    expect(other.off()).toBe("unavailable");
    const live = createTeams(transport);
    live.apply([entry("list", "list", list)]);
    expect(live.available()).toBe(true);
    expect(live.list()?.teams[0].id).toBe(TEAM_ID);
  });

  it("keeps the shown team when a read fails, and names a cursor reset", async () => {
    const { transport } = fake();
    const teams = createTeams(transport);
    await teams.openTeam(TEAM_ID);
    teams.apply([entry("team:" + TEAM_ID, "team", teamView)]);
    expect(teams.team()?.bar.name).toBe("Fixes");
    teams.apply([entry("team:" + TEAM_ID, "failed", { code: "cursor_reset", message: "The page could not be read; the first page will be shown." })]);
    expect(teams.team()?.bar.name, "the shown view stays").toBe("Fixes");
    expect(teams.failure()?.code).toBe("cursor_reset");
    teams.apply([entry("team:" + TEAM_ID, "ended", null)]);
    expect(teams.team()).toBeUndefined();
  });

  it("follows one team, hides it and reads a page through the commands", async () => {
    const { transport, calls } = fake();
    const teams = createTeams(transport);
    await teams.openTeam(TEAM_ID);
    // The list stays followed beside the team, so a lead lookup can page.
    expect(calls.follow.at(-1)).toEqual({ list: true, waiting: true, team: TEAM_ID });
    await teams.hidden(true);
    expect(calls.hidden.at(-1)).toBe(true);
    await teams.page("team:" + TEAM_ID, "t1.next");
    expect(calls.pages.at(-1)).toEqual(["team:" + TEAM_ID, "t1.next"]);
    await teams.closeTeam();
    expect(calls.follow.at(-1)).toEqual({ list: true, waiting: true, team: null });
  });

  it("has the five outcomes: done, failed, uncertain, confirm and pending", async () => {
    const { transport, outcomes } = fake();
    const teams = createTeams(transport);
    outcomes.push({ outcome: "done", receipt: { ok: true }, request_id: "a" });
    outcomes.push({ outcome: "failed", detail: "refused", code: "stale_revision", request_id: "b" });
    outcomes.push({ outcome: "uncertain", detail: "may have been applied", request_id: "c" });
    outcomes.push({ outcome: "confirm", confirmation: { name: "teamStopConfirm", title: "Stop team?", text: "Stop it?", action: "Stop team" } });
    outcomes.push({ outcome: "pending", request_id: "e" });
    expect((await teams.run(call())).outcome).toBe("done");
    expect((await teams.run(call())).outcome).toBe("failed");
    expect((await teams.run(call())).outcome).toBe("uncertain");
    expect(teams.uncertain()?.detail).toBe("may have been applied");
    expect((await teams.run(call())).outcome).toBe("confirm");
    expect(teams.confirmation()?.confirmation.title).toBe("Stop team?");
    expect((await teams.run(call())).outcome).toBe("pending");
  });

  it("a destructive action runs only after the confirmation, with the same intent", async () => {
    const { transport, calls, outcomes } = fake();
    const teams = createTeams(transport);
    const stop = call("stop");
    outcomes.push({ outcome: "confirm", confirmation: { name: "teamStopConfirm", title: "Stop team?", text: "Stop it?", action: "Stop team" } });
    const first = await teams.run(stop);
    expect(first.outcome).toBe("confirm");
    expect(calls.acts).toHaveLength(1);
    expect(calls.acts[0].confirmed).toBeFalsy();
    outcomes.push({ outcome: "done", receipt: { ok: true }, request_id: stop.request_id });
    await teams.confirm();
    expect(calls.acts).toHaveLength(2);
    expect(calls.acts[1].request_id).toBe(stop.request_id);
    expect(calls.acts[1].confirmed).toBe(true);
    expect(teams.confirmation()).toBeNull();
  });

  it("an uncertain action offers only Check and Dismiss", async () => {
    const { transport, outcomes, calls } = fake();
    const teams = createTeams(transport);
    const act = call();
    outcomes.push({ outcome: "uncertain", detail: "may have been applied", request_id: act.request_id });
    await teams.run(act);
    expect(teams.uncertain()?.call.request_id).toBe(act.request_id);
    outcomes.push({ outcome: "done", receipt: { ok: true }, request_id: act.request_id });
    await teams.check();
    expect(calls.again.at(-1)?.request_id).toBe(act.request_id);
    expect(teams.uncertain()).toBeNull();
    outcomes.push({ outcome: "uncertain", detail: "still unknown", request_id: act.request_id });
    await teams.run(act);
    await teams.dismiss();
    expect(calls.dismissed.at(-1)?.request_id).toBe(act.request_id);
    expect(teams.uncertain()).toBeNull();
  });

  it("loads the catalog lazily, and reads it again after a save for the saved template's digest", async () => {
    const { transport } = fake();
    // The command line's catalog: a save adds the row, and only its catalog read gives the digest.
    let served = structuredClone(catalog);
    let reads = 0;
    transport.catalog = async () => { reads += 1; return { ok: true, catalog: structuredClone(served) }; };
    (transport as TeamsTransport).saveTemplate = async (template: TeamCatalogTemplate) => { served = { ...served, templates: [...served.templates, { ...template, digest: "saved-digest" }] }; };
    const teams = createTeams(transport);
    await teams.openNewTeam();
    expect(teams.newTeamOpen()).toBe(true);
    expect(teams.catalog()?.templates[0].id).toBe("bugfix");
    await teams.openNewTeam();
    expect(reads, "an open catalog is not read again").toBe(1);
    const { digest: _shown, ...row } = catalog.templates[0];
    await teams.saveTemplate({ ...row, id: "bugfix-mine", name: "My bugfix" });
    expect(reads).toBe(2);
    expect(teams.catalog()?.templates.find((template) => template.id === "bugfix-mine")?.digest).toBe("saved-digest");
    // A read that fails keeps the catalog shown and tells why.
    transport.catalog = async () => { throw new Error("read_failed: offline"); };
    expect(await teams.reloadCatalog()).toBe(false);
    expect(teams.catalog()?.templates.some((template) => template.id === "bugfix-mine")).toBe(true);
    expect(teams.catalogError()).toContain("offline");
  });

  it("checks a project folder through the core", async () => {
    const { transport } = fake();
    const teams = createTeams(transport);
    expect(await teams.project("/workspace/example")).toEqual({
      git: true, head: "0123456789abcdef0123456789abcdef01234567", branch: "main", reason: "",
    });
  });

  it("derives a session's team from the followed list, without probing or a per-session cache", () => {
    const { transport, calls } = fake();
    const teams = createTeams(transport);
    teams.apply([entry("list", "list", list)]);
    const identity = { machine: "00000000-0000-4000-8000-0000000000aa", launchId: "00000000-0000-4000-8000-000000000064", runId: null };
    expect(teams.teamOfSession(identity)).toEqual({ state: "lead", team: TEAM_ID });
    expect(calls.follow, "a lookup follows nothing by itself").toEqual([]);
    // Another session is not a lead, and nothing per session is kept.
    for (let n = 0; n < 128; n++) expect(teams.teamOfSession({ ...identity, launchId: `00000000-0000-4000-9000-${String(n).padStart(12, "0")}` })).toEqual({ state: "none" });
    expect(teams.listPages()).toHaveLength(1);
  });
});

// Product-written list pages, including the final empty EOF page. Their Rust
// producer pins the wire cursors and core views in teams_screen.rs.
import screenFixture from "../ui/screens/fixtures/teams-screen.json";
import type { TeamList } from "./teams";
const screenPages = screenFixture.pages as unknown as TeamList[];

it("keeps one selected browser page beyond the lookup cache and ignores a late prior page", async () => {
  const { transport, calls } = fake(), teams = createTeams(transport);
  const apply = (page: TeamList) => teams.apply([entry("list", "list", page)]);
  apply(screenPages[0]);
  for (const page of screenPages.slice(1)) { await teams.page("list", page.after!); apply(page); }
  expect(teams.listPages()).toHaveLength(5);
  expect(teams.listPage()).toEqual(screenPages.at(-1));
  await teams.firstListPage();
  apply(screenPages[0]); apply(screenPages[6]);
  expect(calls.pages.at(-1)).toEqual(["list", null]);
  expect(teams.listPage()).toEqual(screenPages[0]);
  expect(teams.list()).toEqual(screenPages[0]);
});

it("releases an interrupted lookup request when the browser resets its cursor", async () => {
  const { transport, calls } = fake(), teams = createTeams(transport);
  teams.apply([entry("list", "list", screenPages[0])]);
  const ref = screenPages[1].teams[0].lead_session!;
  const identity = { machine: ref.machine, launchId: ref.session_id.slice("launch_id:".length), runId: null };
  expect(teams.teamOfSession(identity)).toMatchObject({ state: "unknown" });
  expect(calls.pages.at(-1)).toEqual(["list", screenPages[0].next]);
  await teams.firstListPage();
  teams.apply([entry("list", "list", screenPages[0])]);
  calls.pages.length = 0;
  teams.teamOfSession(identity);
  expect(calls.pages).toEqual([["list", screenPages[0].next]]);
});

it("restores the selected list cursor after a transport refusal", async () => {
  const { transport, calls } = fake(), teams = createTeams(transport);
  teams.apply([entry("list", "list", screenPages[0])]);
  transport.page = async () => { throw new Error("private fixture refusal"); };
  await expect(teams.page("list", screenPages[0].next)).rejects.toThrow("private fixture refusal");
  transport.page = async (key, cursor) => { calls.pages.push([key, cursor]); };
  await teams.firstListPage(); expect(calls.pages).toEqual([]);
  teams.apply([entry("list", "list", screenPages[0])]);
  expect(teams.listPage()).toEqual(screenPages[0]);
});

it("restores a retained browser page after the native reader has ended", async () => {
  const { transport, calls } = fake(), teams = createTeams(transport);
  teams.apply([entry("list", "list", screenPages[0])]);
  await teams.page("list", screenPages[1].after!);
  teams.apply([entry("list", "list", screenPages[1])]);
  teams.apply([entry("list", "ended", {})]);
  calls.pages.length = 0;
  await teams.firstListPage();
  expect(calls.pages).toEqual([["list", null]]);
});
