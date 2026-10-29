import { beforeEach, describe, expect, it, vi } from "vitest";
import * as organization from "../../store/organization";
import { applyBatch, catalog, rowId } from "../../store/catalog";
import { rowOut } from "../../store/testing";
import { emptyBatch, type GroupingOut } from "../../types";
import { applyDrop, placement } from "./organize";
import type { Session } from "./types";
import type { Row } from "./model";

// The drop half of organize.ts against a mocked organization: which edit a
// drop sends, and where `placement` reads a row's project.

// store/ui.ts reads the page's query; the tests run without a page.
vi.hoisted(() => { vi.stubGlobal("location", new URL("http://localhost/")); });

const state = vi.hoisted(() => ({
  /** Row id -> project, as the Rust side assigns them. */
  assigned: {} as Record<string, string>,
  listed: ["alpha", "beta", "ungrouped"],
  loaded: true,
  archived: {} as Record<string, Session>,
}));

vi.mock("../../store/organization", () => ({
  INBOX: "ungrouped",
  grouping: vi.fn((): Partial<GroupingOut> => ({ loaded: state.loaded, default_project: "beta" })),
  groupOf: vi.fn((id: string) => state.assigned[id] ?? "beta"),
  headerOf: vi.fn((id: string) => (state.listed.includes(id) ? { id } : undefined)),
  moveProject: vi.fn(() => Promise.resolve({ outcome: "done", conflicts: 0 })),
  moveToProject: vi.fn(() => Promise.resolve({ outcome: "done", conflicts: 0 })),
}));
vi.mock("../../store/archive", () => ({ archive: { row: (key: string) => state.archived[key] } }));
// The bulk-selection half reads the list; the drops do not.
vi.mock("./model", () => ({ listRows: () => [] }));

const mocked = vi.mocked(organization);
const A = rowId("", "run:1"), B = rowId("", "run:2"), C = rowId("", "run:3");
// Every row's own `project` is the folder's; only the assignments place it.
applyBatch({ ...emptyBatch(), upserts: [rowOut("", 1), rowOut("", 2), rowOut("", 3)] });

const header = (project: string): Row =>
  ({ type: "header", id: `h:${project}`, project, name: project, color: null, palette: 0, collapsed: false, total: 0, attention: 0, unread: 0, working: 0 });
const card = (id: string): Row => ({ type: "card", id, session: catalog.rows[id], expandable: false, expanded: false, compactRow: false });
/** Rows at the list's fixed heights: header 40, card 94. */
const layout = (rows: Row[]) => {
  const heights = rows.map((r) => (r.type === "header" ? 40 : 94));
  let y = 0;
  return { rows, heights, offsets: heights.map((h) => { const t = y; y += h; return t; }) };
};

beforeEach(() => {
  mocked.moveProject.mockClear();
  mocked.moveToProject.mockClear();
  state.assigned = { [A]: "alpha", [B]: "alpha", [C]: "beta" };
  state.listed = ["alpha", "beta", "ungrouped"];
  state.loaded = true;
  state.archived = {};
});

describe("project drops", () => {
  it("moves a project before another", () => {
    applyDrop({ kind: "group", id: "alpha" }, { group: "gamma", before: "beta" });
    expect(mocked.moveProject).toHaveBeenCalledTimes(1);
    expect(mocked.moveProject).toHaveBeenCalledWith("alpha", "beta");
  });

  it("moves a project to the end", () => {
    applyDrop({ kind: "group", id: "alpha" }, { group: "", line: 300, inset: 8 });
    expect(mocked.moveProject).toHaveBeenCalledWith("alpha", undefined);
  });

  it("makes no call for a drop before the dragged item itself", () => {
    applyDrop({ kind: "group", id: "alpha" }, { group: "alpha", before: "alpha" });
    applyDrop({ kind: "session", id: A }, { group: "alpha", before: A });
    expect(mocked.moveProject).not.toHaveBeenCalled();
    expect(mocked.moveToProject).not.toHaveBeenCalled();
  });
});

