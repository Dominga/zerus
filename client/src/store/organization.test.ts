import { beforeEach, describe, expect, it, vi } from "vitest";
import { createRoot } from "solid-js";
import * as bridge from "../bridge";
import { createOrganization, memberOf, SYNC_NOTICE, LAYOUT_NOTICE_TITLE, LAYOUT_NOTICE_TEXT } from "./organization";
import { toSession } from "./catalog";
import { rowOut } from "./testing";
import { emptyBatch, type Batch, type GroupingOut } from "../types";

vi.mock("../bridge", () => ({
  moveToProject: vi.fn(), createProject: vi.fn(), renameProject: vi.fn(), setProjectColor: vi.fn(), deleteProject: vi.fn(),
  moveProject: vi.fn(), sortProject: vi.fn(), setProjectCollapsed: vi.fn(() => Promise.resolve()),
}));
vi.mock("./actions", () => ({ notify: vi.fn(), clearNotice: vi.fn() }));

const mocked = vi.mocked(bridge);
const header = (id: string, name: string) => ({ id, name, color: null, accessible: true, collapsed: false, is_default: id === "ungrouped", palette: 0 });
const ready = (extra: Partial<GroupingOut> = {}): GroupingOut => ({
  loaded: true, default_project: "ungrouped", headers: [header("alpha", "Alpha"), header("ungrouped", "Inbox")],
  ready: true, reason: null, busy: false, status: "", status_error: false, conflicts: 0, ...extra,
});
const batchOf = (part: Partial<Batch>): Batch => ({ ...emptyBatch(), ...part });
const rows = [{ rawMachine: "workstation", name: "claude/sample-project/review" }, { rawMachine: "studio", name: "codex/aurora/orders-v3" }];

function setup(grouping = ready()) {
  const say = vi.fn();
  const unsay = vi.fn();
  const org = createOrganization(say, unsay);
  org.applyGrouping(batchOf({ grouping }));
  return { org, say, unsay };
}

beforeEach(() => {
  mocked.moveToProject.mockReset();
  mocked.createProject.mockReset();
  mocked.renameProject.mockReset();
  mocked.setProjectColor.mockReset();
  mocked.deleteProject.mockReset();
  mocked.moveProject.mockReset();
  mocked.sortProject.mockReset();
  mocked.setProjectCollapsed.mockReset();
  mocked.setProjectCollapsed.mockResolvedValue(undefined);
});

