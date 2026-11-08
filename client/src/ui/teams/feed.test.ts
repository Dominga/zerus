import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { feedClock, focusedLines, historyLine, inboxKind, historyStage } from "./history";
import { createTeams } from "../../store/teamsView";

// Task 198 (W1): the Feed and Inbox under the plan, as Qt's TeamWorkspace and
// feedLine word them (Qt's " \u00b7 " is ", " here: the UI has no separator dots).
const tasks = [{ id: 3, title: "Saved cards API" }];
const line = (kind: string, detail: Record<string, unknown>) => historyLine({ kind, detail }, tasks);

describe("team feed lines (task 198)", () => {
  it("words the plan, phase and team events", () => {
    expect(line("created", {}).text).toBe("Created the team");
    expect(line("planned", { tasks: 6 }).text).toBe("Changed the plan, 6 tasks");
    expect(line("planned", { errors: 2 }).text).toBe("Changed the plan, 2 errors");
    expect(line("phase_added", { phase: 2, title: "Build", tasks: 4 }).text).toBe("Added phase 2, Build, 4 tasks");
    expect(line("phase_addition_warned", { phase: 2, tasks: [7, 8] })).toMatchObject({ text: "Asked to add 2 tasks to running phase 2, waits for your confirmation", tone: "warn" });
    expect(line("phase_tasks_added", { phase: 2, tasks: [7, 8] }).text).toBe("Added #7, #8 to running phase 2, outside its original scope");
    expect(line("phase_renumbered", { phases: [{ from: 4, to: 3 }] }).text).toBe("Drafts renumbered: phases 4→3");
    expect(line("stopped", {}).tone).toBe("danger");
    expect(line("finished", { accepted: 5, tasks: 6 })).toMatchObject({ text: "Finished, 5 of 6 tasks accepted", tone: "done" });
  });

  it("words a task's life with its title and attempt", () => {
    expect(line("handed_out", { by: "Zerus", to: "Backend", task: 3, attempt: 2 })).toMatchObject({ text: "Task to Backend, #3 Saved cards API, attempt 2", by: "Lead" });
    expect(line("delivered", { task: 3 }).text).toBe("Delivered #3 Saved cards API");
    // Task #362: the lead's use of a member's last reply quotes the reply.
    expect(line("reported", { by: "Lead", for: "Backend", task: 3, attempt: 1, text: "Done: the parser" }))
      .toMatchObject({ text: "Report to Lead, #3 Saved cards API, attempt 1: “Done: the parser”", by: "Lead" });
    expect(line("interim_report", { member: "Backend", task: 3, attempt: 1, text: "half done" }))
      .toMatchObject({ text: "Interim report, #3 Saved cards API, attempt 1: “half done”", tone: "active", by: "Backend" });
    expect(line("unconfirmed", { task: 3, error: "timeout" })).toMatchObject({ text: "Delivery unconfirmed, #3 Saved cards API (timeout)", tone: "danger" });
    expect(line("stage_returned", { stage: "review", task: 3, round: 2, findings: [{}, {}] })).toMatchObject({ text: "Review returned #3 Saved cards API, round 2, 2 findings", tone: "warn" });
    expect(line("accepted", { by: "Pipeline", task: 3, reason: "ok" }).text).toBe("Accepted #3 Saved cards API");
    expect(line("rejected", { task: 3, reason: "no tests" }).text).toBe("Rejected #3 Saved cards API: “no tests”");
    expect(line("member_added", { member: "Tester", by: "Owner", owner_word: "need QA" })).toMatchObject({ text: "Added Tester by Owner: “need QA”", by: "Owner" });
    expect(historyStage("sub_lead")).toBe("Sub-lead");
    expect(historyStage("deploy")).toBe("Deploy");
  });

  it("never separates values with dots", () => {
    const kinds = ["created", "planned", "done", "phase_added", "phase_approved", "phase_moved", "phase_discarded", "all_decided", "finished", "handed_out", "start_late",
      "unconfirmed", "reported", "interim_report", "report_replaced", "question_asked", "question_answered", "member_adopted", "stale_report", "stage_started",
      "stage_passed", "stage_returned", "round_limit", "stages_passed", "no_report", "member_started"];
    for (const kind of kinds) expect(line(kind, { task: 3, attempt: 1, phase: 1, tasks: 2, stage: "review", round: 1, to: "Lead" }).text).not.toMatch(/[\u00b7\u2022]/);
  });

  it("filters the feed by the focused member's role and dates its lines", () => {
    const lines = [{ text: "Delivered #3", by: "Backend", tone: "neutral" as const }, { text: "Task to Tester, #4", by: "Lead", tone: "neutral" as const }, { text: "Created the team", by: "", tone: "neutral" as const }];
    expect(focusedLines(lines, "Tester").map((found) => found.text)).toEqual(["Task to Tester, #4"]);
    expect(focusedLines(lines, "").length).toBe(3);
    expect(feedClock(0)).toBe("");
    expect(feedClock(new Date(2026, 10, 8, 9, 5).getTime() / 1000)).toBe("09:05");
    expect([inboxKind("interim_report"), inboxKind("merge_waiting"), inboxKind("other")]).toEqual(["interim", "merge waiting", "other"]);
  });

  it("serves the watched team's feed and inbox the core sends", () => {
    createRoot((dispose) => {
      const store = createTeams({ follow: async () => {} });
      store.apply([
        { computer: "", key: "feed:t1", kind: "feed", payload: { team: "t1", events: [{ seq: 9, at: 1, kind: "created", detail: {} }], more: true, capped: false } },
        { computer: "", key: "inbox:t1", kind: "inbox", payload: { team: "t1", items: [{ kind: "report", member: "backend", task: 3, summary: "done", since: 1, command: "hgs team accept", detail: {} }], truncated: false } },
      ]);
      expect(store.feed("", "t1")).toMatchObject({ more: true, events: [{ seq: 9 }] });
      expect(store.inbox("", "t1")?.items[0]).toMatchObject({ kind: "report", command: "hgs team accept" });
      expect(store.feed("", "t2")).toBeUndefined();
      dispose();
    });
  });
});
