import { beforeEach, describe, expect, it, vi } from "vitest";

vi.hoisted(() => {
  const store = new Map<string, string>();
  vi.stubGlobal("location", new URL("http://localhost/"));
  vi.stubGlobal("localStorage", { getItem: (key: string) => store.get(key) ?? null, setItem: (key: string, value: string) => { store.set(key, value); }, removeItem: (key: string) => { store.delete(key); } });
  vi.stubGlobal("window", { document: { addEventListener: () => {} }, addEventListener: () => {} });
});
const mocks = vi.hoisted(() => ({ online: vi.fn() }));
vi.mock("../../store/catalog", async (orig) => ({
  ...(await orig<object>()),
  computerFresh: mocks.online,
  computerOffline: (computer: string) => !mocks.online(computer),
}));

import { agentsLabel, badgeKind, cardState, childStatus, chipTooltip, chipWidth, emphasis, fitChips, fitCounters, headerState, lineThree, statusCaption, type RowChip } from "./SessionCard";
import { now } from "../../store/ui";
import type { Session } from "./types";
import { toSession } from "../../store/catalog";
import { currentChildren, workingChildren } from "./subagents";
import { groupRows } from "./group-rows";
import { sessionRowName, stripChildStatus } from "./SessionList";
import { setNow } from "../../store/ui";
import type { Row } from "./group-rows";
import type { CreationCard } from "../../types";
import contract from "../../../../mobile/android/app/src/test/resources/subagent-caption-contract.json";

const row = (extra: Partial<Session> = {}): Session => ({
  computer: "", state: "running", status: "ready", unread: false, unreadReply: true, unreadCandidate: false, draft: false, ...extra,
}) as Session;

describe("session tree item names", () => {
  const card = (extra: Partial<Session> = {}, shape: Partial<Extract<Row, { type: "card" }>> = {}): Row => ({
    type: "card", id: "fixture", expandable: false, expanded: false, compactRow: false,
    session: row({ title: "Reviewer", caption: "Ready", cwd: "/fixture/project", project: "Example", machine: "Fixture PC", ...extra }), ...shape,
  });
  beforeEach(() => mocks.online.mockReturnValue(true));

  it("names full, compact and nested rows from the same display identity", () => {
    for (const shape of [{}, { compactRow: true }, { depth: 1, parentKey: "parent" }]) {
      expect(sessionRowName(card({}, shape))).toBe("Reviewer, Ready, project, Fixture PC");
    }
    expect(sessionRowName(card({ cwd: "" }))).toBe("Reviewer, Ready, Example, Fixture PC");
  });

  it("follows attention, draft, offline and renamed content without a ticking name", () => {
    expect(sessionRowName(card({ status: "attention", caption: "Needs input", title: "Review changed", cwd: "C:\\fixture\\next", machine: "Other PC" })))
      .toBe("Review changed, Needs input, next, Other PC");
    expect(sessionRowName(card({ draft: true }))).toContain(", Draft,");
    const working = card({ status: "working", caption: "Working", turnStarted: 100 });
    setNow(110); const first = sessionRowName(working);
    setNow(120); expect(sessionRowName(working)).toBe(first);
    expect(first).toBe("Reviewer, Working, project, Fixture PC");
    mocks.online.mockReturnValue(false);
    expect(sessionRowName(working)).toBe("Reviewer, Offline, project, Fixture PC");
  });

  it("includes the parent's child count in both compact and full rows", () => {
    for (const compactRow of [false, true]) expect(sessionRowName(card({}, { compactRow,
      team: { total: 8, working: 3, collapsed: true } }))).toContain("8 child sessions, collapsed");
  });

  it("names native children using their current or recorded status, and names creation rows", () => {
    const parent = row({ title: "Parent", cwd: "/fixture/project", machine: "Fixture PC", activity: "idle" });
    const child: Row = { type: "child", id: "child", parent, child: { id: "child", name: "Helper", state: "attention", summary: "" }, first: true, last: true };
    expect(sessionRowName(child)).toBe("Helper, Needs input, project, Fixture PC");
    expect(sessionRowName(card({ subagents: [child.child] }))).toContain("Subagents: 1");
    mocks.online.mockReturnValue(false);
    expect(sessionRowName(child)).toBe("Helper, Recorded, project, Fixture PC");
    const creation = { tag: "Preparing review", host: "", machine: "Fixture PC", caption: "Preparing", target: "/fixture/next" } as CreationCard;
    expect(sessionRowName({ type: "creation", id: "creation", creation })).toBe("Preparing review, Fixture PC, Preparing, /fixture/next");
  });
});