describe("the organization store", () => {
  it("shows a refused layout once as a titled sticky warning through the window bus", () => createRoot((dispose) => {
    const { org, say } = setup(ready({ layout_save_paused: true }));
    expect(say).toHaveBeenCalledExactlyOnceWith("warn", LAYOUT_NOTICE_TEXT, true, undefined, LAYOUT_NOTICE_TITLE);
    org.applyGrouping(batchOf({ grouping: ready({ layout_save_paused: true }) }));
    org.applyGrouping(batchOf({ grouping: ready() }));
    org.applyGrouping(batchOf({ grouping: ready({ layout_save_paused: true }) }));
    expect(say).toHaveBeenCalledTimes(1);
    dispose();
  }));
  it("assigns rows from batches and falls back to the default project", () => createRoot((dispose) => {
    const { org } = setup();
    org.applyGrouping(batchOf({ groups: [{ computer: "", key: "run:1", group: "alpha", rank: 1048576 }] }));
    expect(org.groupOf("\nrun:1")).toBe("alpha");
    expect(org.groupOf("\nrun:2")).toBe("ungrouped");
    org.applyGrouping(batchOf({ removes: [{ computer: "", key: "run:1", reason: null }] }));
    expect(org.groupOf("\nrun:1")).toBe("ungrouped");
    expect(org.headerOf("alpha")?.name).toBe("Alpha");
    dispose();
  }));

  it("drops the assignments of rows a computer's replace left out and keeps the rest", () => createRoot((dispose) => {
    const { org } = setup();
    org.applyGrouping(batchOf({ groups: [
      { computer: "studio", key: "run:1", group: "alpha", rank: 1 },
      { computer: "studio", key: "run:2", group: "alpha", rank: 2 },
      { computer: "", key: "run:1", group: "alpha", rank: 3 },
    ] }));
    // The replace keeps run:2 without resending its group (grouping sends only changes).
    org.applyGrouping(batchOf({ replaces: [{ computer: "studio", rows: [rowOut("studio", 2)] }] }));
    expect(Object.keys(org.groups).sort()).toEqual(["\nrun:1", "studio\nrun:2"]);
    expect(org.groupOf("studio\nrun:2")).toBe("alpha");
    expect(org.groupOf("\nrun:1")).toBe("alpha");
    // An empty replace drops every assignment of that computer only.
    org.applyGrouping(batchOf({ replaces: [{ computer: "", rows: [] }] }));
    expect(Object.keys(org.groups)).toEqual(["studio\nrun:2"]);
    dispose();
  }));

  it("moves a set of rows with one call carrying their members", async () => {
    const { org, say } = setup();
    mocked.moveToProject.mockResolvedValue({ outcome: "done", conflicts: 0 });
    const out = await org.moveToProject(rows, "alpha");
    expect(out).toEqual({ outcome: "done", conflicts: 0 });
    expect(mocked.moveToProject).toHaveBeenCalledTimes(1);
    expect(mocked.moveToProject).toHaveBeenCalledWith(["workstation\nclaude/sample-project/review", "studio\ncodex/aurora/orders-v3"], "alpha", null);
    expect(say).not.toHaveBeenCalled();
    expect(memberOf(rows[0])).toBe("workstation\nclaude/sample-project/review");
  });

  it("builds member keys from the row's own machine, empty before this computer's hello", async () => {
    const { org } = setup();
    // The card shows "this computer"; the Rust member is "\n<name>" (`swarm::member` with the row's machine).
    const local = toSession("", "run:1", { name: "claude/sample-project/review", machine: "" });
    expect(local.machine).toBe("this computer");
    expect(memberOf(local)).toBe("\nclaude/sample-project/review");
    mocked.moveToProject.mockResolvedValue({ outcome: "done", conflicts: 0 });
    await org.moveToProject([local], "alpha", toSession("studio", "run:2", { name: "codex/aurora/orders-v3", machine: "studio" }));
    expect(mocked.moveToProject).toHaveBeenCalledWith(["\nclaude/sample-project/review"], "alpha", "studio\ncodex/aurora/orders-v3");
  });

  it("never writes the live-name member of an archived row", async () => {
    const { org, say } = setup();
    const archived = { rawMachine: "workstation", name: "claude/sample-project/review", state: "archived" as const };
    const why = "Invalid archived session identity. Refresh the archive.";
    expect(await org.moveToProject([rows[1], archived], "alpha")).toEqual({ outcome: "refused", detail: why });
    expect(await org.createProject("New", [archived])).toEqual({ outcome: "refused", detail: why });
    expect(mocked.moveToProject).not.toHaveBeenCalled();
    expect(mocked.createProject).not.toHaveBeenCalled();
    expect(say).toHaveBeenCalledWith("info", why);
  });

  it("refuses before the catalog is ready and for a project that is gone, without a call", async () => {
    const { org, say } = setup(ready({ ready: false, reason: "Project sync is starting." }));
    expect(await org.moveToProject(rows, "alpha")).toEqual({ outcome: "refused", detail: "Project sync is starting." });
    org.applyGrouping(batchOf({ grouping: ready() }));
    expect(await org.renameProject("gone", "x")).toEqual({ outcome: "refused", detail: "This project is no longer listed. Open the menu again." });
    expect(mocked.moveToProject).not.toHaveBeenCalled();
    expect(mocked.renameProject).not.toHaveBeenCalled();
    expect(say).toHaveBeenCalledWith("info", "Project sync is starting.");
  });

  it("shows a rejected call once with Qt's failure text and never repeats it", async () => {
    vi.useFakeTimers();
    try {
      const { org, say } = setup();
      mocked.sortProject.mockRejectedValue("lost the answer");
      const out = await org.sortProject("alpha");
      expect(out).toEqual({ outcome: "kept", detail: "Project sync unavailable: lost the answer" });
      // Neither time nor the batches that follow make the webview send it again; only the Rust side resends.
      vi.advanceTimersByTime(60_000);
      org.applyGrouping(batchOf({ grouping: ready({ busy: true }) }));
      org.applyGrouping(batchOf({ grouping: ready({ status: "Project sync unavailable: lost the answer", status_error: true }) }));
      org.applyGrouping(batchOf({ grouping: ready() }));
      vi.advanceTimersByTime(60_000);
      await Promise.resolve();
      expect(mocked.sortProject).toHaveBeenCalledTimes(1);
      expect(say).toHaveBeenCalledTimes(1);
      expect(say).toHaveBeenCalledWith("error", "Project sync unavailable: lost the answer", true, SYNC_NOTICE);
    } finally {
      vi.useRealTimers();
    }
  });

  it("warns about conflicts on the change and shows a kept edit's status line", async () => {
    const { org, say } = setup();
    mocked.deleteProject.mockResolvedValue({ outcome: "done", conflicts: 2 });
    await org.deleteProject("alpha", "ungrouped");
    expect(say).toHaveBeenCalledWith("warn", "2 project conflict(s) need review in Swarm.", true, SYNC_NOTICE);
    mocked.moveProject.mockResolvedValue({ outcome: "kept", detail: "Project sync unavailable: hgs did not answer in time" });
    await org.moveProject("alpha", undefined);
    expect(mocked.moveProject).toHaveBeenCalledWith("alpha", null);
    expect(mocked.moveProject).toHaveBeenCalledTimes(1);
    expect(say).toHaveBeenCalledWith("error", "Project sync unavailable: hgs did not answer in time", true, SYNC_NOTICE);
    mocked.renameProject.mockResolvedValue({ outcome: "kept", detail: "Project sync needs an updated HGS installation." });
    await org.renameProject("alpha", "Alpha two");
    expect(say).toHaveBeenCalledWith("error", "Project sync needs an updated HGS installation.", true, SYNC_NOTICE);
  });

  it("keeps project sync to one notice that a clean success clears, as Qt's status line", async () => {
    const { org, say, unsay } = setup();
    mocked.sortProject.mockResolvedValueOnce({ outcome: "kept", detail: "Project sync unavailable: hgs did not answer in time" });
    mocked.sortProject.mockRejectedValueOnce("lost the answer");
    mocked.sortProject.mockResolvedValueOnce({ outcome: "done", conflicts: 1 });
    for (let i = 0; i < 3; i++) await org.sortProject("alpha");
    // Kept, rejected and conflicts all speak with the one key, so each replaces the last.
    expect(say.mock.calls.map((call) => call[3])).toEqual([SYNC_NOTICE, SYNC_NOTICE, SYNC_NOTICE]);
    expect(unsay).not.toHaveBeenCalled();
    mocked.sortProject.mockResolvedValueOnce({ outcome: "done", conflicts: 0 });
    await org.sortProject("alpha");
    expect(unsay).toHaveBeenCalledWith(SYNC_NOTICE);
    // A refusal is not a sync outcome: it neither replaces nor clears the line.
    org.applyGrouping(batchOf({ grouping: ready({ ready: false, reason: "Project sync is starting." }) }));
    await org.sortProject("alpha");
    expect(say).toHaveBeenLastCalledWith("info", "Project sync is starting.");
    expect(unsay).toHaveBeenCalledTimes(1);
  });

  it("clears a kept edit's error once the Rust side's resend succeeds, only when the error ends", async () => {
    const { org, say, unsay } = setup();
    mocked.sortProject.mockResolvedValueOnce({ outcome: "kept", detail: "Project sync unavailable: hgs did not answer in time" });
    await org.sortProject("alpha");
    expect(say).toHaveBeenCalledWith("error", "Project sync unavailable: hgs did not answer in time", true, SYNC_NOTICE);
    // The failed apply's status line, then views that keep it: nothing is cleared.
    const failing = { status: "Project sync unavailable: hgs did not answer in time", status_error: true };
    org.applyGrouping(batchOf({ grouping: ready(failing) }));
    org.applyGrouping(batchOf({ grouping: ready({ ...failing, busy: true }) }));
    org.applyGrouping(batchOf({ groups: [{ computer: "", key: "run:1", group: "alpha", rank: 1 }] }));
    expect(unsay).not.toHaveBeenCalled();
    // The resend went through: the status is no error any more, and the notice goes.
    org.applyGrouping(batchOf({ grouping: ready({ status: "Project sync connected to 1 computer(s)" }) }));
    expect(unsay).toHaveBeenCalledExactlyOnceWith(SYNC_NOTICE);
    // A view that was never in error clears nothing.
    org.applyGrouping(batchOf({ grouping: ready({ busy: true }) }));
    expect(unsay).toHaveBeenCalledTimes(1);
  });

  it("shows the hub's notice once as a sticky warning, and again only after it cleared", () => createRoot((dispose) => {
    const notice = "The saved project changes were not applied: the draft file is larger than 4096 bytes";
    const { org, say } = setup(ready({ notice }));
    // The same notice in a later view (another field changed), and a batch without a view.
    org.applyGrouping(batchOf({ grouping: ready({ notice, busy: true }) }));
    org.applyGrouping(batchOf({ groups: [{ computer: "", key: "run:1", group: "alpha", rank: 1 }] }));
    expect(say).toHaveBeenCalledTimes(1);
    expect(say).toHaveBeenCalledWith("warn", notice, true);
    org.applyGrouping(batchOf({ grouping: ready() }));
    expect(say).toHaveBeenCalledTimes(1);
    org.applyGrouping(batchOf({ grouping: ready({ notice }) }));
    expect(say).toHaveBeenCalledTimes(2);
    dispose();
  }));

  it("asks for collapse without an answer to show", async () => {
    const { org, say } = setup();
    org.setProjectCollapsed("alpha", true);
    expect(mocked.setProjectCollapsed).toHaveBeenCalledWith("alpha", true);
    mocked.setProjectCollapsed.mockRejectedValueOnce("no app");
    org.setProjectCollapsed("alpha", false);
    await Promise.resolve();
    expect(say).not.toHaveBeenCalled();
  });
});

