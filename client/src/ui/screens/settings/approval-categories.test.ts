import { describe, expect, it } from "vitest";
import defaults from "../../../store/settings-defaults.json";
import { APPROVAL_CATEGORIES, categoryLabels } from "./approval-categories";

describe("automatic approval categories", () => {
  it("lists the core's categories in order with its per-computer defaults", () => {
    expect(APPROVAL_CATEGORIES.map((c) => c.id)).toEqual(["read", "edit", "shell", "web", "mcp", "agents", "other", "paid"]);
    const table = defaults as Record<string, unknown>;
    for (const category of APPROVAL_CATEGORIES) expect(table[category.key], category.id).toBe(category.on);
    expect(APPROVAL_CATEGORIES.filter((c) => !c.on).map((c) => c.id)).toEqual(["paid"]);
  });
  it("names a journal entry's categories in list order and leaves unknown IDs out", () => {
    expect(categoryLabels(["paid", "future", "shell"])).toEqual(["Shell commands", "Paid usage"]);
    expect(categoryLabels(undefined)).toEqual([]);
  });
});
