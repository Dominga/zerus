import { describe, expect, it } from "vitest";
import { foldedBody, noticeBar, systemNotice } from "./notices";
import { buildTranscript } from "./transcript";
import type { ActivityEvent } from "../../types";
import type { ActivityItem } from "./types";

// Task 208: what Zerus pastes into a session reads as a system notice with its
// sender, never as the owner's "You" (Qt ActivityView.cpp teamMessage).
const HAND_OUT = "[Zerus team \u00b7 from: Lead \u00b7 task: #208 \u00b7 attempt: 1]\nTeam: Zerus Next for the fork. Your role: Frontend (SolidJS).\n"
  + "Task #208: Activity notices look like Qt's\nOwner, 2026-11-07: show them as system notices.\nWhen you finish, report once with hgs team report.";

describe("system notices (task 208)", () => {
  it("reads a team hand-out as Qt's plaque: kind, sender and receiver, task, attempt and the body", () => {
    expect(systemNotice(HAND_OUT)).toMatchObject({
      kind: "Task from team", from: "Lead", to: "Frontend (SolidJS)", task: 208, attempt: 1, title: "Activity notices look like Qt's",
      body: "Owner, 2026-11-07: show them as system notices.", header: "[Zerus team \u00b7 from: Lead \u00b7 task: #208 \u00b7 attempt: 1]",
    });
  });

  it("reads Zerus's inbox notices, reports, planning briefs and the owner's team messages", () => {
    expect(systemNotice("[Zerus team \u00b7 from: Zerus \u00b7 inbox]\n1 new message")).toMatchObject({ kind: "Task from team", from: "Zerus", body: "1 new message" });
    expect(systemNotice("[Zerus team \u00b7 from: Backend \u00b7 task: #4 \u00b7 attempt: 2]\nReport for task #4 Saved cards API: done, tests green.\n\nDecide with hgs team accept"))
      .toMatchObject({ kind: "Report to lead", from: "Backend", to: "Lead", title: "Saved cards API", body: "done, tests green." });
    expect(systemNotice("[Zerus team \u00b7 from: Zerus \u00b7 planning]\nGoal: ship.\n\nPlan the work as phases")).toMatchObject({ kind: "Planning brief", to: "Lead", body: "Goal: ship." });
    expect(systemNotice("[Zerus team \u00b7 from: Zerus \u00b7 all tasks decided]\nTeam: v1.2 release. All tasks are decided.")).toMatchObject({ kind: "All tasks decided", body: "All tasks are decided." });
    expect(systemNotice("[Zerus team \u00b7 from: You \u00b7 task: #3 \u00b7 attempt: 2]\nYour turn ended without a report")).toMatchObject({ kind: "Message from you", from: "You", task: 3 });
  });

  it("reads a header inside a session's envelope and Zerus's sender-less doorbells", () => {
    expect(systemNotice('<zerus-message from="claude/p/lead">\n[Zerus team \u00b7 from: Lead \u00b7 task: #2]\nGo\n</zerus-message>')).toMatchObject({ from: "Lead", task: 2, body: "Go" });
    expect(systemNotice("[Zerus team] 1 new message for you: hgs team inbox")).toMatchObject({ kind: "Zerus notice", from: "Zerus team", body: "1 new message for you: hgs team inbox" });
    expect(systemNotice("[Zerus task 12] Fix the build")).toMatchObject({ kind: "Zerus notice", from: "Zerus", body: "Fix the build" });
  });

  it("never takes the owner's own words for a notice", () => {
    for (const text of ["Please fix the build", "What does [Zerus team \u00b7 from: Lead] mean?", "[Zerus teams] are great", " [zerus team] lower case", ""]) expect(systemNotice(text)).toBeUndefined();
  });

  it("folds the body to three lines as Qt does and says when it hides something", () => {
    expect(foldedBody({ body: "a\nb\nc\nd", full: "x" })).toEqual({ text: "a\nb\nc …", clipped: true });
    expect(foldedBody({ body: "short", full: "short" })).toEqual({ text: "short", clipped: false });
    expect(foldedBody({ body: "short", full: "[hdr]\nshort" }).clipped).toBe(true);
  });

  it("gives Zerus a pink bar and a member its role's colour", () => {
    expect(noticeBar("Zerus").dark).toBe("#f08bb4");
    expect(noticeBar("Zerus team").dark).toBe("#f08bb4");
    expect(noticeBar("Lead").dark).toBe("#b79cff");
    expect(noticeBar("Backend").dark).toBe("#6fa8ff");
  });

  it("turns a pasted notice into a system item and leaves the owner's message and the agent's reply alone", () => {
    const events: ActivityEvent[] = [
      { seq: 1, type: "UserPromptSubmit", at: 10, detail: HAND_OUT },
      { seq: 2, type: "UserPromptSubmit", at: 11, detail: "Please also check the light theme" },
      { seq: 3, type: "Stop", at: 12, detail: "Done: the notices are plaques." },
      { seq: 4, type: "UserPromptSubmit", at: 13, detail: "[Zerus team] Your build has finished." },
      { seq: 5, type: "UserPromptSubmit", at: 14, origin: "peer_message", sender: "claude/p/lead", detail: "Message from claude/p/lead",
        report: "[Zerus team \u00b7 from: Lead \u00b7 task: #9]\nTake this one" },
      { seq: 6, type: "UserPromptSubmit", at: 15, origin: "peer_message", sender: "claude/p/qa", detail: "Message from claude/p/qa", report: "Are you done?" },
    ] as ActivityEvent[];
    const items = buildTranscript(events) as ActivityItem[];
    expect(items.map((item) => [item.kind, item.sender ?? item.label ?? "", item.notice?.kind ?? ""])).toEqual([
      ["system", "Lead", "Task from team"],
      ["user", "", ""],
      ["agent", "", ""],
      ["system", "Zerus team", "Zerus notice"],
      ["system", "Lead", "Task from team"],
      ["system", "Message from claude/p/qa", ""],
    ]);
    // A notice is never "Sent" by the owner.
    expect(items[0].receipt).toBeUndefined();
    expect(items[1].receipt).toBe("Sent");
  });
});
