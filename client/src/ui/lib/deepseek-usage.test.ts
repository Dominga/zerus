import { describe, expect, it } from "vitest";
import { compactCount, exactCount, localUsageGroups, localUsageNotes, spendNotes, spendRows } from "./deepseek-usage";
import { dayTime } from "./usage";
import type { LocalUsage, Spend } from "../../types";

const SINCE = new Date(2026, 10, 3, 14, 20).getTime() / 1000;
const spend = (extra: Partial<Spend> = {}): Spend => ({
  since: SINCE, today: "2026-11-10",
  periods: [
    { period: "today", amounts: [{ currency: "USD", spent: "0.14", granted: "0.00" }], top_up: false, gap_from: null },
    { period: "month", amounts: [{ currency: "USD", spent: "1.20", granted: "0.00" }], top_up: false, gap_from: null },
  ],
  ...extra,
});

describe("DeepSeek spend from balance readings", () => {
  it("shows today's and this month's spend as symbol amounts with exact titles", () => {
    expect(spendRows(spend())).toEqual([
      { label: "Spent today", value: "$0.14", title: "0.14 USD" },
      { label: "Spent this month", value: "$1.20", title: "1.20 USD" },
    ]);
    expect(spendNotes(spend())).toEqual([`Account-wide, from balance changes since ${dayTime(SINCE)}. Top-ups excluded.`]);
    expect(spendRows(spend({ periods: [] }))[0].value).toBe("—");
  });

  it("says when granted balance, a top-up or a gap across the period start is involved", () => {
    const gap = new Date(2026, 10, 9, 22, 10).getTime() / 1000;
    const notes = spendNotes(spend({ periods: [
      { period: "today", amounts: [{ currency: "USD", spent: "0.14", granted: "0.00" }], top_up: true, gap_from: gap },
      { period: "month", amounts: [{ currency: "USD", spent: "1.20", granted: "0.10" }, { currency: "CNY", spent: "0.00", granted: "0.00" }], top_up: false, gap_from: null },
    ] }));
    expect(notes).toEqual([
      `Account-wide, from balance changes since ${dayTime(SINCE)}. Top-ups excluded.`,
      "Includes $0.10 of granted balance, which can also be expired grants.",
      "A check interval with a top-up is left out.",
      `Today includes spend since ${dayTime(gap)}, the previous balance check.`,
    ]);
  });
});

describe("this computer's DeepSeek session usage", () => {
  const usage: LocalUsage = {
    today: "2026-11-10", partial: false, reading: false, unreadable: 0,
    days: [
      { day: "2026-11-10", models: [{ model: "deepseek-flash", requests: 172, cache_hit: 16_549_760, cache_miss: 327_667, output: 172_520 }] },
      { day: "2026-11-02", models: [{ model: "deepseek-v4-pro", requests: 200, cache_hit: 10, cache_miss: 2, output: 1 }, { model: "deepseek-flash", requests: 8, cache_hit: 40, cache_miss: 3, output: 2 }] },
      { day: "2026-10-30", models: [{ model: "deepseek-flash", requests: 1, cache_hit: 1, cache_miss: 1, output: 1 }] },
    ],
  };

  it("groups today and this month per model and totals each day", () => {
    const groups = localUsageGroups(usage);
    expect(groups.today).toEqual([{ model: "deepseek-flash", requests: 172, cache_hit: 16_549_760, cache_miss: 327_667, output: 172_520 }]);
    expect(groups.month).toEqual([
      { model: "deepseek-v4-pro", requests: 200, cache_hit: 10, cache_miss: 2, output: 1 },
      { model: "deepseek-flash", requests: 180, cache_hit: 16_549_800, cache_miss: 327_670, output: 172_522 },
    ]);
    expect(groups.days.map((d) => [d.model, d.requests])).toEqual([["10 Nov", 172], ["2 Nov", 208], ["30 Oct", 1]]);
    expect(localUsageGroups({ ...usage, days: usage.days.slice(1) }).today).toEqual([]);
  });

  it("says when logs are still being read or could not be read", () => {
    expect(localUsageNotes(usage)).toEqual([]);
    expect(localUsageNotes({ ...usage, partial: true, reading: true })).toEqual(["Still reading session logs; totals may grow."]);
    expect(localUsageNotes({ ...usage, partial: true, unreadable: 2 })).toEqual(["2 session log(s) could not be read and are not counted."]);
    expect(localUsageNotes({ ...usage, partial: true, reading: true, unreadable: 1 })).toHaveLength(2);
  });

  it("writes large counts compactly and exactly", () => {
    expect(compactCount(16_549_760)).toBe("16.5M");
    expect(compactCount(172)).toBe("172");
    expect(exactCount(16_549_760)).toBe("16,549,760");
  });
});
