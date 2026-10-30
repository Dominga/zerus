import { describe, expect, it } from "vitest";
import { activityRange, activityText } from "./activity-virtual";
import type { TranscriptItem } from "./types";

describe("Activity copy and virtual retention", () => {
  it("copies every loaded item as visible text, including only expanded disclosures", () => {
    const items: TranscriptItem[] = [
      { id: "first", kind: "agent", at: "10:00", body: "First **reply** with `code` and https://example.com." },
      { id: "tool", kind: "tools", at: "10:01", title: "Bash", body: "npm test", lines: ["hidden output"] },
      { id: "media", kind: "media", media: "voice", at: "10:02", state: "submitted", durationMs: 60_000,
        waveform: [], transcript: "voice transcript" },
      { id: "last", kind: "user", at: "10:03", body: "Last loaded message" },
    ];
    const collapsed = activityText(items);
    expect(collapsed).toContain("First reply with code and https://example.com.");
    expect(collapsed).toContain("Last loaded message");
    expect(collapsed).not.toContain("**reply**");
    expect(collapsed).not.toContain("hidden output");
    expect(collapsed).not.toContain("voice transcript");
    expect(collapsed.indexOf("First reply")).toBeLessThan(collapsed.indexOf("Last loaded message"));
    const expanded = activityText(items, { tool: true, media: true });
    expect(expanded).toContain("hidden output");
    expect(expanded).toContain("voice transcript");
  });

  it("keeps a loading or playing media row mounted while it is scrolled out", () => {
    expect(activityRange({ startIndex: 6, endIndex: 7, overscan: 1, count: 12 }, 1)).toEqual([1, 5, 6, 7, 8]);
  });

  it("retains the playing row in order without duplicating visible rows", () => {
    const range = { startIndex: 4, endIndex: 5, overscan: 1, count: 10 };
    expect(activityRange(range, -1)).toEqual([3, 4, 5, 6]);
    expect(activityRange(range, 0)).toEqual([0, 3, 4, 5, 6]);
    expect(activityRange(range, 8)).toEqual([3, 4, 5, 6, 8]);
    expect(activityRange(range, 5)).toEqual([3, 4, 5, 6]);
    expect(activityRange(range, 10)).toEqual([3, 4, 5, 6]);
  });
});
