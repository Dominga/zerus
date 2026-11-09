import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createComputed, createRoot } from "solid-js";
import { applyBatch } from "./catalog";
import { detailTab, selectedKey, setDetailTab, setSelectedKey } from "./ui";
import { openTasks } from "./taskNavigation";
import { emptyBatch } from "../types";

vi.hoisted(() => vi.stubGlobal("location", new URL("http://localhost/")));
vi.mock("./inspector", () => ({ inspectorOpen: () => true, inspectorTab: () => "tasks", openInspector: () => {} }));
const first = "selection-fixture\nrun:first", second = "selection-fixture\nrun:second";
beforeEach(() => { setDetailTab("activity"); setSelectedKey(first); });
afterEach(() => { setDetailTab("activity"); setSelectedKey(undefined); });

describe("session selection from Task View", () => {
  it("switches to Activity before a different selected session is observed", () => {
    setDetailTab("taskview");
    const seen: string[] = [];
    const dispose = createRoot((stop) => { createComputed(() => { if (selectedKey() === second) seen.push(detailTab()); }); return stop; });
    try {
      setSelectedKey(second);
      expect(detailTab()).toBe("activity");
      expect(seen).toEqual(["activity"]);
    } finally { dispose(); }
  });

  it("keeps Task View for the same selected key", () => {
    setDetailTab("taskview");
    setSelectedKey(first);
    setSelectedKey((current) => current);
    expect(detailTab()).toBe("taskview");
  });

  it("also resets when an updater changes the session or selection is cleared", () => {
    setDetailTab("taskview");
    expect(setSelectedKey(() => second)).toBe(second);
    expect(detailTab()).toBe("activity");
    setDetailTab("taskview"); setSelectedKey();
    expect(selectedKey()).toBeUndefined();
    expect(detailTab()).toBe("activity");
  });

  it.each(["terminal", "native", "processes"] as const)("preserves the other detail choice: %s", (tab) => {
    setDetailTab(tab); setSelectedKey(second);
    expect(detailTab()).toBe(tab);
  });

  it("keeps an explicit task navigation after it changes the selected session", () => {
    applyBatch({ ...emptyBatch(), upserts: [{ computer: "selection-fixture", key: "run:second", row: { name: "codex/example/second", state: "running", run_id: "second" } }] });
    try {
      setDetailTab("taskview");
      openTasks("ungrouped", second, "11111111-1111-4111-8111-111111111111");
      expect(selectedKey()).toBe(second);
      expect(detailTab()).toBe("taskview");
    } finally { applyBatch({ ...emptyBatch(), removes: [{ computer: "selection-fixture", key: "run:second" }] }); }
  });
});
