import { describe, expect, it } from "vitest";
import {
  ALL_UNTIL, addDays, applyDraft, draftHint, draftValid, editDraft, initialRange, localDay, panelPlace, pickerLabel, tabTarget, rangeBounds, rangeCaption,
  stageDraft, startOfDay, type DateRange,
} from "./archive-dates";

// A fixed local noon, so day arithmetic never meets a midnight edge.
const NOW = new Date(2026, 9, 28, 12, 0, 0);
const TODAY = "2026-10-28";
const local = (y: number, m: number, d: number) => new Date(y, m - 1, d).getTime() / 1000;

describe("days", () => {
  it("reads the local day and moves by whole days across month and year ends", () => {
    expect(localDay(NOW)).toBe(TODAY);
    expect(addDays(TODAY, -29)).toBe("2026-09-29");
    expect(addDays("2026-12-31", 1)).toBe("2027-01-01");
    expect(addDays("2024-03-01", -1)).toBe("2024-02-29");
  });

  it("starts a day at local midnight (QDate::startOfDay)", () => {
    expect(startOfDay("2026-10-15")).toBe(local(2026, 10, 15));
  });
});

describe("the applied range (ArchiveDatePicker::range)", () => {
  it("defaults to All dates, with the custom range at the last 30 days", () => {
    const r = initialRange(NOW);
    expect(r).toEqual({ period: 0, first: "2026-09-29", last: TODAY });
    expect(rangeBounds(r, NOW)).toEqual([0, ALL_UNTIL]);
    expect(ALL_UNTIL).toBe(253402300800);
    expect(rangeCaption(r)).toBe("All dates");
  });

  it("bounds the presets by today, through the end of today", () => {
    const end = local(2026, 10, 29);
    const custom = { first: "2026-01-01", last: "2026-01-02" };
    expect(rangeBounds({ period: 1, ...custom }, NOW)).toEqual([local(2026, 10, 28), end]);
    expect(rangeBounds({ period: 2, ...custom }, NOW)).toEqual([local(2026, 10, 22), end]);
    expect(rangeBounds({ period: 3, ...custom }, NOW)).toEqual([local(2026, 9, 29), end]);
    expect(rangeBounds({ period: 4, ...custom }, NOW)).toEqual([local(2026, 1, 1), local(2026, 1, 3)]);
  });

  it("captions the presets and the custom range with an en dash", () => {
    const base = { first: "2026-10-15", last: "2026-10-15" };
    expect(rangeCaption({ period: 1, ...base })).toBe("Today");
    expect(rangeCaption({ period: 2, ...base })).toBe("Last 7 days");
    expect(rangeCaption({ period: 3, ...base })).toBe("Last 30 days");
    expect(rangeCaption({ period: 4, ...base })).toBe("15 Oct 2026");
    expect(rangeCaption({ period: 4, first: "2026-10-01", last: "2026-10-15" })).toBe("1 Oct – 15 Oct 2026");
    expect(rangeCaption({ period: 4, first: "2025-12-30", last: "2026-01-02" })).toBe("30 Dec 2025 – 2 Jan 2026");
    expect(pickerLabel({ period: 2, ...base })).toBe("Archived date: Last 7 days. Choose period");
  });
});