describe("supervised child presentation (zerus-sa9qn.3)", () => {
  it("shows a quiet child's request, reply and error in gray, and its Review later mark as usual", () => {
    mocks.online.mockReturnValue(true);
    expect(badgeKind(row({ status: "attention", quiet: true }))).toBe("muted-attention");
    expect(emphasis(row({ status: "attention", quiet: true }))).toBe("muted");
    expect(badgeKind(row({ status: "error", quiet: true }))).toBe("muted-attention");
    expect(emphasis(row({ status: "error", quiet: true }))).toBe("muted");
    expect(badgeKind(row({ status: "error" }))).toBe("error");
    expect(emphasis(row({ status: "error" }))).toBe("error");
    expect(emphasis(row({ quiet: true, reviewLater: true }))).toBe("attention");
  });
});

describe("session card unread presentation", () => {
  beforeEach(() => mocks.online.mockReset());

  it("shows the badge and edge on a full or compact row when the computer went offline mid-reply", () => {
    mocks.online.mockReturnValue(false);
    expect(badgeKind(row())).toBe("unread");
    expect(emphasis(row())).toBe("unread");
    expect(badgeKind(row({ muted: true }))).toBe("muted-unread");
  });

  it("keeps the same reply hidden while the computer is online", () => {
    mocks.online.mockReturnValue(true);
    expect(badgeKind(row())).toBe("neutral");
    expect(emphasis(row())).toBeUndefined();
  });

  it("names a finished unread reply New reply over the state under it, as Qt's card (task 315)", () => {
    mocks.online.mockReturnValue(true);
    // 205 F2: work-9b8fda6a, stopped with a reply nobody read; the host's state is the user's reply.
    const stopped = row({ state: "stopped", status: "paused", caption: "Stopped", unreadCandidate: true,
      sessionState: "user", stateReason: { kind: "reply", text: "" } });
    expect([badgeKind(stopped), statusCaption(stopped)]).toEqual(["unread", "New reply"]);
    expect(statusCaption({ ...stopped, muted: true } as Session)).toBe("New reply");
    // Attention and work keep their own words.
    expect(statusCaption(row({ status: "attention", caption: "Needs input", unreadCandidate: true }))).toBe("Needs input");
    expect(statusCaption(row({ status: "ready", caption: "Ready" }))).toBe("Ready");
  });
});

describe("recorded child captions shared with Android", () => {
  for (const example of contract.cases) it(example.name, () => {
    mocks.online.mockReturnValue(example.online);
    const session = toSession("host", "run:caption", {
      name: "claude/example/captions", ...example.raw, archive_id: "archive_id" in example ? example.archive_id : undefined,
      subagents: contract.children,
    });
    const before = JSON.stringify(session);
    const current = currentChildren(session, example.online);
    expect(current).toBe(example.current);
    // Inspector reads the original selected row, not groupRows' projected copy.
    expect(workingChildren(session, example.online)).toBe(example.working);
    expect(agentsLabel(session)).toBe(`${example.working}/8`);
    const captions = session.subagents!.map((child) => childStatus(child, current));
    expect(captions.map((status) => status.caption)).toEqual(example.current ? contract.live_captions : Array(8).fill("Recorded"));
    if (example.current) {
      expect(captions[0].kind).toBe("working");
      expect(captions[1].kind).toBe("attention");
      expect(captions[2].kind).toBe("attention");
    } else expect(captions.map((status) => status.kind)).toEqual(Array(8).fill("neutral"));

    const list = groupRows({
      grouping: { loaded: true, default_project: "ungrouped", headers: [], ready: true, reason: null, busy: false, status: "", status_error: false, conflicts: 0 },
      members: new Map([["ungrouped", [session]]]), visible: () => true, online: () => example.online,
      projects: [], expanded: () => true, compact: () => false, includeCompleted: true,
    });
    expect(list.flatMap((item) => item.type === "child" ? [childStatus(item.child, currentChildren(item.parent, example.online)).caption] : []))
      .toEqual(captions.map((status) => status.caption));
    // The collapsed strip has a separate tooltip/badge binding from ChildCard.
    expect(list.flatMap((item) => item.type === "child" ? [stripChildStatus(item)] : [])).toEqual(captions);
    expect(JSON.stringify(session)).toBe(before);
    if (example.current) expect(list[0].type === "card" && list[0].session).toBe(session);
  });
});

