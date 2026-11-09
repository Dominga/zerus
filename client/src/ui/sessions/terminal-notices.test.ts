import { describe, expect, it } from "vitest";
import { terminalNoticeLabel, terminalNoticeRegions } from "./terminal-notices";

// hgs's header format separates its fields with U+00B7.
const header = "[Zerus team \u00b7 from: Lead \u00b7 task: #360 \u00b7 attempt: 1]";
const lines = [
  header,
  "Team: Zerus Next for the fork. Your role: Frontend (DeepSeek).",
  "Task #360: fold the notices.",
  "",
  "The lead's first reply line.",
  "A second reply line.",
  "❯ ",
  "owner message",
  "[Zerus team] 1 new message",
  "A doorbell.",
  "❯ next prompt",
];

describe("terminal team-notice folds", () => {
  it("folds a pasted header and the reply up to the next prompt", () => {
    const [region] = terminalNoticeRegions(lines, 123);
    expect(region).toMatchObject({ start: 0, end: 6, from: "Lead", kind: "Task from team", line: "Team: Zerus Next for the fork. Your role: Frontend (DeepSeek).", at: 123 });
    expect(terminalNoticeLabel(region)).toContain("Lead, Task from team");
    expect(terminalNoticeLabel(region)).toContain("Team: Zerus Next");
  });

  it("also folds Zerus's sender-less notices", () => {
    const regions = terminalNoticeRegions(lines, 0);
    expect(regions.map((region) => [region.start, region.end, region.from])).toEqual([[0, 6, "Lead"], [8, 10, "Zerus team"]]);
  });

  it("leaves an unfinished reply and an owner prompt alone", () => {
    expect(terminalNoticeRegions([header, "still writing"], 0)).toEqual([]);
    expect(terminalNoticeRegions(["❯ owner message", "reply"], 0)).toEqual([]);
  });
});
