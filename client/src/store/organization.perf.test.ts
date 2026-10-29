import { describe, expect, it, vi } from "vitest";
import { createMemo, createRoot } from "solid-js";
import { createCatalog } from "./catalog";
import { createOrganization } from "./organization";
import { groupMembers, groupRows } from "../ui/sessions/model";
import { emptyBatch, type Batch, type GroupingOut, type GroupOut, type RowOut } from "../types";

// The 20,000-row measurement (Global Constraints, "Scale"): phase 2's
// reconcile of one replace into a catalog, then the grouping's own cost, then
// a status-only batch, which must not regroup. Run it on demand, with the
// garbage collector exposed so the heap numbers are after a collection:
//
//   ZERUS_CLIENT_MEASURE=1 NODE_OPTIONS=--expose-gc npx vitest run src/store/organization.perf.test.ts

// The list model's module reads the page's query (store/ui.ts); the test runs without a page.
vi.hoisted(() => { vi.stubGlobal("location", new URL("http://localhost/")); });

const ROWS = 20_000;
const PROJECTS = 50;
const UPSERTS = 1_000;
const GAP = 1 << 20;

/** Node's process and collector (the client's types are the webview's). */
const node = globalThis as unknown as {
  gc?: () => void;
  process: { env: Record<string, string | undefined>; memoryUsage(): { heapUsed: number }; stdout: { write(text: string): void } };
};
const heap = () => { node.gc?.(); return node.process.memoryUsage().heapUsed; };
const mb = (bytes: number) => `${bytes >= 0 ? "+" : ""}${(bytes / 1048576).toFixed(1)} MB`;
const report = (what: string, ms: number, bytes?: number) =>
  node.process.stdout.write(`organization-perf ${what}: ${ms.toFixed(0)} ms${bytes === undefined ? "" : `, ${mb(bytes)}`}\n`);
const batchOf = (part: Partial<Batch>): Batch => ({ ...emptyBatch(), ...part });

/** A row with phase 2's view row fields. */
function rowOut(i: number, status = "ready"): RowOut {
  const project = `project-${i % PROJECTS}`;
  return {
    computer: "", key: `run:${i}`,
    row: {
      name: `claude/${project}/task-${i}`, title: `task-${i}`, agent: "claude", status, caption: status === "working" ? "Using a tool" : "Ready",
      phase: status === "working" ? "tool" : "idle", machine: "workstation", project, folder: project, branch: `feature-${i % 97}`,
      model: "opus", run_id: `run-${i}`, conversation_id: `conv-${i}`, state: "running", activity: status === "working" ? "busy" : "idle",
      allowed_actions: ["pause", "rename", "archive", "terminate"],
    },
  };
}

describe.skipIf(node.process.env.ZERUS_CLIENT_MEASURE !== "1")("20,000 sessions", () => {
  it("reconciles, groups and takes status upserts without regrouping", () => createRoot((dispose) => {
    const rows = Array.from({ length: ROWS }, (_, i) => rowOut(i));
    const catalog = createCatalog();

    // 1. Phase 2's reconcile: one replace of every row.
    let before = heap();
    let started = performance.now();
    catalog.applyBatch(batchOf({ replaces: [{ computer: "", rows }] }));
    report("reconcile", performance.now() - started, heap() - before);
    expect(catalog.catalog.order.length).toBe(ROWS);

    // 2. The grouping: 20,000 assignments in 50 projects, the members and the list rows.
    const organization = createOrganization(() => {});
    const header = (id: string, at: number) => ({ id, name: id, color: null, accessible: true, collapsed: false, is_default: id === "ungrouped", palette: at % 8 });
    const grouping: GroupingOut = {
      loaded: true, default_project: "ungrouped", ready: true, reason: null, busy: false, status: "", status_error: false, conflicts: 0,
      headers: [...Array.from({ length: PROJECTS }, (_, p) => header(`p-${p}`, p)), header("ungrouped", PROJECTS)],
    };
    const groups: GroupOut[] = rows.map((r, i) => ({ computer: "", key: r.key, group: `p-${i % PROJECTS}`, rank: (Math.floor(i / PROJECTS) + 1) * GAP }));
    before = heap();
    started = performance.now();
    organization.applyGrouping(batchOf({ grouping, groups }));
    const members = groupMembers(catalog.catalog.order, catalog.catalog.rows, organization.groups, organization.defaultProject());
    const list = groupRows({ grouping: organization.grouping(), members, visible: () => true, online: () => true, projects: [], expanded: () => false, compact: () => false });
    report("grouping", performance.now() - started, heap() - before);
    expect(list.length).toBe(ROWS + PROJECTS);

    // 3. A batch of status upserts: its reconcile, and the members are not grouped again.
    let runs = 0;
    const grouped = createMemo(() => {
      runs++;
      return groupMembers(catalog.catalog.order, catalog.catalog.rows, organization.groups, organization.defaultProject());
    });
    grouped();
    runs = 0;
    const upserts = Array.from({ length: UPSERTS }, (_, n) => rowOut(n * (ROWS / UPSERTS), "working"));
    const update = batchOf({ upserts });
    started = performance.now();
    catalog.applyBatch(update);
    organization.applyGrouping(update);
    report(`${UPSERTS} status upserts`, performance.now() - started);
    expect(grouped().get("p-0")?.length).toBe(ROWS / PROJECTS);
    expect(runs).toBe(0);
    dispose();
  }), 120_000);
});
