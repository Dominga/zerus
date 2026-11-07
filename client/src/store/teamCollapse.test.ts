import { describe, expect, it } from "vitest";
import { MAX_COLLAPSED_TEAMS, parseTeams, teamId, withTeam } from "./teamCollapse";

const MACHINE = "11111111-1111-4111-8111-111111111111";
const id = (n: number) => `${MACHINE}\n${`${n}`.padStart(8, "0")}-2222-4222-8222-222222222222`;

describe("collapsed parents survive a relaunch (zerus-sa9qn.15)", () => {
  it("names a parent by machine UUID and session ID only", () => {
    expect(teamId(MACHINE, "22222222-2222-4222-8222-222222222222")).toBe(`${MACHINE}\n22222222-2222-4222-8222-222222222222`);
    expect(teamId(MACHINE, undefined)).toBeUndefined();
    expect(teamId(undefined, "22222222-2222-4222-8222-222222222222")).toBeUndefined();
    expect(teamId("arch", "claude/p/lead")).toBeUndefined();
  });

  it("reads only well-formed IDs, once each", () => {
    expect(parseTeams("")).toEqual([]);
    expect(parseTeams("not json")).toEqual([]);
    expect(parseTeams(JSON.stringify({ a: 1 }))).toEqual([]);
    expect(parseTeams(JSON.stringify([id(1), "claude/p/lead", 7, id(1), `${MACHINE}\nname`, id(2)]))).toEqual([id(1), id(2)]);
  });

  it("keeps the newest choices within the bound, expanded unstored", () => {
    expect(withTeam([id(1), id(2)], id(1), true)).toEqual([id(2), id(1)]);
    expect(withTeam([id(1), id(2)], id(1), false)).toEqual([id(2)]);
    const full = Array.from({ length: MAX_COLLAPSED_TEAMS }, (_, n) => id(n));
    const next = withTeam(full, id(MAX_COLLAPSED_TEAMS), true);
    expect(next).toHaveLength(MAX_COLLAPSED_TEAMS);
    expect(next[0]).toBe(id(1));
    expect(next.at(-1)).toBe(id(MAX_COLLAPSED_TEAMS));
    expect(parseTeams(JSON.stringify([...full, id(MAX_COLLAPSED_TEAMS)]))).toHaveLength(MAX_COLLAPSED_TEAMS);
  });
});
