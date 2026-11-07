import { describe, expect, it } from "vitest";
import { collapseTeams } from "./collapsed-teams";

describe("workspace/collapsedTeams", () => {
  it("adds a folded team and keeps the list stable for storage", () => {
    expect(collapseTeams(["b"], "a", true)).toEqual(["a", "b"]);
  });
  it("removes an unfolded team and ignores an unknown one", () => {
    expect(collapseTeams(["a", "b"], "a", false)).toEqual(["b"]);
    expect(collapseTeams(["a"], "missing", false)).toEqual(["a"]);
  });
});