describe("session states on the card (U30, SessionCardDelegate cardStatus)", () => {
  // The first suite above parks the clock at 120 to test a steady name; these cases need the real time.
  beforeEach(() => { mocks.online.mockReset().mockReturnValue(true); setNow(Date.now() / 1000); });
  const facts = (extra: Partial<Session> = {}) => row({ status: "working", unreadReply: false, cacheTtl: { seconds: 300, kept_warm: false }, ...extra });

  it("shows Waiting with its reason on line 3 in the state's tone", () => {
    const s = facts({ sessionState: "deps", stateReason: { kind: "subagents", text: "Waiting for 2 subagents" } });
    expect(badgeKind(s)).toBe("deps");
    expect(statusCaption(s)).toBe("Waiting");
    expect(lineThree(s)).toEqual({ text: "Waiting for 2 subagents", icon: "hourglass", tone: "deps" });
  });

  it("shows Building with what runs, even on an idle agent", () => {
    const s = facts({ status: "ready", sessionState: "build", build: { label: "cargo test", command: "cargo test", since: now() - 120 } });
    expect([badgeKind(s), statusCaption(s)]).toEqual(["build", "Building"]);
    expect(lineThree(s)).toEqual({ text: "Running cargo test, 2m so far", icon: "build", tone: "build" });
  });

  it("counts Cooling down with its heat, then turns Cold with + New and Resume", () => {
    const cooling = facts({ status: "ready", sessionState: "cooling", lastActive: now() - 120 });
    expect([badgeKind(cooling), statusCaption(cooling)]).toEqual(["cooling", "Cooling 3m"]);
    const heat = cardState(cooling).shown.heat;
    expect(heat).toBeGreaterThan(0.55);
    expect(heat).toBeLessThan(0.65);
    expect(cardState(cooling).shown.coldActions).toBe(false);
    const cold = facts({ status: "ready", sessionState: "cold", lastActive: now() - 1000 });
    expect([badgeKind(cold), statusCaption(cold)]).toEqual(["cold", "Cold"]);
    expect(lineThree(cold)).toEqual({ text: "Cache cold 11m", icon: "snow", tone: "cold" });
    expect(cardState(cold).shown.coldActions).toBe(true);
  });

  it("shows Stopped, or the agent's exit, and how to go on", () => {
    const stopped = facts({ status: "ready", sessionState: "stopped" });
    expect([badgeKind(stopped), statusCaption(stopped), lineThree(stopped).text]).toEqual(["stopped", "Stopped", "Stopped by you, resume to continue"]);
    expect(statusCaption(facts({ sessionState: "stopped", processState: "exited" }))).toBe("Agent exited");
  });

  it("lets attention, an error, an archived row and a draft on an idle card keep their badge", () => {
    const state = { sessionState: "deps", stateReason: { kind: "subagents", text: "Waiting for 2 subagents" } };
    expect(badgeKind(facts({ ...state, status: "attention" }))).toBe("attention");
    expect(badgeKind(facts({ ...state, status: "error" }))).toBe("error");
    expect(badgeKind(facts({ ...state, state: "archived" }))).not.toBe("deps");
    const idle = { status: "ready" as const, sessionState: "cooling", lastActive: now() - 60, draft: true };
    expect(badgeKind(facts(idle))).toBe("draft");
    expect(cardState(facts({ ...idle, draft: false })).kind).toBe("cooling");
  });

  it("gives the detail header the card's state, as Qt's header pill (task 315 F3)", () => {
    // 205 F3: the lead's row said Waiting for reports while its header said Ready.
    const lead = facts({ status: "ready", caption: "Ready", sessionState: "deps",
      stateReason: { kind: "team_reports", text: "Waiting for reports: Frontend (SolidJS) #205" } });
    expect(statusCaption(lead)).toBe("Waiting");
    expect(headerState(lead)).toEqual({ kind: "deps", caption: "Waiting", title: "Waiting for reports: Frontend (SolidJS) #205" });
    expect(headerState(facts({ status: "ready", sessionState: "stopped" }))?.caption).toBe("Stopped");
    expect(headerState(facts({ status: "ready", sessionState: "user", stateReason: { kind: "team_lead", text: "Approve the plan" } })))
      .toEqual({ kind: "attention", caption: "Needs you", title: "Approve the plan" });
    // A request that needs action, an archived row and a working agent keep the header's own status.
    expect(headerState(facts({ status: "attention", needsAction: true, sessionState: "deps" }))).toBeUndefined();
    expect(headerState(facts({ sessionState: "deps", state: "archived" }))).toBeUndefined();
    expect(headerState(facts({ sessionState: "working" }))).toBeUndefined();
    expect(headerState(facts({ status: "ready" }))).toBeUndefined();
  });

  it("names the lead's Needs you as attention, with its reason on line 3", () => {
    const s = facts({ status: "ready", sessionState: "user", stateReason: { kind: "team_lead", text: "Approve the plan" } });
    expect([badgeKind(s), statusCaption(s)]).toEqual(["attention", "Needs you"]);
    expect(lineThree(s)).toEqual({ text: "Approve the plan" });
  });

  it("keeps an older host's card and an unreachable machine as they were", () => {
    expect(badgeKind(facts({ status: "ready" }))).toBe("neutral");
    expect(statusCaption(facts({ status: "ready", caption: "Ready" }))).toBe("Ready");
    mocks.online.mockReturnValue(false);
    expect(cardState(facts({ sessionState: "deps", stateReason: { kind: "subagents", text: "Waiting for 2 subagents" } })).kind).toBeUndefined();
  });
});

