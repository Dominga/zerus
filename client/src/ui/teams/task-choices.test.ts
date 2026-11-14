import { describe, expect, it } from "vitest";
import fixture from "../sessions/fixtures/agent-team-views.json";
import type { TeamList, TeamSummary } from "../../store/teams";
import { taskTeamChoices } from "./task-choices";
const base = (fixture as unknown as { list: TeamList }).list.teams[0];
// Status/identity controls derive from the core-generated list template.
// These intentionally constructed controls are not positive backend fixtures.
const team = (status: string, id = status, name = status): TeamSummary => ({ ...base, status, id, name });

describe("task destinations shared by Tasks and TaskCreate", () => {
  it("offers only running and paused teams of the current project", () => {
    const rows = ["draft", "planning", "running", "paused", "stopped", "archived", "finished", "future"].map((s) => team(s));
    rows.push({ ...team("running", "other"), project: "other-project" });
    expect(taskTeamChoices(true, rows, base.project).map((t) => t.id)).toEqual(["running", "paused"]);
  });
  it("filters before applying the eight-item menu bound", () => {
    const rows = Array.from({ length: 8 }, (_, i) => team("archived", `old-${i}`));
    rows.push(team("running", "current"));
    expect(taskTeamChoices(true, rows, base.project).map((t) => t.id)).toEqual(["current"]);
    expect(taskTeamChoices(true, Array.from({ length: 12 }, (_, i) => team("running", `${i}`)), base.project)).toHaveLength(8);
  });
  it("disambiguates live names while retaining full destination IDs", () => {
    const a = team("running", "00000000-0000-4000-8000-00000000beef", "Fixes");
    const b = team("paused", "00000000-0000-4000-8000-00000000beee", "Fixes");
    const choices = taskTeamChoices(true, [a, b], base.project);
    expect(choices.map((c) => c.label)).toEqual(["Fixes (0000beef)", "Fixes (0000beee)"]);
    expect(choices.map((c) => c.id)).toEqual([a.id, b.id]);
    expect(taskTeamChoices(true, [a, { ...b, status: "archived" }], base.project)[0].label).toBe("Fixes");
  });
  it("extends colliding short IDs and preserves preview off and unknown", () => {
    const a = team("running", "11111111-1111-4111-8111-11110000beef", "Same");
    const b = team("running", "22222222-2222-4222-8222-22220000beef", "Same");
    const labels = taskTeamChoices(true, [a, b], base.project).map((c) => c.label);
    expect(new Set(labels).size).toBe(2);
    for (const available of [false, undefined]) expect(taskTeamChoices(available, [a, b], base.project)).toEqual([]);
    expect(taskTeamChoices(true, [a], undefined)).toEqual([]);
  });
});
