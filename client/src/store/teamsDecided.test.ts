import { describe, expect, it } from "vitest";
import { decidedTeamAction, teams, waiting } from "./teams";
import type { TeamWaitingItem } from "../types";

// Task 107: a decision hgs confirmed takes what it decided out of Waiting for
// you at once (the strip, the page, the count), before any read; one that was
// not confirmed changes nothing.
const item = (id: string, proposal: string): TeamWaitingItem =>
  ({ id: `t1/owner_word_asked/${id}`, kind: "owner_word_asked", team: "t1", team_name: "Live", title: id, action: "decide_proposal", since: 1, proposal, detail: {} });

describe("Waiting for you after a confirmed decision", () => {
  it("drops the decided item at once and keeps it gone while an older read lists it", () => {
    const listed = [item("proposal-p1", "p1"), item("proposal-p2", "p2")];
    teams.apply([{ computer: "", key: "waiting", kind: "waiting", payload: { items: listed, total: 2, more: false } }]);
    expect(waiting()?.items.map((shown) => shown.id)).toEqual(["t1/owner_word_asked/proposal-p1", "t1/owner_word_asked/proposal-p2"]);
    decidedTeamAction({ team: "t1", action: { kind: "decide_proposal", proposal: "p1", accept: true, expected_hash: "h1" } });
    expect(waiting()?.items.map((shown) => shown.id)).toEqual(["t1/owner_word_asked/proposal-p2"]);
    expect(waiting()?.total).toBe(1);
    // A read that predates the decision still lists p1: it stays out.
    teams.apply([{ computer: "", key: "waiting", kind: "waiting", payload: { items: listed, total: 2, more: false } }]);
    expect(waiting()?.items.map((shown) => shown.id)).toEqual(["t1/owner_word_asked/proposal-p2"]);
    // Another computer's action decides nothing here.
    decidedTeamAction({ computer: "mac", team: "t1", action: { kind: "decide_proposal", proposal: "p2", accept: true, expected_hash: "h2" } });
    expect(waiting()?.items).toHaveLength(1);
    // The store's read without p1.
    teams.apply([{ computer: "", key: "waiting", kind: "waiting", payload: { items: [listed[1]], total: 1, more: false } }]);
    expect(waiting()?.total).toBe(1);
  });
});