describe("the draft (presets and dates change only the draft; Apply commits)", () => {
  const applied: DateRange = { period: 0, first: "2026-09-29", last: TODAY };

  it("stages a preset's dates, Any date for All dates, and the applied custom range", () => {
    expect(stageDraft(0, applied, NOW)).toEqual({ period: 0, from: "", until: "" });
    expect(stageDraft(1, applied, NOW)).toEqual({ period: 1, from: TODAY, until: TODAY });
    expect(stageDraft(2, applied, NOW)).toEqual({ period: 2, from: "2026-10-22", until: TODAY });
    expect(stageDraft(3, applied, NOW)).toEqual({ period: 3, from: "2026-09-29", until: TODAY });
    expect(stageDraft(4, { period: 4, first: "2026-10-01", last: "2026-10-02" }, NOW)).toEqual({ period: 4, from: "2026-10-01", until: "2026-10-02" });
  });

  it("turns an edit into a custom range, supplying the companion date from All dates", () => {
    expect(editDraft({ period: 0, from: "", until: "" }, "from", "2026-10-15", NOW)).toEqual({ period: 4, from: "2026-10-15", until: TODAY });
    expect(editDraft({ period: 0, from: "", until: "" }, "until", "2026-10-20", NOW)).toEqual({ period: 4, from: "2026-09-29", until: "2026-10-20" });
    expect(editDraft({ period: 1, from: TODAY, until: TODAY }, "from", "2026-10-18", NOW)).toEqual({ period: 4, from: "2026-10-18", until: TODAY });
  });

  it("checks the order and words the hint as Qt", () => {
    expect(draftValid({ period: 0, from: "", until: "" })).toBe(true);
    expect(draftHint({ period: 0, from: "", until: "" })).toBe("All archived dates (no date limits)");
    expect(draftValid({ period: 4, from: "2026-10-15", until: "2026-10-15" })).toBe(true);
    expect(draftHint({ period: 4, from: "2026-10-15", until: "2026-10-15" })).toBe("Both dates included (local time)");
    expect(draftValid({ period: 4, from: "2026-10-15", until: "2026-10-14" })).toBe(false);
    expect(draftHint({ period: 4, from: "2026-10-15", until: "2026-10-14" })).toBe("End date must be on or after the start date.");
    expect(draftValid({ period: 4, from: "", until: TODAY })).toBe(false);
  });

  it("takes only days QDateEdit can hold: a four-digit year from 1970-01-02 through 9999-12-30", () => {
    const custom = (from: string, until: string) => ({ period: 4 as const, from, until });
    expect(draftValid(custom("1970-01-02", "9999-12-30"))).toBe(true);
    for (const [from, until] of [["0099-10-15", TODAY], ["1970-01-01", TODAY], [TODAY, "9999-12-31"], ["", TODAY], [TODAY, ""], ["", ""], ["+12026-01-01", TODAY]]) {
      const d = custom(from, until);
      expect(draftValid(d), `${from} ${until}`).toBe(false);
      // Qt's edits cannot hold such a day: Apply stays off with the neutral hint, never the order error.
      expect(draftHint(d)).toBe("Both dates included (local time)");
      expect(applyDraft(d, applied)).toBeUndefined();
    }
    expect(draftHint(custom("2026-10-15", "2026-10-14"))).toBe("End date must be on or after the start date.");
  });

  it("applies a valid draft and says whether the range changed", () => {
    expect(applyDraft({ period: 0, from: "", until: "" }, applied)).toEqual({ range: applied, changed: false });
    expect(applyDraft({ period: 2, from: "2026-10-22", until: TODAY }, applied)).toEqual({ range: { ...applied, period: 2 }, changed: true });
    const custom = applyDraft({ period: 4, from: "2026-10-18", until: TODAY }, applied);
    expect(custom).toEqual({ range: { period: 4, first: "2026-10-18", last: TODAY }, changed: true });
    expect(applyDraft({ period: 4, from: "2026-10-18", until: TODAY }, custom!.range)?.changed).toBe(false);
    expect(applyDraft({ period: 4, from: "2026-10-15", until: "2026-10-14" }, applied)).toBeUndefined();
  });
});

describe("the picker panel's place (openPanel), in viewport coordinates", () => {
  const view = { width: 1440, height: 900 };
  const button = { left: 64.5, top: 120, bottom: 152 };

  it("sits at the button's left, 6 px below it, 480 px wide", () => {
    expect(panelPlace(button, 300, view)).toEqual({ left: 64.5, top: 158, width: 480 });
  });

  it("goes above the button when it does not fit below, and stays inside the window", () => {
    expect(panelPlace({ left: 64, top: 700, bottom: 732 }, 300, view)).toEqual({ left: 64, top: 394, width: 480 });
    expect(panelPlace({ left: 1300, top: 120, bottom: 152 }, 300, view).left).toBe(952);
    expect(panelPlace({ left: 2, top: 10, bottom: 40 }, 880, view)).toEqual({ left: 8, top: 8, width: 480 });
    expect(panelPlace(button, 300, { width: 400, height: 900 })).toEqual({ left: 8, top: 158, width: 384 });
  });
});

describe("Tab inside the panel (Qt's popup keeps the focus in it)", () => {
  it("wraps from the last control to the first and back, and brings an outside focus in", () => {
    expect(tabTarget(4, 3, false)).toBe(0);
    expect(tabTarget(4, 0, true)).toBe(3);
    expect(tabTarget(4, 1, false)).toBe(2);
    expect(tabTarget(4, 2, true)).toBe(1);
    expect(tabTarget(4, -1, false)).toBe(0);
    expect(tabTarget(4, -1, true)).toBe(3);
    expect(tabTarget(0, -1, false)).toBeUndefined();
  });
});
