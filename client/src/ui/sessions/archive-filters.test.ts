import { describe, expect, it } from "vitest";
import { archiveFilterCounts, archiveMachineChoices, archiveProjectChoices } from "./archive-filters";

const rows = [
  { computer: "", machine: "workstation", group: "alpha", archivedAt: 100 },
  { computer: "box", machine: "box", group: "alpha", archivedAt: 200 },
  { computer: "box", machine: "box", group: "beta", archivedAt: 300 },
  { computer: "box", machine: "box", group: "alpha", archivedAt: 10 },
];
const includes = (at: number) => at >= 100;
const headers = ["beta", "alpha", "empty", "hidden", "counted", "selected"].map((id) =>
  ({ id, name: id.toUpperCase(), color: null, palette: 0, accessible: ["beta", "alpha", "empty"].includes(id), collapsed: false, is_default: false }));
const grouping = { loaded: true, headers };

describe("Archive filter choices and counts (SessionsWindowRows.cpp:266-284, 578-581)", () => {
  it("counts dated archive rows before each corresponding filter, independent of the name's project", () => {
    const counts = archiveFilterCounts(rows, includes, ["box"], ["alpha"]);
    expect([...counts.machine]).toEqual([["", 1], ["box", 1]]);
    expect([...counts.project]).toEqual([["alpha", 1], ["beta", 1]]);
  });

  it("offers every fleet machine even when it has no loaded or live rows", () => {
    const counts = archiveFilterCounts(rows, includes, [], ["beta"]);
    expect(archiveMachineChoices([
      { computer: "", machine: "workstation" }, { computer: "box", machine: "box" }, { computer: "idle", machine: "idle" },
    ], counts.machine)).toEqual([
      { id: "workstation", label: "workstation (this machine)", count: 0, computer: "" },
      { id: "box", label: "box", count: 1, computer: "box" },
      { id: "idle", label: "idle", count: 0, computer: "idle" },
    ]);
  });

  it("offers accessible, counted and selected projects in organization order, including missing selections", () => {
    expect(archiveProjectChoices(grouping, new Map([["alpha", 2], ["counted", 1]]), ["selected", "missing"])).toEqual([
      { id: "beta", label: "BETA", count: 0 }, { id: "alpha", label: "ALPHA", count: 2 },
      { id: "empty", label: "EMPTY", count: 0 }, { id: "counted", label: "COUNTED", count: 1 },
      { id: "selected", label: "SELECTED", count: 0 }, { id: "missing", label: "missing", count: 0 },
    ]);
  });
});
