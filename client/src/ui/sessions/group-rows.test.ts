import { describe, expect, it } from "vitest";
import { toSession } from "../../store/catalog";
import type { TeamSummary, ViewRow } from "../../types";
import type { Session } from "./types";
import { groupRows, type ListInput } from "./group-rows";

const row = (key: string, name: string, extra: Partial<ViewRow> = {}) =>
  toSession("", key, {
    name, agent: "claude", state: "running", resumable: true, activity: "idle",
    process_state: "running", run_id: key.slice(4), conversation_id: `c-${key}`, machine: "box", ...extra,
  });

const team = (lead: string, member: string): TeamSummary => ({
  id: "team-1", name: "Fork", status: "running", project: "p", folder: "/p", plan_mode: "standard",
  lead: "lead", members: [
    { id: "lead", role: "Lead", agent: "claude", state: "active", lead: true,
      session: { host: "", name: lead, run_id: lead.slice(4), conversation_id: `c-${lead}` } },
    { id: "core", role: "Core", agent: "claude", state: "active", lead: false,
      session: { host: "", name: member, run_id: member.slice(4), conversation_id: `c-${member}` } },
  ],
  tasks: 4, accepted: 2, submitted: 0, in_review: 0, in_test: 0, attention: 2, open_proposals: 0,
  waiting_for_lead: false, pending_phase: null, updated: 0, revision: 1,
  phase: 1, phase_done: 2, phase_total: 4, phases_done: 0, phases_total: 1,
});

const input = (expanded: boolean, teamOf: ListInput["teamOf"], collapsed = false): ListInput => {
  const lead = row("run:lead", "claude/p/lead");
  const member = row("run:core", "claude/p/core");
  return {
    grouping: { loaded: false, headers: [] },
    members: new Map([["p", [lead, member]]]),
    visible: () => true,
    online: () => true,
    projects: [],
    expanded: () => expanded,
    compact: () => false,
    teamOf,
    teamCollapsed: () => collapsed,
  } as unknown as ListInput;
};

describe("a team's rows", () => {
  it("nests a member under its lead and gives the lead both bars", () => {
    const lead = row("run:lead", "claude/p/lead");
    const member = row("run:core", "claude/p/core");
    const summary = team(lead.name, member.name);
    const rows = groupRows(input(true, (s) => (s.name === lead.name || s.name === member.name ? summary : undefined)));
    const cards = rows.filter((r) => r.type === "card");
    expect(cards.map((r) => r.session.name)).toEqual([lead.name, member.name]);
    expect(cards[0]).toMatchObject({ depth: 0, team: summary });
    expect(cards[1]).toMatchObject({ depth: 1, teamFirst: true, teamLast: true });
    expect(cards[1].team).toBeUndefined();
  });

  it("hides members while the lead is collapsed, so they are never duplicated", () => {
    const lead = row("run:lead", "claude/p/lead");
    const member = row("run:core", "claude/p/core");
    const summary = team(lead.name, member.name);
    const rows = groupRows(input(false, () => summary, true));
    expect(rows.filter((r) => r.type === "card").map((r) => r.session.name)).toEqual([lead.name]);
  });
});

// Task 431 (owner, 2026-11-09): a project folder drew only its header while its
// sessions were folded away under a lead filed in another project. An expanded
// folder draws every session it lists; a folded one shows its header alone.
const multiTeam = (leadName: string, memberNames: string[]): TeamSummary => ({
  ...team(leadName, memberNames[0]),
  members: [
    { id: "lead", role: "Lead", agent: "claude", state: "active", lead: true,
      session: { host: "", name: leadName, run_id: "lead", conversation_id: "c-lead" } },
    ...memberNames.map((name, index) => ({ id: `m${index}`, role: `Member ${index}`, agent: "claude" as const, state: "active", lead: false,
      session: { host: "", name, run_id: name, conversation_id: `c-${name}` } })),
  ],
});

const folders = (spec: Record<string, Session[]>, collapsed: string[] = []) => {
  const members = new Map<string, Session[]>(Object.entries(spec));
  const headers = Object.keys(spec).map((id, index) => ({ id, name: id === "b" ? "RogueDod" : id.toUpperCase(), color: null, palette: index, collapsed: collapsed.includes(id), accessible: true }));
  return {
    input: {
      grouping: { loaded: true, headers },
      members,
      visible: () => true,
      online: () => true,
      projects: [],
      expanded: () => true,
      compact: () => false,
    } as unknown as ListInput,
    sessions: spec,
  };
};

const cardsOf = (rows: ReturnType<typeof groupRows>, project: string) => {
  const header = rows.findIndex((r) => r.type === "header" && r.project === project);
  if (header < 0) return [];
  const cards: string[] = [];
  for (let index = header + 1; index < rows.length && rows[index].type !== "header"; index++) {
    const r = rows[index];
    if (r.type === "card") cards.push(r.session.name);
  }
  return cards;
};

describe("a project folder's rows (task 431)", () => {
  const four = (prefix: string) => ["one", "two", "three", "four"].map((suffix) => row(`run:${prefix}-${suffix}`, `claude/${prefix}/${suffix}`));

  it("draws every session of an expanded folder and nothing folded away", () => {
    const { input } = folders({ a: four("a"), b: four("b") });
    const rows = groupRows(input);
    expect(cardsOf(rows, "b")).toHaveLength(4);
    // No "show more" row while the whole folder fits: four cards, no +4.
    expect(rows.filter((r) => r.type === "header" && r.project === "b")).toHaveLength(1);
    expect(rows.filter((r) => r.type !== "header" && r.type !== "card")).toHaveLength(0);
  });

  it("draws only the header of a folded folder", () => {
    const { input } = folders({ a: four("a"), b: four("b") }, ["b"]);
    const rows = groupRows(input);
    expect(cardsOf(rows, "b")).toHaveLength(0);
    expect(rows.filter((r) => r.type === "header" && r.project === "b")[0]).toMatchObject({ collapsed: true, total: 4 });
  });

  it("keeps a team's members in their own folder when the lead is filed elsewhere", () => {
    const lead = row("run:boss", "claude/a/boss");
    const members = ["one", "two", "three", "four"].map((suffix) => row(`run:m-${suffix}`, `claude/b/${suffix}`));
    const summary = multiTeam(lead.name, members.map((m) => m.name));
    const { input, sessions } = folders({ a: [lead], b: members });
    const rows = groupRows({ ...input, teamOf: (s) => (s.name === lead.name || members.some((m) => m.name === s.name) ? summary : undefined) } as ListInput);
    // The members' own folder draws them; before the fix it showed the header alone.
    expect(cardsOf(rows, "b")).toEqual(members.map((m) => m.name));
    // The lead's folder draws the lead only, so no session appears twice.
    expect(cardsOf(rows, "a")).toEqual([lead.name]);
    expect(rows.filter((r) => r.type === "card")).toHaveLength(5);
    expect(sessions.a).toHaveLength(1);
  });
});


