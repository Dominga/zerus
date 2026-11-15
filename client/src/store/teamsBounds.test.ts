import { afterEach, describe, expect, it, vi } from "vitest";
import { createTeams, HIDDEN_RECHECK_MS, LIST_PAGE_FRESH_MS, LIST_PAGES_MAX, REVISION_WAIT_MS, UNRESOLVED_MAX, type TeamCall, type TeamOutcome, type TeamsTransport } from "./teams";
import { teamView, TEAM_ID } from "../ui/teams/fixtures";
import type { TeamsOut } from "../types";

// The store's bounds after the second review (findings 2 to 6, 12 and 13):
// the lead lookup counts pages and never answers "none" from an incomplete
// list; the list is kept as pages replaced by their cursor; admission reserves
// a slot before the invoke; an asking view keeps its own refusal; leaving a
// Team tab never restarts discovery while Teams are off; a decision's revision
// read owns its follow; a later waiting page never stands for "nothing waits".

const MACHINE = "00000000-0000-4000-8000-0000000000aa";
const LAUNCH = "00000000-0000-4000-8000-000000000064";
const identity = { machine: MACHINE, launchId: LAUNCH };
const lead = { machine: MACHINE, session_id: `launch_id:${LAUNCH}` };
const summary = (n: number, binding: unknown = null) => ({
  id: `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`, project: "example-project", name: `Team ${n}`,
  status: "running", lead: "lead", home: MACHINE, revision: 1, lead_session: binding,
});
const entry = (key: string, kind: string, payload: unknown): TeamsOut => ({ computer: "", key, kind, payload });
const listPage = (teams: unknown[], next: string | null = null, after: string | null = null) =>
  entry("list", "list", { teams, truncated: false, next, after });
const call = (n: number): TeamCall => ({ team: TEAM_ID, request_id: `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`, expected_revision: 16, action: { kind: "accept", node: n } });

function fake() {
  const transport = {
    follow: vi.fn(async (_: { list: boolean; waiting: boolean; team: string | null }) => {}),
    hidden: vi.fn(async (_: boolean) => {}),
    tasksChanged: vi.fn(async () => {}),
    page: vi.fn(async (_key: string, _cursor: string | null) => {}),
    act: vi.fn(async (c: TeamCall): Promise<TeamOutcome> => ({ outcome: "uncertain", detail: "unknown", request_id: c.request_id })),
    actAgain: vi.fn(async (c: TeamCall): Promise<TeamOutcome> => ({ outcome: "done", receipt: {}, request_id: c.request_id })),
    dismiss: vi.fn(async (_: TeamCall) => {}),
    catalog: vi.fn(async () => ({})), saveTemplate: vi.fn(async () => {}), project: vi.fn(async () => ({})), preview: vi.fn(async () => ({})),
  } satisfies TeamsTransport;
  return { transport, store: createTeams(transport) };
}

afterEach(() => { vi.useRealTimers(); });

describe("the lead lookup counts pages (finding 2)", () => {
  it("a first page of six teams with a next page stays unknown and asks for page two", () => {
    const { store, transport } = fake();
    store.apply([listPage(Array.from({ length: 6 }, (_, n) => summary(n)), "c1")]);
    expect(store.teamOfSession(identity).state).toBe("unknown");
    expect(transport.page).toHaveBeenCalledWith("list", "c1");
  });

  it("five one-row pages with a next page left do not prove absence", () => {
    const { store, transport } = fake();
    for (let n = 0; n < LIST_PAGES_MAX; n++) {
      store.apply([listPage([summary(n)], `c${n + 1}`, n ? `c${n}` : null)]);
      store.teamOfSession(identity);
    }
    expect(store.teamOfSession(identity).state).toBe("unknown");
    expect(transport.page.mock.calls.filter(([key, cursor]) => key === "list" && cursor !== null)).toHaveLength(LIST_PAGES_MAX - 1);
  });

  it("a complete empty list proves none", () => {
    const { store } = fake();
    store.apply([listPage([])]);
    expect(store.teamOfSession(identity)).toEqual({ state: "none" });
  });

  it("a truncated page never proves none", () => {
    const { store } = fake();
    store.apply([entry("list", "list", { teams: [summary(1)], truncated: true, next: null, after: null })]);
    expect(store.teamOfSession(identity).state).toBe("unknown");
  });

  it("finds the lead on the second page and goes back to the first page", () => {
    const { store, transport } = fake();
    store.apply([listPage([summary(1)], "c1")]);
    expect(store.teamOfSession(identity).state).toBe("unknown");
    store.apply([listPage([summary(2, lead)], null, "c1")]);
    expect(store.teamOfSession(identity)).toEqual({ state: "lead", team: summary(2).id });
    expect(transport.page.mock.calls.at(-1)).toEqual(["list", null]);
    // The first page is the list views read, not the last page asked for.
    expect(store.list()?.teams[0].id).toBe(summary(1).id);
  });

  it("needs the machine: a lookup without it is unknown, another machine is not a match", () => {
    const { store } = fake();
    store.apply([listPage([summary(1, lead)])]);
    expect(store.teamOfSession({ launchId: LAUNCH }).state).toBe("unknown");
    expect(store.teamOfSession({ machine: "00000000-0000-4000-8000-0000000000bb", launchId: LAUNCH })).toEqual({ state: "none" });
  });
});

