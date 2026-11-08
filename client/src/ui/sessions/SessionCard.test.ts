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

import { agentsLabel, badgeKind, cardState, childStatus, chipTooltip, emphasis, lineThree, statusCaption } from "./SessionCard";
import { now } from "../../store/ui";
import type { Session } from "./types";
import { toSession } from "../../store/catalog";
import { currentChildren, workingChildren } from "./subagents";
import { groupRows } from "./group-rows";
import { stripChildStatus } from "./SessionList";
import contract from "../../../../mobile/android/app/src/test/resources/subagent-caption-contract.json";

const row = (extra: Partial<Session> = {}): Session => ({
  computer: "", state: "running", status: "ready", unread: false, unreadReply: true, unreadCandidate: false, draft: false, ...extra,
}) as Session;

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

describe("session states on the card (U30, SessionCardDelegate cardStatus)", () => {
  beforeEach(() => mocks.online.mockReset().mockReturnValue(true));
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
