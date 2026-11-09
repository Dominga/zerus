import { describe, expect, it } from "vitest";
import { toSession } from "../../store/catalog";
import type { TeamSummary, ViewRow } from "../../types";
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