describe("the list is kept as bounded pages (finding 3)", () => {
  it("a poll replaces a known team, so a lead bound later is found", () => {
    const { store } = fake();
    store.apply([listPage([summary(1)])]);
    expect(store.teamOfSession(identity)).toEqual({ state: "none" });
    store.apply([listPage([summary(1, lead)])]);
    expect(store.teamOfSession(identity)).toEqual({ state: "lead", team: summary(1).id });
  });

  it("repeated polls of full pages keep one page, and the chain never passes the page bound", () => {
    const { store } = fake();
    for (let poll = 0; poll < 7; poll++) store.apply([listPage(Array.from({ length: 200 }, (_, n) => summary(poll * 200 + n)))]);
    expect(store.listPages()).toHaveLength(200);
    for (let n = 0; n < LIST_PAGES_MAX + 3; n++) store.apply([listPage([summary(n)], `c${n + 1}`, n ? `c${n}` : null)]);
    expect(store.listPages().length).toBeLessThanOrEqual(LIST_PAGES_MAX);
  });

  it("forgets a page that left the chain, so it is read again when it comes back", () => {
    const { store, transport } = fake();
    store.apply([listPage([summary(1)], "c1")]);
    store.apply([listPage([summary(2, lead)], null, "c1")]);
    expect(store.teamOfSession(identity)).toEqual({ state: "lead", team: summary(2).id });
    store.apply([listPage([summary(1)], null)]);
    expect(store.teamOfSession(identity)).toEqual({ state: "none" });
    transport.page.mockClear();
    store.apply([listPage([summary(1)], "c1")]);
    expect(store.teamOfSession(identity), "the old second page is not reused").toEqual({ state: "unknown" });
    expect(transport.page).toHaveBeenCalledWith("list", "c1");
  });

  it("drops a page nobody asked for and a page after a changed first page", () => {
    const { store } = fake();
    store.apply([listPage([summary(9)], null, "stray")]);
    expect(store.listPages()).toHaveLength(0);
    store.apply([listPage([summary(1)], "c1")]);
    store.apply([listPage([summary(2)], null, "c1")]);
    expect(store.listPages()).toHaveLength(2);
    store.apply([listPage([summary(1)], null)]);
    expect(store.listPages().map((team) => team.id)).toEqual([summary(1).id]);
  });
});