describe("the chips on the card name what they count (SessionRowChips::tooltip)", () => {
  const git = { ahead: 1, behind: 3, changed: 2, conflicts: 1, state: "diff" as const };
  it("words services, background commands and git as Qt does", () => {
    const s = row({ services: [{ name: "vite", command: "npm run dev", since: 0 }], backgroundJobs: 2, backgroundCommand: "npm run watch", branch: "main", git });
    expect(chipTooltip(s, "services")).toBe("vite, since unknown\nThese live with the session and are not Background work.");
    expect(chipTooltip(s, "background")).toBe("2 running commands: npm run watch");
    expect(chipTooltip(row({ backgroundJobs: 1 }), "background")).toBe("1 running command");
    expect(chipTooltip(s, "changes")).toBe("2 changed files on main\n1 files with merge conflicts");
    expect(chipTooltip(s, "outgoing")).toBe("1 outgoing commit");
    expect(chipTooltip(s, "incoming")).toBe("3 incoming commits");
    expect(chipTooltip(s, "synced")).toBe("Clean and in sync with the checked upstream");
    expect(chipTooltip(s, "unchecked")).toBe("Not checked yet");
  });
});

// Task 252 (Qt SessionRowChips::layout): when line 2 is full, git folds into
// one More chip before services or background work go, then chips drop from
// the end; the machine badge keeps its room.
const chip = (kind: RowChip["kind"], icon: string, count?: string): RowChip => ({ kind, icon, count, tone: "muted", tip: icon });
const chips = [chip("services", "settings", "2"), chip("background", "active", "1"), chip("git", "git-diff", "12"), chip("git", "git-push", "3"), chip("git", "git-unknown")];
const width = (list: RowChip[]) => list.reduce((sum, c) => sum + chipWidth(c) + 4, 0) - 4;

describe("line 2 chips (task 252)", () => {
  it("shows every chip when they fit", () => {
    expect(fitChips(chips, width(chips), { tip: "git" })).toEqual(chips);
  });

  it("folds git into one More chip with the git tooltip before dropping anything", () => {
    const folded = fitChips(chips, width(chips) - 1, { tip: "Git status" });
    expect(folded.map((c) => c.kind)).toEqual(["services", "background", "more"]);
    expect(folded[2]).toMatchObject({ icon: "more", tip: "Git status" });
    expect(folded[2].count).toBeUndefined();
  });

  it("drops chips from the end when even the folded ones do not fit", () => {
    const folded = [chips[0], chips[1], { ...chip("more", "more"), tip: "g" }];
    expect(fitChips(chips, width(folded) - 1, { tip: "g" }).map((c) => c.kind)).toEqual(["services", "background"]);
    expect(fitChips(chips, 0, { tip: "g" })).toEqual([]);
  });

  it("sizes a chip as Qt does: icon and padding, plus its count", () => {
    expect(chipWidth({})).toBe(24);
    expect(chipWidth({ count: "12" })).toBe(41);
  });
});

describe("compact summary fit (zerus-sa9qn.12)", () => {
  it("shortens once the long labels overflow and lengthens only when their width returns", () => {
    let state = { narrow: false, needed: 0 };
    state = fitCounters(state, 300, 300);
    expect(state).toEqual({ narrow: false, needed: 0 });
    state = fitCounters(state, 342, 300);
    expect(state).toEqual({ narrow: true, needed: 342 });
    // Short labels now fit; a little more room is not enough for the long ones.
    expect(fitCounters(state, 180, 320)).toEqual(state);
    expect(fitCounters(state, 180, 342)).toEqual({ narrow: false, needed: 0 });
  });
});
