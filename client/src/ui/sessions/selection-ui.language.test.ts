import { describe, expect, it, vi } from "vitest";

// The selection bar and menu in Russian, through the real catalog (zerus-wi352).
vi.mock("../../i18n", async (original) => {
  const m = await original<typeof import("../../i18n")>();
  const ru = (await import("../../i18n.ru.json")).default;
  return { ...m, tr: (source: string, ...args: (string | number)[]) => m.translateWithCatalog(ru, "ru", source, ...args) };
});
import { barTools, selectionMenu } from "./selection-ui";
import type { SelectionAction } from "./selection";

const only = (action: SelectionAction, n: number) => (a: SelectionAction) => (a === action ? n : 0);

describe("the selection hints in Russian (zerus-wi352)", () => {
  it("translates the whole enabled Move hint, with the plural form of the selection", () => {
    expect(barTools(only("move", 2), 2, false, true).map((t) => t.hint)).toEqual(["Переместить в проект — 2 из 2 выбранных сессий"]);
    expect(barTools(only("move", 1), 1, false, true).map((t) => t.hint)).toEqual(["Переместить в проект — 1 из 1 выбранной сессии"]);
    expect(barTools(only("move", 3), 21, false, true).map((t) => t.hint)).toEqual(["Переместить в проект — 3 из 21 выбранной сессии"]);
  });
  it("translates every other tool's hint and keeps the translated wait reasons", () => {
    expect(barTools(only("pause", 2), 5, false, true)[0].hint).toBe("Приостановить — 2 из 5 выбранных сессий");
    expect(barTools(only("move", 2), 2, true, true)[0].hint).toBe("Дождитесь окончания текущего действия.");
    expect(barTools(only("move", 2), 2, false, true, "Синхронизация проектов запускается.")[0]).toMatchObject({ enabled: false, hint: "Синхронизация проектов запускается." });
  });
  it("translates the selection menu's heading with its plural form", () => {
    const heading = (n: number) => selectionMenu(only("move", n), n, false, () => {}, () => {})[0];
    expect([heading(1), heading(3), heading(5)]).toEqual([{ heading: "1 выбранная сессия" }, { heading: "3 выбранные сессии" }, { heading: "5 выбранных сессий" }]);
  });
});
