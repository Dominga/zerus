import { expect, it } from "vitest";
import fixture from "./fixtures/lanes-view.json";
import type { TeamView } from "../../store/teams";
import { laneGroups } from "./lanes";
const data = fixture as unknown as { before: TeamView; after: TeamView; closed: TeamView; empty: TeamView; pages: TeamView[]; ids: Record<string, number> };
const nodes = (view: TeamView, member: string | null) => laneGroups(view).find((lane) => lane.member?.id === member || (!lane.member && member === null))!.tasks.map((entry) => entry.task.node);

it("keeps every product member and one unassigned lane, without mutating the page", () => {
  const copy = structuredClone(data.before);
  const groups = laneGroups(data.before);
  expect(groups.map((lane) => lane.member?.id ?? null)).toEqual([...data.before.members.map((member) => member.id), null]);
  const actual = groups.flatMap((lane) => lane.tasks.map((entry) => entry.task.node));
  const expected = data.before.groups.flatMap((group) => group.cards.map((card) => card.node));
  expect(actual.sort((a, b) => a - b)).toEqual(expected.sort((a, b) => a - b)); expect(new Set(actual).size).toBe(actual.length);
  expect(data.before).toEqual(copy);
});

it("uses the review's verified holder and keeps it separate from its implementer", () => {
  expect(nodes(data.before, "m3")).toEqual([1]); expect(nodes(data.before, "m1")).toEqual([data.ids.active]);
  const orphan = data.pages.find((view) => view.groups.some((group) => group.cards.some((card) => card.kind === "stage")))!;
  expect(nodes(orphan, "m3")).toEqual([data.ids.review]); expect(nodes(orphan, "m1")).toEqual([]);
});

it("puts current work before queued work after a real routing update", () => {
  expect(nodes(data.before, "m2")).toEqual([data.ids.assigned]);
  expect(nodes(data.after, "m1")).toEqual([data.ids.active, data.ids.assigned]);
  expect(nodes(data.after, "m2")).toEqual([]);
});

it("keeps queue rank order and breaks a real equal-rank tie by task UUID", () => {
  expect(nodes(data.before, null)).toEqual([data.ids.early, data.ids.tie_low, data.ids.tie_high, data.ids.late]);
});

it("does not reconstruct settled history after the product archives accepted and cancelled tasks", () => {
  expect(data.closed.groups.flatMap((group) => group.cards)).toEqual([]);
  expect(laneGroups(data.closed).flatMap((lane) => lane.tasks)).toEqual([]);
  expect(laneGroups(data.closed)).toHaveLength(data.closed.members.length + 1);
});

it("keeps empty roster lanes and Unassigned on an empty product page", () => {
  const groups = laneGroups(data.empty);
  expect(groups).toHaveLength(data.empty.members.length + 1);
  expect(groups.every((group) => group.tasks.length === 0)).toBe(true);
});

it("keeps work visible as unassigned when its holder is missing from a synthetic UI roster", () => {
  // A negative scope control: the product card is unchanged; one member is withheld.
  const view = { ...data.before, members: data.before.members.filter((member) => member.id !== "m3") };
  expect(nodes(view, null)).toContain(1);
});

it("ignores a contradictory synthetic stage target instead of adopting its worker", () => {
  // Deliberate corruption of one nested metadata field, not a positive backend row.
  const view = structuredClone(data.before);
  const task = view.groups.flatMap((group) => group.cards).find((task) => task.node === 1)!;
  task.stages[0].target = 999;
  expect(nodes(view, "m3")).toEqual([]); expect(nodes(view, "m1")).toContain(1);
});
