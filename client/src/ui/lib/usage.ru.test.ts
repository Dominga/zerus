import { describe, expect, it, vi } from "vitest";
import catalog from "../../i18n.ru.json";
import { usageOf } from "../../store/testing";

// The compact DeepSeek balance and its hover text in Russian, through the shipped catalog.
vi.mock("../../i18n", async (original) => {
  const i18n = await original<typeof import("../../i18n")>();
  return { ...i18n, tr: (source: string, ...args: (string | number)[]) => i18n.translateWithCatalog(catalog, "ru", source, ...args) };
});
const { balanceCompact, balanceTitle, stateText } = await import("./usage");

describe("DeepSeek balance in Russian", () => {
  it("keeps the symbol amount and names the balance and its parts", () => {
    const u = usageOf({ provider: "dsh", status: "configured", balances: [
      { kind: "Total", balance: "6.20", currency: "USD" }, { kind: "Topped up", balance: "6.20", currency: "USD" }, { kind: "Granted", balance: "0.00", currency: "USD" }] });
    expect(balanceCompact(u)).toBe("$6.20");
    expect(balanceTitle(u)).toBe("Баланс: 6.20 USD\nПополнено: 6.20 USD\nНачислено: 0.00 USD");
    expect(stateText(usageOf({ provider: "dsh", status: "configured" }))).toBe("Ключ API настроен. Zerus не удалось получить баланс DeepSeek; обновите, чтобы повторить.");
  });
});