describe("admission reserves before the invoke (finding 4)", () => {
  it("nine overlapping calls admit eight and refuse the ninth before any reply", async () => {
    const { store, transport } = fake();
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    transport.act.mockImplementation(async (c) => { await gate; return { outcome: "uncertain", detail: "unknown", request_id: c.request_id }; });
    const sends = Array.from({ length: UNRESOLVED_MAX + 1 }, (_, n) => store.run(call(n)));
    expect(transport.act).toHaveBeenCalledTimes(UNRESOLVED_MAX);
    release();
    const outcomes = await Promise.all(sends);
    expect(outcomes.filter((outcome) => outcome.outcome === "failed")).toHaveLength(1);
    expect(store.unresolved()).toHaveLength(UNRESOLVED_MAX);
  });

  it("busy stays while any call is in flight, not until the first one ends", async () => {
    const { store, transport } = fake();
    const gates: (() => void)[] = [];
    transport.act.mockImplementation((c) => new Promise((resolve) => gates.push(() => resolve({ outcome: "done", receipt: {}, request_id: c.request_id }))));
    const a = store.run(call(1)), b = store.run(call(2));
    expect(store.busy()).toBe(true);
    gates[0](); await a;
    expect(store.busy(), "the second call is still in flight").toBe(true);
    gates[1](); await b;
    expect(store.busy()).toBe(false);
  });

  it("the same request id is never sent twice at once", async () => {
    const { store, transport } = fake();
    let release!: () => void;
    transport.act.mockImplementation((c) => new Promise((resolve) => { release = () => resolve({ outcome: "done", receipt: {}, request_id: c.request_id }); }));
    const first = store.run(call(1));
    const second = await store.run(call(1));
    expect(second.outcome).toBe("pending");
    expect(transport.act).toHaveBeenCalledTimes(1);
    release(); await first;
  });
});

describe("an asking view keeps its own refusal (finding 5)", () => {
  it("a read failure and another view's refusal leave it in place", async () => {
    const { store, transport } = fake();
    transport.act.mockResolvedValueOnce({ outcome: "failed", code: "refused", detail: "Cannot pause member.", request_id: call(1).request_id });
    await store.run(call(1), "team-tab");
    store.apply([entry("waiting", "failed", { code: "invalid", message: "temporary read failure" })]);
    expect(store.failureFor("team-tab")?.message).toBe("Cannot pause member.");
    transport.act.mockResolvedValueOnce({ outcome: "failed", code: "refused", detail: "Another view's refusal.", request_id: call(2).request_id });
    await store.run(call(2), "waiting");
    expect(store.failureFor("team-tab")?.message).toBe("Cannot pause member.");
    expect(store.failureFor("waiting")?.message).toBe("Another view's refusal.");
    expect(store.readFailureFor("waiting")?.message).toBe("temporary read failure");
  });

  it("a successful read clears that read's failure only", () => {
    const { store } = fake();
    store.apply([entry("waiting", "failed", { code: "invalid", message: "temporary read failure" })]);
    store.apply([entry("list", "failed", { code: "invalid", message: "list failure" })]);
    store.apply([entry("waiting", "waiting", { items: [], next: null, after: null })]);
    expect(store.readFailureFor("waiting")).toBeNull();
    expect(store.readFailureFor("list")?.message).toBe("list failure");
  });

  it("keeps a bounded number of asking views", async () => {
    const { store, transport } = fake();
    for (let n = 0; n < 40; n++) {
      transport.act.mockResolvedValueOnce({ outcome: "failed", code: "refused", detail: `refusal ${n}`, request_id: call(n).request_id });
      await store.run(call(n), `view-${n}`);
    }
    expect(store.failureFor("view-39")?.message).toBe("refusal 39");
    expect(store.failureFor("view-0")).toBeNull();
  });
});

describe("leaving a Team tab never restarts discovery (finding 6)", () => {
  it("open team, Teams off, close team: nothing is followed again", async () => {
    const { store, transport } = fake();
    await store.probe();
    await store.openTeam(TEAM_ID);
    store.apply([entry("list", "failed", { code: "teams_inactive", message: "off" })]);
    await store.closeTeam();
    await store.openWaiting();
    await store.hidden(false);
    expect(transport.follow.mock.calls.at(-1)?.[0]).toEqual({ list: false, waiting: false, team: null });
    expect(transport.hidden.mock.calls.at(-1)?.[0]).toBe(true);
  });

  it("the application's recheck restarts discovery and keeps the open tab's team", async () => {
    const { store, transport } = fake();
    await store.openTeam(TEAM_ID);
    store.apply([entry("list", "failed", { code: "teams_inactive", message: "off" })]);
    await store.recheck();
    expect(transport.follow.mock.calls.at(-1)?.[0]).toEqual({ list: true, waiting: true, team: TEAM_ID });
  });

  it("a Team tab keeps the list followed so the lookup can page", async () => {
    const { store, transport } = fake();
    await store.openTeam(TEAM_ID);
    expect(transport.follow.mock.calls.at(-1)?.[0]).toEqual({ list: true, waiting: true, team: TEAM_ID });
  });
});

