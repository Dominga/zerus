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

import { agentsLabel, badgeKind, childStatus, emphasis, fitCounters } from "./SessionCard";
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