it("keeps request-tagged archive deltas out of live assignments", () => createRoot((dispose) => {
  const { org } = setup();
  org.applyGrouping(batchOf({ groups: [
    { computer: "", key: "archive:old", group: "alpha", rank: 10, archive_request: 20 },
    { computer: "", key: "run:live", group: "alpha", rank: 11 },
  ] }));
  expect(org.groups["\narchive:old"]).toBeUndefined();
  expect(org.groups["\nrun:live"]).toEqual({ group: "alpha", rank: 11 });
  dispose();
}));

it("moves and creates projects with archive identities, including an archived before member", async () => {
  const { org } = setup();
  const archived = { ...rows[0], state: "archived" as const, archiveId: "10000000-0000-4000-8000-000000000001" };
  const before = { ...rows[0], state: "archived" as const, archiveId: "10000000-0000-4000-8000-000000000002" };
  mocked.moveToProject.mockResolvedValue({ outcome: "done", conflicts: 0 });
  mocked.createProject.mockResolvedValue({ outcome: "done", conflicts: 0 });
  await org.moveToProject([archived, rows[1]], "alpha", before);
  expect(mocked.moveToProject).toHaveBeenCalledWith([`workstation\narchive\n${archived.archiveId}`, "studio\ncodex/aurora/orders-v3"], "alpha", `workstation\narchive\n${before.archiveId}`);
  await org.createProject("New", [archived]);
  expect(mocked.createProject).toHaveBeenCalledWith("New", [`workstation\narchive\n${archived.archiveId}`]);
  mocked.moveToProject.mockClear();
  await org.moveToProject([archived], "alpha", { ...before, archiveId: undefined });
  expect(mocked.moveToProject).not.toHaveBeenCalled();
});

it("reads a loaded archived row's current group without growing the live assignments", () => {
  let group = "alpha";
  const org = createOrganization(vi.fn(), vi.fn(), (key) => key === "archive" ? group : undefined);
  expect(org.groupOf("archive")).toBe("alpha");
  group = "beta";
  expect(org.groupOf("archive")).toBe("beta");
  expect(Object.keys(org.groups)).toEqual([]);
});


it("routes only color through the existing organization writer and refuses missing projects", async () => {
  await createRoot(async (dispose) => {
    const { org } = setup();
    mocked.setProjectColor.mockResolvedValue({ outcome: "done", conflicts: 0 });
    expect((await org.setProjectColor("alpha", "#abcdef")).outcome).toBe("done");
    expect(mocked.setProjectColor).toHaveBeenCalledWith("alpha", "#abcdef");
    expect((await org.setProjectColor("missing", "#abcdef")).outcome).toBe("refused");
    expect(mocked.setProjectColor).toHaveBeenCalledTimes(1);
    dispose();
  });
});