describe("a decision's revision read owns its follow (finding 12)", () => {
  it("a reply that lands during the follow call is not lost", async () => {
    const { store, transport } = fake();
    transport.follow.mockImplementation(async (follow) => { if (follow.team) store.apply([entry(`team:${TEAM_ID}`, "team", teamView)]); });
    expect(await store.revisionOf(TEAM_ID)).toBe(16);
  });

  it("waits the stated bound, longer than one read", async () => {
    vi.useFakeTimers();
    const { store } = fake();
    const pending = store.revisionOf(TEAM_ID);
    await vi.advanceTimersByTimeAsync(16_000);
    store.apply([entry(`team:${TEAM_ID}`, "team", teamView)]);
    expect(await pending).toBe(16);
    expect(REVISION_WAIT_MS).toBeGreaterThan(30_000);
  });

  it("settles a read failure at once and a silent read at the bound", async () => {
    vi.useFakeTimers();
    const { store } = fake();
    const failing = store.revisionOf(TEAM_ID);
    await Promise.resolve();
    store.apply([entry(`team:${TEAM_ID}`, "failed", { code: "not_found", message: "no such team" })]);
    expect(await failing).toBeUndefined();
    const silent = store.revisionOf(TEAM_ID);
    await vi.advanceTimersByTimeAsync(REVISION_WAIT_MS);
    expect(await silent).toBeUndefined();
  });

  it("restores the Waiting scope after the decision", async () => {
    const { store, transport } = fake();
    await store.openWaiting();
    const card = { id: `${TEAM_ID}:1`, team: TEAM_ID, card: 1, kind: "integration", node: 1, text: "approve", state: "open", by: "service", created_ms: 0 };
    const pending = store.decide(card, "approve");
    await Promise.resolve(); await Promise.resolve();
    store.apply([entry(`team:${TEAM_ID}`, "team", teamView)]);
    await pending;
    expect(transport.follow.mock.calls.at(-1)?.[0]).toEqual({ list: true, waiting: true, team: null });
    expect(store.team(), "the borrowed team is not shown as a followed one").toBeUndefined();
  });

  it("does not restore over a follow that changed meanwhile", async () => {
    const { store, transport } = fake();
    const pending = store.revisionOf(TEAM_ID);
    await Promise.resolve();
    await store.openTeam("00000000-0000-4000-8000-00000000cafe");
    store.apply([entry(`team:${TEAM_ID}`, "team", teamView)]);
    await pending;
    expect(transport.follow.mock.calls.at(-1)?.[0]).toEqual({ list: true, waiting: true, team: "00000000-0000-4000-8000-00000000cafe" });
  });

  it("two overlapping reads of the same team both settle", async () => {
    const { store } = fake();
    const a = store.revisionOf(TEAM_ID), b = store.revisionOf(TEAM_ID);
    await Promise.resolve();
    store.apply([entry(`team:${TEAM_ID}`, "team", teamView)]);
    expect(await a).toBe(16);
    expect(await b).toBe(16);
  });
});

describe("a later waiting page (finding 13)", () => {
  const item = { id: `${TEAM_ID}:1`, team: TEAM_ID, card: 1, kind: "note", node: null, text: "note", state: "open", by: "local", created_ms: 0 };
  it("the rail count comes from the first page, never from a later one", () => {
    const { store } = fake();
    store.apply([entry("waiting", "waiting", { items: [item], next: "w1", after: null })]);
    expect(store.waitingCount()).toBe(2);
    store.apply([entry("waiting", "waiting", { items: [], next: null, after: "w1" })]);
    expect(store.waitingCount(), "an empty later page is not zero").toBe(2);
    expect(store.waitingLater()).toBe(true);
  });

  it("opening Waiting and leaving it go back to the first page", async () => {
    const { store, transport } = fake();
    store.apply([entry("waiting", "waiting", { items: [], next: null, after: "w1" })]);
    await store.openWaiting();
    expect(transport.page).toHaveBeenLastCalledWith("waiting", null);
    transport.page.mockClear();
    await store.firstWaitingPage();
    expect(transport.page).toHaveBeenLastCalledWith("waiting", null);
  });
});

