import { describe, expect, it } from "vitest";
import { changesView, diffOutcome, formatElapsed, pathParts, withRequest, workingLine } from "./inspector-data";

// Qt parity (I3, I5d): the Changes card (SessionsWindow.cpp updateChangesCard,
// showDiff) and the working line (updateWorkingLine, WorkingLine::elapsed).

const status = {
  state: "ok", path: "/work/orbit", branch: "feat/checkout", changed_files: 4,
  changes: {
    added: 120, deleted: 7, tracked_files: 52, untracked_files: 3, truncated: true,
    files: [
      { path: "src/api/cards.rs", added: 100, deleted: 7 },
      { path: "assets/logo.png", added: null, deleted: null },
      { path: "docs/notes.md", added: null, deleted: null, untracked: true },
      { path: "src/new.rs", added: 20, deleted: 0 },
    ],
  },
};

describe("the Changes card", () => {
  it("lists each file with its lines, the header's totals and the files left out", () => {
    const view = changesView(status);
    expect(view.aside).toBe("+120 \u22127");
    expect(view.rows).toEqual([
      { path: "src/api/cards.rs", added: "+100", deleted: "\u22127" },
      { path: "assets/logo.png", added: "bin", deleted: "" },
      { path: "docs/notes.md", added: "new", deleted: "" },
      { path: "src/new.rs", added: "+20", deleted: "\u22120" },
    ]);
    // 52 tracked and 3 untracked files, 4 listed.
    expect(view.more).toBe(51);
    expect(view).toMatchObject({ empty: "", known: true, clean: false });
    expect(changesView({ ...status, changes: { ...status.changes, truncated: false } }).more).toBe(0);
  });

  it("says why there is nothing to list", () => {
    expect(changesView({ state: "ok", changed_files: 0 })).toEqual({ aside: "", rows: [], more: 0, empty: "Working tree is clean.", known: true, clean: true });
    for (const value of [undefined, {}, { state: "unavailable" }, { state: "folder_unavailable", changed_files: 0 }, { state: "changed_during_check" }]) {
      expect(changesView(value)).toMatchObject({ aside: "", rows: [], empty: "Git status not reported for this folder.", known: false, clean: false });
    }
    // Changed files without a file list (an older hgs): not clean, nothing to list.
    expect(changesView({ state: "ok", changed_files: 2 })).toMatchObject({ aside: "", empty: "", known: true, clean: false });
  });

  it("keeps a path's file name while its folders elide", () => {
    expect(pathParts("src/ui/sessions/Inspector.tsx")).toEqual(["src/ui/sessions/", "Inspector.tsx"]);
    expect(pathParts("C:\\work\\orbit\\main.rs")).toEqual(["C:\\work\\orbit\\", "main.rs"]);
    expect(pathParts("README.md")).toEqual(["", "README.md"]);
    const long = `src/${"a".repeat(45)}.rs`;
    expect(pathParts(long)).toEqual([long.slice(0, -20), long.slice(-20)]);
    expect(pathParts("")).toEqual(["", ""]);
  });

  it("asks the agent to commit after the draft, never in place of it", () => {
    const ask = "Commit the current changes with a short message.";
    expect(withRequest("", ask)).toBe(ask);
    expect(withRequest("  \n ", ask)).toBe(ask);
    expect(withRequest("  Check the tests first.\n\n", ask)).toBe(`Check the tests first.\n${ask}`);
  });

  it("opens the diff, or words why it cannot", () => {
    expect(diffOutcome({ state: "ok", path: "/work/orbit", diff: "+x\n", truncated: false, bytes: 3 }, "")).toEqual({ title: "Diff: /work/orbit", text: "+x\n", note: "" });
    expect(diffOutcome({ state: "ok", path: "/work/orbit", diff: "+x\n", truncated: true, bytes: 700_000 }, "")).toMatchObject({ note: "Showing the first 256 KiB of 683 KiB." });
    expect(diffOutcome({ state: "too_large" }, "")).toEqual({ notice: "The diff is larger than 1 MiB; open it in Terminal." });
    expect(diffOutcome({ state: "folder_unavailable" }, "")).toEqual({ notice: "The diff could not be read: folder_unavailable" });
    expect(diffOutcome({}, "timed out")).toEqual({ notice: "The diff could not be read: timed out" });
  });
});

describe("the working line", () => {
  const live = { online: true, state: "running", processState: "running" };
  const busy = {
    activity: "busy", phase: "working", run_id: "run-2", tool_detail: "cargo test -p core", current_tool: "Bash",
    activity_detail: "Running tests", activity_summary: "Testing",
    task_lists: { main: { run_id: "run-2", items: [{ title: "Read the spec", status: "completed" }, { title: "  Wire\n the   API ", status: "in_progress" }] } },
  };

  it("shows the plan's step of the current run, else the tool, else the activity", () => {
    expect(workingLine(busy, live)).toBe("Wire the API");
    // An earlier run's plan is not this turn's.
    expect(workingLine({ ...busy, task_lists: { main: { ...busy.task_lists.main, run_id: "run-1" } } }, live)).toBe("cargo test -p core");
    expect(workingLine({ ...busy, task_lists: undefined, tool_detail: "" }, live)).toBe("Bash");
    expect(workingLine({ ...busy, task_lists: undefined, tool_detail: "", current_tool: "" }, live)).toBe("Running tests");
    expect(workingLine({ ...busy, task_lists: undefined, tool_detail: "", current_tool: "", activity_detail: "" }, live)).toBe("Testing");
    expect(workingLine({ activity: "busy" }, live)).toBe("Working");
  });

  it("is hidden while the session does not work", () => {
    expect(workingLine(busy, { ...live, online: false })).toBeUndefined();
    expect(workingLine(busy, { ...live, state: "paused" })).toBeUndefined();
    expect(workingLine(busy, { ...live, processState: "exited" })).toBeUndefined();
    expect(workingLine(busy, live, true)).toBeUndefined();
    expect(workingLine({ ...busy, activity: "idle" }, live)).toBeUndefined();
    expect(workingLine({ ...busy, phase: "approval" }, live)).toBeUndefined();
    expect(workingLine({ ...busy, phase: "question" }, live)).toBeUndefined();
    expect(workingLine(undefined, live)).toBeUndefined();
  });

  it("counts the turn as Qt does", () => {
    expect(formatElapsed(0)).toBe("0s");
    expect(formatElapsed(12.9)).toBe("12s");
    expect(formatElapsed(59)).toBe("59s");
    expect(formatElapsed(60)).toBe("1m 00s");
    expect(formatElapsed(545)).toBe("9m 05s");
    expect(formatElapsed(3599)).toBe("59m 59s");
    expect(formatElapsed(3600)).toBe("1h 00m");
    expect(formatElapsed(3900)).toBe("1h 05m");
    expect(formatElapsed(-5)).toBe("0s");
    expect(formatElapsed(Number.NaN)).toBe("0s");
  });
});