describe("session drops", () => {
  it("moves a session inside its own project before a row, or to its end", () => {
    applyDrop({ kind: "session", id: A }, { group: "alpha", before: B });
    expect(mocked.moveToProject).toHaveBeenLastCalledWith([catalog.rows[A]], "alpha", catalog.rows[B]);
    applyDrop({ kind: "session", id: B }, { group: "alpha" });
    expect(mocked.moveToProject).toHaveBeenLastCalledWith([catalog.rows[B]], "alpha", undefined);
    expect(mocked.moveToProject).toHaveBeenCalledTimes(2);
  });

  it("moves a session into another project", () => {
    applyDrop({ kind: "session", id: A }, { group: "beta", before: C });
    expect(mocked.moveToProject).toHaveBeenCalledWith([catalog.rows[A]], "beta", catalog.rows[C]);
    applyDrop({ kind: "session", id: C }, { group: "alpha", box: { top: 6, height: 30 } });
    expect(mocked.moveToProject).toHaveBeenLastCalledWith([catalog.rows[C]], "alpha", undefined);
  });

  it("ignores a project that is not listed, as Qt's moveSession does", () => {
    state.listed = ["alpha", "beta"];
    applyDrop({ kind: "session", id: A }, { group: "ungrouped" });
    expect(mocked.moveToProject).not.toHaveBeenCalled();
    // Before the first catalog nothing is listed yet: the store answers with its reason.
    state.loaded = false;
    applyDrop({ kind: "session", id: A }, { group: "ungrouped" });
    expect(mocked.moveToProject).toHaveBeenCalledTimes(1);
  });
});

describe("placement", () => {
  const { rows, offsets, heights } = layout([header("alpha"), card(A), card(C), header("beta"), card(B)]);
  const session = { kind: "session" as const, id: A };

  it("takes a card's project from its assignment, not from the row's folder project", () => {
    expect(catalog.rows[A].project).toBe("sample-project");
    // The upper half of the card at 40..134 places before it, in its assigned project.
    expect(placement(rows, offsets, heights, C, 140, session)).toMatchObject({ group: "beta", before: C });
    state.assigned[C] = "gamma";
    expect(placement(rows, offsets, heights, C, 140, session)).toMatchObject({ group: "gamma", before: C });
  });

  it("puts the next card as the target only when it shares the project", () => {
    // The lower half of A (40..134): C follows, but C is in beta, so A's project ends there.
    expect(placement(rows, offsets, heights, A, 120, session)).toMatchObject({ group: "alpha", before: undefined });
    state.assigned[C] = "alpha";
    expect(placement(rows, offsets, heights, A, 120, session)).toMatchObject({ group: "alpha", before: C });
  });

  it("sends a drop on the blank area to the Inbox's end, not the default project", () => {
    const blank = placement(rows, offsets, heights, undefined, 900, session);
    expect(blank).toMatchObject({ group: "ungrouped" });
    expect(blank.before).toBeUndefined();
    expect(placement(rows, offsets, heights, undefined, 900, { kind: "group", id: "alpha" })).toMatchObject({ group: "" });
  });

  it("places a project before the next header from the lower half of its own", () => {
    const group = { kind: "group" as const, id: "alpha" };
    expect(placement(rows, offsets, heights, "h:alpha", 5, group)).toMatchObject({ before: "alpha" });
    expect(placement(rows, offsets, heights, "h:alpha", 35, group)).toMatchObject({ before: "beta" });
  });
});

it("drops loaded archive rows before archive identities and ignores an archive row no longer loaded", () => {
  const a = { ...catalog.rows[A], key: "\narchive:a", catalogKey: "archive:a", state: "archived", archiveId: "a" } as Session;
  const b = { ...catalog.rows[B], key: "\narchive:b", catalogKey: "archive:b", state: "archived", archiveId: "b" } as Session;
  state.archived = { [a.key]: a, [b.key]: b };
  applyDrop({ kind: "session", id: a.key }, { group: "beta", before: b.key });
  expect(mocked.moveToProject).toHaveBeenLastCalledWith([a], "beta", b);
  mocked.moveToProject.mockClear();
  delete state.archived[a.key];
  applyDrop({ kind: "session", id: a.key }, { group: "beta" });
  expect(mocked.moveToProject).not.toHaveBeenCalled();
});