describe("second confirmation", () => {
  it("a refreshed second page replaces its previous lead binding (finding 1)", () => {
    const { store } = fake();
    store.apply([listPage([summary(1)], "c1")]);
    store.apply([listPage([summary(2)], null, "c1")]);
    expect(store.teamOfSession(identity)).toEqual({ state: "none" });
    store.apply([listPage([summary(2, lead)], null, "c1")]);
    expect(store.teamOfSession(identity)).toEqual({ state: "lead", team: summary(2).id });
  });

  it("a stale second page read again is taken (finding 1)", () => {
    vi.useFakeTimers();
    vi.setSystemTime(1000);
    const { store, transport } = fake();
    store.apply([listPage([summary(1)], "c1")]);
    store.apply([listPage([summary(2)], null, "c1")]);
    vi.setSystemTime(1000 + LIST_PAGE_FRESH_MS + 1);
    expect(store.teamOfSession(identity).state).toBe("unknown");
    expect(transport.page).toHaveBeenCalledWith("list", "c1");
    store.apply([listPage([summary(2, lead)], null, "c1")]);
    expect(store.teamOfSession(identity)).toEqual({ state: "lead", team: summary(2).id });
  });

  it("a long-hidden recheck keeps the list the core does not publish again (finding 2)", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1000);
    const { store, transport } = fake();
    // The core publishes a list once per scope; an unchanged follow starts none.
    let following = false;
    transport.follow.mockImplementation(async (follow) => {
      if (follow.list && !following) { following = true; store.apply([listPage([summary(1, lead)])]); }
      else if (!follow.list) following = false;
    });
    await store.probe();
    expect(store.teamOfSession(identity)).toEqual({ state: "lead", team: summary(1).id });
    await store.hidden(true);
    vi.setSystemTime(1000 + HIDDEN_RECHECK_MS + 1);
    await store.hidden(false);
    expect(transport.follow).toHaveBeenCalledTimes(2);
    expect(store.teamOfSession(identity)).toEqual({ state: "lead", team: summary(1).id });
    await store.recheck();
    expect(store.teamOfSession(identity), "Settings' read the state again keeps it too").toEqual({ state: "lead", team: summary(1).id });
  });

  it("a preview change keeps the list until the off answer replaces it (finding 2)", async () => {
    const { store, transport } = fake();
    store.apply([listPage([summary(1, lead)])]);
    transport.preview.mockResolvedValueOnce({ ok: true, preview: "off" });
    expect(await store.preview(false)).toEqual({ outcome: "done" });
    expect(store.teamOfSession(identity), "nothing replaced it yet").toEqual({ state: "lead", team: summary(1).id });
    store.apply([entry("list", "failed", { code: "teams_inactive", message: "off" })]);
    expect(store.listPages()).toEqual([]);
    expect(store.available()).toBe(false);
  });

  it("a retired list scope forgets the reader's page selection", () => {
    const { store, transport } = fake();
    store.apply([listPage([summary(1)], "c1")]);
    store.teamOfSession(identity);
    expect(transport.page).toHaveBeenLastCalledWith("list", "c1");
    store.apply([entry("list", "ended", null)]);
    transport.page.mockClear();
    store.apply([listPage([summary(1, lead)])]);
    expect(store.teamOfSession(identity)).toEqual({ state: "lead", team: summary(1).id });
    expect(transport.page, "the reader is already on the first page").not.toHaveBeenCalled();
  });

  it("a circular cursor ends the walk as an incomplete list (finding 4)", () => {
    const { store } = fake();
    store.apply([listPage([summary(1)], "c1")]);
    store.apply([listPage([summary(2)], "c1", "c1")]);
    expect(store.listPages().map((team) => team.id)).toEqual([summary(1).id, summary(2).id]);
    expect(store.teamOfSession(identity)).toEqual({ state: "unknown", exhausted: true });
    expect(store.summaryOf("00000000-0000-4000-8000-0000000000ff")).toEqual({ state: "unknown" });
    expect(store.listNext()).toBeNull();
    // A longer cycle too.
    const other = fake().store;
    other.apply([listPage([summary(1)], "c1")]);
    other.apply([listPage([summary(2)], "c2", "c1")]);
    other.apply([listPage([summary(3)], "c1", "c2")]);
    expect(other.teamOfSession(identity)).toEqual({ state: "unknown", exhausted: true });
    expect(other.listPages()).toHaveLength(3);
  });
});
