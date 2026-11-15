import { expect, it, vi } from "vitest";
import catalog from "../../i18n.ru.json";
import fixture from "./fixtures/phase-pane.json";
import type { PhaseRecord } from "../../store/team-phases";
import { phaseHeader } from "./phaseHeader";

vi.mock("../../i18n", async (original) => {
  const module = await original<typeof import("../../i18n")>();
  return { ...module, tr: (text: string, ...args: (string | number)[]) => module.translateWithCatalog(catalog, "ru", text, ...args) };
});

it("translates the built-in queue header produced by the real CLI and core", () => {
  const queue = fixture.plan.list.rows[0] as PhaseRecord;
  expect(queue.title).toBe("Queue");
  expect(phaseHeader(queue).title).toBe("Очередь");
  expect(phaseHeader(fixture.plan.draft.phase as PhaseRecord).state).toBe("Черновик");
});

it("keeps authored titles verbatim even when they match a catalog key", () => {
  // Formatter boundary controls derived from the producer's phase records.
  const plan = { ...fixture.plan.draft.phase, title: "Queue" } as PhaseRecord;
  const renamed = { ...fixture.plan.list.rows[0], title: "Support" } as PhaseRecord;
  expect(phaseHeader(plan).title).toBe("Queue");
  expect(phaseHeader(renamed).title).toBe("Support");
});
