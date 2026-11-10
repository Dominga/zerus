import { describe, expect, it, vi } from "vitest";
import catalog from "../../i18n.ru.json";

// The spend and session usage texts in Russian, through the shipped catalog.
vi.mock("../../i18n", async (original) => {
  const i18n = await original<typeof import("../../i18n")>();
  return { ...i18n, uiLanguage: () => "ru", tr: (source: string, ...args: (string | number)[]) => i18n.translateWithCatalog(catalog, "ru", source, ...args) };
});
const { compactCount, localUsageGroups, spendNotes, spendRows } = await import("./deepseek-usage");

describe("DeepSeek spend and usage in Russian", () => {
  it("translates labels, notes, day names and compact counts", () => {
    const spend = { since: new Date(2026, 10, 3, 14, 20).getTime() / 1000, today: "2026-11-10", periods: [
      { period: "today", amounts: [{ currency: "USD", spent: "0.14", granted: "0.00" }], top_up: true, gap_from: null }] };
    expect(spendRows(spend).map((r) => r.label)).toEqual(["Потрачено сегодня", "Потрачено за месяц"]);
    expect(spendNotes(spend)).toEqual([
      "По всей учётной записи, по изменениям баланса с 3 ноября 14:20. Пополнения не учитываются.",
      "Интервал между проверками с пополнением не учитывается.",
    ]);
    expect(localUsageGroups({ today: "2026-11-10", partial: false, days: [{ day: "2026-11-02", models: [] }] }).days[0].model).toBe("2 ноября");
    expect(compactCount(16_549_760)).toBe("16,5 млн");
  });
});
