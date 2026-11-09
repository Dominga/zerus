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
    teamFolded: () => collapsed,
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
    expect(cards[0]).toMatchObject({ teamSummary: summary });
    expect(cards[1]).toMatchObject({ depth: 1, teamFirst: true, teamLast: true });
    expect(cards[1].teamSummary).toBeUndefined();
  });

  it("hides members while the lead is collapsed, so they are never duplicated", () => {
    const lead = row("run:lead", "claude/p/lead");
    const member = row("run:core", "claude/p/core");
    const summary = team(lead.name, member.name);
    const rows = groupRows(input(false, () => summary, true));
    expect(rows.filter((r) => r.type === "card").map((r) => r.session.name)).toEqual([lead.name]);
  });

  it("folds a team under its lead and restores its members in order (task 425)", () => {
    const lead = row("run:lead", "claude/p/lead");
    const member = row("run:core", "claude/p/core");
    const summary = team(lead.name, member.name);
    const teamOf = (s: { name: string }) => (s.name === lead.name || s.name === member.name ? summary : undefined);
    const cards = (collapsed: boolean) => groupRows(input(true, teamOf, collapsed)).filter((r) => r.type === "card");
    const folded = cards(true);
    expect(folded.map((r) => r.session.name)).toEqual([lead.name]);
    // The lead keeps its strip and stays expandable, so its toggle can unfold the team again.
    expect(folded[0]).toMatchObject({ teamSummary: summary, expandable: true });
    const open = cards(false);
    expect(open.map((r) => r.session.name)).toEqual([lead.name, member.name]);
    expect(open[1]).toMatchObject({ depth: 1, teamFirst: true, teamLast: true });
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

  it("lists every live member under its lead, in team order, whatever folder each member is filed in (task 455)", () => {
    // Task 455 replaces 431's rule (members stayed in their own folder): the owner's team had its lead
    // in ZERUS and 13 of 14 members in their own worktree folders, and showed one member.
    const lead = row("run:boss", "claude/a/boss");
    const nearby = row("run:m-near", "claude/a/near");
    const away = ["one", "two", "three"].map((suffix) => row(`run:m-${suffix}`, `claude/b/${suffix}`));
    const elsewhere = row("run:m-far", "claude/c/far");
    const roster = [away[1], nearby, elsewhere, away[0], away[2]];
    const summary = multiTeam(lead.name, roster.map((m) => m.name));
    const all = [lead, nearby, ...away, elsewhere];
    const { input } = folders({ a: [lead, nearby], b: away, c: [elsewhere] });
    const rows = groupRows({ ...input, teamOf: (s) => (all.some((m) => m.name === s.name) ? summary : undefined) } as ListInput);
    expect(cardsOf(rows, "a")).toEqual([lead.name, ...roster.map((m) => m.name)]);
    // Nowhere else, and their emptied folders are not drawn as headers alone.
    expect(cardsOf(rows, "b")).toEqual([]);
    expect(rows.filter((r) => r.type === "header" && (r.project === "b" || r.project === "c"))).toEqual([]);
    expect(rows.filter((r) => r.type === "card")).toHaveLength(all.length);
    const cards = rows.filter((r) => r.type === "card");
    // The strip's count is the member rows shown; the tree's ends are marked.
    expect(cards[0]).toMatchObject({ teamSummary: summary, teamMembersShown: roster.length, expandable: true, expanded: true });
    expect(cards.slice(1).map((r) => r.depth)).toEqual(roster.map(() => 1));
    expect(cards[1]).toMatchObject({ teamFirst: true, teamLast: false });
    expect(cards.at(-1)).toMatchObject({ teamFirst: false, teamLast: true });
  });

  it("leaves the members in their own folders while the lead is not shown, and keeps them nowhere while the team is folded", () => {
    const lead = row("run:boss", "claude/a/boss");
    const members = ["one", "two"].map((suffix) => row(`run:m-${suffix}`, `claude/b/${suffix}`));
    const summary = multiTeam(lead.name, members.map((m) => m.name));
    const teamOf = (s: { name: string }) => (s.name === lead.name || members.some((m) => m.name === s.name) ? summary : undefined);
    const { input } = folders({ a: [lead], b: members });
    // The lead filtered out: its members list in their own folder, as ordinary rows.
    const hidden = groupRows({ ...input, visible: (s: Session) => s.key !== lead.key, teamOf } as ListInput);
    expect(cardsOf(hidden, "b")).toEqual(members.map((m) => m.name));
    expect(hidden.filter((r) => r.type === "card" && r.depth)).toEqual([]);
    // Folded: the lead alone, its strip counting the members it hides.
    const folded = groupRows({ ...input, teamOf, teamFolded: () => true } as ListInput);
    expect(folded.filter((r) => r.type === "card").map((r) => r.session.name)).toEqual([lead.name]);
    expect(folded.find((r) => r.type === "card")).toMatchObject({ teamMembersShown: 2, expanded: false, expandable: true });
  });

  it("keeps a sub-team's lead in its own place with its own team", () => {
    const lead = row("run:boss", "claude/a/boss");
    const sublead = row("run:sub", "claude/b/sub");
    const worker = row("run:w", "claude/b/worker");
    const subworker = row("run:sw", "claude/c/subworker");
    const parent = multiTeam(lead.name, [sublead.name, worker.name]);
    const child = { ...multiTeam(sublead.name, [subworker.name]), id: "t2", name: "Sub" };
    const teamOf = (s: { name: string }) => (s.name === lead.name || s.name === worker.name ? parent : s.name === sublead.name || s.name === subworker.name ? child : undefined);
    const { input } = folders({ a: [lead], b: [sublead, worker], c: [subworker] });
    const rows = groupRows({ ...input, teamOf } as ListInput);
    expect(cardsOf(rows, "a")).toEqual([lead.name, worker.name]);
    expect(cardsOf(rows, "b")).toEqual([sublead.name, subworker.name]);
  });
});


