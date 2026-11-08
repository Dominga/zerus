import { describe, expect, it } from "vitest";
import { Virtualizer } from "@tanstack/solid-virtual";
import { activityFlow, activityGeometryValid, activityRange, activityText, checkActivityGeometry, type ActivityGeometryRow } from "./activity-virtual";
import { loadMarkdown } from "./markdown";
import type { TranscriptItem } from "./types";
import { ACTIVITY_RESIZE_OVERSCAN } from "./activity-heights";
import { TOOL_OUTPUT } from "./tool-output";

const geometryRow = (index: number, top: number, bottom: number, start = top): ActivityGeometryRow => ({
  index, key: `item-${index}`, top, bottom, height: bottom - top, start, domStart: top,
});

describe("Activity copy and virtual retention", () => {
  it("copies every loaded item as visible text, including only expanded disclosures", async () => {
    await loadMarkdown();
    const items: TranscriptItem[] = [
      { id: "first", kind: "agent", at: "10:00", body: "First **reply** with `code` and https://example.com." },
      { id: "tool", kind: "tools", at: "10:01", title: "Bash", body: "npm test",
        events: [{ label: "Tool finished / Bash", at: "10:01", size: 13, detail: () => "hidden output" }] },
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
    expect(expanded).toContain("Tool finished / Bash");
    expect(expanded).toContain("hidden output");
    expect(expanded).toContain("voice transcript");
  });

  it("copies an expanded tool output as far as it shows (zerus-5wmh.13.8.1)", () => {
    const big = "z".repeat(5_000_000);
    const items: TranscriptItem[] = [{ id: "tool", kind: "tool", at: "10:01", title: "Tool finished / Bash", body: "z…",
      events: [{ label: "Tool finished / Bash", at: "10:01", size: big.length, detail: () => big }] }];
    expect(activityText(items, { tool: true }).length).toBeLessThan(TOOL_OUTPUT.first + 200);
    expect(activityText(items, { tool: true }, { "tool#0": TOOL_OUTPUT.first + TOOL_OUTPUT.step }))
      .toContain("z".repeat(TOOL_OUTPUT.first + TOOL_OUTPUT.step));
  });

  it("copies thinking and an opened subagent report as their Markdown shows (zerus-5wmh.13.8.2)", async () => {
    await loadMarkdown();
    const items: TranscriptItem[] = [
      { id: "think", kind: "thinking", at: "10:00", body: "Check **the** `config` first" },
      { id: "report", kind: "report", at: "10:01", title: "Done", body: "# Result\n\n- **one** item" },
    ];
    expect(activityText(items)).toContain("Check the config first");
    expect(activityText(items)).not.toContain("**");
    expect(activityText(items)).not.toContain("one item");
    expect(activityText(items, { report: true })).toContain("Result\none item");
  });

  it("keeps a loading or playing media row mounted while it is scrolled out", () => {
    expect(activityRange({ startIndex: 6, endIndex: 7, overscan: 1, count: 12 }, 1)).toEqual([1, 5, 6, 7, 8]);
  });

  it("keeps the playing and selected rows and a resize margin around its visible window", () => {
    const virtualizer = new Virtualizer<HTMLElement, HTMLElement>({
      count: 100, getScrollElement: () => null, estimateSize: () => 100,
      initialRect: { width: 400, height: 300 }, initialOffset: 5000,
      observeElementRect: () => undefined, observeElementOffset: () => undefined,
      scrollToFn: () => {}, overscan: ACTIVITY_RESIZE_OVERSCAN,
      rangeExtractor: range => activityRange(range, 0, [49, 90]),
    });
    expect(virtualizer.getVirtualItems().map(item => item.index)).toEqual([0, 49, 50, 51, 52, 53, 90]);
  });

  it("copies the current CommonMark tree without losing nested text or table cells", async () => {
    await loadMarkdown();
    const text = activityText([{ id: "nested", kind: "agent", at: "10:00", body:
      "# Heading\n\n> A **quoted** ~~reply~~.\n\n- First\n  - Nested `code`\n\n| Name | Value |\n| --- | --- |\n| One | **Two** |" }]);
    expect(text).toContain("Heading");
    expect(text).toContain("A quoted reply.");
    expect(text).toContain("Nested code");
    expect(text).toContain("Name\tValue\nOne\tTwo");
    expect(text).not.toContain("**");
    expect(text).not.toContain("~~");
  });

  it("retains the playing row in order without duplicating visible rows", () => {
    const range = { startIndex: 4, endIndex: 5, overscan: 1, count: 10 };
    expect(activityRange(range, -1)).toEqual([3, 4, 5, 6]);
    expect(activityRange(range, 0)).toEqual([0, 3, 4, 5, 6]);
    expect(activityRange(range, 8)).toEqual([3, 4, 5, 6, 8]);
    expect(activityRange(range, 5)).toEqual([3, 4, 5, 6]);
    expect(activityRange(range, 10)).toEqual([3, 4, 5, 6]);
    expect(activityRange(range, 0, [1, 2, 9, 9, -1, 10])).toEqual([0, 1, 2, 3, 4, 5, 6, 9]);
  });
});

describe("Activity virtual geometry", () => {
  it("accepts disjoint mounted rows with a visible first row", () => {
    const report = checkActivityGeometry([geometryRow(4, 100, 220, 0), geometryRow(5, 230, 410, 130)], 100, 500);
    expect(report.overlaps).toEqual([]);
    expect(report.firstVisibleGap).toBe(0);
    expect(activityGeometryValid(report)).toBe(true);
  });

  it("reports painted and virtual-start overlap for the same adjacent rows", () => {
    const report = checkActivityGeometry([geometryRow(4, 100, 320, 0), geometryRow(5, 300, 350, 200)], 100, 500);
    expect(report.overlaps).toHaveLength(1);
    expect(report.overlaps[0]).toMatchObject({ pixels: 20, virtualPixels: 20 });
    expect(activityGeometryValid(report)).toBe(false);
  });

  it("rejects a missing partly visible top row even below the old 96 px estimate", () => {
    for (const pixels of [2, 5, 22, 96]) {
      const report = checkActivityGeometry([geometryRow(8, 100 + pixels, 350)], 100, 500);
      expect(report.firstVisibleGap).toBe(pixels);
      expect(activityGeometryValid(report)).toBe(false);
    }
    expect(activityGeometryValid(checkActivityGeometry([geometryRow(8, 100.5, 350)], 100, 500))).toBe(true);
  });

  it("allows the history header and an actual inter-row gap, without hiding missing rows", () => {
    expect(activityGeometryValid(checkActivityGeometry([geometryRow(0, 146, 350)], 100, 500))).toBe(true);
    expect(activityGeometryValid(checkActivityGeometry([geometryRow(7, 40, 98), geometryRow(8, 108, 350)], 100, 500))).toBe(true);
    expect(activityGeometryValid(checkActivityGeometry([geometryRow(6, 40, 98), geometryRow(8, 108, 350)], 100, 500))).toBe(false);
  });

  it("rejects a large first-visible gap and empty samples", () => {
    const gap = checkActivityGeometry([geometryRow(8, 220, 350, 120)], 100, 500);
    expect(gap.firstVisibleGap).toBe(120);
    expect(activityGeometryValid(gap)).toBe(false);
    expect(activityGeometryValid(checkActivityGeometry([], 100, 500))).toBe(false);
    expect(activityGeometryValid(checkActivityGeometry([geometryRow(8, -300, -200)], 100, 500))).toBe(false);
  });

  it("places flow rows by their first start and the space before each later row", () => {
    const flow = activityFlow([
      { key: "first", start: 500, end: 650 },
      { key: "second", start: 660, end: 900 },
      { key: "retained", start: 1400, end: 1500 },
    ]);
    expect(flow.padding).toBe(500);
    expect([...flow.margins.entries()]).toEqual([["first", 0], ["second", 10], ["retained", 500]]);
    expect(activityFlow([])).toEqual({ padding: 0, margins: new Map(), starts: new Map() });
  });
});
