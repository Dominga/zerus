import { describe, expect, it } from "vitest";
import { noReportReply, reminderText, remindMember } from "./noReport";
import type { TeamInboxItem, TeamMemberView, TeamTaskView, TeamView } from "../../types";

// Task #415: the no_report inbox item carries the attempt's last reply; the
// card shows it and offers it as the report; Remind sends the member's report
// command as a draft in its session.
function item(over: Partial<TeamInboxItem>): TeamInboxItem {
  return { kind: "no_report", member: "Backend", task: 7, summary: "", since: 0, command: "", detail: "", ...over };
}

function task(over: Partial<TeamTaskView> = {}): TeamTaskView {
  return { id: 7, title: "Paging API", status: "in_progress", assignee: "api", phase: 1, deps: [], estimate_minutes: 0,
    attempts: 2, attempt_state: "delivered", decided_by: "", pinned: false, output: [], waits: "", no_report: true, attempt_id: "a7", ...over };
}

function team(): TeamView {
  return {
    id: "t1", name: "Checkout", status: "running", project: "", folder: "", goal: "", plan_mode: "", lead: "lead",
    revision: 1, created: 0, updated: 0, waiting_for_lead: false, pending_phase: null, settings: { auto_accept: false },
    members: [{ id: "api", role: "Backend", agent: "codex", model: "", effort: "", account: "", computer: "", folder: "",
      state: "active", activity: "idle", phase: "", error: "", lead: false, session: { host: "studio", name: "codex/x/api", run_id: "r", conversation_id: "c" },
      live: true, stuck: false, question: false, queue: [], minutes_without_progress: 0, host_shared: false } as TeamMemberView],
    tasks: [], phases: [],
    proposals: [], attention: [], validation: { errors: [], warnings: [] }, health: { critical_path: 0, parallelism: 0, warnings: [] },
    ready: [], cycle: [], draft: null, truncated: false,
  } as unknown as TeamView;
}

describe("noReportReply (task #415)", () => {
  it("reads the no_report item's kept reply for the task", () => {
    const items = [item({ task: 7, detail: "Done: the parser and its tests" }), item({ task: 8, detail: "Other" })];
    expect(noReportReply(items, 7)).toBe("Done: the parser and its tests");
    expect(noReportReply(items, 9)).toBe("");
  });

  it("ignores a report item and a non-string detail", () => {
    expect(noReportReply([item({ kind: "report", detail: "A report" })], 7)).toBe("");
    expect(noReportReply([item({ detail: { text: "obj" } })], 7)).toBe("");
    expect(noReportReply([item({ detail: "   " })], 7)).toBe("");
  });
});

describe("reminderText (task #415)", () => {
  it("names the task, its attempt, the host and the report command", () => {
    const text = reminderText(team(), task(), team().members[0]);
    expect(text).toContain("[Zerus team, from: You, task: #7, attempt: 2]");
    expect(text).toContain("Your turn ended without a report for task #7 Paging API.");
    expect(text).toContain("hgs @studio team report --json");
    const body = text.split("\n").at(-1)!.trim();
    const json = JSON.parse(body);
    expect(json).toMatchObject({ team: "t1", task: 7, attempt: "a7", text: "..." });
    expect(json.request_id).toMatch(/^[0-9a-f-]{36}$/);
  });

  it("leaves the host out when the member's session has none", () => {
    const own = team();
    own.members[0].session = { host: "", name: "codex/x/api", run_id: "r", conversation_id: "c" };
    expect(reminderText(own, task(), own.members[0])).toContain("hgs team report --json");
    expect(reminderText(own, task(), own.members[0])).not.toContain("@studio");
  });
});

describe("remindMember (task #415)", () => {
  it("says the member's session is missing when it is not listed", async () => {
    expect(await remindMember("c1", team(), task())).toBe("missing");
    // A task whose member has no session cannot be reminded either.
    const noSession = team();
    noSession.members[0].session = null;
    expect(await remindMember("c1", noSession, task())).toBe("missing");
  });
});
