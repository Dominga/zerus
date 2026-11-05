import { describe, expect, it } from "vitest";
import { legacyProjectRow, legacyProjectStatus } from "./legacyProjects";

const report = (state: string, reasons: string[] = []) => ({ family: "legacy-projects", state, keys: ["projects", "folders"], reasons });
describe("legacy project migration report", () => {
  it("distinguishes completion, unknown confirmation and manual review", () => {
    expect(legacyProjectRow(report("imported"))?.status).toBe("Imported");
    const pending = legacyProjectRow(report("waiting", ["legacy_projects_pending"]))!;
    expect(pending.status).toBe("Waiting for confirmation");
    expect(pending.reasons[0].text).toContain("will not be applied again");
    const blocked = legacyProjectRow(report("blocked", ["legacy_projects_history", "legacy_projects_windows_unsupported"]))!;
    expect(blocked.status).toBe("Needs review");
    expect(blocked.reasons[0].text).toContain("manually");
    expect(blocked.reasons[1].text).toContain("not shared");
    expect(blocked.keys).toEqual(["projects", "folders"]);
  });
  it("does not show an empty family as an earlier import", () => {
    expect(legacyProjectRow({ family: "legacy-projects", state: "empty", keys: [], reasons: [] })).toBeUndefined();
  });
  it("refuses unbounded, future or source-valued report fields", () => {
    for (const bad of [undefined, null, [], {}, report("future"),
      { ...report("blocked"), keys: ["/srv/private-fixture", "folders"] },
      { ...report("blocked"), reasons: ["private-project-name"] },
      { ...report("blocked"), reasons: Array(9).fill("legacy_projects_pending") },
      { ...report("blocked"), keys: ["projects", "projects"] },
      { ...report("blocked"), reasons: ["legacy_projects_history", "legacy_projects_history"] },
    ]) expect(legacyProjectRow(bad)).toBeUndefined();
  });
});

it("explains blocked read recovery without adding a retry-apply action", () => {
  const row = legacyProjectRow(report("blocked", ["legacy_projects_pending", "legacy_projects_read_blocked"]))!;
  expect(row.status).toBe("Needs review");
  expect(row.reasons[1].text).toContain("Reads retry automatically");
  expect(row.reasons[1].text).toContain("will not be applied again");
  expect(legacyProjectStatus("Project import verification is blocked. Retrying reads automatically.")).toContain("Retrying reads");
  expect(legacyProjectStatus("Unrelated status")).toBe("Unrelated status");
});

it("says a changed computer is checked again at the next start, keeping its fixed code (N1)", () => {
  const row = legacyProjectRow(report("blocked", ["legacy_projects_changed"]))!;
  expect(row.reasons).toEqual([{ code: "legacy_projects_changed", text: "The computer or project catalog changed. It is checked again the next time Zerus starts." }]);
});
