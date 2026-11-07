import { describe, expect, it } from "vitest";
import { ledTeamOf, leadsTeam, teamAnswers } from "./leadTeam";
import type { TeamList, TeamListedMember, TeamSummary } from "../../types";

// Qt U57 (SessionsWindow.cpp:7616-7626): the lead's own session shows its
// team's open proposals. A sub-team is handed to a member of the parent team,
// whose session becomes the sub-team's lead while staying a parent member, so
// the team a session leads must win over the team it belongs to.

const at = (host: string, name: string) => ({ host, name, run_id: "r", conversation_id: "c" });

function member(id: string, lead: boolean, name = `claude/p/${id}`): TeamListedMember {
  return { id, role: lead ? "Lead" : id, agent: "claude", state: "active", lead, session: at("", name) };
}

function team(id: string, over: Partial<TeamSummary> = {}): TeamSummary {
  return {
    id, name: id, status: "running", project: "", folder: "/work/p", plan_mode: "standard", lead: "lead",
    members: [member("lead", true, `claude/p/${id}-lead`)], tasks: 0, accepted: 0, submitted: 0, in_review: 0, in_test: 0,
    attention: 0, open_proposals: 0, waiting_for_lead: false, pending_phase: null, updated: 1, revision: 1, ...over,
  };
}

const list = (...teams: TeamSummary[]): TeamList => ({ teams, truncated: false });

describe("the team a session leads", () => {
  it("is the team whose lead it is", () => {
    const live = team("live", { members: [member("lead", true, "claude/p/lead")] });
    expect(ledTeamOf(list(live), { computer: "", name: "claude/p/lead" })?.id).toBe("live");
  });

  it("is the sub-team, not the parent team the same session is also a member of", () => {
    const parent = team("parent", { members: [member("lead", true, "claude/p/parent-lead"), member("dev", false, "claude/p/dev")] });
    // The parent task's assignee's session becomes the sub-team's lead (commands.rs parent_create).
    const child = team("child", { parent: "parent", members: [member("dev", true, "claude/p/dev")] });
    expect(ledTeamOf(list(parent, child), { computer: "", name: "claude/p/dev" })?.id).toBe("child");
    // The same list through a member who leads nothing.
    expect(ledTeamOf(list(parent, child), { computer: "", name: "claude/p/parent-lead" })?.id).toBe("parent");
  });

  it("is nothing for a member, an archived team or a session the list does not hold", () => {
    const parent = team("parent", { members: [member("lead", true, "claude/p/lead"), member("dev", false, "claude/p/dev")] });
    const gone = team("gone", { status: "archived", members: [member("dev", true, "claude/p/dev")] });
    expect(ledTeamOf(list(parent, gone), { computer: "", name: "claude/p/dev" })).toBeUndefined();
    expect(ledTeamOf(list(parent, gone), { computer: "", name: "claude/p/other" })).toBeUndefined();
    expect(ledTeamOf(undefined, { computer: "", name: "claude/p/dev" })).toBeUndefined();
    expect(ledTeamOf(list(parent), undefined)).toBeUndefined();
  });

  it("the live read says who leads, so a roster change drops the cards", () => {
    expect(leadsTeam({ lead: "dev", members: [member("dev", true, "claude/p/dev")] }, { computer: "", name: "claude/p/dev" })).toBe(true);
    // The lead flag alone names it too, as Qt's `member->lead || member->id == team->lead`.
    expect(leadsTeam({ lead: "dev", members: [member("dev", false, "claude/p/dev")] }, { computer: "", name: "claude/p/dev" })).toBe(true);
    expect(leadsTeam({ lead: "lead", members: [member("dev", false, "claude/p/dev")] }, { computer: "", name: "claude/p/dev" })).toBe(false);
  });
});

describe("the Sessions Team tab's \"?\" count", () => {
  it("holds what waits for the owner, open proposals included", () => {
    expect(teamAnswers(undefined)).toBe(0);
    expect(teamAnswers({ attention: 2, open_proposals: 0 })).toBe(2);
    // Qt U57: the lead's open proposals are owner decisions too.
    expect(teamAnswers({ attention: 2, open_proposals: 3 })).toBe(5);
  });
});
