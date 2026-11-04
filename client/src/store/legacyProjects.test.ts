import { describe, expect, it } from "vitest";
import { legacyProjectRow } from "./legacyProjects";

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
      { ...report("blocked"), reasons: Array(8).fill("legacy_projects_pending") },
      { ...report("blocked"), keys: ["projects", "projects"] },
      { ...report("blocked"), reasons: ["legacy_projects_history", "legacy_projects_history"] },
    ]) expect(legacyProjectRow(bad)).toBeUndefined();
  });
});
