import { describe, expect, it, vi } from "vitest";
import { createComputed, createRoot } from "solid-js";
import { receive } from "./session";
import { catalog } from "./catalog";
import { grouping, groupOf } from "./organization";
import { rowOut } from "./testing";
import { emptyBatch, type GroupingOut } from "../types";

// The wiring reads the page's query (`?select=`, store/ui.ts); the tests run without a page.
vi.hoisted(() => { vi.stubGlobal("location", new URL("http://localhost/")); });

const loaded = (): GroupingOut => ({
  loaded: true, default_project: "ungrouped", ready: true, reason: null, busy: false, status: "", status_error: false, conflicts: 0,
  headers: [
    { id: "alpha", name: "Alpha", color: null, accessible: true, collapsed: false, is_default: false, palette: 0 },
    { id: "ungrouped", name: "General", color: null, accessible: true, collapsed: false, is_default: true, palette: 1 },
  ],
});

describe("receive", () => {
  it("applies a batch's rows and its grouping in one reactive update", () => createRoot((dispose) => {
    const seen: string[] = [];
    createComputed(() => { seen.push(`${catalog.order.length} ${grouping().loaded} ${groupOf("\nrun:1")}`); });
    seen.length = 0;
    receive({ ...emptyBatch(), upserts: [rowOut("", 1)], groups: [{ computer: "", key: "run:1", group: "alpha", rank: 1 }], grouping: loaded() });
    // One run with everything applied: never the new row in the default project, nor the headers without the row.
    expect(seen).toEqual(["1 true alpha"]);
    dispose();
  }));
});
